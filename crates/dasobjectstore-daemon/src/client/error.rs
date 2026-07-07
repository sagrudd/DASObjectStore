use crate::api::{DaemonJobValidationError, DaemonRequestValidationError};
use std::fmt::{self, Display};

#[derive(Debug)]
pub enum DaemonClientError {
    RequestValidation(DaemonRequestValidationError),
    JobValidation(DaemonJobValidationError),
    Transport(String),
    UnexpectedResponse {
        expected: &'static str,
        actual: &'static str,
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
