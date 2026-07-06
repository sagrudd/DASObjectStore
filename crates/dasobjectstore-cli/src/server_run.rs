use crate::server_cli::ServerCli;
use dasobjectstore_gui_api::{StandaloneServerConfig, StandaloneServerConfigError};
use std::fmt::{self, Display};
use std::io::{self, Write};

pub(crate) fn run(cli: &ServerCli, writer: &mut impl Write) -> Result<(), ServerRunError> {
    let config = cli.server_config();
    config.validate()?;

    if !cli.check_config() {
        return Err(ServerRunError::StartupRequiresTlsAssetHandling);
    }

    if cli.json() {
        serde_json::to_writer_pretty(&mut *writer, &config)?;
        writer.write_all(b"\n")?;
    } else {
        write_pretty_config(&config, writer)?;
    }

    Ok(())
}

#[derive(Debug)]
pub(crate) enum ServerRunError {
    Config(StandaloneServerConfigError),
    Io(io::Error),
    Json(serde_json::Error),
    StartupRequiresTlsAssetHandling,
}

impl Display for ServerRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(formatter, "{err}"),
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run, ServerRunError};
    use crate::server_cli::ServerCli;
    use clap::Parser;

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
        assert_eq!(output["bind_address"], "127.0.0.1");
        assert_eq!(output["https_port"], 8448);
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
}
