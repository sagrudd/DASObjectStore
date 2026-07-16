//! Administration request and response contracts for the Web API.

use super::*;

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct GuiActionPlanResponse {
    pub action: String,
    pub execution: String,
    pub argv: Vec<String>,
    pub mutates_pool: bool,
    pub writes_recovery_metadata: bool,
    pub confirmation_required: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GuiActionPlanRequest {
    pub action: String,
    pub store_id: Option<String>,
    pub store_class: Option<String>,
    pub store_copies: Option<u8>,
    pub bucket: Option<String>,
    pub writer_group: Option<String>,
    pub ssd_root: Option<String>,
    pub public: Option<bool>,
    pub writeable: Option<bool>,
    pub capacity_behavior: Option<String>,
    pub retention: Option<String>,
    pub endpoint_export_mode: Option<String>,
    pub subobject_name: Option<String>,
    pub parent_store_id: Option<String>,
    pub parent_subobject_name: Option<String>,
    pub subobject_capacity_limit_bytes: Option<u64>,
    pub subobject_object_type: Option<String>,
    pub subobject_inherits_object_type: Option<bool>,
    pub subobject_s3_routing: Option<String>,
    pub ssd_device: Option<String>,
    pub hdd_devices: Vec<String>,
    pub mount_root: Option<String>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    pub allow_format: bool,
    pub existing_data_acknowledged: bool,
    pub confirmation_phrase: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreResponse {
    pub accepted: CreateObjectStoreAcceptedResponse,
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub writer_group: String,
    pub ssd_root: String,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreRequest {
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub writer_group: String,
    pub ssd_root: String,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObjectStoreIngestPolicyRequest {
    pub store_id: String,
    pub ingest_mode: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectStoreIngestPolicyResponse {
    pub job_id: String,
    pub store_id: String,
    pub previous_ingest_mode: String,
    pub ingest_mode: String,
    pub changed: bool,
    pub dry_run: bool,
    pub administrator_actor: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestControlRequest {
    pub action: String,
    pub reason: String,
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct IngestControlResponse {
    pub state: String,
    pub changed: bool,
    pub dry_run: bool,
    pub reason: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosurePrepareResponse {
    pub accepted: EnclosurePrepareAcceptedResponse,
    pub ssd_device: String,
    pub hdd_devices: Vec<EnclosurePrepareHddDevice>,
    pub mount_root: String,
    pub filesystem: String,
    pub owner: Option<String>,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosurePrepareAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AdminJobStatusResponse {
    pub job: AdminJobSummary,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AdminJobSummary {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub progress: AdminJobProgress,
    pub percent_complete: Option<u8>,
    pub submitted_at_utc: String,
    pub updated_at_utc: String,
    pub actor: Option<String>,
    pub failure_message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct AdminJobProgress {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AdminJobCancelResponse {
    pub job_id: String,
    pub accepted: bool,
    pub state: String,
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdminJobCancelRequest {
    pub reason: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosurePrepareHddDevice {
    pub disk_id: String,
    pub device_path: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EnclosurePrepareRequest {
    pub ssd_device: String,
    pub hdd_devices: Vec<EnclosurePrepareHddDevice>,
    pub mount_root: Option<String>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub allow_format: bool,
    pub existing_data_acknowledged: bool,
    pub confirmation_marker: Option<String>,
}
