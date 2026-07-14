use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestResourcePolicy {
    pub worker_counts: DaemonIngestWorkerCounts,
    pub memory_budget_bytes: u64,
    pub ssd_reserve_bytes: u64,
    pub hdd_queue_depth: u32,
    pub verification_parallelism: u16,
    pub system_safety_reserve: DaemonIngestSystemSafetyReserve,
}

impl Default for DaemonIngestResourcePolicy {
    fn default() -> Self {
        let worker_counts = DaemonIngestWorkerCounts::default();

        Self {
            worker_counts,
            memory_budget_bytes: 1024 * 1024 * 1024,
            ssd_reserve_bytes: 10 * 1024 * 1024 * 1024,
            hdd_queue_depth: 64,
            verification_parallelism: worker_counts.verification,
            system_safety_reserve: DaemonIngestSystemSafetyReserve::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestWorkerCounts {
    pub scan: u16,
    pub source_read: u16,
    pub ssd_stage: u16,
    pub checksum_manifest: u16,
    pub hdd_placement: u16,
    pub hdd_write: u16,
    pub verification: u16,
    pub finalization: u16,
}

impl Default for DaemonIngestWorkerCounts {
    fn default() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|cores| cores.get().min(u16::MAX as usize) as u16)
            .unwrap_or(1)
            .max(1);
        let coordination_workers = 1;
        let disk_workers = cores.clamp(1, 8);
        let cpu_workers = cores.saturating_sub(1).max(1).min(8);

        Self {
            scan: coordination_workers,
            source_read: disk_workers.min(4),
            ssd_stage: disk_workers.min(4),
            checksum_manifest: cpu_workers,
            hdd_placement: coordination_workers,
            hdd_write: disk_workers,
            verification: cpu_workers,
            finalization: coordination_workers,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSystemSafetyReserve {
    pub cpu_cores: u16,
    pub memory_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestResourceReservation {
    pub cpu_cores: u16,
    pub memory_bytes: u64,
    pub socket_workers: u16,
    pub io_workers: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestResourceBudget {
    pub cpu_cores: u16,
    pub memory_bytes: u64,
    pub socket_workers: u16,
    pub io_workers: u16,
}

impl DaemonIngestResourceBudget {
    pub fn from_policy(policy: DaemonIngestResourcePolicy, available_cpu_cores: u16) -> Self {
        let cpu_cores = available_cpu_cores
            .saturating_sub(policy.system_safety_reserve.cpu_cores)
            .max(1);
        let memory_bytes = policy
            .memory_budget_bytes
            .saturating_sub(policy.system_safety_reserve.memory_bytes);
        let socket_workers = policy
            .worker_counts
            .scan
            .saturating_add(policy.worker_counts.finalization)
            .max(1);
        let io_workers = policy
            .worker_counts
            .source_read
            .saturating_add(policy.worker_counts.ssd_stage)
            .saturating_add(policy.worker_counts.hdd_write)
            .max(1);
        Self {
            cpu_cores,
            memory_bytes,
            socket_workers,
            io_workers,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DaemonIngestResourceUsage(DaemonIngestResourceReservation);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DaemonIngestResourceState {
    budget: DaemonIngestResourceBudget,
    usage: DaemonIngestResourceUsage,
}

#[derive(Clone, Debug)]
pub struct DaemonIngestResourceGate {
    state: Arc<Mutex<DaemonIngestResourceState>>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DaemonIngestResourceReservationError {
    BudgetExceeded {
        resource: &'static str,
        requested: u64,
        available: u64,
    },
}

#[derive(Debug)]
pub struct DaemonIngestResourceLease {
    gate: DaemonIngestResourceGate,
    reservation: DaemonIngestResourceReservation,
}

impl DaemonIngestResourceGate {
    pub fn new(budget: DaemonIngestResourceBudget) -> Self {
        Self {
            state: Arc::new(Mutex::new(DaemonIngestResourceState {
                budget,
                usage: DaemonIngestResourceUsage(DaemonIngestResourceReservation::default()),
            })),
        }
    }

    pub fn from_policy(policy: DaemonIngestResourcePolicy, available_cpu_cores: u16) -> Self {
        Self::new(DaemonIngestResourceBudget::from_policy(
            policy,
            available_cpu_cores,
        ))
    }

    /// Replace the admission budget without dropping existing leases.
    ///
    /// A live policy update may temporarily set a budget below current usage;
    /// existing work is allowed to drain, while new reservations fail closed
    /// until sufficient capacity is available again. The update and each
    /// reservation share one mutex so a policy refresh cannot race an
    /// admission decision.
    pub fn reconfigure(&self, budget: DaemonIngestResourceBudget) {
        let mut state = self.state.lock().expect("ingest resource gate lock");
        state.budget = budget;
    }

    /// Return the effective budget and usage atomically for diagnostics.
    pub fn snapshot(&self) -> (DaemonIngestResourceBudget, DaemonIngestResourceReservation) {
        let state = self.state.lock().expect("ingest resource gate lock");
        (state.budget, state.usage.0)
    }

    pub fn try_reserve(
        &self,
        reservation: DaemonIngestResourceReservation,
    ) -> Result<DaemonIngestResourceLease, DaemonIngestResourceReservationError> {
        let mut state = self.state.lock().expect("ingest resource gate lock");
        check_resource(
            "cpu_cores",
            u64::from(state.usage.0.cpu_cores),
            u64::from(reservation.cpu_cores),
            u64::from(state.budget.cpu_cores),
        )?;
        check_resource(
            "memory_bytes",
            state.usage.0.memory_bytes,
            reservation.memory_bytes,
            state.budget.memory_bytes,
        )?;
        check_resource(
            "socket_workers",
            u64::from(state.usage.0.socket_workers),
            u64::from(reservation.socket_workers),
            u64::from(state.budget.socket_workers),
        )?;
        check_resource(
            "io_workers",
            u64::from(state.usage.0.io_workers),
            u64::from(reservation.io_workers),
            u64::from(state.budget.io_workers),
        )?;
        state.usage.0.cpu_cores = state
            .usage
            .0
            .cpu_cores
            .saturating_add(reservation.cpu_cores);
        state.usage.0.memory_bytes = state
            .usage
            .0
            .memory_bytes
            .saturating_add(reservation.memory_bytes);
        state.usage.0.socket_workers = state
            .usage
            .0
            .socket_workers
            .saturating_add(reservation.socket_workers);
        state.usage.0.io_workers = state
            .usage
            .0
            .io_workers
            .saturating_add(reservation.io_workers);
        drop(state);
        Ok(DaemonIngestResourceLease {
            gate: self.clone(),
            reservation,
        })
    }

    #[cfg(test)]
    fn usage(&self) -> DaemonIngestResourceReservation {
        self.snapshot().1
    }
}

impl Drop for DaemonIngestResourceLease {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().expect("ingest resource gate lock");
        state.usage.0.cpu_cores = state
            .usage
            .0
            .cpu_cores
            .saturating_sub(self.reservation.cpu_cores);
        state.usage.0.memory_bytes = state
            .usage
            .0
            .memory_bytes
            .saturating_sub(self.reservation.memory_bytes);
        state.usage.0.socket_workers = state
            .usage
            .0
            .socket_workers
            .saturating_sub(self.reservation.socket_workers);
        state.usage.0.io_workers = state
            .usage
            .0
            .io_workers
            .saturating_sub(self.reservation.io_workers);
    }
}

fn check_resource(
    resource: &'static str,
    used: u64,
    requested: u64,
    budget: u64,
) -> Result<(), DaemonIngestResourceReservationError> {
    let available = budget.saturating_sub(used);
    if requested > available {
        return Err(DaemonIngestResourceReservationError::BudgetExceeded {
            resource,
            requested,
            available,
        });
    }
    Ok(())
}

impl Default for DaemonIngestSystemSafetyReserve {
    fn default() -> Self {
        let cpu_cores = std::thread::available_parallelism()
            .map(|cores| u16::from(cores.get() > 2))
            .unwrap_or(0);

        Self {
            cpu_cores,
            memory_bytes: 512 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reservation() -> DaemonIngestResourceReservation {
        DaemonIngestResourceReservation {
            cpu_cores: 1,
            memory_bytes: 100,
            socket_workers: 1,
            io_workers: 2,
        }
    }

    #[test]
    fn resource_gate_rejects_overlapping_reservations_without_overbooking() {
        let gate = DaemonIngestResourceGate::new(DaemonIngestResourceBudget {
            cpu_cores: 1,
            memory_bytes: 150,
            socket_workers: 1,
            io_workers: 2,
        });
        let lease = gate.try_reserve(reservation()).expect("first reservation");
        let error = gate
            .try_reserve(reservation())
            .expect_err("second reservation exceeds every budget");
        assert_eq!(
            error,
            DaemonIngestResourceReservationError::BudgetExceeded {
                resource: "cpu_cores",
                requested: 1,
                available: 0,
            }
        );
        assert_eq!(gate.usage(), reservation());
        drop(lease);
        assert_eq!(gate.usage(), DaemonIngestResourceReservation::default());
    }

    #[test]
    fn resource_gate_releases_memory_and_io_on_lease_drop() {
        let gate = DaemonIngestResourceGate::new(DaemonIngestResourceBudget {
            cpu_cores: 2,
            memory_bytes: 200,
            socket_workers: 2,
            io_workers: 4,
        });
        let first = gate.try_reserve(reservation()).expect("first reservation");
        let error = gate
            .try_reserve(DaemonIngestResourceReservation {
                cpu_cores: 1,
                memory_bytes: 101,
                socket_workers: 1,
                io_workers: 2,
            })
            .expect_err("memory is fully reserved");
        assert_eq!(
            error,
            DaemonIngestResourceReservationError::BudgetExceeded {
                resource: "memory_bytes",
                requested: 101,
                available: 100,
            }
        );
        drop(first);
        assert!(gate.try_reserve(reservation()).is_ok());
    }

    #[test]
    fn policy_constructor_applies_configured_memory_budget() {
        let policy = DaemonIngestResourcePolicy {
            memory_budget_bytes: 100,
            system_safety_reserve: DaemonIngestSystemSafetyReserve {
                cpu_cores: 0,
                memory_bytes: 0,
            },
            ..DaemonIngestResourcePolicy::default()
        };
        let gate = DaemonIngestResourceGate::from_policy(policy, 2);
        let _lease = gate
            .try_reserve(DaemonIngestResourceReservation {
                cpu_cores: 1,
                memory_bytes: 100,
                socket_workers: 1,
                io_workers: 2,
            })
            .expect("configured budget admits one reservation");
        let error = gate
            .try_reserve(DaemonIngestResourceReservation {
                cpu_cores: 1,
                memory_bytes: 1,
                socket_workers: 1,
                io_workers: 2,
            })
            .expect_err("configured memory budget rejects over-admission");
        assert_eq!(
            error,
            DaemonIngestResourceReservationError::BudgetExceeded {
                resource: "memory_bytes",
                requested: 1,
                available: 0,
            }
        );
    }

    #[test]
    fn reconfigure_is_atomic_and_allows_existing_work_to_drain() {
        let gate = DaemonIngestResourceGate::new(DaemonIngestResourceBudget {
            cpu_cores: 2,
            memory_bytes: 200,
            socket_workers: 2,
            io_workers: 4,
        });
        let lease = gate
            .try_reserve(reservation())
            .expect("initial reservation");
        gate.reconfigure(DaemonIngestResourceBudget {
            cpu_cores: 1,
            memory_bytes: 50,
            socket_workers: 1,
            io_workers: 2,
        });

        assert_eq!(
            gate.snapshot(),
            (
                DaemonIngestResourceBudget {
                    cpu_cores: 1,
                    memory_bytes: 50,
                    socket_workers: 1,
                    io_workers: 2,
                },
                reservation(),
            )
        );
        let error = gate
            .try_reserve(DaemonIngestResourceReservation {
                cpu_cores: 1,
                memory_bytes: 1,
                socket_workers: 1,
                io_workers: 1,
            })
            .expect_err("new work must fail while the lowered budget is saturated");
        assert_eq!(
            error,
            DaemonIngestResourceReservationError::BudgetExceeded {
                resource: "cpu_cores",
                requested: 1,
                available: 0,
            }
        );
        drop(lease);
        assert!(gate
            .try_reserve(DaemonIngestResourceReservation {
                cpu_cores: 1,
                memory_bytes: 50,
                socket_workers: 1,
                io_workers: 2,
            })
            .is_ok());
    }
}
