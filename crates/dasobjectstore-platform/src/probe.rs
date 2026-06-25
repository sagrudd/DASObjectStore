use crate::model::ProbeReport;
use std::fmt::{self, Display};
use std::process::Command;

pub trait ProbeProvider {
    fn probe(&self) -> Result<ProbeReport, ProbeError>;
}

pub trait CommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError>;
}

#[derive(Debug, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
        let output =
            Command::new(command)
                .args(args)
                .output()
                .map_err(|err| ProbeError::CommandFailed {
                    command: command.to_string(),
                    message: err.to_string(),
                })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(ProbeError::CommandFailed {
                command: command.to_string(),
                message: stderr,
            });
        }

        String::from_utf8(output.stdout).map_err(|err| ProbeError::ParseFailed {
            source: command.to_string(),
            message: err.to_string(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
