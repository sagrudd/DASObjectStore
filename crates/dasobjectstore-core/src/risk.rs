//! Risk gates for operations that can lose data or bypass normal safety paths.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RiskyOperation {
    DirectToHddImport,
    ForceRetire,
    ForceReadWriteImport,
}

impl RiskyOperation {
    pub fn name(self) -> &'static str {
        match self {
            Self::DirectToHddImport => "direct_to_hdd_import",
            Self::ForceRetire => "force_retire",
            Self::ForceReadWriteImport => "force_read_write_import",
        }
    }

    pub fn confirmation_phrase(self) -> &'static str {
        match self {
            Self::DirectToHddImport => "confirm direct-to-hdd import",
            Self::ForceRetire => "confirm force retire",
            Self::ForceReadWriteImport => "confirm force read-write import",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RiskPolicy {
    pub allow_direct_to_hdd_import: bool,
    pub allow_force_retire: bool,
    pub allow_force_read_write_import: bool,
}

impl RiskPolicy {
    pub fn allows(self, operation: RiskyOperation) -> bool {
        match operation {
            RiskyOperation::DirectToHddImport => self.allow_direct_to_hdd_import,
            RiskyOperation::ForceRetire => self.allow_force_retire,
            RiskyOperation::ForceReadWriteImport => self.allow_force_read_write_import,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActionConfirmation {
    pub phrase: String,
}

impl ActionConfirmation {
    pub fn new(phrase: impl Into<String>) -> Self {
        Self {
            phrase: phrase.into(),
        }
    }

    pub fn for_operation(operation: RiskyOperation) -> Self {
        Self::new(operation.confirmation_phrase())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiskGate {
    pub policy: RiskPolicy,
}

impl RiskGate {
    pub fn new(policy: RiskPolicy) -> Self {
        Self { policy }
    }

    pub fn evaluate(
        &self,
        operation: RiskyOperation,
        confirmation: &ActionConfirmation,
    ) -> Result<(), RiskGateError> {
        if !self.policy.allows(operation) {
            return Err(RiskGateError::PolicyDoesNotAllow { operation });
        }

        let required_phrase = operation.confirmation_phrase();
        let provided_phrase = confirmation.phrase.trim();

        if provided_phrase.is_empty() {
            return Err(RiskGateError::MissingConfirmation {
                operation,
                required_phrase,
            });
        }

        if provided_phrase != required_phrase {
            return Err(RiskGateError::ConfirmationMismatch {
                operation,
                required_phrase,
            });
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskGateError {
    PolicyDoesNotAllow {
        operation: RiskyOperation,
    },
    MissingConfirmation {
        operation: RiskyOperation,
        required_phrase: &'static str,
    },
    ConfirmationMismatch {
        operation: RiskyOperation,
        required_phrase: &'static str,
    },
}

impl Display for RiskGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyDoesNotAllow { operation } => {
                write!(
                    formatter,
                    "risk policy does not allow operation {}",
                    operation.name()
                )
            }
            Self::MissingConfirmation {
                required_phrase, ..
            } => {
                write!(
                    formatter,
                    "missing action confirmation; pass `{required_phrase}`"
                )
            }
            Self::ConfirmationMismatch {
                required_phrase, ..
            } => {
                write!(
                    formatter,
                    "action confirmation mismatch; pass `{required_phrase}`"
                )
            }
        }
    }
}

impl std::error::Error for RiskGateError {}

#[cfg(test)]
mod tests {
    use super::{ActionConfirmation, RiskGate, RiskGateError, RiskPolicy, RiskyOperation};

    #[test]
    fn operation_names_are_stable_snake_case() {
        assert_eq!(
            RiskyOperation::DirectToHddImport.name(),
            "direct_to_hdd_import"
        );
        assert_eq!(RiskyOperation::ForceRetire.name(), "force_retire");
        assert_eq!(
            RiskyOperation::ForceReadWriteImport.name(),
            "force_read_write_import"
        );
    }

    #[test]
    fn denies_each_operation_without_policy_allowance() {
        let gate = RiskGate::new(RiskPolicy::default());

        for operation in all_risky_operations() {
            let confirmation = ActionConfirmation::for_operation(operation);
            let err = gate
                .evaluate(operation, &confirmation)
                .expect_err("default policy denies risky operations");

            assert_eq!(err, RiskGateError::PolicyDoesNotAllow { operation });
        }
    }

    #[test]
    fn rejects_each_allowed_operation_without_confirmation() {
        for operation in all_risky_operations() {
            let gate = RiskGate::new(policy_allowing(operation));
            let err = gate
                .evaluate(operation, &ActionConfirmation::default())
                .expect_err("confirmation is mandatory");

            assert_eq!(
                err,
                RiskGateError::MissingConfirmation {
                    operation,
                    required_phrase: operation.confirmation_phrase()
                }
            );
        }
    }

    #[test]
    fn rejects_each_allowed_operation_with_wrong_confirmation() {
        for operation in all_risky_operations() {
            let gate = RiskGate::new(policy_allowing(operation));
            let err = gate
                .evaluate(
                    operation,
                    &ActionConfirmation::new("confirm something else"),
                )
                .expect_err("confirmation must match operation");

            assert_eq!(
                err,
                RiskGateError::ConfirmationMismatch {
                    operation,
                    required_phrase: operation.confirmation_phrase()
                }
            );
        }
    }

    #[test]
    fn accepts_operation_with_policy_allowance_and_confirmation() {
        let gate = RiskGate::new(RiskPolicy {
            allow_force_retire: true,
            ..RiskPolicy::default()
        });

        gate.evaluate(
            RiskyOperation::ForceRetire,
            &ActionConfirmation::for_operation(RiskyOperation::ForceRetire),
        )
        .expect("policy allowance plus matching confirmation should pass");
    }

    #[test]
    fn policy_allowance_is_per_operation() {
        let policy = RiskPolicy {
            allow_direct_to_hdd_import: true,
            ..RiskPolicy::default()
        };

        assert!(policy.allows(RiskyOperation::DirectToHddImport));
        assert!(!policy.allows(RiskyOperation::ForceRetire));
        assert!(!policy.allows(RiskyOperation::ForceReadWriteImport));
    }

    fn all_risky_operations() -> [RiskyOperation; 3] {
        [
            RiskyOperation::DirectToHddImport,
            RiskyOperation::ForceRetire,
            RiskyOperation::ForceReadWriteImport,
        ]
    }

    fn policy_allowing(operation: RiskyOperation) -> RiskPolicy {
        match operation {
            RiskyOperation::DirectToHddImport => RiskPolicy {
                allow_direct_to_hdd_import: true,
                ..RiskPolicy::default()
            },
            RiskyOperation::ForceRetire => RiskPolicy {
                allow_force_retire: true,
                ..RiskPolicy::default()
            },
            RiskyOperation::ForceReadWriteImport => RiskPolicy {
                allow_force_read_write_import: true,
                ..RiskPolicy::default()
            },
        }
    }
}
