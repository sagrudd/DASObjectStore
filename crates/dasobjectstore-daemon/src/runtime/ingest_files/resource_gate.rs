use crate::api::{
    DaemonIngestResourceBudget, DaemonIngestResourceGate, DaemonIngestResourcePolicy,
    DaemonIngestResourceReservation, DaemonIngestResourceReservationError,
};
use std::sync::{Arc, OnceLock};

const INGEST_RESOURCE_MEMORY_RESERVATION_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn reserve_ingest_resources(
) -> Result<crate::api::DaemonIngestResourceLease, DaemonIngestResourceReservationError> {
    shared_ingest_resource_gate().try_reserve(DaemonIngestResourceReservation {
        cpu_cores: 1,
        memory_bytes: INGEST_RESOURCE_MEMORY_RESERVATION_BYTES,
        socket_workers: 1,
        io_workers: 2,
    })
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
