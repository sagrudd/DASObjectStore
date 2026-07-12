use super::*;

pub(super) struct DiskPlacement {
    pub(super) disk_id: DiskId,
    pub(super) root_path: PathBuf,
}

#[derive(Clone, Debug)]
pub(super) struct DiskPlacementState {
    pub(super) disk_id: DiskId,
    pub(super) root_path: PathBuf,
    pub(super) active: usize,
    pub(super) total_bytes: u64,
    pub(super) available_bytes: u64,
    pub(super) assigned_bytes: u64,
    pub(super) completed_seconds: f64,
}

#[derive(Debug)]
pub(super) struct DiskPlacementScheduler {
    pub(super) disks: Vec<DiskPlacementState>,
    pub(super) logical_file_disks: BTreeMap<u32, BTreeSet<DiskId>>,
}

pub(super) type SharedDiskPlacementScheduler = Arc<(Mutex<DiskPlacementScheduler>, Condvar)>;

impl DiskPlacementScheduler {
    pub(super) fn new(disks: &[(DiskId, PathBuf)]) -> Result<Self, CliError> {
        Ok(Self {
            disks: disks
                .iter()
                .map(|(disk_id, root_path)| {
                    fs::create_dir_all(root_path)?;
                    let capacity = measure_ssd_capacity(root_path)?;
                    Ok(DiskPlacementState {
                        disk_id: disk_id.clone(),
                        root_path: root_path.clone(),
                        active: 0,
                        total_bytes: capacity.total_bytes,
                        available_bytes: capacity.available_bytes,
                        assigned_bytes: 0,
                        completed_seconds: 0.0,
                    })
                })
                .collect::<Result<Vec<_>, CliError>>()?,
            logical_file_disks: BTreeMap::new(),
        })
    }

    pub(super) fn reserve_disk_for_file(&mut self, file_index: u32) -> Option<DiskPlacement> {
        let already_assigned = self
            .logical_file_disks
            .get(&file_index)
            .cloned()
            .unwrap_or_default();
        let index = self.select_idle_disk(|disk| !already_assigned.contains(&disk.disk_id))?;
        self.reserve_disk_index(file_index, index)
    }

    pub(super) fn select_idle_disk(
        &self,
        accepts_disk: impl Fn(&DiskPlacementState) -> bool,
    ) -> Option<usize> {
        self.disks
            .iter()
            .enumerate()
            .filter(|(_, disk)| disk.active == 0 && accepts_disk(disk))
            .max_by(|(_, left), (_, right)| compare_disk_free_fraction(left, right))
            .map(|(index, _)| index)
    }

    pub(super) fn reserve_disk_index(
        &mut self,
        file_index: u32,
        index: usize,
    ) -> Option<DiskPlacement> {
        let disk = self.disks.get_mut(index)?;
        disk.active = 1;
        self.logical_file_disks
            .entry(file_index)
            .or_default()
            .insert(disk.disk_id.clone());
        Some(DiskPlacement {
            disk_id: disk.disk_id.clone(),
            root_path: disk.root_path.clone(),
        })
    }

    pub(super) fn complete_disk(&mut self, disk_id: &DiskId, bytes: u64, seconds: f64) {
        if let Some(disk) = self.disks.iter_mut().find(|disk| &disk.disk_id == disk_id) {
            disk.active = disk.active.saturating_sub(1);
            disk.assigned_bytes = disk.assigned_bytes.saturating_add(bytes);
            disk.completed_seconds += seconds.max(0.0);
        }
    }
}

pub(super) fn new_shared_disk_placement_scheduler(
    disks: &[(DiskId, PathBuf)],
) -> Result<SharedDiskPlacementScheduler, CliError> {
    Ok(Arc::new((
        Mutex::new(DiskPlacementScheduler::new(disks)?),
        Condvar::new(),
    )))
}

pub(super) fn reserve_performance_disk_for_file(
    scheduler: &SharedDiskPlacementScheduler,
    file_index: u32,
) -> Result<DiskPlacement, CliError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        CliError::CommandFailed("performance-test disk scheduler lock poisoned".to_string())
    })?;
    loop {
        check_performance_cancelled()?;
        if let Some(placement) = scheduler.reserve_disk_for_file(file_index) {
            return Ok(placement);
        }
        let result = condvar
            .wait_timeout(scheduler, Duration::from_millis(250))
            .map_err(|_| {
                CliError::CommandFailed("performance-test disk scheduler lock poisoned".to_string())
            })?;
        scheduler = result.0;
    }
}

pub(super) fn complete_performance_disk(
    scheduler: &SharedDiskPlacementScheduler,
    disk_id: &DiskId,
    bytes: u64,
    seconds: f64,
) -> Result<(), CliError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        CliError::CommandFailed("performance-test disk scheduler lock poisoned".to_string())
    })?;
    scheduler.complete_disk(disk_id, bytes, seconds);
    condvar.notify_one();
    Ok(())
}

fn compare_disk_free_fraction(
    left: &DiskPlacementState,
    right: &DiskPlacementState,
) -> std::cmp::Ordering {
    let left_free = left.available_bytes.saturating_sub(left.assigned_bytes);
    let right_free = right.available_bytes.saturating_sub(right.assigned_bytes);
    (u128::from(left_free) * u128::from(right.total_bytes.max(1)))
        .cmp(&(u128::from(right_free) * u128::from(left.total_bytes.max(1))))
        .then_with(|| left_free.cmp(&right_free))
        .then_with(|| right.completed_seconds.total_cmp(&left.completed_seconds))
        .then_with(|| right.disk_id.cmp(&left.disk_id))
}

pub(super) fn hdd_queue_capacity(concurrency: usize, redundancy: usize) -> usize {
    concurrency
        .saturating_mul(redundancy)
        .saturating_mul(2)
        .clamp(1, 64)
}
