use crate::cli::{Cli, Command, ProbeArgs};
#[cfg(target_os = "linux")]
use dasobjectstore_platform::linux::LinuxProbeProvider;
#[cfg(target_os = "macos")]
use dasobjectstore_platform::macos::MacosProbeProvider;
use dasobjectstore_platform::{group_enclosures, ProbeError, ProbeProvider, ProbeReport};
use std::fmt::{self, Display};
use std::io::{self, Write};

pub(crate) fn run(cli: &Cli, writer: &mut impl Write) -> Result<(), CliError> {
    match cli.command() {
        Some(Command::Probe(args)) => run_probe(args, writer),
        _ => Ok(()),
    }
}

fn run_probe(args: &ProbeArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if !args.json() {
        return Err(CliError::UnsupportedProbeFormat);
    }

    let mut report = probe_current_platform()?;
    report.enclosures = group_enclosures(&report.disks);
    serde_json::to_writer(&mut *writer, &report)?;
    writer.write_all(b"\n")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    LinuxProbeProvider::system().probe()
}

#[cfg(target_os = "macos")]
fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    MacosProbeProvider::system().probe()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    Err(ProbeError::UnsupportedPlatform {
        platform: std::env::consts::OS.to_string(),
    })
}

#[derive(Debug)]
pub(crate) enum CliError {
    Io(io::Error),
    Json(serde_json::Error),
    Probe(ProbeError),
    UnsupportedProbeFormat,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to write command output: {err}"),
            Self::Json(err) => write!(formatter, "failed to encode JSON output: {err}"),
            Self::Probe(err) => write!(formatter, "{err}"),
            Self::UnsupportedProbeFormat => {
                formatter.write_str("probe requires an output format; use `--json`")
            }
        }
    }
}

impl std::error::Error for CliError {}

impl From<io::Error> for CliError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<ProbeError> for CliError {
    fn from(err: ProbeError) -> Self {
        Self::Probe(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{run, CliError};
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn probe_without_format_returns_clear_error() {
        let cli = Cli::try_parse_from(["dasobjectstore", "probe"]).expect("probe parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("format is required");

        assert!(matches!(err, CliError::UnsupportedProbeFormat));
    }
}
