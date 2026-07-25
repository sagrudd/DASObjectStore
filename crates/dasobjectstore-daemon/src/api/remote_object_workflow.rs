use super::DaemonRequestValidationError;
use base64::Engine as _;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use serde::{Deserialize, Serialize};

pub const REMOTE_OBJECT_WORKFLOW_SCHEMA_VERSION: &str = "dasobjectstore.remote_object_workflow.v1";
pub const REMOTE_OBJECT_SNAPSHOT_DEFAULT_LIMIT: u32 = 1_000;
pub const REMOTE_OBJECT_SNAPSHOT_MAX_LIMIT: u32 = 20_000;

/// Path-free, catalogue-authoritative inventory request. The cursor is opaque
/// and binds subsequent pages to the insertion high-water mark of page one.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectSnapshotRequest {
    pub store_id: StoreId,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_snapshot_limit")]
    pub limit: u32,
}

impl RemoteObjectSnapshotRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        validate_store_and_prefix(&self.store_id, &self.prefix)?;
        if self.limit == 0 || self.limit > REMOTE_OBJECT_SNAPSHOT_MAX_LIMIT {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "limit",
                value: self.limit.to_string(),
            });
        }
        if let Some(cursor) = &self.cursor {
            decode_snapshot_cursor(cursor).map_err(|_| {
                DaemonRequestValidationError::UnsupportedFieldValue {
                    field: "cursor",
                    value: "invalid opaque cursor".to_string(),
                }
            })?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectSnapshotResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub prefix: String,
    pub snapshot_id: String,
    pub objects: Vec<RemoteObjectSnapshotEntry>,
    pub total_objects: u64,
    pub next_cursor: Option<String>,
    pub complete: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectSnapshotEntry {
    pub key: String,
    pub version: u64,
    pub object_id: ObjectId,
    pub size_bytes: u64,
    pub checksum: RemoteObjectChecksum,
    pub provider_visibility: RemoteProviderVisibility,
    pub group: RemoteObjectGroupRelationship,
    pub lifecycle_state: String,
    pub readiness: RemoteObjectReadiness,
    pub placement: RemoteObjectPlacementSummary,
    pub updated_at_utc: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteProviderVisibility {
    /// This catalogue-only read did not contact the provider.
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectGroupRelationship {
    pub payload_key: String,
    pub member_role: RemoteObjectGroupMemberRole,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteObjectGroupMemberRole {
    Payload,
    Manifest,
    ChecksumSidecar,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectChecksum {
    pub algorithm: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteObjectReadiness {
    Available,
    Settling,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectPlacementSummary {
    pub active_ssd_copy: bool,
    pub hdd_copy_count: u64,
    pub verified_hdd_copy_count: u64,
    pub durable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectGroupStatusRequest {
    pub store_id: StoreId,
    pub key: String,
}

impl RemoteObjectGroupStatusRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        validate_store_and_prefix(&self.store_id, &self.key)?;
        if self.key.ends_with('/') {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "key",
                value: self.key.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectGroupStatusResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub key: String,
    pub state: RemoteObjectGroupState,
    pub payload: Option<RemoteObjectSnapshotEntry>,
    pub manifest: Option<RemoteObjectSnapshotEntry>,
    pub checksum_sidecar: Option<RemoteObjectSnapshotEntry>,
    pub catalogue_complete: bool,
    pub durable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteObjectGroupState {
    Absent,
    PartialProviderGroup,
    ProviderComplete,
    ReconciliationQueued,
    SsdAcknowledged,
    HddSettled,
    VerificationFailed,
    RepairRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SnapshotCursor {
    snapshot_high_water: u64,
    after_key: String,
    after_version: u64,
}

pub(crate) fn encode_snapshot_cursor(
    snapshot_high_water: u64,
    after_key: String,
    after_version: u64,
) -> String {
    let bytes = serde_json::to_vec(&SnapshotCursor {
        snapshot_high_water,
        after_key,
        after_version,
    })
    .expect("snapshot cursor serialization is infallible");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn decode_snapshot_cursor(value: &str) -> Result<(u64, String, u64), serde_json::Error> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|error| {
            serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
    let cursor: SnapshotCursor = serde_json::from_slice(&bytes)?;
    Ok((
        cursor.snapshot_high_water,
        cursor.after_key,
        cursor.after_version,
    ))
}

pub(crate) fn snapshot_id(snapshot_high_water: u64) -> String {
    format!("s3-bindings-v1-{snapshot_high_water}")
}

fn default_snapshot_limit() -> u32 {
    REMOTE_OBJECT_SNAPSHOT_DEFAULT_LIMIT
}

fn validate_store_and_prefix(
    store_id: &StoreId,
    value: &str,
) -> Result<(), DaemonRequestValidationError> {
    if store_id.as_str().trim().is_empty() {
        return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
    }
    if value.contains('\0') || value.starts_with('/') || value.contains("../") {
        return Err(DaemonRequestValidationError::UnsupportedFieldValue {
            field: "prefix",
            value: value.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_cursor_round_trips_arbitrary_s3_key() {
        let encoded = encode_snapshot_cursor(41, "EPICv1/a:b% c.tar".to_string(), 3);
        assert_eq!(
            decode_snapshot_cursor(&encoded).expect("cursor"),
            (41, "EPICv1/a:b% c.tar".to_string(), 3)
        );
        assert!(!encoded.contains("EPICv1"));
    }

    #[test]
    fn snapshot_limit_is_bounded_at_twenty_thousand() {
        let request = RemoteObjectSnapshotRequest {
            store_id: StoreId::new("epic_collection").expect("store"),
            prefix: "EPICv1/".to_string(),
            cursor: None,
            limit: 20_001,
        };
        assert!(request.validate().is_err());
    }
}
