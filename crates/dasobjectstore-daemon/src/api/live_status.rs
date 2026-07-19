use super::DaemonIngestProgressEvent;
use serde::{Deserialize, Serialize};

pub const LIVE_STATUS_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatusConnectionOrigin {
    LocalUnixSocket,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusActor {
    pub uid: u32,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusIngest {
    pub sequence: u64,
    pub updated_at_utc: String,
    pub origin: LiveStatusConnectionOrigin,
    /// Transport-observed host label. Local Unix-socket work is necessarily
    /// running on the daemon appliance; no client-supplied hostname is trusted.
    pub host_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<LiveStatusActor>,
    pub progress: DaemonIngestProgressEvent,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusAggregate {
    pub connected_hosts: u32,
    pub active_stores: u32,
    pub active_ingests: u32,
    pub source_read_bytes_per_second: u64,
    pub ssd_write_bytes_per_second: u64,
    pub hdd_write_bytes_per_second: u64,
    pub active_hdd_transfers: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusGarbageCollection {
    pub running: bool,
    pub last_completed_at_utc: Option<String>,
    pub scanned_bytes: u64,
    pub reclaimable_bytes: u64,
    pub reclaimed_bytes: u64,
    pub retained_items: u64,
    /// Path-free, bounded reasons explaining why staging remains allocated.
    pub retained_reasons: Vec<LiveStatusGarbageCollectionRetained>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusGarbageCollectionRetained {
    pub category: String,
    pub reason: String,
    pub items: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusResponse {
    pub schema_version: u16,
    pub sequence: u64,
    pub generated_at_utc: String,
    pub aggregate: LiveStatusAggregate,
    pub active: Vec<LiveStatusIngest>,
    pub recent: Vec<LiveStatusIngest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub garbage_collection: Option<LiveStatusGarbageCollection>,
}
