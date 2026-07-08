use clap::Parser;
use dasobjectstore_core::DEFAULT_STANDALONE_CONFIG_PATH;
use dasobjectstore_gui_api::{StandaloneServerConfig, StandaloneTlsConfig};
use std::path::PathBuf;

/// Standalone HTTPS server for the DASObjectStore Web UI and API.
#[derive(Debug, Parser)]
#[command(name = "dasobjectstore-server", version = dasobjectstore_core::VERSION)]
pub(crate) struct ServerCli {
    /// JSON configuration file for standalone mode.
    #[arg(long, default_value = DEFAULT_STANDALONE_CONFIG_PATH)]
    config: PathBuf,
    /// Host address to bind for standalone mode; overrides config.
    #[arg(long)]
    bind_address: Option<String>,
    /// HTTPS port for standalone mode; overrides config.
    #[arg(long)]
    https_port: Option<u16>,
    /// Public HTTPS base URL advertised by standalone mode; overrides config.
    #[arg(long)]
    public_base_url: Option<String>,
    /// Product state root for local standalone mode; overrides config.
    #[arg(long)]
    product_root: Option<PathBuf>,
    /// TLS certificate path.
    #[arg(long)]
    tls_certificate_path: Option<PathBuf>,
    /// TLS private key path.
    #[arg(long)]
    tls_private_key_path: Option<PathBuf>,
    /// Validate and print the resolved server configuration without starting.
    #[arg(long)]
    check_config: bool,
    /// Create self-signed TLS assets when both certificate and key are missing.
    #[arg(long)]
    generate_missing_tls: bool,
    /// Emit configuration output as JSON.
    #[arg(long)]
    json: bool,
}

impl ServerCli {
    pub(crate) fn server_config(&self) -> Result<StandaloneServerConfig, std::io::Error> {
        let mut config = match std::fs::read_to_string(&self.config) {
            Ok(contents) => serde_json::from_str::<StandaloneServerConfig>(&contents)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                StandaloneServerConfig::default()
            }
            Err(err) => return Err(err),
        };

        if let Some(bind_address) = &self.bind_address {
            config.bind_address = bind_address.clone();
        }
        if let Some(https_port) = self.https_port {
            config.https_port = https_port;
        }
        if let Some(public_base_url) = &self.public_base_url {
            config.public_base_url = public_base_url.clone();
        }
        if let Some(product_root) = &self.product_root {
            config.product_root = product_root.clone();
        }

        let product_root = config.product_root.clone();
        let default_tls = StandaloneTlsConfig::under_product_root(&product_root);
        if let Some(certificate_path) = &self.tls_certificate_path {
            config.tls.certificate_path = certificate_path.clone();
        } else if self.product_root.is_some() {
            config.tls.certificate_path = default_tls.certificate_path;
        }
        if let Some(private_key_path) = &self.tls_private_key_path {
            config.tls.private_key_path = private_key_path.clone();
        } else if self.product_root.is_some() {
            config.tls.private_key_path = default_tls.private_key_path;
        }

        Ok(config)
    }

    pub(crate) fn check_config(&self) -> bool {
        self.check_config
    }

    pub(crate) fn generate_missing_tls(&self) -> bool {
        self.generate_missing_tls
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[cfg(test)]
mod tests {
    use super::ServerCli;
    use clap::Parser;
    use dasobjectstore_core::{
        DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_HTTPS_PORT,
    };
    use std::path::Path;

    #[test]
    fn parses_default_check_config() {
        let cli = ServerCli::try_parse_from(["dasobjectstore-server", "--check-config"])
            .expect("server CLI parses");
        let config = cli.server_config().expect("default config loads");

        assert!(cli.check_config());
        assert!(!cli.generate_missing_tls());
        assert_eq!(config.bind_address, DEFAULT_STANDALONE_BIND_ADDRESS);
        assert_eq!(config.https_port, DEFAULT_STANDALONE_HTTPS_PORT);
        assert_eq!(config.product_root, Path::new(DEFAULT_PRODUCT_ROOT));
        config.validate().expect("default server config is valid");
    }

    #[test]
    fn parses_linux_appliance_bind_address() {
        let cli = ServerCli::try_parse_from([
            "dasobjectstore-server",
            "--check-config",
            "--bind-address",
            "0.0.0.0",
        ])
        .expect("server CLI parses");
        let config = cli.server_config().expect("default config loads");

        assert_eq!(
            config.socket_addr().expect("socket address"),
            "0.0.0.0:8448".parse().expect("expected socket address")
        );
    }

    #[test]
    fn parses_custom_tls_paths() {
        let cli = ServerCli::try_parse_from([
            "dasobjectstore-server",
            "--check-config",
            "--tls-certificate-path",
            "/tmp/dasobjectstore/server.crt",
            "--tls-private-key-path",
            "/tmp/dasobjectstore/server.key",
        ])
        .expect("server CLI parses");
        let config = cli.server_config().expect("default config loads");

        assert_eq!(
            config.tls.certificate_path,
            Path::new("/tmp/dasobjectstore/server.crt")
        );
        assert_eq!(
            config.tls.private_key_path,
            Path::new("/tmp/dasobjectstore/server.key")
        );
    }

    #[test]
    fn parses_generate_missing_tls_flag() {
        let cli = ServerCli::try_parse_from([
            "dasobjectstore-server",
            "--check-config",
            "--generate-missing-tls",
        ])
        .expect("server CLI parses");

        assert!(cli.generate_missing_tls());
    }
}
