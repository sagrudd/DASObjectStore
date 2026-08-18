use dasobjectstore_core::SynoptikonProjectionSettlementV1;
use serde::{Deserialize, Serialize};

pub const SYNOPTIKON_PROJECTION_PREPARE_V1_SCHEMA: &str =
    "dasobjectstore.synoptikon_projection_prepare.v1";
pub const SYNOPTIKON_PROJECTION_SETTLE_V1_SCHEMA: &str =
    "dasobjectstore.synoptikon_projection_settle.v1";
pub const SYNOPTIKON_PROJECTION_FIXED_PEER_USER: &str = "syno-plug-demo";
pub const SYNOPTIKON_PROJECTION_MAX_BODY_BYTES: u64 = 1024 * 1024;
pub const SYNOPTIKON_PROJECTION_FIXED_STORE_ID: &str = "synoptikon-demo";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionPrepareRequest {
    pub schema_version: String,
    pub logical_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionPrepareResponse {
    pub schema_version: String,
    pub intent_id: String,
    pub projection_id: String,
    pub object_store_id: String,
    pub object_key: String,
    pub object_version: u64,
    pub generation: u64,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub expires_at_unix_seconds: u64,
    pub exact_replay: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionSettleRequest {
    pub schema_version: String,
    pub intent_id: String,
}

impl SynoptikonProjectionSettleRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != SYNOPTIKON_PROJECTION_SETTLE_V1_SCHEMA
            || self.intent_id.is_empty()
            || self.intent_id.len() > 128
            || !self
                .intent_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("invalid_synoptikon_projection_settlement");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionSettleResponse {
    pub schema_version: String,
    pub settlement_id: String,
    pub settlement: SynoptikonProjectionSettlementV1,
    pub exact_replay: bool,
}
