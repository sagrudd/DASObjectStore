//! Versioned authority envelope for host-composed administrative mutations.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

/// Stable envelope for a Pistis-verified subject crossing the host API to
/// daemon boundary. The Unix peer credentials remain the non-forgeable half
/// of the authority decision; this value deliberately contains no token,
/// password, local user name, UID, GID, group, or sudo assertion.
pub const PREVERIFIED_HOST_SUBJECT_SCHEMA_VERSION: &str =
    "dasobjectstore.preverified_host_subject.v1";

/// Identity asserted by the reviewed DAS GUI/API host adapter.
pub const PREVERIFIED_HOST_GUI_API_PEER_IDENTITY: &str = "dasobjectstore-gui-api";

/// Identity asserted by the reviewed Monas product host adapter.
pub const PREVERIFIED_HOST_MONAS_PEER_IDENTITY: &str = "mnemosyne-monas";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreverifiedHostSubject {
    pub schema_version: String,
    pub peer_identity: String,
    pub subject_id: String,
    pub session_id: String,
    pub correlation_id: String,
}

impl PreverifiedHostSubject {
    pub fn validate(&self) -> Result<(), PreverifiedHostSubjectValidationError> {
        if self.schema_version != PREVERIFIED_HOST_SUBJECT_SCHEMA_VERSION {
            return Err(
                PreverifiedHostSubjectValidationError::UnsupportedSchemaVersion(
                    self.schema_version.clone(),
                ),
            );
        }
        if !matches!(
            self.peer_identity.as_str(),
            PREVERIFIED_HOST_GUI_API_PEER_IDENTITY | PREVERIFIED_HOST_MONAS_PEER_IDENTITY
        ) {
            return Err(
                PreverifiedHostSubjectValidationError::UnsupportedPeerIdentity(
                    self.peer_identity.clone(),
                ),
            );
        }
        for (field, value) in [
            ("subject_id", &self.subject_id),
            ("session_id", &self.session_id),
            ("correlation_id", &self.correlation_id),
        ] {
            if !is_safe_identifier(value) {
                return Err(PreverifiedHostSubjectValidationError::InvalidIdentifier {
                    field,
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreverifiedHostSubjectValidationError {
    UnsupportedSchemaVersion(String),
    UnsupportedPeerIdentity(String),
    InvalidIdentifier { field: &'static str, value: String },
}

impl Display for PreverifiedHostSubjectValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion(value) => {
                write!(
                    formatter,
                    "unsupported preverified host subject schema version {value:?}"
                )
            }
            Self::UnsupportedPeerIdentity(value) => {
                write!(
                    formatter,
                    "unsupported preverified host peer identity {value:?}"
                )
            }
            Self::InvalidIdentifier { field, value } => {
                write!(formatter, "invalid preverified host {field} {value:?}")
            }
        }
    }
}

impl std::error::Error for PreverifiedHostSubjectValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        PreverifiedHostSubject, PreverifiedHostSubjectValidationError,
        PREVERIFIED_HOST_GUI_API_PEER_IDENTITY, PREVERIFIED_HOST_MONAS_PEER_IDENTITY,
        PREVERIFIED_HOST_SUBJECT_SCHEMA_VERSION,
    };

    fn subject() -> PreverifiedHostSubject {
        PreverifiedHostSubject {
            schema_version: PREVERIFIED_HOST_SUBJECT_SCHEMA_VERSION.to_string(),
            peer_identity: PREVERIFIED_HOST_GUI_API_PEER_IDENTITY.to_string(),
            subject_id: "pistis:operator-1".to_string(),
            session_id: "session-1".to_string(),
            correlation_id: "request-1".to_string(),
        }
    }

    #[test]
    fn accepts_versioned_non_secret_subject_identifiers() {
        assert!(subject().validate().is_ok());
    }

    #[test]
    fn accepts_the_separate_monas_host_identity() {
        let mut value = subject();
        value.peer_identity = PREVERIFIED_HOST_MONAS_PEER_IDENTITY.to_string();
        assert!(value.validate().is_ok());
    }

    #[test]
    fn rejects_unsupported_adapter_identity() {
        let mut value = subject();
        value.peer_identity = "unreviewed-adapter".to_string();
        assert!(matches!(
            value.validate(),
            Err(PreverifiedHostSubjectValidationError::UnsupportedPeerIdentity(_))
        ));
    }

    #[test]
    fn rejects_os_identity_shaped_subject_data() {
        let mut value = subject();
        value.subject_id = "root sudo".to_string();
        assert!(matches!(
            value.validate(),
            Err(PreverifiedHostSubjectValidationError::InvalidIdentifier {
                field: "subject_id",
                ..
            })
        ));
    }
}
