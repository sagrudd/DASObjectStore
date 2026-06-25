use crate::model::ProbeReport;
use std::fmt::{self, Display};

pub trait ProbeProvider {
    fn probe(&self) -> Result<ProbeReport, ProbeError>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProbeError {
    CommandFailed { command: String, message: String },
    ParseFailed { source: String, message: String },
    UnsupportedPlatform { platform: String },
}

impl Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed { command, message } => {
                write!(formatter, "probe command `{command}` failed: {message}")
            }
            Self::ParseFailed { source, message } => {
                write!(formatter, "failed to parse {source}: {message}")
            }
            Self::UnsupportedPlatform { platform } => {
                write!(formatter, "unsupported probe platform: {platform}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

#[cfg(test)]
mod tests {
    use super::ProbeError;

    #[test]
    fn formats_probe_errors() {
        let err = ProbeError::UnsupportedPlatform {
            platform: "plan9".to_string(),
        };

        assert_eq!(err.to_string(), "unsupported probe platform: plan9");
    }
}
