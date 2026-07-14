use crate::api::UpsertEndpointInventoryRequest;
use crate::runtime::DaemonServiceRuntimeError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_ENDPOINT_REGISTRY_PATH: &str = "/opt/dasobjectstore/endpoints.json";
pub const ENDPOINT_REGISTRY_ENV: &str = "DASOBJECTSTORE_ENDPOINTS_PATH";
pub const ENDPOINT_REGISTRY_SCHEMA: &str = "dasobjectstore.endpoint_inventory_registry.v1";

static ENDPOINT_REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointRegistryUpsertSummary {
    pub registry_path: PathBuf,
    pub endpoint_id: String,
    pub endpoint_count: usize,
}

pub fn default_endpoint_registry_path() -> PathBuf {
    std::env::var_os(ENDPOINT_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ENDPOINT_REGISTRY_PATH))
}

pub fn upsert_endpoint_inventory_record(
    path: impl AsRef<Path>,
    request: &UpsertEndpointInventoryRequest,
) -> Result<EndpointRegistryUpsertSummary, DaemonServiceRuntimeError> {
    let _guard = ENDPOINT_REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("endpoint registry lock poisoned");
    let path = path.as_ref();
    let mut registry = read_endpoint_registry(path)?;
    registry.upsert(EndpointRegistryEntry::from_request(request));
    if !request.dry_run {
        write_endpoint_registry(path, &registry)?;
    }

    Ok(EndpointRegistryUpsertSummary {
        registry_path: path.to_path_buf(),
        endpoint_id: request.endpoint_id.clone(),
        endpoint_count: registry.endpoints.len(),
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct EndpointRegistryFile {
    schema_version: String,
    endpoints: Vec<EndpointRegistryEntry>,
}

impl Default for EndpointRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: ENDPOINT_REGISTRY_SCHEMA.to_string(),
            endpoints: Vec::new(),
        }
    }
}

impl EndpointRegistryFile {
    fn upsert(&mut self, entry: EndpointRegistryEntry) {
        match self
            .endpoints
            .iter_mut()
            .find(|existing| existing.endpoint_id == entry.endpoint_id)
        {
            Some(existing) => *existing = entry,
            None => self.endpoints.push(entry),
        }
        self.endpoints
            .sort_by(|left, right| left.endpoint_id.cmp(&right.endpoint_id));
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct EndpointRegistryEntry {
    endpoint_id: String,
    display_name: String,
    kind: crate::api::DaemonEndpointKind,
    object_service_url: String,
    validation: crate::api::DaemonEndpointValidation,
    manager_product_id: String,
    active_bindings: Vec<crate::api::DaemonEndpointBinding>,
}

impl EndpointRegistryEntry {
    fn from_request(request: &UpsertEndpointInventoryRequest) -> Self {
        Self {
            endpoint_id: request.endpoint_id.clone(),
            display_name: request.display_name.clone(),
            kind: request.kind,
            object_service_url: request.object_service_url.clone(),
            validation: request.validation.clone(),
            manager_product_id: request.manager_product_id.clone(),
            active_bindings: request.active_bindings.clone(),
        }
    }
}

fn read_endpoint_registry(path: &Path) -> Result<EndpointRegistryFile, DaemonServiceRuntimeError> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EndpointRegistryFile::default());
        }
        Err(error) => {
            return Err(DaemonServiceRuntimeError::EndpointRegistryIo {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }
    };
    serde_json::from_str(&data).map_err(|error| {
        DaemonServiceRuntimeError::InvalidEndpointRegistryJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn write_endpoint_registry(
    path: &Path,
    registry: &EndpointRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: path.to_path_buf(),
            message: "endpoint registry has no parent".to_string(),
        })?;
    fs::create_dir_all(parent).map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
        path: parent.to_path_buf(),
        message: error.to_string(),
    })?;
    let data = serde_json::to_vec_pretty(registry).map_err(|error| {
        DaemonServiceRuntimeError::InvalidEndpointRegistryJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("endpoints"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: temporary.clone(),
            message: error.to_string(),
        })?;
    file.write_all(&data)
        .and_then(|_| file.sync_all())
        .map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: temporary.clone(),
            message: error.to_string(),
        })?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| {
        DaemonServiceRuntimeError::EndpointRegistryIo {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::upsert_endpoint_inventory_record;
    use crate::api::{
        DaemonEndpointKind, DaemonEndpointValidation, DaemonEndpointValidationState,
        UpsertEndpointInventoryRequest, ENDPOINT_RECORD_CONFIRMATION,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn upserts_endpoint_record_without_overwriting_other_records() {
        let root = temp_root("endpoint-registry-upsert");
        let path = root.join("endpoints.json");

        upsert_endpoint_inventory_record(&path, &request("endpoint-b", "Endpoint B", false))
            .expect("first endpoint upserts");
        upsert_endpoint_inventory_record(&path, &request("endpoint-a", "Endpoint A", false))
            .expect("second endpoint upserts");
        upsert_endpoint_inventory_record(&path, &request("endpoint-b", "Endpoint B2", false))
            .expect("existing endpoint updates");

        let data = fs::read_to_string(&path).expect("registry reads");

        assert!(data.contains("\"schema_version\""));
        assert!(data.contains("Endpoint A"));
        assert!(data.contains("Endpoint B2"));
        assert!(!data.contains("Endpoint B\""));

        let entries = fs::read_dir(path.parent().expect("parent"))
            .expect("read parent")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries");
        assert_eq!(entries.len(), 1);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn concurrent_endpoint_upserts_preserve_both_records() {
        let root = temp_root("endpoint-registry-concurrent");
        let path = root.join("endpoints.json");
        let left_path = path.clone();
        let left = std::thread::spawn(move || {
            upsert_endpoint_inventory_record(&left_path, &request("endpoint-a", "A", false))
                .expect("left endpoint")
        });
        let right_path = path.clone();
        let right = std::thread::spawn(move || {
            upsert_endpoint_inventory_record(&right_path, &request("endpoint-b", "B", false))
                .expect("right endpoint")
        });
        left.join().expect("left joins");
        right.join().expect("right joins");

        let data = fs::read_to_string(&path).expect("registry reads");
        assert!(data.contains("endpoint-a"));
        assert!(data.contains("endpoint-b"));
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn dry_run_does_not_write_registry() {
        let root = temp_root("endpoint-registry-dry-run");
        let path = root.join("endpoints.json");

        let summary = upsert_endpoint_inventory_record(&path, &request("endpoint-a", "A", true))
            .expect("dry run computes");

        assert_eq!(summary.endpoint_count, 1);
        assert!(!path.exists());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn request(
        endpoint_id: &str,
        display_name: &str,
        dry_run: bool,
    ) -> UpsertEndpointInventoryRequest {
        UpsertEndpointInventoryRequest {
            endpoint_id: endpoint_id.to_string(),
            display_name: display_name.to_string(),
            kind: DaemonEndpointKind::DasobjectstoreNfs,
            object_service_url: "https://nas.example.test:9443".to_string(),
            validation: DaemonEndpointValidation {
                state: DaemonEndpointValidationState::Validated,
                checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                message: None,
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: Vec::new(),
            dry_run,
            client_request_id: None,
            administrator_actor: Some("admin".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-daemon-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
