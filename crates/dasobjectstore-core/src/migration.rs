//! Resumable profile-promotion state machine.
//!
//! This state records safety boundaries only; a daemon migration worker owns
//! copying, verification, persistence, and actual source retirement.

use crate::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const STORE_MIGRATION_SCHEMA_VERSION: u16 = 1;

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
#[serde(deny_unknown_fields)]
pub struct StoreMigration {
    pub schema_version: u16,
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
            schema_version: STORE_MIGRATION_SCHEMA_VERSION,
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

    pub fn save_atomic(&self, path: impl AsRef<Path>) -> Result<(), MigrationPersistenceError> {
        self.validate_persisted_state()?;
        let path = path.as_ref();
        let parent = path
            .parent()
            .ok_or_else(|| MigrationPersistenceError::Io("checkpoint has no parent".to_string()))?;
        fs::create_dir_all(parent).map_err(io_error)?;
        let temporary = temporary_checkpoint_path(path);
        let payload = serde_json::to_vec_pretty(self)
            .map_err(|error| MigrationPersistenceError::Malformed(error.to_string()))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        file.write_all(&payload).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        fs::rename(&temporary, path).map_err(io_error)?;
        File::open(parent)
            .map_err(io_error)?
            .sync_all()
            .map_err(io_error)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, MigrationPersistenceError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(io_error)?;
        let migration: Self = serde_json::from_reader(file)
            .map_err(|error| MigrationPersistenceError::Malformed(error.to_string()))?;
        if migration.schema_version != STORE_MIGRATION_SCHEMA_VERSION {
            return Err(MigrationPersistenceError::UnsupportedSchema {
                found: migration.schema_version,
                supported: STORE_MIGRATION_SCHEMA_VERSION,
            });
        }
        migration.validate_persisted_state()?;
        Ok(migration)
    }

    fn validate_persisted_state(&self) -> Result<(), MigrationPersistenceError> {
        if self.migration_id.trim().is_empty() {
            return Err(MigrationPersistenceError::InvalidState(
                "migration_id must not be blank".to_string(),
            ));
        }
        if self.source_store_id == self.destination_store_id {
            return Err(MigrationPersistenceError::InvalidState(
                "migration source and destination must differ".to_string(),
            ));
        }
        if self.state == MigrationState::Completed && self.source_retained {
            return Err(MigrationPersistenceError::InvalidState(
                "completed migration must not retain source".to_string(),
            ));
        }
        if self.state != MigrationState::Completed && !self.source_retained {
            return Err(MigrationPersistenceError::InvalidState(
                "incomplete migration must retain source".to_string(),
            ));
        }
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

fn temporary_checkpoint_path(path: &Path) -> PathBuf {
    let mut temporary = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("json");
    temporary.set_extension(format!("{extension}.tmp"));
    temporary
}

fn io_error(error: std::io::Error) -> MigrationPersistenceError {
    MigrationPersistenceError::Io(error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationPersistenceError {
    Io(String),
    Malformed(String),
    UnsupportedSchema { found: u16, supported: u16 },
    InvalidState(String),
}

impl Display for MigrationPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "migration checkpoint I/O failed: {message}"),
            Self::Malformed(message) => {
                write!(formatter, "malformed migration checkpoint: {message}")
            }
            Self::UnsupportedSchema { found, supported } => {
                write!(
                    formatter,
                    "unsupported migration schema {found}; supported {supported}"
                )
            }
            Self::InvalidState(message) => {
                write!(formatter, "invalid migration checkpoint: {message}")
            }
        }
    }
}

impl std::error::Error for MigrationPersistenceError {}

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

    #[test]
    fn saves_and_reloads_atomic_checkpoint() {
        let (source, destination) = stores();
        let mut migration = StoreMigration::new("migration-persist", source, destination)
            .expect("migration creates");
        migration.start_copy().expect("copy starts");
        let path = checkpoint_path("persist");
        migration.save_atomic(&path).expect("checkpoint saves");
        let loaded = StoreMigration::load(&path).expect("checkpoint loads");
        assert_eq!(loaded, migration);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_future_schema_and_invalid_retirement_state() {
        let path = checkpoint_path("future");
        fs::create_dir_all(path.parent().expect("checkpoint parent")).expect("parent creates");
        fs::write(
            &path,
            r#"{"schema_version":2,"migration_id":"future","source_store_id":"source","destination_store_id":"destination","state":"planned","source_retained":true}"#,
        )
        .expect("future checkpoint writes");
        assert_eq!(
            StoreMigration::load(&path),
            Err(MigrationPersistenceError::UnsupportedSchema {
                found: 2,
                supported: STORE_MIGRATION_SCHEMA_VERSION,
            })
        );
        let _ = fs::remove_file(path);

        let (source, destination) = stores();
        let invalid = StoreMigration {
            schema_version: STORE_MIGRATION_SCHEMA_VERSION,
            migration_id: "invalid".to_string(),
            source_store_id: source,
            destination_store_id: destination,
            state: MigrationState::Planned,
            source_retained: false,
        };
        assert!(matches!(
            invalid.save_atomic(checkpoint_path("invalid")),
            Err(MigrationPersistenceError::InvalidState(_))
        ));
    }

    fn checkpoint_path(name: &str) -> PathBuf {
        std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("dasobjectstore-migration-{name}.json"))
    }
}
