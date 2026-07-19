use crate::api::{
    DaemonIngestProgressEvent, DaemonIngestStage, LiveStatusActor, LiveStatusAggregate,
    LiveStatusConnectionOrigin, LiveStatusGarbageCollection, LiveStatusIngest, LiveStatusResponse,
    LIVE_STATUS_SCHEMA_VERSION,
};
use crate::auth::DaemonLocalActor;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

const MAX_RECENT_TERMINAL_INGESTS: usize = 64;

#[derive(Debug, Default)]
struct LiveStatusState {
    sequence: u64,
    active: BTreeMap<String, LiveStatusIngest>,
    recent: VecDeque<LiveStatusIngest>,
    garbage_collection: Option<LiveStatusGarbageCollection>,
}

#[derive(Debug, Default)]
pub struct LiveStatusRegistry {
    state: Mutex<LiveStatusState>,
}

impl LiveStatusRegistry {
    pub fn record_garbage_collection(&self, report: LiveStatusGarbageCollection) {
        let mut state = self
            .state
            .lock()
            .expect("live status registry lock poisoned");
        state.sequence = state.sequence.saturating_add(1);
        state.garbage_collection = Some(report);
    }
    pub fn record(
        &self,
        mut progress: DaemonIngestProgressEvent,
        actor: Option<&DaemonLocalActor>,
        updated_at_utc: impl Into<String>,
    ) {
        // Runtime messages can include host source paths. The dashboard contract deliberately
        // carries structured progress only and never republishes those free-form diagnostics.
        progress.message = None;
        let terminal = matches!(
            progress.stage,
            DaemonIngestStage::Complete | DaemonIngestStage::Failed | DaemonIngestStage::Cancelled
        );
        let key = progress.job_id.to_string();
        let mut state = self
            .state
            .lock()
            .expect("live status registry lock poisoned");
        state.sequence = state.sequence.saturating_add(1);
        let item = LiveStatusIngest {
            sequence: state.sequence,
            updated_at_utc: updated_at_utc.into(),
            origin: LiveStatusConnectionOrigin::LocalUnixSocket,
            host_display_name: local_appliance_name(),
            actor: actor.map(|actor| LiveStatusActor {
                uid: actor.uid,
                display_name: actor.display_name(),
            }),
            progress,
        };
        if terminal {
            state.active.remove(&key);
            state.recent.push_front(item);
            state.recent.truncate(MAX_RECENT_TERMINAL_INGESTS);
        } else {
            state.active.insert(key, item);
        }
    }

    pub fn snapshot(&self, generated_at_utc: impl Into<String>) -> LiveStatusResponse {
        let state = self
            .state
            .lock()
            .expect("live status registry lock poisoned");
        let active = state.active.values().cloned().collect::<Vec<_>>();
        let mut hosts = BTreeSet::new();
        let mut stores = BTreeSet::new();
        let mut aggregate = LiveStatusAggregate {
            active_ingests: active.len().try_into().unwrap_or(u32::MAX),
            ..LiveStatusAggregate::default()
        };
        for item in &active {
            hosts.insert(item.host_display_name.clone());
            stores.insert(item.progress.endpoint.to_string());
            aggregate.active_hdd_transfers = aggregate.active_hdd_transfers.saturating_add(
                item.progress
                    .active_hdd_transfers
                    .len()
                    .try_into()
                    .unwrap_or(u32::MAX),
            );
            if let Some(telemetry) = item.progress.telemetry {
                aggregate.source_read_bytes_per_second = aggregate
                    .source_read_bytes_per_second
                    .saturating_add(telemetry.throughput.source_read_bytes_per_second);
                aggregate.ssd_write_bytes_per_second = aggregate
                    .ssd_write_bytes_per_second
                    .saturating_add(telemetry.throughput.ssd_write_bytes_per_second);
                aggregate.hdd_write_bytes_per_second = aggregate
                    .hdd_write_bytes_per_second
                    .saturating_add(telemetry.throughput.aggregate_hdd_write_bytes_per_second);
            }
        }
        aggregate.connected_hosts = hosts.len().try_into().unwrap_or(u32::MAX);
        aggregate.active_stores = stores.len().try_into().unwrap_or(u32::MAX);
        LiveStatusResponse {
            schema_version: LIVE_STATUS_SCHEMA_VERSION,
            sequence: state.sequence,
            generated_at_utc: generated_at_utc.into(),
            aggregate,
            active,
            recent: state.recent.iter().cloned().collect(),
            garbage_collection: state.garbage_collection.clone(),
        }
    }
}

fn local_appliance_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "DAS appliance".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::{IngestJobId, StoreId};

    fn event(stage: DaemonIngestStage) -> DaemonIngestProgressEvent {
        DaemonIngestProgressEvent {
            job_id: IngestJobId::new("job-live").unwrap(),
            endpoint: StoreId::new("research").unwrap(),
            stage,
            pipeline_stage: None,
            work_bytes_done: 10,
            work_bytes_total: Some(20),
            source_bytes_done: None,
            source_bytes_total: None,
            stage_bytes_done: None,
            stage_bytes_total: None,
            files_done: 1,
            files_total: Some(2),
            current_object_id: None,
            ssd_pressure: None,
            telemetry: None,
            active_hdd_transfers: Vec::new(),
            resource_policy: None,
            message: Some("failed at /private/source/secret.fast5".into()),
        }
    }

    #[test]
    fn terminal_progress_moves_to_bounded_recent_history_and_redacts_messages() {
        let registry = LiveStatusRegistry::default();
        let actor = DaemonLocalActor::new(1000).with_username("writer");
        registry.record(
            event(DaemonIngestStage::SsdIngest),
            Some(&actor),
            "2026-01-01T00:00:00Z",
        );
        let active = registry.snapshot("2026-01-01T00:00:01Z");
        assert_eq!(active.aggregate.active_ingests, 1);
        assert_eq!(active.aggregate.connected_hosts, 1);
        assert_eq!(active.active[0].progress.message, None);

        registry.record(
            event(DaemonIngestStage::Complete),
            Some(&actor),
            "2026-01-01T00:00:02Z",
        );
        let complete = registry.snapshot("2026-01-01T00:00:03Z");
        assert!(complete.active.is_empty());
        assert_eq!(complete.recent.len(), 1);
        assert_eq!(complete.sequence, 2);
    }

    #[test]
    fn garbage_collection_status_is_sequenced_and_path_free() {
        let registry = LiveStatusRegistry::default();
        registry.record_garbage_collection(LiveStatusGarbageCollection {
            reclaimed_bytes: 42,
            retained_items: 1,
            retained_reasons: vec![crate::api::LiveStatusGarbageCollectionRetained {
                category: "reconciliation".to_string(),
                reason: "incomplete resumable manifest".to_string(),
                items: 1,
                bytes: 7,
            }],
            ..LiveStatusGarbageCollection::default()
        });
        let snapshot = registry.snapshot("2026-01-01T00:00:00Z");
        assert_eq!(snapshot.sequence, 1);
        let collection = snapshot.garbage_collection.expect("collection status");
        assert_eq!(collection.reclaimed_bytes, 42);
        assert_eq!(collection.retained_reasons[0].category, "reconciliation");
        assert!(!serde_json::to_string(&collection)
            .expect("serialize")
            .contains("/srv/"));
    }
}
