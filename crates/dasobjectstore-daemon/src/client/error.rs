use crate::api::{DaemonJobValidationError, DaemonRequestValidationError};
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Debug)]
pub enum DaemonClientError {
    RequestValidation(DaemonRequestValidationError),
    JobValidation(DaemonJobValidationError),
    Transport(String),
    UnexpectedResponse {
        expected: &'static str,
        actual: &'static str,
    },
    UnixSocketTransportPlanned {
        socket_path: PathBuf,
    },
}

impl Display for DaemonClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestValidation(err) => write!(formatter, "{err}"),
            Self::JobValidation(err) => write!(formatter, "{err}"),
            Self::Transport(message) => write!(formatter, "daemon transport failed: {message}"),
            Self::UnexpectedResponse { expected, actual } => {
                write!(
                    formatter,
                    "daemon returned {actual} response where {expected} was expected"
                )
            }
            Self::UnixSocketTransportPlanned { socket_path } => write!(
                formatter,
                "Unix-domain socket daemon transport is planned but not implemented yet: {}",
                socket_path.display()
            ),
        }
    }
}

impl std::error::Error for DaemonClientError {}

impl From<DaemonRequestValidationError> for DaemonClientError {
    fn from(err: DaemonRequestValidationError) -> Self {
        Self::RequestValidation(err)
    }
}

impl From<DaemonJobValidationError> for DaemonClientError {
    fn from(err: DaemonJobValidationError) -> Self {
        Self::JobValidation(err)
    }
}
