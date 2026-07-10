use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreVerifyRequest {
    pub store_id: Option<StoreId>,
    pub hash_payloads: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreVerifyResponse {
    pub report: StoreVerifyReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreVerifyReport {
    pub metadata_path: String,
    pub stores_scanned: u64,
    pub objects_scanned: u64,
    pub placements_scanned: u64,
    pub payloads_checked: u64,
    pub payload_bytes_checked: u64,
    pub missing_payloads: u64,
    pub orphan_payloads: u64,
    pub size_mismatches: u64,
    pub hash_mismatches: u64,
    pub unverified_placements: u64,
    pub duplicate_content_groups: u64,
    pub duplicate_placement_rows: u64,
    pub io_errors: u64,
    pub healthy: bool,
    pub findings: Vec<String>,
}
