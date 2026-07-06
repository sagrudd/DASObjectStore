use crate::{StandaloneServerConfig, StandaloneServerConfigError, StandaloneTlsConfig};
use rcgen::generate_simple_self_signed;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandaloneTlsAssets {
    pub certificate_pem: String,
    pub private_key_pem: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneTlsAssetReport {
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
    pub generated: bool,
}

pub fn load_standalone_tls_assets(
    tls: &StandaloneTlsConfig,
) -> Result<StandaloneTlsAssets, StandaloneTlsAssetError> {
    tls.validate()?;

    Ok(StandaloneTlsAssets {
        certificate_pem: read_non_empty_pem("certificate_path", &tls.certificate_path)?,
        private_key_pem: read_non_empty_pem("private_key_path", &tls.private_key_path)?,
    })
}

pub fn ensure_standalone_tls_assets(
    config: &StandaloneServerConfig,
) -> Result<StandaloneTlsAssetReport, StandaloneTlsAssetError> {
    config.validate()?;

    let cert_exists = config.tls.certificate_path.exists();
    let key_exists = config.tls.private_key_path.exists();
    match (cert_exists, key_exists) {
        (true, true) => {
            load_standalone_tls_assets(&config.tls)?;
            Ok(report(&config.tls, false))
        }
        (false, false) => {
            generate_standalone_tls_assets(config)?;
            load_standalone_tls_assets(&config.tls)?;
            Ok(report(&config.tls, true))
        }
        _ => Err(StandaloneTlsAssetError::PartialTlsAssetSet {
            certificate_path: config.tls.certificate_path.clone(),
            private_key_path: config.tls.private_key_path.clone(),
        }),
    }
}

#[derive(Debug)]
pub enum StandaloneTlsAssetError {
    Config(StandaloneServerConfigError),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    EmptyPem {
        field: &'static str,
        path: PathBuf,
    },
    PartialTlsAssetSet {
        certificate_path: PathBuf,
        private_key_path: PathBuf,
    },
    Generation(rcgen::Error),
}

impl Display for StandaloneTlsAssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(formatter, "{err}"),
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "TLS asset IO failed at {}: {source}",
                    path.display()
                )
            }
            Self::EmptyPem { field, path } => {
                write!(formatter, "{field} is empty at {}", path.display())
            }
            Self::PartialTlsAssetSet {
                certificate_path,
                private_key_path,
            } => write!(
                formatter,
                "TLS assets must exist as a complete certificate/key pair: {} and {}",
                certificate_path.display(),
                private_key_path.display()
            ),
            Self::Generation(err) => write!(formatter, "TLS asset generation failed: {err}"),
        }
    }
}

impl std::error::Error for StandaloneTlsAssetError {}

impl From<StandaloneServerConfigError> for StandaloneTlsAssetError {
    fn from(err: StandaloneServerConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<rcgen::Error> for StandaloneTlsAssetError {
    fn from(err: rcgen::Error) -> Self {
        Self::Generation(err)
    }
}

fn generate_standalone_tls_assets(
    config: &StandaloneServerConfig,
) -> Result<(), StandaloneTlsAssetError> {
    create_parent_dir(&config.tls.certificate_path)?;
    create_parent_dir(&config.tls.private_key_path)?;

    let certified_key = generate_simple_self_signed(subject_alt_names(config))?;
    write_new_file(
        &config.tls.private_key_path,
        certified_key.signing_key.serialize_pem().as_bytes(),
        0o600,
    )?;
    write_new_file(
        &config.tls.certificate_path,
        certified_key.cert.pem().as_bytes(),
        0o644,
    )?;

    Ok(())
}

fn read_non_empty_pem(field: &'static str, path: &Path) -> Result<String, StandaloneTlsAssetError> {
    let pem = fs::read_to_string(path).map_err(|source| StandaloneTlsAssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if pem.trim().is_empty() {
        return Err(StandaloneTlsAssetError::EmptyPem {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(pem)
}

fn create_parent_dir(path: &Path) -> Result<(), StandaloneTlsAssetError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StandaloneTlsAssetError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), StandaloneTlsAssetError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| StandaloneTlsAssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes)
        .map_err(|source| StandaloneTlsAssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    set_file_permissions(path, mode)?;
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path, mode: u32) -> Result<(), StandaloneTlsAssetError> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| {
        StandaloneTlsAssetError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path, _mode: u32) -> Result<(), StandaloneTlsAssetError> {
    Ok(())
}

fn subject_alt_names(config: &StandaloneServerConfig) -> Vec<String> {
    let mut names = Vec::new();
    push_unique(&mut names, "localhost");
    push_unique(&mut names, &config.bind_address);
    if let Some(host) = https_host(&config.public_base_url) {
        push_unique(&mut names, host);
    }
    names
}

fn https_host(public_base_url: &str) -> Option<&str> {
    let rest = public_base_url.strip_prefix("https://")?;
    let authority = rest.split('/').next().unwrap_or(rest);
    authority.split(':').next().filter(|host| !host.is_empty())
}

fn push_unique(names: &mut Vec<String>, value: &str) {
    if !names.iter().any(|name| name == value) {
        names.push(value.to_string());
    }
}

fn report(tls: &StandaloneTlsConfig, generated: bool) -> StandaloneTlsAssetReport {
    StandaloneTlsAssetReport {
        certificate_path: tls.certificate_path.clone(),
        private_key_path: tls.private_key_path.clone(),
        generated,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_standalone_tls_assets, load_standalone_tls_assets, StandaloneTlsAssetError,
    };
    use crate::StandaloneServerConfig;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn generates_and_loads_missing_tls_assets() {
        let root = temp_root("generate-load");
        let config = config_under(&root);

        let report =
            ensure_standalone_tls_assets(&config).expect("missing TLS assets are generated");
        let assets = load_standalone_tls_assets(&config.tls).expect("generated assets load");

        assert!(report.generated);
        assert!(report.certificate_path.exists());
        assert!(report.private_key_path.exists());
        assert!(assets.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(assets.private_key_pem.contains("BEGIN"));

        cleanup(&root);
    }

    #[test]
    fn loads_existing_tls_assets_without_regenerating() {
        let root = temp_root("load-existing");
        let config = config_under(&root);
        ensure_standalone_tls_assets(&config).expect("initial TLS assets generated");

        let report = ensure_standalone_tls_assets(&config).expect("existing assets load");

        assert!(!report.generated);

        cleanup(&root);
    }

    #[test]
    fn rejects_partial_tls_asset_set() {
        let root = temp_root("partial");
        let config = config_under(&root);
        fs::create_dir_all(config.tls.certificate_path.parent().expect("cert parent"))
            .expect("cert parent created");
        fs::write(&config.tls.certificate_path, "certificate").expect("cert written");

        let err = ensure_standalone_tls_assets(&config).expect_err("partial set rejected");

        assert!(matches!(
            err,
            StandaloneTlsAssetError::PartialTlsAssetSet { .. }
        ));

        cleanup(&root);
    }

    fn config_under(root: &Path) -> StandaloneServerConfig {
        StandaloneServerConfig {
            product_root: root.to_path_buf(),
            tls: crate::StandaloneTlsConfig::under_product_root(root),
            ..StandaloneServerConfig::default()
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-tls-assets-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
