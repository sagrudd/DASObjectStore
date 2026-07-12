use super::{check_performance_cancelled, performance_sync_all, CliError};
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub(super) const PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY: usize = 8;

struct PerformanceSsdSettleJob {
    path: PathBuf,
    file: File,
}

pub(super) struct PerformanceSsdSettler {
    sender: Option<mpsc::SyncSender<PerformanceSsdSettleJob>>,
    handle: Option<thread::JoinHandle<Result<(), CliError>>>,
    completed: Arc<AtomicU32>,
}

impl PerformanceSsdSettler {
    pub(super) fn start(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<PerformanceSsdSettleJob>(capacity);
        let completed = Arc::new(AtomicU32::new(0));
        let worker_completed = Arc::clone(&completed);
        let handle = thread::spawn(move || -> Result<(), CliError> {
            loop {
                check_performance_cancelled()?;
                let job = match receiver.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                };
                performance_sync_all(&job.file).map_err(|err| {
                    CliError::CommandFailed(format!(
                        "performance-test SSD settle failed for {}: {err}",
                        job.path.display()
                    ))
                })?;
                worker_completed.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        });
        Self {
            sender: Some(sender),
            handle: Some(handle),
            completed,
        }
    }

    pub(super) fn submit(&self, path: PathBuf, file: File) -> Result<(), CliError> {
        let sender = self.sender.as_ref().ok_or_else(|| {
            CliError::CommandFailed("performance-test SSD settler is closed".to_string())
        })?;
        let mut pending = Some(PerformanceSsdSettleJob { path, file });
        loop {
            check_performance_cancelled()?;
            let job = pending.take().expect("pending SSD settle job");
            match sender.try_send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Full(job)) => {
                    pending = Some(job);
                    thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(CliError::CommandFailed(
                        "performance-test SSD settler stopped early".to_string(),
                    ));
                }
            }
        }
    }

    pub(super) fn finish(mut self) -> Result<u32, CliError> {
        drop(self.sender.take());
        self.join_worker()?;
        Ok(self.completed.load(Ordering::SeqCst))
    }

    fn join_worker(&mut self) -> Result<(), CliError> {
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(CliError::CommandFailed(
                    "performance-test SSD settler panicked".to_string(),
                )),
            }
        } else {
            Ok(())
        }
    }
}

impl Drop for PerformanceSsdSettler {
    fn drop(&mut self) {
        drop(self.sender.take());
        let _ = self.join_worker();
    }
}
