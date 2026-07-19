//! Privacy-preserving GUI projection of the daemon live-ingest snapshot.

use crate::daemon_bridge::{DaemonBridge, DaemonBridgeError};
use dasobjectstore_daemon::{
    DaemonClient, DaemonIngestHddTransferPhase, DaemonIngestStage, DaemonRuntimeConfig,
    LiveStatusIngest, LiveStatusRequest, LiveStatusResponse, UnixSocketDaemonTransport,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const LIVE_STATUS_VIEW_SCHEMA_VERSION: u16 = 1;
pub const LIVE_STATUS_SUGGESTED_REFRESH_MILLIS: u32 = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatusAvailabilityView {
    Available,
    Degraded,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusAggregateView {
    pub connected_hosts: u32,
    pub active_stores: u32,
    pub active_ingests: u32,
    pub source_read_bytes_per_second: u64,
    pub ssd_write_bytes_per_second: u64,
    pub hdd_write_bytes_per_second: u64,
    pub active_hdd_transfers: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusHostView {
    pub display_name: String,
    pub actors: Vec<String>,
    pub active_ingests: u32,
    pub object_stores: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusStoreWriterView {
    pub store_id: String,
    pub hosts: Vec<String>,
    pub active_ingests: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusProgressView {
    pub job_id: String,
    pub store_id: String,
    pub host: Option<String>,
    pub state: String,
    pub pipeline_stage: Option<String>,
    /// Object basename only. Host paths and full object namespaces never cross this boundary.
    pub current_item: Option<String>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub files_done: u64,
    pub files_total: Option<u64>,
    pub bytes_per_second: u64,
    pub updated_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusHddTransferView {
    pub job_id: String,
    pub store_id: String,
    pub disk_id: String,
    pub copy_number: u8,
    /// Object basename only. The daemon's relative path is deliberately discarded.
    pub current_item: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bytes_per_second: u64,
    pub phase: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusWarningView {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusGarbageCollectionView {
    pub running: bool,
    pub last_completed_at_utc: Option<String>,
    pub scanned_bytes: u64,
    pub reclaimed_bytes: u64,
    pub retained_items: u64,
    pub retained_reasons: Vec<LiveStatusWarningView>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveStatusWorkspaceView {
    pub schema_version: u16,
    pub availability: LiveStatusAvailabilityView,
    pub sequence: u64,
    pub generated_at_utc: Option<String>,
    pub suggested_refresh_millis: u32,
    pub aggregate: LiveStatusAggregateView,
    pub hosts: Vec<LiveStatusHostView>,
    pub store_writers: Vec<LiveStatusStoreWriterView>,
    pub ssd_ingests: Vec<LiveStatusProgressView>,
    pub hdd_transfers: Vec<LiveStatusHddTransferView>,
    pub recent: Vec<LiveStatusProgressView>,
    pub garbage_collection: Option<LiveStatusGarbageCollectionView>,
    pub warnings: Vec<LiveStatusWarningView>,
}

impl LiveStatusWorkspaceView {
    pub fn degraded(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: LIVE_STATUS_VIEW_SCHEMA_VERSION,
            availability: LiveStatusAvailabilityView::Degraded,
            sequence: 0,
            generated_at_utc: None,
            suggested_refresh_millis: LIVE_STATUS_SUGGESTED_REFRESH_MILLIS,
            aggregate: LiveStatusAggregateView::default(),
            hosts: Vec::new(),
            store_writers: Vec::new(),
            ssd_ingests: Vec::new(),
            hdd_transfers: Vec::new(),
            recent: Vec::new(),
            garbage_collection: None,
            warnings: vec![LiveStatusWarningView {
                code: code.into(),
                message: message.into(),
            }],
        }
    }
}

pub(crate) async fn live_status_workspace(bridge: Arc<DaemonBridge>) -> LiveStatusWorkspaceView {
    match bridge
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .live_status(LiveStatusRequest)
            .map_err(|error| error.to_string())
        })
        .await
    {
        Ok(snapshot) => workspace_from_daemon(snapshot),
        Err(error) => degraded_from_bridge(error),
    }
}

pub(crate) fn workspace_from_daemon(snapshot: LiveStatusResponse) -> LiveStatusWorkspaceView {
    let mut hosts: BTreeMap<String, (u32, BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    let mut stores: BTreeMap<String, (u32, BTreeSet<String>)> = BTreeMap::new();
    let mut ssd_ingests = Vec::new();
    let mut hdd_transfers = Vec::new();

    for ingest in &snapshot.active {
        let host = ingest
            .actor
            .as_ref()
            .map(|actor| actor.display_name.clone());
        let store_id = ingest.progress.endpoint.to_string();
        {
            let entry = hosts.entry(ingest.host_display_name.clone()).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1.insert(store_id.clone());
            if let Some(actor) = host.as_ref() {
                entry.2.insert(actor.clone());
            }
        }
        let store = stores.entry(store_id.clone()).or_default();
        store.0 = store.0.saturating_add(1);
        if let Some(host) = host {
            store.1.insert(host);
        }
        if matches!(
            ingest.progress.stage,
            DaemonIngestStage::Queued | DaemonIngestStage::SsdIngest
        ) {
            ssd_ingests.push(progress_from_daemon(ingest));
        }
        hdd_transfers.extend(hdd_transfers_from_daemon(ingest));
    }

    let garbage_collection =
        snapshot
            .garbage_collection
            .as_ref()
            .map(|collection| LiveStatusGarbageCollectionView {
                running: collection.running,
                last_completed_at_utc: collection.last_completed_at_utc.clone(),
                scanned_bytes: collection.scanned_bytes,
                reclaimed_bytes: collection.reclaimed_bytes,
                retained_items: collection.retained_items,
                retained_reasons: collection
                    .retained_reasons
                    .iter()
                    .map(|retained| LiveStatusWarningView {
                        code: format!("garbage_collection.{}", retained.category),
                        message: format!(
                            "{} item(s), {} byte(s): {}",
                            retained.items, retained.bytes, retained.reason
                        ),
                    })
                    .collect(),
            });
    let warnings = snapshot
        .garbage_collection
        .as_ref()
        .and_then(|collection| collection.last_error.as_ref())
        .map(|message| {
            vec![LiveStatusWarningView {
                code: "garbage_collection.failed_closed".to_string(),
                message: message.clone(),
            }]
        })
        .unwrap_or_default();

    LiveStatusWorkspaceView {
        schema_version: LIVE_STATUS_VIEW_SCHEMA_VERSION,
        availability: LiveStatusAvailabilityView::Available,
        sequence: snapshot.sequence,
        generated_at_utc: Some(snapshot.generated_at_utc),
        suggested_refresh_millis: LIVE_STATUS_SUGGESTED_REFRESH_MILLIS,
        aggregate: LiveStatusAggregateView {
            connected_hosts: snapshot.aggregate.connected_hosts,
            active_stores: snapshot.aggregate.active_stores,
            active_ingests: snapshot.aggregate.active_ingests,
            source_read_bytes_per_second: snapshot.aggregate.source_read_bytes_per_second,
            ssd_write_bytes_per_second: snapshot.aggregate.ssd_write_bytes_per_second,
            hdd_write_bytes_per_second: snapshot.aggregate.hdd_write_bytes_per_second,
            active_hdd_transfers: snapshot.aggregate.active_hdd_transfers,
        },
        hosts: hosts
            .into_iter()
            .map(
                |(display_name, (active_ingests, stores, actors))| LiveStatusHostView {
                    display_name,
                    actors: actors.into_iter().collect(),
                    active_ingests,
                    object_stores: stores.into_iter().collect(),
                },
            )
            .collect(),
        store_writers: stores
            .into_iter()
            .map(
                |(store_id, (active_ingests, hosts))| LiveStatusStoreWriterView {
                    store_id,
                    hosts: hosts.into_iter().collect(),
                    active_ingests,
                },
            )
            .collect(),
        ssd_ingests,
        hdd_transfers,
        recent: snapshot.recent.iter().map(progress_from_daemon).collect(),
        garbage_collection,
        warnings,
    }
}

fn progress_from_daemon(ingest: &LiveStatusIngest) -> LiveStatusProgressView {
    let throughput = ingest
        .progress
        .telemetry
        .map(|telemetry| telemetry.throughput.current_bytes_per_second)
        .unwrap_or_default();
    LiveStatusProgressView {
        job_id: ingest.progress.job_id.to_string(),
        store_id: ingest.progress.endpoint.to_string(),
        host: ingest
            .actor
            .as_ref()
            .map(|actor| actor.display_name.clone()),
        state: stage_name(&ingest.progress.stage).to_string(),
        pipeline_stage: ingest
            .progress
            .pipeline_stage
            .map(|stage| format!("{stage:?}").to_ascii_lowercase()),
        current_item: ingest
            .progress
            .current_object_id
            .as_ref()
            .map(ToString::to_string)
            .and_then(|value| private_basename(&value)),
        bytes_done: ingest.progress.work_bytes_done,
        bytes_total: ingest.progress.work_bytes_total,
        files_done: ingest.progress.files_done,
        files_total: ingest.progress.files_total,
        bytes_per_second: throughput,
        updated_at_utc: ingest.updated_at_utc.clone(),
    }
}

fn hdd_transfers_from_daemon(ingest: &LiveStatusIngest) -> Vec<LiveStatusHddTransferView> {
    ingest
        .progress
        .active_hdd_transfers
        .iter()
        .map(|transfer| LiveStatusHddTransferView {
            job_id: ingest.progress.job_id.to_string(),
            store_id: ingest.progress.endpoint.to_string(),
            disk_id: transfer.disk_id.to_string(),
            copy_number: transfer.copy_number,
            current_item: private_basename(&transfer.relative_path)
                .unwrap_or_else(|| "item".into()),
            bytes_done: transfer.bytes_done,
            bytes_total: transfer.bytes_total,
            bytes_per_second: transfer.bytes_per_second,
            phase: match transfer.phase {
                DaemonIngestHddTransferPhase::Writing => "writing",
                DaemonIngestHddTransferPhase::Fsync => "fsync",
                DaemonIngestHddTransferPhase::Rename => "rename",
            }
            .to_string(),
        })
        .collect()
}

fn stage_name(stage: &DaemonIngestStage) -> &'static str {
    match stage {
        DaemonIngestStage::Queued => "queued",
        DaemonIngestStage::SsdIngest => "ssd_ingest",
        DaemonIngestStage::HddCopy { .. } => "hdd_copy",
        DaemonIngestStage::Complete => "complete",
        DaemonIngestStage::Failed => "failed",
        DaemonIngestStage::Cancelled => "cancelled",
    }
}

fn private_basename(value: &str) -> Option<String> {
    value
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .map(str::to_string)
}

fn degraded_from_bridge(error: DaemonBridgeError) -> LiveStatusWorkspaceView {
    let (code, message) = match error {
        DaemonBridgeError::Busy => (
            "live_status_busy",
            "Live status capacity is busy; the dashboard will retry automatically.",
        ),
        DaemonBridgeError::CircuitOpen => (
            "live_status_temporarily_unavailable",
            "Live status is temporarily unavailable; the dashboard will retry automatically.",
        ),
        DaemonBridgeError::Deadline => (
            "live_status_deadline_exceeded",
            "Live status exceeded its deadline; the dashboard will retry automatically.",
        ),
        DaemonBridgeError::Join(_) | DaemonBridgeError::Client(_) => (
            "live_status_daemon_unavailable",
            "The DASObjectStore daemon is not currently providing live status.",
        ),
    };
    LiveStatusWorkspaceView::degraded(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
    use dasobjectstore_daemon::{
        DaemonIngestHddActiveTransfer, DaemonIngestProgressEvent, LiveStatusActor,
        LiveStatusAggregate, LiveStatusConnectionOrigin,
    };

    fn ingest() -> LiveStatusIngest {
        LiveStatusIngest {
            sequence: 8,
            updated_at_utc: "2026-07-19T12:00:00Z".into(),
            origin: LiveStatusConnectionOrigin::LocalUnixSocket,
            host_display_name: "DAS appliance".into(),
            actor: Some(LiveStatusActor {
                uid: 1000,
                display_name: "workstation-a".into(),
            }),
            progress: DaemonIngestProgressEvent {
                job_id: IngestJobId::new("ingest-8").unwrap(),
                endpoint: StoreId::new("research").unwrap(),
                stage: DaemonIngestStage::SsdIngest,
                pipeline_stage: None,
                work_bytes_done: 50,
                work_bytes_total: Some(100),
                source_bytes_done: Some(50),
                source_bytes_total: Some(100),
                stage_bytes_done: Some(50),
                stage_bytes_total: Some(100),
                files_done: 1,
                files_total: Some(2),
                current_object_id: Some(ObjectId::new("private/folder/sample.fast5").unwrap()),
                ssd_pressure: None,
                telemetry: None,
                active_hdd_transfers: vec![DaemonIngestHddActiveTransfer {
                    file_index: 1,
                    files_total: Some(2),
                    object_id: ObjectId::new("private/folder/sample.fast5").unwrap(),
                    relative_path: "/secret/source/private/folder/sample.fast5".into(),
                    disk_id: DiskId::new("disk-1").unwrap(),
                    copy_number: 1,
                    bytes_done: 25,
                    bytes_total: 100,
                    bytes_per_second: 10,
                    phase: DaemonIngestHddTransferPhase::Writing,
                    fsync_duration_millis: None,
                    rename_duration_millis: None,
                }],
                resource_policy: None,
                message: Some("failure at /secret/source/private/folder/sample.fast5".into()),
            },
        }
    }

    #[test]
    fn aggregation_builds_stable_host_store_and_progress_views() {
        let item = ingest();
        let view = workspace_from_daemon(LiveStatusResponse {
            schema_version: 1,
            sequence: 8,
            generated_at_utc: "2026-07-19T12:00:01Z".into(),
            aggregate: LiveStatusAggregate {
                connected_hosts: 1,
                active_stores: 1,
                active_ingests: 1,
                source_read_bytes_per_second: 12,
                ssd_write_bytes_per_second: 11,
                hdd_write_bytes_per_second: 10,
                active_hdd_transfers: 1,
            },
            active: vec![item],
            recent: Vec::new(),
            garbage_collection: None,
        });
        assert_eq!(view.availability, LiveStatusAvailabilityView::Available);
        assert_eq!(view.suggested_refresh_millis, 1_000);
        assert_eq!(view.hosts[0].object_stores, ["research"]);
        assert_eq!(view.store_writers[0].hosts, ["workstation-a"]);
        assert_eq!(
            view.ssd_ingests[0].current_item.as_deref(),
            Some("sample.fast5")
        );
        assert_eq!(view.hdd_transfers[0].disk_id, "disk-1");
    }

    #[test]
    fn serialized_view_never_exposes_paths_uids_or_daemon_messages() {
        let view = workspace_from_daemon(LiveStatusResponse {
            schema_version: 1,
            sequence: 8,
            generated_at_utc: "now".into(),
            aggregate: LiveStatusAggregate::default(),
            active: vec![ingest()],
            recent: Vec::new(),
            garbage_collection: None,
        });
        let encoded = serde_json::to_string(&view).unwrap();
        assert!(!encoded.contains("/secret/source"));
        assert!(!encoded.contains("private/folder"));
        assert!(!encoded.contains("\"uid\""));
        assert!(!encoded.contains("failure at"));
        assert!(encoded.contains("sample.fast5"));
    }

    #[test]
    fn degraded_view_is_explicit_and_safe_to_poll() {
        let view = LiveStatusWorkspaceView::degraded("offline", "daemon unavailable");
        assert_eq!(view.availability, LiveStatusAvailabilityView::Degraded);
        assert_eq!(view.suggested_refresh_millis, 1_000);
        assert!(view.ssd_ingests.is_empty());
        assert_eq!(view.warnings[0].code, "offline");
    }
}
