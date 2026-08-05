//! Dedicated TLS listener for the DASObjectStore direct S3 data plane.
//!
//! This binary intentionally has no Web UI, browser session, user identity,
//! or credential-issuance surface.  It keeps the existing authenticated
//! SigV4 gateway on port 3900 available while the legacy standalone Web
//! listener is retired in favour of Monas-hosted, Pistis-verified routes.

use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use dasobjectstore_gui_api::s3_gateway_router;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

const DEFAULT_CONFIG_PATH: &str = "/etc/dasobjectstore/s3-gateway.json";
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 3900;
const DEFAULT_MAX_CONCURRENT_UPLOADS: usize = 32;

#[derive(Debug, Parser)]
#[command(name = "dasobjectstore-s3-gateway", version = dasobjectstore_core::VERSION)]
struct Cli {
    /// Explicit gateway configuration. A missing configuration is an error;
    /// this data-plane listener never manufactures TLS or authority state.
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config: PathBuf,
    /// Validate and print the resolved configuration without binding a port.
    #[arg(long)]
    check_config: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectS3GatewayConfig {
    #[serde(default = "default_bind_address")]
    bind_address: String,
    #[serde(default = "default_port")]
    port: u16,
    certificate_path: PathBuf,
    private_key_path: PathBuf,
    #[serde(default = "default_max_concurrent_uploads")]
    max_concurrent_uploads: usize,
}

impl DirectS3GatewayConfig {
    fn socket_addr(&self) -> Result<SocketAddr, String> {
        let address = self
            .bind_address
            .parse::<IpAddr>()
            .map_err(|_| format!("invalid S3 gateway bind address: {}", self.bind_address))?;
        if self.port == 0 {
            return Err("S3 gateway port must be non-zero".to_owned());
        }
        Ok(SocketAddr::new(address, self.port))
    }

    fn validate(&self) -> Result<(), String> {
        self.socket_addr()?;
        if !self.certificate_path.is_absolute() || !self.private_key_path.is_absolute() {
            return Err("S3 gateway TLS paths must be absolute".to_owned());
        }
        if self.max_concurrent_uploads == 0 || self.max_concurrent_uploads > 256 {
            return Err("S3 gateway max_concurrent_uploads must be in 1..=256".to_owned());
        }
        Ok(())
    }
}

fn default_bind_address() -> String {
    DEFAULT_BIND_ADDRESS.to_owned()
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_max_concurrent_uploads() -> usize {
    DEFAULT_MAX_CONCURRENT_UPLOADS
}

fn load_config(path: &PathBuf) -> Result<DirectS3GatewayConfig, String> {
    let contents = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "unable to read S3 gateway configuration {}: {error}",
            path.display()
        )
    })?;
    let config: DirectS3GatewayConfig = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "invalid S3 gateway configuration {}: {error}",
            path.display()
        )
    })?;
    config.validate()?;
    Ok(config)
}

#[tokio::main]
async fn main() -> ExitCode {
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        eprintln!("unable to install the DASObjectStore TLS crypto provider");
        return ExitCode::FAILURE;
    }
    let cli = Cli::parse();
    let config = match load_config(&cli.config) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if cli.check_config {
        match serde_json::to_string_pretty(&config) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("unable to serialize S3 gateway configuration: {error}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }
    let address = match config.socket_addr() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let tls = match RustlsConfig::from_pem_file(&config.certificate_path, &config.private_key_path)
        .await
    {
        Ok(tls) => tls,
        Err(error) => {
            eprintln!("unable to load S3 gateway TLS assets: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = axum_server::bind_rustls(address, tls)
        .serve(s3_gateway_router(config.max_concurrent_uploads).into_make_service())
        .await
    {
        eprintln!("S3 gateway failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::DirectS3GatewayConfig;
    use std::path::PathBuf;

    fn valid() -> DirectS3GatewayConfig {
        DirectS3GatewayConfig {
            bind_address: "127.0.0.1".to_owned(),
            port: 3900,
            certificate_path: PathBuf::from("/etc/dasobjectstore/tls/server.crt"),
            private_key_path: PathBuf::from("/etc/dasobjectstore/tls/server.key"),
            max_concurrent_uploads: 32,
        }
    }

    #[test]
    fn gateway_config_requires_explicit_absolute_tls_paths() {
        let mut config = valid();
        config.certificate_path = PathBuf::from("server.crt");
        assert!(config.validate().is_err());
    }

    #[test]
    fn gateway_config_rejects_invalid_concurrency() {
        let mut config = valid();
        config.max_concurrent_uploads = 0;
        assert!(config.validate().is_err());
    }
}
