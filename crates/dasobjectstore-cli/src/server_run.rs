use crate::server_cli::ServerCli;
use dasobjectstore_gui_api::{
    ensure_standalone_tls_assets, StandaloneServerConfig, StandaloneServerConfigError,
    StandaloneTlsAssetError, StandaloneTlsAssetReport,
};
use std::fmt::{self, Display};
use std::io::{self, Write};

pub(crate) fn run(cli: &ServerCli, writer: &mut impl Write) -> Result<(), ServerRunError> {
    let config = cli.server_config();
    config.validate()?;
    let tls_report = if cli.generate_missing_tls() {
        Some(ensure_standalone_tls_assets(&config)?)
    } else {
        None
    };

    if !cli.check_config() {
        return Err(ServerRunError::StartupRequiresTlsAssetHandling);
    }

    if cli.json() {
        write_json_config(&config, tls_report.as_ref(), writer)?;
        writer.write_all(b"\n")?;
    } else {
        write_pretty_config(&config, tls_report.as_ref(), writer)?;
    }

    Ok(())
}

#[derive(Debug)]
pub(crate) enum ServerRunError {
    Config(StandaloneServerConfigError),
    Tls(StandaloneTlsAssetError),
    Io(io::Error),
    Json(serde_json::Error),
    StartupRequiresTlsAssetHandling,
}

impl Display for ServerRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(formatter, "{err}"),
            Self::Tls(err) => write!(formatter, "{err}"),
            Self::Io(err) => write!(formatter, "server output failed: {err}"),
            Self::Json(err) => write!(formatter, "server JSON output failed: {err}"),
            Self::StartupRequiresTlsAssetHandling => write!(
                formatter,
                "server startup requires the TLS asset loading milestone; use --check-config to validate the entry point"
            ),
        }
    }
}

impl std::error::Error for ServerRunError {}

impl From<StandaloneServerConfigError> for ServerRunError {
    fn from(err: StandaloneServerConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<StandaloneTlsAssetError> for ServerRunError {
    fn from(err: StandaloneTlsAssetError) -> Self {
        Self::Tls(err)
    }
}

impl From<io::Error> for ServerRunError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for ServerRunError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

fn write_pretty_config(
    config: &StandaloneServerConfig,
    tls_report: Option<&StandaloneTlsAssetReport>,
    writer: &mut impl Write,
) -> Result<(), ServerRunError> {
    writeln!(writer, "DASObjectStore standalone server configuration OK")?;
    writeln!(writer, "bind: {}", config.socket_addr()?)?;
    writeln!(writer, "public_base_url: {}", config.public_base_url)?;
    writeln!(writer, "product_root: {}", config.product_root.display())?;
    writeln!(
        writer,
        "tls_certificate_path: {}",
        config.tls.certificate_path.display()
    )?;
    writeln!(
        writer,
        "tls_private_key_path: {}",
        config.tls.private_key_path.display()
    )?;
    if let Some(tls_report) = tls_report {
        writeln!(writer, "tls_generated: {}", tls_report.generated)?;
    }
    Ok(())
}

fn write_json_config(
    config: &StandaloneServerConfig,
    tls_report: Option<&StandaloneTlsAssetReport>,
    writer: &mut impl Write,
) -> Result<(), ServerRunError> {
    serde_json::to_writer_pretty(
        &mut *writer,
        &serde_json::json!({
            "server": config,
            "tls_assets": tls_report,
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run, ServerRunError};
    use crate::server_cli::ServerCli;
    use clap::Parser;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn emits_pretty_check_config() {
        let cli = ServerCli::try_parse_from(["dasobjectstore-server", "--check-config"])
            .expect("server CLI parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("check config runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("DASObjectStore standalone server configuration OK"));
        assert!(output.contains("bind: 127.0.0.1:8448"));
    }

    #[test]
    fn emits_json_check_config() {
        let cli = ServerCli::try_parse_from(["dasobjectstore-server", "--check-config", "--json"])
            .expect("server CLI parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("check config runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("server config JSON parses");
        assert_eq!(output["server"]["bind_address"], "127.0.0.1");
        assert_eq!(output["server"]["https_port"], 8448);
        assert_eq!(output["tls_assets"], serde_json::Value::Null);
    }

    #[test]
    fn generates_missing_tls_assets_when_requested() {
        let root = temp_root("server-run-generate");
        let cli = ServerCli::try_parse_from([
            "dasobjectstore-server",
            "--check-config",
            "--generate-missing-tls",
            "--product-root",
            root.to_str().expect("root path"),
        ])
        .expect("server CLI parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("check config runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("tls_generated: true"));
        assert!(root.join("tls/server.crt").exists());
        assert!(root.join("tls/server.key").exists());

        cleanup(&root);
    }

    #[test]
    fn refuses_startup_until_tls_asset_handling_lands() {
        let cli = ServerCli::try_parse_from(["dasobjectstore-server"]).expect("server CLI parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("startup is blocked");

        assert!(matches!(
            err,
            ServerRunError::StartupRequiresTlsAssetHandling
        ));
        assert!(output.is_empty());
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-server-run-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
