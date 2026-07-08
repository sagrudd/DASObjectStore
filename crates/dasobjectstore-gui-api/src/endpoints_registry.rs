use crate::endpoints::{
    EndpointBindingReadinessView, EndpointBindingView, EndpointInventoryItemView,
    EndpointInventoryView, EndpointKindView, EndpointValidationStateView, EndpointValidationView,
    EndpointWarningSeverityView, EndpointWarningView,
};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_ENDPOINTS_REGISTRY_PATH: &str = "/opt/dasobjectstore/endpoints.json";
pub(crate) const ENDPOINTS_REGISTRY_ENV: &str = "DASOBJECTSTORE_ENDPOINTS_PATH";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EndpointInventorySnapshot {
    pub path: PathBuf,
    pub inventory: EndpointInventoryView,
}

pub(crate) fn default_endpoints_registry_path() -> PathBuf {
    std::env::var_os(ENDPOINTS_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ENDPOINTS_REGISTRY_PATH))
}

pub(crate) fn read_endpoint_inventory(path: &Path) -> EndpointInventorySnapshot {
    match read_endpoint_entries(path) {
        Ok(entries) => EndpointInventorySnapshot {
            path: path.to_path_buf(),
            inventory: EndpointInventoryView::from_endpoints(
                entries
                    .into_iter()
                    .map(EndpointRegistryEntry::into_view)
                    .collect(),
            ),
        },
        Err(EndpointRegistryError::Missing) => EndpointInventorySnapshot {
            path: path.to_path_buf(),
            inventory: EndpointInventoryView::from_endpoints(Vec::new()).with_warnings(vec![
                EndpointWarningView::registry(
                    "endpoint_registry_missing",
                    EndpointWarningSeverityView::Warning,
                    format!("Endpoint registry is not present at {}.", path.display()),
                ),
            ]),
        },
        Err(EndpointRegistryError::Read(error)) => EndpointInventorySnapshot {
            path: path.to_path_buf(),
            inventory: EndpointInventoryView::from_endpoints(Vec::new()).with_warnings(vec![
                EndpointWarningView::registry(
                    "endpoint_registry_unreadable",
                    EndpointWarningSeverityView::Warning,
                    format!(
                        "Endpoint registry {} could not be read: {error}.",
                        path.display()
                    ),
                ),
            ]),
        },
        Err(EndpointRegistryError::Json(error)) => EndpointInventorySnapshot {
            path: path.to_path_buf(),
            inventory: EndpointInventoryView::from_endpoints(Vec::new()).with_warnings(vec![
                EndpointWarningView::registry(
                    "endpoint_registry_invalid",
                    EndpointWarningSeverityView::Critical,
                    format!(
                        "Endpoint registry {} is not valid JSON: {error}.",
                        path.display()
                    ),
                ),
            ]),
        },
    }
}

fn read_endpoint_entries(path: &Path) -> Result<Vec<EndpointRegistryEntry>, EndpointRegistryError> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(EndpointRegistryError::Missing);
        }
        Err(error) => return Err(EndpointRegistryError::Read(error)),
    };
    let registry: EndpointRegistryFile =
        serde_json::from_str(&data).map_err(EndpointRegistryError::Json)?;
    Ok(registry.entries())
}

#[derive(Debug)]
enum EndpointRegistryError {
    Missing,
    Read(std::io::Error),
    Json(serde_json::Error),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
enum EndpointRegistryFile {
    Object {
        endpoints: Vec<EndpointRegistryEntry>,
    },
    List(Vec<EndpointRegistryEntry>),
}

impl EndpointRegistryFile {
    fn entries(self) -> Vec<EndpointRegistryEntry> {
        match self {
            Self::Object { endpoints } => endpoints,
            Self::List(endpoints) => endpoints,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct EndpointRegistryEntry {
    endpoint_id: String,
    display_name: String,
    kind: EndpointKindView,
    object_service_url: String,
    validation: EndpointValidationRegistryEntry,
    #[serde(default = "default_manager_product_id")]
    manager_product_id: String,
    #[serde(default)]
    active_bindings: Vec<EndpointBindingRegistryEntry>,
}

impl EndpointRegistryEntry {
    fn into_view(self) -> EndpointInventoryItemView {
        let mut view = EndpointInventoryItemView::new(
            self.endpoint_id,
            self.display_name,
            self.kind,
            self.object_service_url,
            self.validation.into_view(),
        );
        view.manager_product_id = self.manager_product_id;
        for binding in self.active_bindings {
            view = view.with_binding(binding.into_view());
        }
        view
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct EndpointValidationRegistryEntry {
    state: EndpointValidationStateView,
    #[serde(default)]
    checked_at_utc: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl EndpointValidationRegistryEntry {
    fn into_view(self) -> EndpointValidationView {
        let mut view = EndpointValidationView::new(self.state);
        view.checked_at_utc = self.checked_at_utc;
        view.message = self.message;
        view
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct EndpointBindingRegistryEntry {
    binding_id: String,
    governance_domain: String,
    store_id: String,
    readiness: EndpointBindingReadinessView,
}

impl EndpointBindingRegistryEntry {
    fn into_view(self) -> EndpointBindingView {
        EndpointBindingView::new(
            self.binding_id,
            self.governance_domain,
            self.store_id,
            self.readiness,
        )
    }
}

fn default_manager_product_id() -> String {
    "dasobjectstore".to_string()
}

#[cfg(test)]
mod tests {
    use super::read_endpoint_inventory;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_object_endpoint_registry() {
        let root = temp_root("endpoints-object");
        let path = root.join("endpoints.json");
        fs::write(
            &path,
            r#"{
              "endpoints": [{
                "endpoint_id": "endpoint-nfs",
                "display_name": "NAS staging",
                "kind": "dasobjectstore_nfs",
                "object_service_url": "https://nas.example.test:9443",
                "validation": {
                  "state": "degraded",
                  "checked_at_utc": "2026-07-09T00:01:00Z",
                  "message": "Runtime probe reported stale mount."
                },
                "active_bindings": [{
                  "binding_id": "binding-a",
                  "governance_domain": "synoptikon-dev",
                  "store_id": "raw-public",
                  "readiness": "blocked"
                }]
              }]
            }"#,
        )
        .expect("registry write");

        let snapshot = read_endpoint_inventory(&path);

        assert_eq!(snapshot.path, path);
        assert_eq!(snapshot.inventory.endpoint_count, 1);
        assert_eq!(snapshot.inventory.degraded_endpoint_count, 1);
        assert_eq!(snapshot.inventory.binding_count, 1);
        assert_eq!(snapshot.inventory.endpoints[0].endpoint_id, "endpoint-nfs");
        assert_eq!(
            snapshot.inventory.endpoints[0]
                .validation
                .message
                .as_deref(),
            Some("Runtime probe reported stale mount.")
        );
        assert!(snapshot
            .inventory
            .warnings
            .iter()
            .any(|warning| warning.code == "endpoint_degraded"));
        assert!(snapshot
            .inventory
            .warnings
            .iter()
            .any(|warning| warning.code == "endpoint_binding_blocked"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn reads_list_endpoint_registry() {
        let root = temp_root("endpoints-list");
        let path = root.join("endpoints.json");
        fs::write(
            &path,
            r#"[{
              "endpoint_id": "endpoint-s3",
              "display_name": "S3 export",
              "kind": "s3_compatible",
              "object_service_url": "https://s3.example.test",
              "validation": {"state": "validated"}
            }]"#,
        )
        .expect("registry write");

        let snapshot = read_endpoint_inventory(&path);

        assert!(snapshot.inventory.warnings.is_empty());
        assert_eq!(snapshot.inventory.endpoints[0].endpoint_id, "endpoint-s3");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn missing_endpoint_registry_reports_warning() {
        let root = temp_root("endpoints-missing");
        let snapshot = read_endpoint_inventory(&root.join("missing.json"));

        assert_eq!(snapshot.inventory.endpoint_count, 0);
        assert!(snapshot
            .inventory
            .warnings
            .iter()
            .any(|warning| warning.code == "endpoint_registry_missing"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
