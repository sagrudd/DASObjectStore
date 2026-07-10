//! Bounded daemon progress delivery outside the ingest I/O path.

use super::DaemonIngestFilesRuntimeError;
use crate::api::{DaemonIngestHddActiveTransfer, DaemonIngestProgressEvent, DaemonIngestStage};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use std::time::{Duration, Instant};

const PROGRESS_BYTE_CADENCE: u64 = 1024 * 1024;
const PROGRESS_TIME_CADENCE: Duration = Duration::from_millis(100);

pub(super) struct IngestProgressCoalescer<F> {
    emit: F,
    last_emitted_at: Option<Instant>,
    last_emitted_bytes: u64,
    last_phase: Option<ProgressPhase>,
    pending: Option<DaemonIngestProgressEvent>,
}

impl<F> IngestProgressCoalescer<F>
where
    F: FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
{
    pub(super) fn new(emit: F) -> Self {
        Self {
            emit,
            last_emitted_at: None,
            last_emitted_bytes: 0,
            last_phase: None,
            pending: None,
        }
    }

    pub(super) fn publish(
        &mut self,
        event: DaemonIngestProgressEvent,
    ) -> Result<(), DaemonIngestFilesRuntimeError> {
        let phase = ProgressPhase::from_event(&event);
        if self.last_phase.as_ref() != Some(&phase)
            || is_terminal(&event.stage)
            || self.byte_cadence_reached(&event)
            || self.time_cadence_reached()
        {
            self.pending = None;
            return self.emit(event, phase);
        }

        self.pending = Some(event);
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<(), DaemonIngestFilesRuntimeError> {
        if let Some(event) = self.pending.take() {
            let phase = ProgressPhase::from_event(&event);
            self.emit(event, phase)?;
        }
        Ok(())
    }

    fn emit(
        &mut self,
        event: DaemonIngestProgressEvent,
        phase: ProgressPhase,
    ) -> Result<(), DaemonIngestFilesRuntimeError> {
        self.last_emitted_at = Some(Instant::now());
        self.last_emitted_bytes = progress_bytes(&event);
        self.last_phase = Some(phase);
        (self.emit)(event)
    }

    fn byte_cadence_reached(&self, event: &DaemonIngestProgressEvent) -> bool {
        progress_bytes(event).saturating_sub(self.last_emitted_bytes) >= PROGRESS_BYTE_CADENCE
    }

    fn time_cadence_reached(&self) -> bool {
        self.last_emitted_at
            .is_none_or(|at| at.elapsed() >= PROGRESS_TIME_CADENCE)
    }
}

fn progress_bytes(event: &DaemonIngestProgressEvent) -> u64 {
    event
        .source_bytes_done
        .unwrap_or(0)
        .max(event.work_bytes_done)
}

fn is_terminal(stage: &DaemonIngestStage) -> bool {
    matches!(
        stage,
        DaemonIngestStage::Complete | DaemonIngestStage::Failed | DaemonIngestStage::Cancelled
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgressPhase {
    stage: DaemonIngestStage,
    pipeline_stage: Option<crate::api::DaemonIngestPipelineStage>,
    current_object_id: Option<ObjectId>,
    active_targets: Vec<(DiskId, u8)>,
}

impl ProgressPhase {
    fn from_event(event: &DaemonIngestProgressEvent) -> Self {
        Self {
            stage: event.stage.clone(),
            pipeline_stage: event.pipeline_stage,
            current_object_id: event.current_object_id.clone(),
            active_targets: active_targets(&event.active_hdd_transfers),
        }
    }
}

fn active_targets(transfers: &[DaemonIngestHddActiveTransfer]) -> Vec<(DiskId, u8)> {
    transfers
        .iter()
        .map(|transfer| (transfer.disk_id.clone(), transfer.copy_number))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::IngestProgressCoalescer;
    use crate::api::{DaemonIngestPipelineStage, DaemonIngestProgressEvent, DaemonIngestStage};
    use dasobjectstore_core::ids::{IngestJobId, StoreId};

    #[test]
    fn coalesces_byte_callbacks_but_preserves_phase_transitions_and_final_frame() {
        let mut emitted = Vec::new();
        {
            let mut coalescer = IngestProgressCoalescer::new(|event| {
                emitted.push(event);
                Ok(())
            });
            coalescer
                .publish(event(0, DaemonIngestStage::SsdIngest))
                .expect("initial frame");
            coalescer
                .publish(event(64 * 1024, DaemonIngestStage::SsdIngest))
                .expect("small callback is coalesced");
            coalescer
                .publish(event(1024 * 1024, DaemonIngestStage::SsdIngest))
                .expect("byte cadence frame");
            coalescer
                .publish(event(1024 * 1024 + 64 * 1024, DaemonIngestStage::Complete))
                .expect("terminal frame");
        }

        assert_eq!(emitted.len(), 3);
        assert_eq!(emitted[1].work_bytes_done, 1024 * 1024);
        assert_eq!(emitted[2].stage, DaemonIngestStage::Complete);
    }

    #[test]
    fn flushes_the_latest_pending_progress_frame() {
        let mut emitted = Vec::new();
        {
            let mut coalescer = IngestProgressCoalescer::new(|event| {
                emitted.push(event);
                Ok(())
            });
            coalescer
                .publish(event(0, DaemonIngestStage::SsdIngest))
                .expect("initial frame");
            coalescer
                .publish(event(64 * 1024, DaemonIngestStage::SsdIngest))
                .expect("small callback is pending");
            coalescer.flush().expect("flush succeeds");
        }

        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[1].work_bytes_done, 64 * 1024);
    }

    fn event(work_bytes_done: u64, stage: DaemonIngestStage) -> DaemonIngestProgressEvent {
        DaemonIngestProgressEvent {
            job_id: IngestJobId::new("ingest-progress-test").expect("job id"),
            endpoint: StoreId::new("test-store").expect("store id"),
            stage,
            pipeline_stage: Some(DaemonIngestPipelineStage::SsdStage),
            work_bytes_done,
            work_bytes_total: Some(2 * 1024 * 1024),
            source_bytes_done: Some(work_bytes_done),
            source_bytes_total: Some(2 * 1024 * 1024),
            stage_bytes_done: Some(work_bytes_done),
            stage_bytes_total: Some(2 * 1024 * 1024),
            files_done: 0,
            files_total: Some(1),
            current_object_id: None,
            ssd_pressure: None,
            telemetry: None,
            active_hdd_transfers: Vec::new(),
            resource_policy: None,
            message: None,
        }
    }
}
