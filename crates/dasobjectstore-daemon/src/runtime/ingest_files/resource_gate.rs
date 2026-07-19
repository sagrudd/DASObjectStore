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
            memory_bytes: DaemonIngestResourceBudget::TRANSACTION_MEMORY_BYTES,
            socket_workers: 1,
            io_workers: DaemonIngestResourceBudget::TRANSACTION_IO_WORKERS,
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
        let budget = DaemonIngestResourceBudget::from_policy(policy, available_cpu_cores);
        Arc::new(DaemonIngestResourceGate::new(budget))
    }))
}
