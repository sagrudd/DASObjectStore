//! Garage object-service provider integration.

use crate::compose::{render_store_binding, validate_render_request};
use crate::provider::{
    ComposeRenderRequest, ObjectServiceError, ObjectServiceProvider, ObjectServiceProviderId,
    ProviderDescriptor, RenderedCompose, ServiceState, ServiceStatus,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::str::FromStr;

pub const DEFAULT_GARAGE_IMAGE: &str = "dxflrs/garage:v2.3.0";
pub const DEFAULT_GARAGE_SERVICE_NAME: &str = "garage";
pub const DEFAULT_GARAGE_API_PORT: u16 = 3900;
pub const DEFAULT_GARAGE_CONFIG_PATH: &str = "/etc/dasobjectstore/garage.toml";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageDataDirectory {
    pub host_path: String,
    pub container_path: String,
    pub capacity: Option<String>,
    pub read_only: bool,
}

impl GarageDataDirectory {
    pub fn writable(
        host_path: impl Into<String>,
        container_path: impl Into<String>,
        capacity: impl Into<String>,
    ) -> Self {
        Self {
            host_path: host_path.into(),
            container_path: container_path.into(),
            capacity: Some(capacity.into()),
            read_only: false,
        }
    }

    pub fn read_only(host_path: impl Into<String>, container_path: impl Into<String>) -> Self {
        Self {
            host_path: host_path.into(),
            container_path: container_path.into(),
            capacity: None,
            read_only: true,
        }
    }
}

impl FromStr for GarageDataDirectory {
    type Err = ObjectServiceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (paths, mode) = value.rsplit_once('=').ok_or_else(|| {
            ObjectServiceError::InvalidConfiguration(format!(
                "invalid Garage data directory `{value}`; expected HOST_PATH=CONTAINER_PATH=CAPACITY|read-only"
            ))
        })?;
        let (host_path, container_path) = paths.split_once('=').ok_or_else(|| {
            ObjectServiceError::InvalidConfiguration(format!(
                "invalid Garage data directory `{value}`; expected HOST_PATH=CONTAINER_PATH=CAPACITY|read-only"
            ))
        })?;
        let directory = if mode == "read-only" {
            Self::read_only(host_path, container_path)
        } else {
            Self::writable(host_path, container_path, mode)
        };
        validate_data_directory_entry(&directory)?;
        Ok(directory)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageProviderConfig {
    pub service_name: String,
    pub image: String,
    pub bind_address: String,
    pub api_port: u16,
    /// Optional host-side API port. The container keeps `api_port`, allowing
    /// the direct ingress gateway to own public 3900 while Garage remains on
    /// its retained internal 3900 listener and is published on loopback 3901.
    pub published_api_port: Option<u16>,
    pub rpc_port: u16,
    pub web_port: u16,
    pub admin_port: u16,
    pub config_path: String,
    pub replication_factor: u8,
    pub rpc_secret: Option<String>,
    pub admin_token: Option<String>,
    pub metrics_token: Option<String>,
    /// Explicit Garage 2.3 data directories. An empty list preserves the
    /// legacy single path supplied by `ComposeRenderRequest::hdd_data_path`.
    pub data_directories: Vec<GarageDataDirectory>,
}

impl Default for GarageProviderConfig {
    fn default() -> Self {
        Self {
            service_name: DEFAULT_GARAGE_SERVICE_NAME.to_string(),
            image: DEFAULT_GARAGE_IMAGE.to_string(),
            bind_address: "127.0.0.1".to_string(),
            api_port: DEFAULT_GARAGE_API_PORT,
            published_api_port: None,
            rpc_port: DEFAULT_GARAGE_API_PORT + 1,
            web_port: DEFAULT_GARAGE_API_PORT + 2,
            admin_port: DEFAULT_GARAGE_API_PORT + 3,
            config_path: DEFAULT_GARAGE_CONFIG_PATH.to_string(),
            replication_factor: 1,
            rpc_secret: None,
            admin_token: None,
            metrics_token: None,
            data_directories: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GarageProvider {
    descriptor: ProviderDescriptor,
    config: GarageProviderConfig,
}

impl Default for GarageProvider {
    fn default() -> Self {
        Self::new(GarageProviderConfig::default())
    }
}

impl GarageProvider {
    pub fn new(config: GarageProviderConfig) -> Self {
        Self {
            descriptor: ProviderDescriptor::garage(),
            config,
        }
    }

    pub fn config(&self) -> &GarageProviderConfig {
        &self.config
    }

    pub fn render_garage_config(&self) -> Result<String, ObjectServiceError> {
        validate_config(&self.config)?;
        let secrets = validate_config_secrets(&self.config)?;
        let data_directories = if self.config.data_directories.is_empty() {
            "data_dir = \"/var/lib/garage/data\"".to_string()
        } else {
            render_garage_data_directories(&self.config.data_directories)?
        };

        Ok(format!(
            r#"metadata_dir = "/var/lib/garage/meta"
{data_directories}
db_engine = "sqlite"
replication_factor = {replication_factor}
compression_level = 0
block_size = "10M"

rpc_bind_addr = "[::]:{rpc_port}"
rpc_public_addr = "127.0.0.1:{rpc_port}"
rpc_secret = "{rpc_secret}"

[s3_api]
s3_region = "garage"
api_bind_addr = "[::]:{api_port}"

[s3_web]
bind_addr = "[::]:{web_port}"
root_domain = ".web.garage.localhost"
index = "index.html"

[admin]
api_bind_addr = "[::]:{admin_port}"
admin_token = "{admin_token}"
metrics_token = "{metrics_token}"
"#,
            replication_factor = self.config.replication_factor,
            rpc_port = self.config.rpc_port,
            api_port = self.config.api_port,
            web_port = self.config.web_port,
            admin_port = self.config.admin_port,
            rpc_secret = secrets.rpc_secret,
            admin_token = secrets.admin_token,
            metrics_token = secrets.metrics_token,
            data_directories = data_directories,
        ))
    }
}

pub fn render_garage_data_directories(
    directories: &[GarageDataDirectory],
) -> Result<String, ObjectServiceError> {
    validate_data_directories(directories)?;
    let mut rendered = String::from("data_dir = [\n");
    for directory in directories {
        if directory.read_only {
            rendered.push_str(&format!(
                "    {{ path = \"{}\", read_only = true }},\n",
                escape_toml_string(&directory.container_path)
            ));
        } else {
            rendered.push_str(&format!(
                "    {{ path = \"{}\", capacity = \"{}\" }},\n",
                escape_toml_string(&directory.container_path),
                escape_toml_string(directory.capacity.as_deref().unwrap_or_default())
            ));
        }
    }
    rendered.push(']');
    Ok(rendered)
}

impl ObjectServiceProvider for GarageProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn render_compose(
        &self,
        request: &ComposeRenderRequest,
    ) -> Result<RenderedCompose, ObjectServiceError> {
        validate_render_request(request)?;
        validate_config(&self.config)?;

        let mut yaml = String::new();
        yaml.push_str(&format!("name: {}\n", request.project_name));
        yaml.push_str("services:\n");
        yaml.push_str(&format!("  {}:\n", self.config.service_name));
        yaml.push_str(&format!("    image: {}\n", self.config.image));
        yaml.push_str("    init: true\n");
        yaml.push_str("    restart: \"no\"\n");
        yaml.push_str("    stop_grace_period: 30s\n");
        yaml.push_str("    ports:\n");
        let published_api_port = self
            .config
            .published_api_port
            .unwrap_or(self.config.api_port);
        yaml.push_str(&render_port_mapping(
            &self.config.bind_address,
            published_api_port,
            self.config.api_port,
        ));
        yaml.push_str(&render_port_mapping(
            &self.config.bind_address,
            published_api_port + 1,
            self.config.rpc_port,
        ));
        yaml.push_str(&render_port_mapping(
            &self.config.bind_address,
            published_api_port + 2,
            self.config.web_port,
        ));
        yaml.push_str(&render_port_mapping(
            &self.config.bind_address,
            published_api_port + 3,
            self.config.admin_port,
        ));
        yaml.push_str("    volumes:\n");
        yaml.push_str(&format!(
            "      - \"{}:/etc/garage.toml:ro\"\n",
            escape_yaml_string(&self.config.config_path)
        ));
        yaml.push_str(&format!(
            "      - \"{}:/var/lib/garage/meta\"\n",
            escape_yaml_string(&request.ssd_metadata_path)
        ));
        if self.config.data_directories.is_empty() {
            yaml.push_str(&format!(
                "      - \"{}:/var/lib/garage/data\"\n",
                escape_yaml_string(&request.hdd_data_path)
            ));
        } else {
            for directory in &self.config.data_directories {
                yaml.push_str(&format!(
                    "      - \"{}:{}{}\"\n",
                    escape_yaml_string(&directory.host_path),
                    escape_yaml_string(&directory.container_path),
                    if directory.read_only { ":ro" } else { "" }
                ));
            }
        }
        yaml.push_str("    healthcheck:\n");
        yaml.push_str("      test: [\"CMD\", \"/garage\", \"status\"]\n");
        yaml.push_str("      interval: 10s\n");
        yaml.push_str("      timeout: 5s\n");
        yaml.push_str("      retries: 12\n");
        yaml.push_str("      start_period: 20s\n");
        yaml.push_str("    environment:\n");
        yaml.push_str("      DASOBJECTSTORE_PROVIDER: garage\n");
        yaml.push_str("    command: [\"/garage\", \"server\", \"--single-node\"]\n");
        yaml.push_str("x-dasobjectstore:\n");
        yaml.push_str("  provider: garage\n");
        yaml.push_str(&format!("  config_path: {}\n", self.config.config_path));
        yaml.push_str("  bucket_provisioning: live-garage-admin\n");
        yaml.push_str("  stores:\n");
        for binding in &request.store_bindings {
            yaml.push_str(&render_store_binding(binding));
        }

        Ok(RenderedCompose {
            provider_id: ObjectServiceProviderId::Garage,
            compose_yaml: yaml,
        })
    }

    fn inspect_status(&self) -> Result<ServiceStatus, ObjectServiceError> {
        Ok(ServiceStatus {
            provider_id: ObjectServiceProviderId::Garage,
            state: ServiceState::Unknown,
            endpoint: Some(format!(
                "http://{}:{}",
                self.config.bind_address, self.config.api_port
            )),
            message: Some(
                "Garage runtime status inspection is not wired to Docker Compose yet".to_string(),
            ),
        })
    }
}

fn render_port_mapping(bind_address: &str, host_port: u16, container_port: u16) -> String {
    format!(
        "      - \"{}:{}:{}\"\n",
        escape_yaml_string(bind_address),
        host_port,
        container_port
    )
}

fn escape_yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn validate_config(config: &GarageProviderConfig) -> Result<(), ObjectServiceError> {
    reject_blank("service_name", &config.service_name)?;
    reject_blank("image", &config.image)?;
    reject_blank("bind_address", &config.bind_address)?;
    reject_blank("config_path", &config.config_path)?;

    if config.api_port == 0
        || config.rpc_port == 0
        || config.web_port == 0
        || config.admin_port == 0
    {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage ports must be greater than zero".to_string(),
        ));
    }
    if config
        .published_api_port
        .is_some_and(|port| port == 0 || port > u16::MAX - 3)
    {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage published API port must leave room for four listener mappings".to_string(),
        ));
    }
    if config.replication_factor == 0 {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage replication_factor must be greater than zero".to_string(),
        ));
    }
    if !config.data_directories.is_empty() {
        validate_data_directories(&config.data_directories)?;
    }

    Ok(())
}

fn validate_data_directories(
    directories: &[GarageDataDirectory],
) -> Result<(), ObjectServiceError> {
    if directories.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage data directory list must not be empty".to_string(),
        ));
    }
    let mut host_paths = BTreeSet::new();
    let mut container_paths = BTreeSet::new();
    let mut writable = 0usize;
    for directory in directories {
        validate_data_directory_entry(directory)?;
        if !host_paths.insert(directory.host_path.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate Garage host data directory: {}",
                directory.host_path
            )));
        }
        if !container_paths.insert(directory.container_path.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate Garage container data directory: {}",
                directory.container_path
            )));
        }
        if !directory.read_only {
            writable += 1;
        }
    }
    if writable == 0 {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage data directory list requires at least one writable directory".to_string(),
        ));
    }
    Ok(())
}

fn validate_data_directory_entry(
    directory: &GarageDataDirectory,
) -> Result<(), ObjectServiceError> {
    reject_blank("data directory host_path", &directory.host_path)?;
    reject_blank("data directory container_path", &directory.container_path)?;
    if !Path::new(&directory.host_path).is_absolute()
        || !Path::new(&directory.container_path).is_absolute()
    {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage data directory host and container paths must be absolute".to_string(),
        ));
    }
    if directory.host_path.contains('\n')
        || directory.host_path.contains('\r')
        || directory.container_path.contains('\n')
        || directory.container_path.contains('\r')
    {
        return Err(ObjectServiceError::InvalidConfiguration(
            "Garage data directory paths must not contain newlines".to_string(),
        ));
    }
    match (directory.read_only, directory.capacity.as_deref()) {
        (true, None) => Ok(()),
        (true, Some(_)) => Err(ObjectServiceError::InvalidConfiguration(
            "read-only Garage data directories must not declare capacity".to_string(),
        )),
        (false, Some(capacity)) if valid_capacity(capacity) => Ok(()),
        (false, Some(_)) => Err(ObjectServiceError::InvalidConfiguration(
            "Garage writable data directory capacity must use digits with an optional decimal point and storage suffix"
                .to_string(),
        )),
        (false, None) => Err(ObjectServiceError::InvalidConfiguration(
            "Garage writable data directories require capacity".to_string(),
        )),
    }
}

fn valid_capacity(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.' || byte.is_ascii_alphabetic())
}

struct GarageConfigSecrets<'a> {
    rpc_secret: &'a str,
    admin_token: &'a str,
    metrics_token: &'a str,
}

fn validate_config_secrets(
    config: &GarageProviderConfig,
) -> Result<GarageConfigSecrets<'_>, ObjectServiceError> {
    Ok(GarageConfigSecrets {
        rpc_secret: require_secret("rpc_secret", &config.rpc_secret)?,
        admin_token: require_secret("admin_token", &config.admin_token)?,
        metrics_token: require_secret("metrics_token", &config.metrics_token)?,
    })
}

fn require_secret<'a>(
    field: &str,
    value: &'a Option<String>,
) -> Result<&'a str, ObjectServiceError> {
    let value = value.as_deref().ok_or_else(|| {
        ObjectServiceError::InvalidConfiguration(format!(
            "Garage {field} must be generated before rendering garage.toml"
        ))
    })?;
    reject_blank(field, value)?;
    Ok(value)
}

fn reject_blank(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.trim().is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "Garage {field} must not be blank"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GarageDataDirectory, GarageProvider, GarageProviderConfig, DEFAULT_GARAGE_IMAGE};
    use crate::provider::StoreBucketBinding;
    use crate::provider::{ComposeRenderRequest, ObjectServiceProvider};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};

    #[test]
    fn default_descriptor_selects_garage() {
        let provider = GarageProvider::default();

        assert_eq!(provider.descriptor().display_name, "Garage");
        assert_eq!(provider.config().image, DEFAULT_GARAGE_IMAGE);
    }

    #[test]
    fn renders_garage_specific_compose() {
        let provider = GarageProvider::default();
        let rendered = provider
            .render_compose(&request())
            .expect("Garage compose renders");

        assert_eq!(rendered.provider_id.name(), "garage");
        assert!(rendered
            .compose_yaml
            .contains("image: dxflrs/garage:v2.3.0"));
        assert!(rendered
            .compose_yaml
            .contains("\"/etc/dasobjectstore/garage.toml:/etc/garage.toml:ro\""));
        assert!(rendered.compose_yaml.contains("/var/lib/garage/meta"));
        assert!(rendered.compose_yaml.contains("/var/lib/garage/data"));
        assert!(!rendered.compose_yaml.contains("DASOBJECTSTORE_BUCKETS"));
        assert!(!rendered.compose_yaml.contains("GARAGE_DEFAULT_ACCESS_KEY"));
        assert!(rendered
            .compose_yaml
            .contains("command: [\"/garage\", \"server\", \"--single-node\"]"));
        assert!(rendered.compose_yaml.contains("restart: \"no\""));
        assert!(rendered
            .compose_yaml
            .contains("test: [\"CMD\", \"/garage\", \"status\"]"));
        assert!(rendered
            .compose_yaml
            .contains("bucket_provisioning: live-garage-admin"));
        assert!(rendered.compose_yaml.contains("\"127.0.0.1:3900:3900\""));
        assert!(rendered
            .compose_yaml
            .contains("credential_reference: secret://generated"));
    }

    #[test]
    fn renders_garage_remote_bind_address_when_requested() {
        let provider = GarageProvider::new(GarageProviderConfig {
            bind_address: "0.0.0.0".to_string(),
            ..GarageProviderConfig::default()
        });

        let rendered = provider
            .render_compose(&request())
            .expect("Garage compose renders");

        assert!(rendered.compose_yaml.contains("\"0.0.0.0:3900:3900\""));
        assert!(rendered.compose_yaml.contains("\"0.0.0.0:3901:3901\""));
    }

    #[test]
    fn publishes_retained_container_listener_on_private_gateway_port() {
        let provider = GarageProvider::new(GarageProviderConfig {
            published_api_port: Some(4900),
            ..GarageProviderConfig::default()
        });
        let rendered = provider
            .render_compose(&request())
            .expect("Garage compose renders");

        assert!(rendered.compose_yaml.contains("\"127.0.0.1:4900:3900\""));
        assert!(rendered.compose_yaml.contains("\"127.0.0.1:4901:3901\""));
        assert_eq!(provider.config().api_port, 3900);
    }

    #[test]
    fn quotes_host_paths_with_spaces_and_yaml_delimiters() {
        let provider = GarageProvider::new(GarageProviderConfig {
            config_path: "/Volumes/Seagate/DAS ObjectStore/runtime/garage.toml".to_string(),
            ..GarageProviderConfig::default()
        });
        let mut request = request();
        request.ssd_metadata_path = "/Volumes/Seagate/DAS ObjectStore/runtime/meta".to_string();
        request.hdd_data_path = "/Volumes/Seagate/DAS ObjectStore/runtime/data".to_string();

        let rendered = provider
            .render_compose(&request)
            .expect("Garage compose renders");

        assert!(rendered.compose_yaml.contains(
            "\"/Volumes/Seagate/DAS ObjectStore/runtime/garage.toml:/etc/garage.toml:ro\""
        ));
        assert!(rendered
            .compose_yaml
            .contains("\"/Volumes/Seagate/DAS ObjectStore/runtime/meta:/var/lib/garage/meta\""));
    }

    #[test]
    fn renders_matching_garage_config() {
        let provider = GarageProvider::new(GarageProviderConfig {
            api_port: 4900,
            rpc_port: 4901,
            web_port: 4902,
            admin_port: 4903,
            rpc_secret: Some(secret("0")),
            admin_token: Some(secret("1")),
            metrics_token: Some(secret("2")),
            ..GarageProviderConfig::default()
        });
        let config = provider.render_garage_config().expect("config renders");

        assert!(config.contains("metadata_dir = \"/var/lib/garage/meta\""));
        assert!(config.contains("data_dir = \"/var/lib/garage/data\""));
        assert!(config.contains("api_bind_addr = \"[::]:4900\""));
        assert!(config.contains("rpc_bind_addr = \"[::]:4901\""));
        assert!(config.contains("api_bind_addr = \"[::]:4903\""));
        assert!(config.contains(&format!("rpc_secret = \"{}\"", secret("0"))));
    }

    #[test]
    fn renders_native_multi_hdd_storage_and_read_only_legacy_path() {
        let directories = vec![
            GarageDataDirectory::read_only(
                "/srv/dasobjectstore/hdd/garage",
                "/var/lib/garage/data-legacy",
            ),
            GarageDataDirectory::writable(
                "/srv/dasobjectstore/hdd/qnap-1057/garage",
                "/var/lib/garage/data/qnap-1057",
                "4T",
            ),
            GarageDataDirectory::writable(
                "/srv/dasobjectstore/hdd/qnap-1063/garage",
                "/var/lib/garage/data/qnap-1063",
                "3T",
            ),
        ];
        let provider = GarageProvider::new(GarageProviderConfig {
            rpc_secret: Some(secret("0")),
            admin_token: Some(secret("1")),
            metrics_token: Some(secret("2")),
            data_directories: directories,
            ..GarageProviderConfig::default()
        });

        let compose = provider
            .render_compose(&request())
            .expect("multi-HDD compose renders")
            .compose_yaml;
        let config = provider.render_garage_config().expect("config renders");

        assert!(
            compose.contains("\"/srv/dasobjectstore/hdd/garage:/var/lib/garage/data-legacy:ro\"")
        );
        assert!(compose.contains(
            "\"/srv/dasobjectstore/hdd/qnap-1057/garage:/var/lib/garage/data/qnap-1057\""
        ));
        assert!(!compose.contains("\"/srv/dasobjectstore/hdd/garage:/var/lib/garage/data\""));
        assert!(config.contains("{ path = \"/var/lib/garage/data-legacy\", read_only = true }"));
        assert!(config.contains("{ path = \"/var/lib/garage/data/qnap-1057\", capacity = \"4T\" }"));
        assert!(config.contains("{ path = \"/var/lib/garage/data/qnap-1063\", capacity = \"3T\" }"));
    }

    #[test]
    fn rejects_multi_hdd_storage_without_writable_directory() {
        let provider = GarageProvider::new(GarageProviderConfig {
            data_directories: vec![GarageDataDirectory::read_only(
                "/srv/dasobjectstore/hdd/garage",
                "/var/lib/garage/data-legacy",
            )],
            ..GarageProviderConfig::default()
        });

        let error = provider
            .render_compose(&request())
            .expect_err("read-only-only config rejected");

        assert!(error
            .to_string()
            .contains("requires at least one writable directory"));
    }

    #[test]
    fn rejects_blank_image() {
        let provider = GarageProvider::new(GarageProviderConfig {
            image: " ".to_string(),
            ..GarageProviderConfig::default()
        });

        let err = provider
            .render_compose(&request())
            .expect_err("blank image rejected");

        assert!(err.to_string().contains("Garage image must not be blank"));
    }

    #[test]
    fn rejects_config_rendering_without_secrets() {
        let provider = GarageProvider::default();

        let err = provider
            .render_garage_config()
            .expect_err("missing secrets rejected");

        assert!(err
            .to_string()
            .contains("Garage rpc_secret must be generated"));
    }

    fn request() -> ComposeRenderRequest {
        ComposeRenderRequest {
            project_name: "dasobjectstore-test".to_string(),
            ssd_metadata_path: "/srv/dasobjectstore/ssd/garage".to_string(),
            hdd_data_path: "/srv/dasobjectstore/hdd/garage".to_string(),
            store_bindings: vec![StoreBucketBinding {
                store_id: StoreId::new("generated").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: "dos-generated".to_string(),
                credential_reference: "secret://generated".to_string(),
            }],
        }
    }

    fn secret(suffix: &str) -> String {
        format!("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde{suffix}")
    }
}
