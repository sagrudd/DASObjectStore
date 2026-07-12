//! Resumable profile-promotion state machine.
//!
//! This state records safety boundaries only; a daemon migration worker owns
//! copying, verification, persistence, and actual source retirement.

use crate::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    Planned,
    Copying,
    DestinationVerified,
    RetirementPending,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreMigration {
    pub migration_id: String,
    pub source_store_id: StoreId,
    pub destination_store_id: StoreId,
    pub state: MigrationState,
    pub source_retained: bool,
}

impl StoreMigration {
    pub fn new(
        migration_id: impl Into<String>,
        source_store_id: StoreId,
        destination_store_id: StoreId,
    ) -> Result<Self, MigrationTransitionError> {
        let migration_id = migration_id.into();
        if migration_id.trim().is_empty() {
            return Err(MigrationTransitionError::BlankMigrationId);
        }
        if source_store_id == destination_store_id {
            return Err(MigrationTransitionError::SameSourceAndDestination);
        }
        Ok(Self {
            migration_id,
            source_store_id,
            destination_store_id,
            state: MigrationState::Planned,
            source_retained: true,
        })
    }

    pub fn start_copy(&mut self) -> Result<(), MigrationTransitionError> {
        self.transition(MigrationState::Planned, MigrationState::Copying)
    }

    pub fn mark_destination_verified(&mut self) -> Result<(), MigrationTransitionError> {
        self.transition(MigrationState::Copying, MigrationState::RetirementPending)
    }

    pub fn confirm_source_retirement(&mut self) -> Result<(), MigrationTransitionError> {
        if self.state != MigrationState::RetirementPending {
            return Err(MigrationTransitionError::InvalidTransition {
                state: self.state,
                action: "confirm_source_retirement",
            });
        }
        self.source_retained = false;
        self.state = MigrationState::Completed;
        Ok(())
    }

    pub fn fail(&mut self) -> Result<(), MigrationTransitionError> {
        if matches!(
            self.state,
            MigrationState::Completed | MigrationState::Failed
        ) {
            return Err(MigrationTransitionError::InvalidTransition {
                state: self.state,
                action: "fail",
            });
        }
        self.state = MigrationState::Failed;
        self.source_retained = true;
        Ok(())
    }

    fn transition(
        &mut self,
        expected: MigrationState,
        next: MigrationState,
    ) -> Result<(), MigrationTransitionError> {
        if self.state != expected {
            return Err(MigrationTransitionError::InvalidTransition {
                state: self.state,
                action: match next {
                    MigrationState::Copying => "start_copy",
                    MigrationState::RetirementPending => "mark_destination_verified",
                    _ => "transition",
                },
            });
        }
        self.state = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationTransitionError {
    BlankMigrationId,
    SameSourceAndDestination,
    InvalidTransition {
        state: MigrationState,
        action: &'static str,
    },
}

impl Display for MigrationTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankMigrationId => formatter.write_str("migration_id must not be blank"),
            Self::SameSourceAndDestination => {
                formatter.write_str("migration source and destination must differ")
            }
            Self::InvalidTransition { state, action } => {
                write!(formatter, "cannot {action} while migration is {state:?}")
            }
        }
    }
}

impl std::error::Error for MigrationTransitionError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores() -> (StoreId, StoreId) {
        (
            StoreId::new("source").expect("source id"),
            StoreId::new("destination").expect("destination id"),
        )
    }

    #[test]
    fn retains_source_until_verified_and_explicitly_retired() {
        let (source, destination) = stores();
        let mut migration =
            StoreMigration::new("migration-1", source, destination).expect("migration creates");
        assert_eq!(migration.state, MigrationState::Planned);
        assert!(migration.source_retained);
        migration.start_copy().expect("copy starts");
        migration
            .mark_destination_verified()
            .expect("destination verifies");
        assert_eq!(migration.state, MigrationState::RetirementPending);
        assert!(migration.source_retained);
        migration
            .confirm_source_retirement()
            .expect("retirement confirms");
        assert_eq!(migration.state, MigrationState::Completed);
        assert!(!migration.source_retained);
    }

    #[test]
    fn failed_migration_keeps_source_and_rejects_invalid_transitions() {
        let (source, destination) = stores();
        let mut migration =
            StoreMigration::new("migration-2", source, destination).expect("migration creates");
        assert!(migration.confirm_source_retirement().is_err());
        migration.start_copy().expect("copy starts");
        migration.fail().expect("migration fails");
        assert_eq!(migration.state, MigrationState::Failed);
        assert!(migration.source_retained);
        assert!(migration.start_copy().is_err());
        assert!(migration.fail().is_err());
    }

    #[test]
    fn rejects_blank_id_and_same_store_migration() {
        let (source, destination) = stores();
        assert_eq!(
            StoreMigration::new("  ", source.clone(), destination.clone()),
            Err(MigrationTransitionError::BlankMigrationId)
        );
        assert_eq!(
            StoreMigration::new("migration-3", source.clone(), source.clone()),
            Err(MigrationTransitionError::SameSourceAndDestination)
        );
        let encoded = serde_json::to_value(
            StoreMigration::new("migration-4", source, destination).expect("migration creates"),
        )
        .expect("migration serializes");
        assert_eq!(encoded["state"], "planned");
        assert_eq!(encoded["source_retained"], true);
    }
}
