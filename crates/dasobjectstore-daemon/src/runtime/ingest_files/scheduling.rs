//! HDD settlement capacity and concurrency scheduling.

use super::DaemonIngestFilesRuntimeError;
use dasobjectstore_core::ids::DiskId;
use dasobjectstore_metadata::{measure_ssd_capacity, DiskCopyRoot};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

pub(super) const MAX_HDD_SETTLEMENT_WORKERS: usize = 32;

#[derive(Clone, Debug)]
pub(super) struct HddSettlementDiskState {
    pub(super) disk_id: DiskId,
    pub(super) root_path: PathBuf,
    pub(super) active: bool,
    pub(super) total_bytes: u64,
    pub(super) available_bytes: u64,
    pub(super) assigned_bytes: u64,
}

#[derive(Debug)]
pub(super) struct HddSettlementScheduler {
    pub(crate) disks: Vec<HddSettlementDiskState>,
}

pub(super) type SharedHddSettlementScheduler = Arc<(Mutex<HddSettlementScheduler>, Condvar)>;

impl HddSettlementScheduler {
    pub(super) fn new(roots: &[DiskCopyRoot]) -> Result<Self, DaemonIngestFilesRuntimeError> {
        let mut seen_disk_ids = BTreeSet::new();
        for root in roots {
            if !seen_disk_ids.insert(root.disk_id.clone()) {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
                    "managed HDD root inventory contains duplicate disk ID {}; redundant copies require distinct physical disks",
                    root.disk_id
                )));
            }
        }

        Ok(Self {
            disks: roots
                .iter()
                .map(|root| {
                    let capacity = measure_ssd_capacity(&root.root_path)?;
                    Ok(HddSettlementDiskState {
                        disk_id: root.disk_id.clone(),
                        root_path: root.root_path.clone(),
                        active: false,
                        total_bytes: capacity.total_bytes,
                        available_bytes: capacity.available_bytes,
                        assigned_bytes: 0,
                    })
                })
                .collect::<Result<Vec<_>, DaemonIngestFilesRuntimeError>>()?,
        })
    }

    pub(crate) fn reserve_roots(
        &mut self,
        copy_count: usize,
        object_size_bytes: u64,
    ) -> Result<Option<Vec<DiskCopyRoot>>, DaemonIngestFilesRuntimeError> {
        let eligible_count = self
            .disks
            .iter()
            .filter(|disk| disk.projected_available_bytes() >= object_size_bytes)
            .count();
        if eligible_count < copy_count {
            return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "HDD settlement needs {copy_count} disk(s) with at least {object_size_bytes} byte(s) free; found {eligible_count}"
            )));
        }

        let mut candidates = self
            .disks
            .iter()
            .enumerate()
            .filter(|(_, disk)| {
                !disk.active && disk.projected_available_bytes() >= object_size_bytes
            })
            .collect::<Vec<_>>();
        if candidates.len() < copy_count {
            return Ok(None);
        }
        candidates.sort_by(|(_, left), (_, right)| compare_hdd_settlement_disks(right, left));
        let selected = candidates
            .into_iter()
            .take(copy_count)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut roots = Vec::with_capacity(copy_count);
        for index in selected {
            let disk = &mut self.disks[index];
            disk.active = true;
            roots.push(DiskCopyRoot::new(
                disk.disk_id.clone(),
                disk.root_path.clone(),
            ));
        }
        Ok(Some(roots))
    }

    pub(crate) fn release_roots(&mut self, roots: &[DiskCopyRoot], bytes_per_root: u64) {
        for root in roots {
            if let Some(disk) = self
                .disks
                .iter_mut()
                .find(|disk| disk.disk_id == root.disk_id)
            {
                disk.active = false;
                disk.assigned_bytes = disk.assigned_bytes.saturating_add(bytes_per_root);
            }
        }
    }
}

impl HddSettlementDiskState {
    fn projected_available_bytes(&self) -> u64 {
        self.available_bytes.saturating_sub(self.assigned_bytes)
    }
}

pub(super) fn new_shared_hdd_settlement_scheduler(
    roots: &[DiskCopyRoot],
) -> Result<SharedHddSettlementScheduler, DaemonIngestFilesRuntimeError> {
    Ok(Arc::new((
        Mutex::new(HddSettlementScheduler::new(roots)?),
        Condvar::new(),
    )))
}

pub(super) fn reserve_hdd_settlement_roots(
    scheduler: &SharedHddSettlementScheduler,
    copy_count: usize,
    object_size_bytes: u64,
) -> Result<Vec<DiskCopyRoot>, DaemonIngestFilesRuntimeError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        DaemonIngestFilesRuntimeError::CommandFailed(
            "HDD settlement scheduler lock poisoned".to_string(),
        )
    })?;
    loop {
        if let Some(roots) = scheduler.reserve_roots(copy_count, object_size_bytes)? {
            return Ok(roots);
        }
        scheduler = condvar.wait(scheduler).map_err(|_| {
            DaemonIngestFilesRuntimeError::CommandFailed(
                "HDD settlement scheduler lock poisoned".to_string(),
            )
        })?;
    }
}

pub(super) fn release_hdd_settlement_roots(
    scheduler: &SharedHddSettlementScheduler,
    roots: &[DiskCopyRoot],
    bytes_per_root: u64,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        DaemonIngestFilesRuntimeError::CommandFailed(
            "HDD settlement scheduler lock poisoned".to_string(),
        )
    })?;
    scheduler.release_roots(roots, bytes_per_root);
    condvar.notify_all();
    Ok(())
}

fn compare_hdd_settlement_disks(
    left: &HddSettlementDiskState,
    right: &HddSettlementDiskState,
) -> std::cmp::Ordering {
    let left_free = left.projected_available_bytes();
    let right_free = right.projected_available_bytes();
    (u128::from(left_free) * u128::from(right.total_bytes.max(1)))
        .cmp(&(u128::from(right_free) * u128::from(left.total_bytes.max(1))))
        .then_with(|| left_free.cmp(&right_free))
        .then_with(|| right.disk_id.cmp(&left.disk_id))
}

pub(super) fn resolve_hdd_worker_count(
    requested: Option<usize>,
    managed_hdd_count: usize,
) -> Result<usize, DaemonIngestFilesRuntimeError> {
    if managed_hdd_count == 0 {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(
            "ingest files requires at least one managed HDD root".to_string(),
        ));
    }
    let maximum = managed_hdd_count.min(MAX_HDD_SETTLEMENT_WORKERS);
    let workers = requested.unwrap_or_else(|| default_hdd_worker_count(managed_hdd_count));
    if workers == 0 {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(
            "HDD worker count must be greater than zero".to_string(),
        ));
    }
    if workers > maximum {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "HDD worker count {workers} exceeds available managed HDD writers {maximum}"
        )));
    }
    Ok(workers)
}

pub(super) fn default_hdd_worker_count(managed_hdd_count: usize) -> usize {
    managed_hdd_count
        .saturating_sub(2)
        .max(2)
        .min(managed_hdd_count)
}
