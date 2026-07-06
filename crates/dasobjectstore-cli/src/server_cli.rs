use clap::Parser;
use dasobjectstore_core::{
    DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_HTTPS_PORT,
};
use dasobjectstore_gui_api::{
    StandaloneServerConfig, StandaloneTlsConfig, DEFAULT_STANDALONE_PUBLIC_BASE_URL,
};
use std::path::PathBuf;

/// Standalone HTTPS server for the DASObjectStore Web UI and API.
#[derive(Debug, Parser)]
#[command(name = "dasobjectstore-server", version = dasobjectstore_core::VERSION)]
pub(crate) struct ServerCli {
    /// Host address to bind for standalone mode.
    #[arg(long, default_value = DEFAULT_STANDALONE_BIND_ADDRESS)]
    bind_address: String,
    /// HTTPS port for standalone mode.
    #[arg(long, default_value_t = DEFAULT_STANDALONE_HTTPS_PORT)]
    https_port: u16,
    /// Public HTTPS base URL advertised by standalone mode.
    #[arg(long, default_value = DEFAULT_STANDALONE_PUBLIC_BASE_URL)]
    public_base_url: String,
    /// Product state root for local standalone mode.
    #[arg(long, default_value = DEFAULT_PRODUCT_ROOT)]
    product_root: PathBuf,
    /// TLS certificate path.
    #[arg(long)]
    tls_certificate_path: Option<PathBuf>,
    /// TLS private key path.
    #[arg(long)]
    tls_private_key_path: Option<PathBuf>,
    /// Validate and print the resolved server configuration without starting.
    #[arg(long)]
    check_config: bool,
    /// Emit configuration output as JSON.
    #[arg(long)]
    json: bool,
}

impl ServerCli {
    pub(crate) fn server_config(&self) -> StandaloneServerConfig {
        let product_root = self.product_root.clone();
        let default_tls = StandaloneTlsConfig::under_product_root(&product_root);
        let tls = StandaloneTlsConfig {
            certificate_path: self
                .tls_certificate_path
                .clone()
                .unwrap_or(default_tls.certificate_path),
            private_key_path: self
                .tls_private_key_path
                .clone()
                .unwrap_or(default_tls.private_key_path),
        };

        StandaloneServerConfig {
            bind_address: self.bind_address.clone(),
            https_port: self.https_port,
            public_base_url: self.public_base_url.clone(),
            product_root,
            tls,
        }
    }

    pub(crate) fn check_config(&self) -> bool {
        self.check_config
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
        let config = cli.server_config();

        assert!(cli.check_config());
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
        let config = cli.server_config();

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
        let config = cli.server_config();

        assert_eq!(
            config.tls.certificate_path,
            Path::new("/tmp/dasobjectstore/server.crt")
        );
        assert_eq!(
            config.tls.private_key_path,
            Path::new("/tmp/dasobjectstore/server.key")
        );
    }
}
