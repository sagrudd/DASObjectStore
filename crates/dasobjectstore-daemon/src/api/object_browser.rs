use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::ObjectState;
use dasobjectstore_core::object_type::ObjectType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const OBJECT_BROWSER_MAX_PAGE_LIMIT: u16 = 500;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserRequest {
    pub endpoint: StoreId,
    pub prefix: Option<String>,
    pub search: Option<String>,
    pub sort: ObjectBrowserSort,
    pub page: ObjectBrowserPageRequest,
    pub include_placement: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_actor: Option<ObjectBrowserDelegatedActor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectDownloadRequest {
    pub endpoint: StoreId,
    pub object_id: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_actor: Option<ObjectBrowserDelegatedActor>,
}

impl ObjectDownloadRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.endpoint.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "endpoint" });
        }
        if self.object_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "object_id" });
        }
        validate_delegated_actor(self.delegated_actor.as_ref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectDownloadResponse {
    pub endpoint: StoreId,
    pub store_id: StoreId,
    pub object_id: ObjectId,
    pub file_name: String,
    pub source_disk_id: DiskId,
    pub source_path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectFolderDownloadRequest {
    pub endpoint: StoreId,
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_actor: Option<ObjectBrowserDelegatedActor>,
}

impl ObjectFolderDownloadRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.endpoint.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "endpoint" });
        }
        if self.prefix.trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "prefix" });
        }
        validate_delegated_actor(self.delegated_actor.as_ref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectFolderDownloadResponse {
    pub endpoint: StoreId,
    pub store_id: StoreId,
    pub prefix: String,
    pub archive_name: String,
    pub total_files: u64,
    pub total_source_bytes: u64,
    pub entries: Vec<ObjectFolderArchiveEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectFolderArchiveEntry {
    pub object_id: ObjectId,
    pub archive_path: String,
    pub source_disk_id: DiskId,
    pub source_path: PathBuf,
    pub size_bytes: u64,
}

impl ObjectBrowserRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        reject_blank_optional("prefix", self.prefix.as_deref())?;
        reject_blank_optional("search", self.search.as_deref())?;
        reject_blank_optional("cursor", self.page.cursor.as_deref())?;
        validate_delegated_actor(self.delegated_actor.as_ref())?;
        if self.page.limit == 0 || self.page.limit > OBJECT_BROWSER_MAX_PAGE_LIMIT {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "limit",
                value: self.page.limit.to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserDelegatedActor {
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_gid: Option<u32>,
    #[serde(default)]
    pub groups: Vec<String>,
}

impl ObjectBrowserDelegatedActor {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        reject_blank_required("delegated_actor.username", &self.username)?;
        reject_unsafe_local_name("delegated_actor.username", &self.username)?;
        for group in &self.groups {
            reject_blank_required("delegated_actor.groups", group)?;
            reject_unsafe_local_name("delegated_actor.groups", group)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserPageRequest {
    pub cursor: Option<String>,
    pub limit: u16,
}

impl Default for ObjectBrowserPageRequest {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectBrowserSort {
    #[default]
    NameAsc,
    NameDesc,
    SizeAsc,
    SizeDesc,
    ModifiedAsc,
    ModifiedDesc,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserResponse {
    pub endpoint: StoreId,
    pub prefix: String,
    pub breadcrumbs: Vec<ObjectBrowserBreadcrumb>,
    pub folders: Vec<ObjectBrowserFolderNode>,
    pub files: Vec<ObjectBrowserFileNode>,
    pub next_cursor: Option<String>,
    pub total_entries: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserBreadcrumb {
    pub name: String,
    pub prefix: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserFolderNode {
    pub name: String,
    pub prefix: String,
    pub object_count: Option<u64>,
    pub total_size_bytes: Option<u64>,
    pub readiness: ObjectBrowserReadinessState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserFileNode {
    pub object_id: ObjectId,
    pub name: String,
    pub path: String,
    pub object_type: ObjectType,
    pub size_bytes: u64,
    pub modified_at_utc: Option<String>,
    pub checksum: Option<ObjectBrowserChecksum>,
    pub readiness: ObjectBrowserReadinessState,
    pub lifecycle_state: ObjectState,
    pub copy_count: u16,
    pub placements: Vec<ObjectBrowserPlacement>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserChecksum {
    pub algorithm: String,
    pub value: String,
    pub verified_at_utc: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectBrowserReadinessState {
    Available,
    SsdOnly,
    Settling,
    Degraded,
    RedownloadRequired,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectBrowserPlacement {
    pub disk_id: Option<DiskId>,
    pub disk_label: Option<String>,
    pub location: ObjectBrowserPlacementLocation,
    pub state: ObjectBrowserPlacementState,
    pub size_bytes: u64,
    pub checksum: Option<ObjectBrowserChecksum>,
    pub verified_at_utc: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectBrowserPlacementLocation {
    SsdLanding,
    HddSettled,
    ExternalEndpoint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectBrowserPlacementState {
    Verified,
    Pending,
    Missing,
    Degraded,
}

fn reject_blank_optional(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), DaemonRequestValidationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(DaemonRequestValidationError::BlankField { field });
    }
    Ok(())
}

fn reject_blank_required(
    field: &'static str,
    value: &str,
) -> Result<(), DaemonRequestValidationError> {
    if value.trim().is_empty() {
        return Err(DaemonRequestValidationError::BlankField { field });
    }
    Ok(())
}

fn reject_unsafe_local_name(
    field: &'static str,
    value: &str,
) -> Result<(), DaemonRequestValidationError> {
    if value.chars().any(|candidate| {
        !(candidate.is_ascii_alphanumeric() || matches!(candidate, '_' | '-' | '.'))
    }) {
        return Err(DaemonRequestValidationError::UnsupportedFieldValue {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_delegated_actor(
    actor: Option<&ObjectBrowserDelegatedActor>,
) -> Result<(), DaemonRequestValidationError> {
    if let Some(actor) = actor {
        actor.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ObjectBrowserBreadcrumb, ObjectBrowserChecksum, ObjectBrowserFileNode,
        ObjectBrowserFolderNode, ObjectBrowserPageRequest, ObjectBrowserPlacement,
        ObjectBrowserPlacementLocation, ObjectBrowserPlacementState, ObjectBrowserReadinessState,
        ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort, ObjectDownloadRequest,
        ObjectDownloadResponse, ObjectFolderArchiveEntry, ObjectFolderDownloadRequest,
        ObjectFolderDownloadResponse, OBJECT_BROWSER_MAX_PAGE_LIMIT,
    };
    use crate::api::DaemonRequestValidationError;
    use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::ObjectState;
    use dasobjectstore_core::object_type::ObjectType;

    #[test]
    fn object_browser_request_serializes_with_stable_sort_case() {
        let request = ObjectBrowserRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: Some("ENA/Xenognostikon".to_string()),
            search: Some("vervet".to_string()),
            sort: ObjectBrowserSort::ModifiedDesc,
            page: ObjectBrowserPageRequest {
                cursor: Some("cursor-1".to_string()),
                limit: 50,
            },
            include_placement: true,
            delegated_actor: None,
        };

        request.validate().expect("request is valid");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["endpoint"], "ena");
        assert_eq!(encoded["sort"], "modified_desc");
        assert_eq!(encoded["page"]["limit"], 50);
    }

    #[test]
    fn object_download_request_and_response_serialize_stably() {
        let request = ObjectDownloadRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            object_id: ObjectId::new("ENA/Xenognostikon/metadata.tsv").expect("object id"),
            delegated_actor: None,
        };

        request.validate().expect("request is valid");
        let encoded = serde_json::to_value(&request).expect("request serializes");

        assert_eq!(encoded["endpoint"], "ena");
        assert_eq!(encoded["object_id"], "ENA/Xenognostikon/metadata.tsv");

        let response = ObjectDownloadResponse {
            endpoint: StoreId::new("ena").expect("store id"),
            store_id: StoreId::new("ena").expect("store id"),
            object_id: request.object_id,
            file_name: "metadata.tsv".to_string(),
            source_disk_id: DiskId::new("disk-a").expect("disk id"),
            source_path: "/srv/dasobjectstore/hdd/disk-a/objects/aa/object/payload".into(),
            size_bytes: 512,
        };
        let encoded = serde_json::to_value(response).expect("response serializes");

        assert_eq!(encoded["file_name"], "metadata.tsv");
        assert_eq!(encoded["size_bytes"], 512);
    }

    #[test]
    fn object_folder_download_request_and_response_serialize_stably() {
        let request = ObjectFolderDownloadRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: "ENA/Xenognostikon".to_string(),
            delegated_actor: None,
        };

        request.validate().expect("request is valid");
        let encoded = serde_json::to_value(&request).expect("request serializes");

        assert_eq!(encoded["endpoint"], "ena");
        assert_eq!(encoded["prefix"], "ENA/Xenognostikon");

        let response = ObjectFolderDownloadResponse {
            endpoint: StoreId::new("ena").expect("store id"),
            store_id: StoreId::new("ena").expect("store id"),
            prefix: request.prefix,
            archive_name: "Xenognostikon.tar.gz".to_string(),
            total_files: 1,
            total_source_bytes: 512,
            entries: vec![ObjectFolderArchiveEntry {
                object_id: ObjectId::new("ENA/Xenognostikon/metadata.tsv").expect("object id"),
                archive_path: "metadata.tsv".to_string(),
                source_disk_id: DiskId::new("disk-a").expect("disk id"),
                source_path: "/srv/dasobjectstore/hdd/disk-a/objects/aa/object/payload".into(),
                size_bytes: 512,
            }],
        };
        let encoded = serde_json::to_value(response).expect("response serializes");

        assert_eq!(encoded["archive_name"], "Xenognostikon.tar.gz");
        assert_eq!(encoded["entries"][0]["archive_path"], "metadata.tsv");
    }

    #[test]
    fn object_browser_response_carries_file_readiness_and_placements() {
        let response = ObjectBrowserResponse {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: "ENA/Xenognostikon".to_string(),
            breadcrumbs: vec![ObjectBrowserBreadcrumb {
                name: "ENA".to_string(),
                prefix: "ENA".to_string(),
            }],
            folders: vec![ObjectBrowserFolderNode {
                name: "Vervet".to_string(),
                prefix: "ENA/Xenognostikon/Vervet".to_string(),
                object_count: Some(2),
                total_size_bytes: Some(1024),
                readiness: ObjectBrowserReadinessState::Available,
            }],
            files: vec![ObjectBrowserFileNode {
                object_id: ObjectId::new("ENA/Xenognostikon/metadata.tsv").expect("object id"),
                name: "metadata.tsv".to_string(),
                path: "ENA/Xenognostikon/metadata.tsv".to_string(),
                object_type: ObjectType::Naive,
                size_bytes: 512,
                modified_at_utc: Some("2026-07-09T09:37:51Z".to_string()),
                checksum: Some(ObjectBrowserChecksum {
                    algorithm: "sha256".to_string(),
                    value: "abc123".to_string(),
                    verified_at_utc: Some("2026-07-09T09:38:00Z".to_string()),
                }),
                readiness: ObjectBrowserReadinessState::Available,
                lifecycle_state: ObjectState::Protected,
                copy_count: 1,
                placements: vec![ObjectBrowserPlacement {
                    disk_id: Some(DiskId::new("qnap-1057").expect("disk id")),
                    disk_label: Some("QNAP bay 1".to_string()),
                    location: ObjectBrowserPlacementLocation::HddSettled,
                    state: ObjectBrowserPlacementState::Verified,
                    size_bytes: 512,
                    checksum: None,
                    verified_at_utc: Some("2026-07-09T09:38:00Z".to_string()),
                }],
            }],
            next_cursor: None,
            total_entries: Some(2),
        };

        let encoded = serde_json::to_value(response).expect("response serializes");

        assert_eq!(encoded["files"][0]["readiness"], "available");
        assert_eq!(
            encoded["files"][0]["placements"][0]["location"],
            "hdd_settled"
        );
        assert_eq!(encoded["files"][0]["lifecycle_state"], "Protected");
    }

    #[test]
    fn object_browser_request_rejects_unbounded_page_limits() {
        let request = ObjectBrowserRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: None,
            search: None,
            sort: ObjectBrowserSort::NameAsc,
            page: ObjectBrowserPageRequest {
                cursor: None,
                limit: OBJECT_BROWSER_MAX_PAGE_LIMIT + 1,
            },
            include_placement: false,
            delegated_actor: None,
        };

        let err = request.validate().expect_err("oversized page rejected");

        assert_eq!(
            err,
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "limit",
                value: (OBJECT_BROWSER_MAX_PAGE_LIMIT + 1).to_string(),
            }
        );
    }

    #[test]
    fn object_browser_request_rejects_blank_prefix() {
        let request = ObjectBrowserRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: Some(" ".to_string()),
            search: None,
            sort: ObjectBrowserSort::NameAsc,
            page: ObjectBrowserPageRequest::default(),
            include_placement: false,
            delegated_actor: None,
        };

        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::BlankField { field: "prefix" })
        ));
    }
}
