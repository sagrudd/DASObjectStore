use super::super::capacity_provider::CapacityAdmissionProvider;
use super::DaemonIngestFilesRuntimeError;
use super::LocalFileIngestExecutor;
use super::SubmitIngestFilesRequest;
use super::SubmitIngestFilesResponse;
use crate::api::{
    DaemonIngestProgressEvent, DaemonIngestResourceBudget, DaemonIngestResourceGate,
    DaemonIngestResourcePolicy, DaemonIngestResourceReservation,
    DaemonIngestResourceReservationError,
};
use std::sync::{Arc, OnceLock};

const INGEST_RESOURCE_MEMORY_RESERVATION_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn submit_ingest_files_with_resource_gate(
    request: SubmitIngestFilesRequest,
    accepted_at_utc: &str,
    progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    capacity_provider: Option<Arc<dyn CapacityAdmissionProvider>>,
    resource_gate: Option<Arc<DaemonIngestResourceGate>>,
) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
    let mut executor = LocalFileIngestExecutor::from_environment()
        .with_capacity_admission_provider(capacity_provider);
    executor.resource_gate = resource_gate;
    let mut progress = super::progress::IngestProgressCoalescer::new(progress);
    let response = executor.submit(request, accepted_at_utc, |event| progress.publish(event))?;
    progress.flush()?;
    Ok(response)
}

pub(super) fn reserve_ingest_resources(
    configured_gate: Option<Arc<DaemonIngestResourceGate>>,
) -> Result<crate::api::DaemonIngestResourceLease, DaemonIngestResourceReservationError> {
    configured_gate
        .unwrap_or_else(shared_ingest_resource_gate)
        .try_reserve(DaemonIngestResourceReservation {
            cpu_cores: 1,
            memory_bytes: INGEST_RESOURCE_MEMORY_RESERVATION_BYTES,
            socket_workers: 1,
            io_workers: 2,
        })
}

pub(super) fn resource_admission_error(
    error: DaemonIngestResourceReservationError,
) -> DaemonIngestFilesRuntimeError {
    DaemonIngestFilesRuntimeError::CommandFailed(format!(
        "ingest resource admission rejected: {error:?}"
    ))
}

fn shared_ingest_resource_gate() -> Arc<DaemonIngestResourceGate> {
    static GATE: OnceLock<Arc<DaemonIngestResourceGate>> = OnceLock::new();
    Arc::clone(GATE.get_or_init(|| {
        let policy = DaemonIngestResourcePolicy::default();
        let available_cpu_cores = std::thread::available_parallelism()
            .map(|cores| cores.get().min(u16::MAX as usize) as u16)
            .unwrap_or(1)
            .max(1);
        let mut budget = DaemonIngestResourceBudget::from_policy(policy, available_cpu_cores);
        // The packaged daemon may accept several independent jobs while each
        // still retains one control lane and two I/O lanes. Keep the shared
        // gate bounded without serializing normal job admission.
        budget.socket_workers = budget.socket_workers.max(8);
        budget.io_workers = budget.io_workers.max(16);
        Arc::new(DaemonIngestResourceGate::new(budget))
    }))
}
