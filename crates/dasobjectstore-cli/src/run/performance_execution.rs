use super::*;

pub(super) struct SsdPipelineJob {
    pub(super) file_index: u32,
    pub(super) copy_index: usize,
    pub(super) relative_path: PathBuf,
    pub(super) ssd_path: PathBuf,
    pub(super) size_bytes: u64,
}

pub(super) fn try_submit_pending_ssd_pipeline_jobs(
    sender: &mpsc::SyncSender<SsdPipelineJob>,
    pending_jobs: &mut VecDeque<SsdPipelineJob>,
    submitted_hdd_jobs: &mut usize,
) -> Result<bool, CliError> {
    let mut submitted_any = false;
    while let Some(job) = pending_jobs.pop_front() {
        match sender.try_send(job) {
            Ok(()) => {
                *submitted_hdd_jobs += 1;
                submitted_any = true;
            }
            Err(mpsc::TrySendError::Full(job)) => {
                pending_jobs.push_front(job);
                return Ok(submitted_any);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(CliError::CommandFailed(
                    "performance-test HDD workers stopped early".to_string(),
                ));
            }
        }
    }
    Ok(submitted_any)
}

#[derive(Debug)]
pub(super) struct DirectHddJob {
    pub(super) payload: PerformancePayload,
    pub(super) copy_index: usize,
}

pub(super) type ActiveHddWriteKey = (u32, usize);
pub(super) type ActiveHddWriteMap = Arc<Mutex<BTreeMap<ActiveHddWriteKey, ActiveHddWrite>>>;

#[derive(Clone, Debug)]
pub(super) struct ActiveHddWrite {
    pub(super) file_index: u32,
    pub(super) copy_index: usize,
    pub(super) relative_path: PathBuf,
    pub(super) disk_id: DiskId,
    pub(super) size_bytes: u64,
    pub(super) bytes_written: u64,
    pub(super) started: Instant,
    pub(super) phase: PerformanceCopyProgressPhase,
}
