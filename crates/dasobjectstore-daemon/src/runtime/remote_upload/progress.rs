use super::super::{admin_jobs::AdminJobRegistry, service::DaemonServiceRuntimeError};
use super::{
    non_blank, RemoteUploadAdmissionGate, RemoteUploadRuntimeSnapshot,
    RemoteUploadS3TransferJobOutcome, RemoteUploadS3TransferJobSummary,
};
use crate::api::{
    DaemonJobEvent, DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobProgress,
    DaemonJobState, DaemonJobSummary,
};
use dasobjectstore_core::remote_upload::RemoteUploadBackpressureAction;
use dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds;
use std::sync::Arc;

pub struct RemoteUploadS3TransferProgressReporter<'a> {
    registry: &'a dyn AdminJobRegistry,
    gate: Arc<RemoteUploadAdmissionGate>,
    job_id: DaemonJobId,
    object_store: String,
    source_bytes: u64,
    submitted_at_utc: String,
    started_at_utc: String,
    actor: Option<String>,
    events: Vec<DaemonJobEvent>,
}

impl RemoteUploadS3TransferProgressReporter<'_> {
    pub fn record_progress(
        &mut self,
        update: RemoteUploadS3TransferProgressUpdate,
    ) -> Result<DaemonJobEvent, DaemonServiceRuntimeError> {
        let bytes_done = update.bytes_done.min(self.source_bytes);
        let updated_at_utc = update.updated_at_utc;
        let telemetry = remote_upload_progress_telemetry_with_s3_rate(
            update.telemetry,
            bytes_done,
            &self.started_at_utc,
            &updated_at_utc,
        );
        let telemetry =
            remote_upload_progress_telemetry_with_runtime_snapshot(telemetry, self.gate.snapshot());
        let message = remote_upload_progress_message(
            update.message.unwrap_or_else(|| {
                format!(
                    "remote upload S3 transfer running for {}",
                    self.object_store
                )
            }),
            telemetry.as_ref(),
        );
        let job = DaemonJobSummary {
            job_id: self.job_id.clone(),
            kind: DaemonJobKind::RemoteUpload,
            state: DaemonJobState::Running,
            progress: DaemonJobProgress {
                stage: "remote_s3_transfer_running".to_string(),
                work_bytes_done: bytes_done,
                work_bytes_total: self.source_bytes,
                work_units_done: 0,
                work_units_total: 1,
                message: Some(message),
            },
            submitted_at_utc: self.submitted_at_utc.clone(),
            updated_at_utc,
            actor: self.actor.clone(),
            failure_message: None,
        };
        let event = daemon_job_event_for_summary(job.clone());
        self.registry.record(job)?;
        self.events.push(event.clone());
        Ok(event)
    }

    pub(super) fn into_events(self) -> Vec<DaemonJobEvent> {
        self.events
    }
}

impl<'a> RemoteUploadS3TransferProgressReporter<'a> {
    pub(super) fn new(
        registry: &'a dyn AdminJobRegistry,
        gate: Arc<RemoteUploadAdmissionGate>,
        job_id: String,
        object_store: String,
        source_bytes: u64,
        submitted_at_utc: String,
        started_at_utc: String,
        actor: Option<String>,
    ) -> Result<Self, DaemonServiceRuntimeError> {
        Ok(Self {
            registry,
            gate,
            job_id: DaemonJobId::new(job_id.clone())
                .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job_id))?,
            object_store,
            source_bytes,
            submitted_at_utc,
            started_at_utc,
            actor,
            events: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3TransferProgressUpdate {
    pub bytes_done: u64,
    pub updated_at_utc: String,
    pub telemetry: Option<RemoteUploadProgressTelemetry>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemoteUploadProgressTelemetry {
    pub source_scan_count: Option<u64>,
    pub staged_bytes: Option<u64>,
    pub s3_bytes_per_second: Option<u64>,
    pub ssd_queue_depth: Option<u32>,
    pub hdd_landing_queue_depth: Option<u32>,
    pub active_hdd_writers: Option<u16>,
    pub verification_state: Option<String>,
    pub session_renewal_status: Option<String>,
}

impl RemoteUploadProgressTelemetry {
    pub fn is_empty(&self) -> bool {
        self.source_scan_count.is_none()
            && self.staged_bytes.is_none()
            && self.s3_bytes_per_second.is_none()
            && self.ssd_queue_depth.is_none()
            && self.hdd_landing_queue_depth.is_none()
            && self.active_hdd_writers.is_none()
            && self.verification_state.is_none()
            && self.session_renewal_status.is_none()
    }
}

impl RemoteUploadS3TransferJobSummary {
    pub fn daemon_job_summary(
        &self,
        submitted_at_utc: impl Into<String>,
        updated_at_utc: impl Into<String>,
        actor: Option<String>,
    ) -> Result<DaemonJobSummary, DaemonJobIdError> {
        let job_id = DaemonJobId::new(self.job_id.clone())?;
        let state = self.daemon_job_state();
        let failure_message = self.failure_message();

        Ok(DaemonJobSummary {
            job_id,
            kind: DaemonJobKind::RemoteUpload,
            state,
            progress: self.daemon_job_progress(),
            submitted_at_utc: submitted_at_utc.into(),
            updated_at_utc: updated_at_utc.into(),
            actor,
            failure_message,
        })
    }

    pub fn daemon_job_event(
        &self,
        submitted_at_utc: impl Into<String>,
        updated_at_utc: impl Into<String>,
        actor: Option<String>,
    ) -> Result<DaemonJobEvent, DaemonJobIdError> {
        let job = self.daemon_job_summary(submitted_at_utc, updated_at_utc, actor)?;
        Ok(daemon_job_event_for_summary(job))
    }

    fn daemon_job_state(&self) -> DaemonJobState {
        match &self.outcome {
            RemoteUploadS3TransferJobOutcome::Completed => DaemonJobState::Complete,
            RemoteUploadS3TransferJobOutcome::Failed { .. } => DaemonJobState::Failed,
            RemoteUploadS3TransferJobOutcome::NotAdmitted(decision) => match decision.action {
                RemoteUploadBackpressureAction::Accept
                | RemoteUploadBackpressureAction::PauseNewTransfers => DaemonJobState::Waiting,
                RemoteUploadBackpressureAction::RejectNewTransfers => DaemonJobState::Failed,
            },
        }
    }

    fn daemon_job_progress(&self) -> DaemonJobProgress {
        match &self.outcome {
            RemoteUploadS3TransferJobOutcome::Completed => DaemonJobProgress {
                stage: "remote_s3_transfer_complete".to_string(),
                work_bytes_done: self.source_bytes,
                work_bytes_total: self.source_bytes,
                work_units_done: 1,
                work_units_total: 1,
                message: Some(format!(
                    "remote upload S3 transfer completed for {}",
                    self.object_store
                )),
            },
            RemoteUploadS3TransferJobOutcome::Failed { error } => DaemonJobProgress {
                stage: "remote_s3_transfer_failed".to_string(),
                work_bytes_done: 0,
                work_bytes_total: self.source_bytes,
                work_units_done: 0,
                work_units_total: 1,
                message: Some(error.clone()),
            },
            RemoteUploadS3TransferJobOutcome::NotAdmitted(decision) => DaemonJobProgress {
                stage: match decision.action {
                    RemoteUploadBackpressureAction::RejectNewTransfers => {
                        "remote_s3_admission_rejected"
                    }
                    RemoteUploadBackpressureAction::Accept
                    | RemoteUploadBackpressureAction::PauseNewTransfers => {
                        "remote_s3_admission_wait"
                    }
                }
                .to_string(),
                work_bytes_done: 0,
                work_bytes_total: self.source_bytes,
                work_units_done: 0,
                work_units_total: 1,
                message: Some(decision.message.clone()),
            },
        }
    }

    fn failure_message(&self) -> Option<String> {
        match &self.outcome {
            RemoteUploadS3TransferJobOutcome::Failed { error } => Some(error.clone()),
            RemoteUploadS3TransferJobOutcome::NotAdmitted(decision)
                if decision.action == RemoteUploadBackpressureAction::RejectNewTransfers =>
            {
                Some(decision.message.clone())
            }
            _ => None,
        }
    }
}

pub(super) fn daemon_job_event_for_summary(job: DaemonJobSummary) -> DaemonJobEvent {
    match job.state {
        DaemonJobState::Complete => DaemonJobEvent::Complete(job),
        DaemonJobState::Failed => DaemonJobEvent::Failed(job),
        _ => DaemonJobEvent::Progress(job),
    }
}

fn remote_upload_progress_message(
    base: String,
    telemetry: Option<&RemoteUploadProgressTelemetry>,
) -> String {
    let Some(telemetry) = telemetry.filter(|telemetry| !telemetry.is_empty()) else {
        return base;
    };
    let mut fields = Vec::new();
    if let Some(value) = telemetry.source_scan_count {
        fields.push(format!("source_files={value}"));
    }
    if let Some(value) = telemetry.staged_bytes {
        fields.push(format!("staged_bytes={value}"));
    }
    if let Some(value) = telemetry.s3_bytes_per_second {
        fields.push(format!("s3_bytes_per_second={value}"));
    }
    if let Some(value) = telemetry.ssd_queue_depth {
        fields.push(format!("ssd_queue_depth={value}"));
    }
    if let Some(value) = telemetry.hdd_landing_queue_depth {
        fields.push(format!("hdd_landing_queue_depth={value}"));
    }
    if let Some(value) = telemetry.active_hdd_writers {
        fields.push(format!("active_hdd_writers={value}"));
    }
    if let Some(value) = non_blank(telemetry.verification_state.as_deref()) {
        fields.push(format!("verification_state={value}"));
    }
    if let Some(value) = non_blank(telemetry.session_renewal_status.as_deref()) {
        fields.push(format!("session_renewal_status={value}"));
    }
    if fields.is_empty() {
        base
    } else {
        format!("{base} | {}", fields.join(" "))
    }
}

fn remote_upload_progress_telemetry_with_s3_rate(
    mut telemetry: Option<RemoteUploadProgressTelemetry>,
    bytes_done: u64,
    started_at_utc: &str,
    updated_at_utc: &str,
) -> Option<RemoteUploadProgressTelemetry> {
    if telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.s3_bytes_per_second)
        .is_some()
    {
        return telemetry;
    }
    let Some(rate) = remote_upload_s3_bytes_per_second(bytes_done, started_at_utc, updated_at_utc)
    else {
        return telemetry;
    };
    match &mut telemetry {
        Some(telemetry) => telemetry.s3_bytes_per_second = Some(rate),
        None => {
            telemetry = Some(RemoteUploadProgressTelemetry {
                s3_bytes_per_second: Some(rate),
                ..RemoteUploadProgressTelemetry::default()
            });
        }
    }
    telemetry
}

fn remote_upload_progress_telemetry_with_runtime_snapshot(
    mut telemetry: Option<RemoteUploadProgressTelemetry>,
    snapshot: RemoteUploadRuntimeSnapshot,
) -> Option<RemoteUploadProgressTelemetry> {
    if snapshot.ssd_stage_queue_depth == 0
        && snapshot.hdd_landing_queue_depth == 0
        && snapshot.active_hdd_writers == 0
        && snapshot.verification_queue_depth == 0
    {
        return telemetry;
    }
    let fields = telemetry.get_or_insert_with(RemoteUploadProgressTelemetry::default);
    if fields.ssd_queue_depth.is_none() && snapshot.ssd_stage_queue_depth > 0 {
        fields.ssd_queue_depth = Some(snapshot.ssd_stage_queue_depth);
    }
    if fields.hdd_landing_queue_depth.is_none() && snapshot.hdd_landing_queue_depth > 0 {
        fields.hdd_landing_queue_depth = Some(snapshot.hdd_landing_queue_depth);
    }
    if fields.active_hdd_writers.is_none() && snapshot.active_hdd_writers > 0 {
        fields.active_hdd_writers = Some(snapshot.active_hdd_writers);
    }
    if fields.verification_state.is_none() && snapshot.verification_queue_depth > 0 {
        fields.verification_state = Some("pending".to_string());
    }
    telemetry
}

fn remote_upload_s3_bytes_per_second(
    bytes_done: u64,
    started_at_utc: &str,
    updated_at_utc: &str,
) -> Option<u64> {
    let started = parse_canonical_utc_timestamp_seconds(started_at_utc.trim())?;
    let updated = parse_canonical_utc_timestamp_seconds(updated_at_utc.trim())?;
    let elapsed = u64::try_from(updated.checked_sub(started)?).ok()?;
    if elapsed == 0 {
        return None;
    }
    let rate = bytes_done / elapsed;
    (rate > 0).then_some(rate)
}
