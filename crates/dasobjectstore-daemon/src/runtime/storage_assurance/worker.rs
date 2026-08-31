use super::{
    assurance_destage_copying, assurance_primary_work_pending, persist_report,
    run_one_disk_housekeeping, LiveStatusRegistry, StorageAssuranceAction, StorageAssuranceConfig,
    StorageAssuranceReport,
};
#[cfg(target_os = "linux")]
use std::fs;
use std::io;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IdleObservation {
    primary_work_pending: bool,
    destage_copying: bool,
    live_ingests: bool,
    garbage_collection_running: bool,
    io_bytes_per_second: u64,
}

#[derive(Debug)]
struct IdleGate {
    idle_since: Option<Instant>,
    required_idle: Duration,
    maximum_io_bytes_per_second: u64,
    parallel_destage: bool,
}

impl IdleGate {
    fn new(
        required_idle: Duration,
        maximum_io_bytes_per_second: u64,
        parallel_destage: bool,
    ) -> Self {
        Self {
            idle_since: None,
            required_idle,
            maximum_io_bytes_per_second,
            parallel_destage,
        }
    }

    fn observe(&mut self, now: Instant, observation: IdleObservation) -> bool {
        // Destage owns a separate capacity claim and target selection. A
        // bounded rebalance may run alongside it, so a fuller HDD member does
        // not remain full merely because the SSD has a backlog. Foreground
        // ingest and collection still preempt before the next IO chunk.
        let destage_busy = observation.primary_work_pending && !self.parallel_destage;
        let external_io_busy = observation.io_bytes_per_second > self.maximum_io_bytes_per_second
            && !(self.parallel_destage && observation.destage_copying);
        let busy = destage_busy
            || observation.live_ingests
            || observation.garbage_collection_running
            || external_io_busy;
        if busy {
            self.idle_since = None;
            return false;
        }
        let idle_since = self.idle_since.get_or_insert(now);
        now.duration_since(*idle_since) >= self.required_idle
    }

    fn reset(&mut self) {
        self.idle_since = None;
    }
}

/// Start the low-priority daemon-owned `disk_housekeeping` worker.
pub fn spawn_disk_housekeeping_loop(
    config: StorageAssuranceConfig,
    live_status_registry: Arc<LiveStatusRegistry>,
    now_utc: fn() -> String,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("disk_housekeeping".to_string())
        .spawn(move || {
            let mut gate = IdleGate::new(
                Duration::from_secs(config.idle_grace_seconds),
                config.idle_io_bytes_per_second,
                config.parallel_destage,
            );
            let mut previous_io = read_linux_disk_io_bytes().ok();
            let mut previous_io_at = Instant::now();
            loop {
                thread::sleep(Duration::from_secs(config.poll_seconds));
                let observed_at = Instant::now();
                let current_io = read_linux_disk_io_bytes().ok();
                let io_rate = match (previous_io, current_io) {
                    (Some(previous), Some(current)) => current
                        .saturating_sub(previous)
                        .checked_div(observed_at.duration_since(previous_io_at).as_secs().max(1))
                        .unwrap_or(0),
                    _ => u64::MAX,
                };
                previous_io = current_io;
                previous_io_at = observed_at;
                let snapshot = live_status_registry.snapshot(now_utc());
                let primary_work_pending =
                    assurance_primary_work_pending(&config.live_sqlite_path).unwrap_or(true);
                let destage_copying =
                    assurance_destage_copying(&config.live_sqlite_path).unwrap_or(true);
                let observation = IdleObservation {
                    primary_work_pending,
                    destage_copying,
                    live_ingests: snapshot.aggregate.active_ingests > 0,
                    garbage_collection_running: snapshot
                        .garbage_collection
                        .is_some_and(|collection| collection.running),
                    io_bytes_per_second: io_rate,
                };
                if !gate.observe(observed_at, observation) {
                    continue;
                }
                let result = run_one_disk_housekeeping(
                    &config,
                    Arc::clone(&live_status_registry),
                    &now_utc(),
                );
                match result {
                    Ok(report) => {
                        if let Err(error) = persist_report(&config.latest_report_path, &report) {
                            eprintln!("disk_housekeeping report persistence failed: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("disk_housekeeping retained source data: {error}");
                        let report = StorageAssuranceReport {
                            schema: "dasobjectstore.disk_housekeeping.report.v1",
                            completed_at_utc: now_utc(),
                            success: false,
                            action: StorageAssuranceAction::Idle,
                            object_id: None,
                            source_disk_id: None,
                            destination_disk_id: None,
                            bytes: 0,
                            source_removed: false,
                            message: error.to_string(),
                        };
                        if let Err(report_error) =
                            persist_report(&config.latest_report_path, &report)
                        {
                            eprintln!(
                                "disk_housekeeping failure report persistence failed: {report_error}"
                            );
                        }
                    }
                }
                gate.reset();
                previous_io = read_linux_disk_io_bytes().ok();
                previous_io_at = Instant::now();
            }
        })
        .expect("disk_housekeeping thread should start")
}

#[cfg(target_os = "linux")]
fn read_linux_disk_io_bytes() -> Result<u64, io::Error> {
    let content = fs::read_to_string("/proc/diskstats")?;
    let mut sectors = 0u64;
    for line in content.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() >= 10 {
            sectors = sectors
                .saturating_add(fields[5].parse::<u64>().unwrap_or(0))
                .saturating_add(fields[9].parse::<u64>().unwrap_or(0));
        }
    }
    Ok(sectors.saturating_mul(512))
}

#[cfg(not(target_os = "linux"))]
fn read_linux_disk_io_bytes() -> Result<u64, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "host disk IO sampling requires Linux",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_gate_requires_continuous_quiescence_and_resets_on_io() {
        let start = Instant::now();
        let mut gate = IdleGate::new(Duration::from_secs(60), 100, false);
        let idle = IdleObservation {
            primary_work_pending: false,
            destage_copying: false,
            live_ingests: false,
            garbage_collection_running: false,
            io_bytes_per_second: 0,
        };
        assert!(!gate.observe(start, idle));
        assert!(!gate.observe(start + Duration::from_secs(59), idle));
        assert!(gate.observe(start + Duration::from_secs(60), idle));
        assert!(!gate.observe(
            start + Duration::from_secs(61),
            IdleObservation {
                io_bytes_per_second: 101,
                ..idle
            }
        ));
        assert!(!gate.observe(start + Duration::from_secs(120), idle));
    }

    #[test]
    fn idle_gate_starts_rebalance_alongside_durable_destage_but_not_ingest() {
        let start = Instant::now();
        let mut gate = IdleGate::new(Duration::from_secs(60), 100, true);
        let destage = IdleObservation {
            primary_work_pending: true,
            destage_copying: true,
            live_ingests: false,
            garbage_collection_running: false,
            // The destage writer itself can exceed the ordinary idle probe.
            io_bytes_per_second: 1_000,
        };
        assert!(!gate.observe(start, destage));
        assert!(gate.observe(start + Duration::from_secs(60), destage));
        assert!(!gate.observe(
            start + Duration::from_secs(61),
            IdleObservation {
                live_ingests: true,
                ..destage
            }
        ));
    }
}
