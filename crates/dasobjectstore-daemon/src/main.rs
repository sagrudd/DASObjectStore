use dasobjectstore_daemon::{DaemonRuntimeConfig, DEFAULT_DAEMON_CONFIG_PATH};
use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = DaemonArgs::parse(env::args().skip(1))?;
    if args.help {
        print_help();
        return Ok(());
    }

    let config = read_config(&args.config_path)?;
    config.validate().map_err(|err| err.to_string())?;

    if args.check_config {
        println!("Daemon config is valid: {}", args.config_path.display());
        return Ok(());
    }

    Err("dasobjectstored runtime loop is not implemented yet; use --check-config for package validation".to_string())
}

fn read_config(path: &PathBuf) -> Result<DaemonRuntimeConfig, String> {
    let file = File::open(path)
        .map_err(|err| format!("failed to open daemon config {}: {err}", path.display()))?;
    serde_json::from_reader(file)
        .map_err(|err| format!("failed to parse daemon config {}: {err}", path.display()))
}

fn print_help() {
    println!("Usage: dasobjectstored [--config <PATH>] [--check-config]");
}

#[derive(Debug)]
struct DaemonArgs {
    config_path: PathBuf,
    check_config: bool,
    help: bool,
}

impl DaemonArgs {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut config_path = PathBuf::from(DEFAULT_DAEMON_CONFIG_PATH);
        let mut check_config = false;
        let mut help = false;
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--config requires a path".to_string())?;
                    config_path = PathBuf::from(value);
                }
                "--check-config" => check_config = true,
                "-h" | "--help" => help = true,
                value => return Err(format!("unsupported dasobjectstored argument: {value}")),
            }
        }

        Ok(Self {
            config_path,
            check_config,
            help,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonArgs;
    use std::path::PathBuf;

    #[test]
    fn parses_config_and_check_flag() {
        let args = DaemonArgs::parse([
            "--config".to_string(),
            "/etc/dasobjectstore/daemon.json".to_string(),
            "--check-config".to_string(),
        ])
        .expect("args parse");

        assert_eq!(
            args.config_path,
            PathBuf::from("/etc/dasobjectstore/daemon.json")
        );
        assert!(args.check_config);
    }

    #[test]
    fn rejects_missing_config_path() {
        let err = DaemonArgs::parse(["--config".to_string()]).expect_err("missing path rejected");

        assert_eq!(err, "--config requires a path");
    }
}
