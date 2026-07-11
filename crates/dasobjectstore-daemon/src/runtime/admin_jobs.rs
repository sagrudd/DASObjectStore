use crate::api::{
    DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobId, DaemonJobListRequest,
    DaemonJobListResponse, DaemonJobState, DaemonJobStatusRequest, DaemonJobStatusResponse,
    DaemonJobSummary,
};
use crate::runtime::DaemonServiceRuntimeError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const ADMIN_JOB_REGISTRY_DIR_NAME: &str = "admin-jobs";
pub const ADMIN_JOB_REGISTRY_FILE_NAME: &str = "jobs.json";
pub const ADMIN_JOB_REGISTRY_SCHEMA: &str = "dasobjectstore.admin_jobs.v1";

pub trait AdminJobRegistry: Send + Sync {
    fn record(&self, job: DaemonJobSummary) -> Result<(), DaemonServiceRuntimeError>;

    fn status(
        &self,
        request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError>;

    fn list(
        &self,
        request: DaemonJobListRequest,
    ) -> Result<DaemonJobListResponse, DaemonServiceRuntimeError>;

    fn cancel(
        &self,
        request: DaemonJobCancelRequest,
        cancelled_at_utc: &str,
    ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError>;

    fn mark_interrupted_at_startup(
        &self,
        interrupted_at_utc: &str,
    ) -> Result<usize, DaemonServiceRuntimeError>;
}

pub fn admin_job_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(ADMIN_JOB_REGISTRY_DIR_NAME)
        .join(ADMIN_JOB_REGISTRY_FILE_NAME)
}

#[derive(Debug)]
pub struct FileBackedAdminJobRegistry {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedAdminJobRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AdminJobRegistry for FileBackedAdminJobRegistry {
    fn record(&self, job: DaemonJobSummary) -> Result<(), DaemonServiceRuntimeError> {
        let _guard = self.lock.lock().expect("admin job registry lock poisoned");
        let mut registry = read_registry(&self.path)?;
        registry.upsert(job);
        write_registry(&self.path, &registry)
    }

    fn status(
        &self,
        request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
        let _guard = self.lock.lock().expect("admin job registry lock poisoned");
        let registry = read_registry(&self.path)?;
        let job = registry.job(&request.job_id).cloned().ok_or_else(|| {
            DaemonServiceRuntimeError::JobNotFound {
                job_id: request.job_id.to_string(),
            }
        })?;
        Ok(DaemonJobStatusResponse { job })
    }

    fn list(
        &self,
        request: DaemonJobListRequest,
    ) -> Result<DaemonJobListResponse, DaemonServiceRuntimeError> {
        let _guard = self.lock.lock().expect("admin job registry lock poisoned");
        let registry = read_registry(&self.path)?;
        let mut jobs = registry.jobs.clone();
        jobs.sort_by(|left, right| right.updated_at_utc.cmp(&left.updated_at_utc));
        if let Some(limit) = request.limit {
            jobs.truncate(limit);
        }
        Ok(DaemonJobListResponse { jobs })
    }

    fn cancel(
        &self,
        request: DaemonJobCancelRequest,
        cancelled_at_utc: &str,
    ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
        let _guard = self.lock.lock().expect("admin job registry lock poisoned");
        let mut registry = read_registry(&self.path)?;
        let job = registry.job_mut(&request.job_id).ok_or_else(|| {
            DaemonServiceRuntimeError::JobNotFound {
                job_id: request.job_id.to_string(),
            }
        })?;

        let accepted = !matches!(
            job.state,
            DaemonJobState::Complete | DaemonJobState::Failed | DaemonJobState::Cancelled
        );
        if accepted {
            job.state = DaemonJobState::Cancelled;
            job.updated_at_utc = cancelled_at_utc.to_string();
            job.progress.message = request.reason.or_else(|| Some("cancelled".to_string()));
        }

        let response = DaemonJobCancelResponse {
            job_id: request.job_id,
            accepted,
            state: job.state.clone(),
        };
        write_registry(&self.path, &registry)?;
        Ok(response)
    }

    fn mark_interrupted_at_startup(
        &self,
        interrupted_at_utc: &str,
    ) -> Result<usize, DaemonServiceRuntimeError> {
        let _guard = self.lock.lock().expect("admin job registry lock poisoned");
        let mut registry = read_registry(&self.path)?;
        let mut interrupted = 0;
        for job in &mut registry.jobs {
            if matches!(
                job.state,
                DaemonJobState::Queued | DaemonJobState::Running | DaemonJobState::Waiting
            ) {
                job.state = DaemonJobState::Failed;
                job.updated_at_utc = interrupted_at_utc.to_string();
                job.progress.stage = "interrupted".to_string();
                let message =
                    "interrupted because dasobjectstored restarted; inspect and rerun safely"
                        .to_string();
                job.progress.message = Some(message.clone());
                job.failure_message = Some(message);
                interrupted += 1;
            }
        }
        if interrupted > 0 {
            write_registry(&self.path, &registry)?;
        }
        Ok(interrupted)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct AdminJobRegistryFile {
    schema_version: String,
    jobs: Vec<DaemonJobSummary>,
}

impl Default for AdminJobRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: ADMIN_JOB_REGISTRY_SCHEMA.to_string(),
            jobs: Vec::new(),
        }
    }
}

impl AdminJobRegistryFile {
    fn job(&self, job_id: &DaemonJobId) -> Option<&DaemonJobSummary> {
        self.jobs.iter().find(|job| &job.job_id == job_id)
    }

    fn job_mut(&mut self, job_id: &DaemonJobId) -> Option<&mut DaemonJobSummary> {
        self.jobs.iter_mut().find(|job| &job.job_id == job_id)
    }

    fn upsert(&mut self, job: DaemonJobSummary) {
        if let Some(existing) = self.job_mut(&job.job_id) {
            *existing = job;
        } else {
            self.jobs.push(job);
        }
    }
}

fn read_registry(path: &Path) -> Result<AdminJobRegistryFile, DaemonServiceRuntimeError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            DaemonServiceRuntimeError::InvalidJobRegistryJson {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AdminJobRegistryFile::default())
        }
        Err(error) => Err(DaemonServiceRuntimeError::JobRegistryIo {
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

fn write_registry(
    path: &Path,
    registry: &AdminJobRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| DaemonServiceRuntimeError::JobRegistryIo {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    let encoded = serde_json::to_vec_pretty(registry).map_err(|error| {
        DaemonServiceRuntimeError::InvalidJobRegistryJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    fs::write(path, encoded).map_err(|error| DaemonServiceRuntimeError::JobRegistryIo {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{admin_job_registry_path, AdminJobRegistry, FileBackedAdminJobRegistry};
    use crate::api::{
        DaemonJobCancelRequest, DaemonJobId, DaemonJobKind, DaemonJobListRequest,
        DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest, DaemonJobSummary,
    };
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn builds_registry_path_under_state_dir() {
        assert_eq!(
            admin_job_registry_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore/admin-jobs/jobs.json")
        );
    }

    #[test]
    fn persists_and_reads_recorded_jobs() {
        let root = temp_root("persist");
        let path = admin_job_registry_path(&root);
        let registry = FileBackedAdminJobRegistry::new(&path);

        registry
            .record(job("job-1", DaemonJobState::Running))
            .expect("recorded");
        let response = registry
            .status(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("job-1").expect("job id"),
            })
            .expect("status loaded");

        assert_eq!(response.job.job_id.as_str(), "job-1");
        assert_eq!(response.job.state, DaemonJobState::Running);
        assert!(path.exists());

        cleanup(&root);
    }

    #[test]
    fn lists_jobs_by_latest_update_with_optional_limit() {
        let root = temp_root("list");
        let path = admin_job_registry_path(&root);
        let registry = FileBackedAdminJobRegistry::new(&path);

        registry
            .record(job_with_update(
                "job-old",
                DaemonJobState::Complete,
                "2026-07-08T20:18:00Z",
            ))
            .expect("old job recorded");
        registry
            .record(job_with_update(
                "job-new",
                DaemonJobState::Running,
                "2026-07-08T20:22:00Z",
            ))
            .expect("new job recorded");

        let response = registry
            .list(DaemonJobListRequest { limit: Some(1) })
            .expect("jobs listed");

        assert_eq!(response.jobs.len(), 1);
        assert_eq!(response.jobs[0].job_id.as_str(), "job-new");

        cleanup(&root);
    }

    #[test]
    fn cancellation_updates_non_terminal_job() {
        let root = temp_root("cancel");
        let path = admin_job_registry_path(&root);
        let registry = FileBackedAdminJobRegistry::new(&path);

        registry
            .record(job("job-1", DaemonJobState::Running))
            .expect("recorded");
        let response = registry
            .cancel(
                DaemonJobCancelRequest {
                    job_id: DaemonJobId::new("job-1").expect("job id"),
                    reason: Some("operator requested cancellation".to_string()),
                },
                "2026-07-08T20:20:00Z",
            )
            .expect("cancelled");

        assert!(response.accepted);
        assert_eq!(response.state, DaemonJobState::Cancelled);
        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("job-1").expect("job id"),
            })
            .expect("status loaded");
        assert_eq!(status.job.state, DaemonJobState::Cancelled);
        assert_eq!(status.job.updated_at_utc, "2026-07-08T20:20:00Z");

        cleanup(&root);
    }

    #[test]
    fn cancellation_does_not_reopen_terminal_job() {
        let root = temp_root("terminal");
        let path = admin_job_registry_path(&root);
        let registry = FileBackedAdminJobRegistry::new(&path);

        registry
            .record(job("job-1", DaemonJobState::Complete))
            .expect("recorded");
        let response = registry
            .cancel(
                DaemonJobCancelRequest {
                    job_id: DaemonJobId::new("job-1").expect("job id"),
                    reason: Some("too late".to_string()),
                },
                "2026-07-08T20:21:00Z",
            )
            .expect("cancel checked");

        assert!(!response.accepted);
        assert_eq!(response.state, DaemonJobState::Complete);

        cleanup(&root);
    }

    #[test]
    fn startup_marks_only_nonterminal_jobs_interrupted() {
        let root = temp_root("startup-interruption");
        let path = admin_job_registry_path(&root);
        let registry = FileBackedAdminJobRegistry::new(&path);
        registry
            .record(job("running", DaemonJobState::Running))
            .expect("recorded");
        registry
            .record(job("complete", DaemonJobState::Complete))
            .expect("recorded");

        assert_eq!(
            registry
                .mark_interrupted_at_startup("2026-07-09T00:00:00Z")
                .expect("recovered"),
            1
        );
        let running = registry
            .status(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("running").expect("job"),
            })
            .expect("status");
        assert_eq!(running.job.state, DaemonJobState::Failed);
        assert_eq!(running.job.progress.stage, "interrupted");
        assert!(running
            .job
            .failure_message
            .unwrap_or_default()
            .contains("restarted"));
        let complete = registry
            .status(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("complete").expect("job"),
            })
            .expect("status");
        assert_eq!(complete.job.state, DaemonJobState::Complete);
        assert_eq!(
            registry
                .mark_interrupted_at_startup("2026-07-09T00:01:00Z")
                .expect("idempotent"),
            0
        );
        cleanup(&root);
    }

    fn job(job_id: &str, state: DaemonJobState) -> DaemonJobSummary {
        job_with_update(job_id, state, "2026-07-08T20:19:00Z")
    }

    fn job_with_update(
        job_id: &str,
        state: DaemonJobState,
        updated_at_utc: &str,
    ) -> DaemonJobSummary {
        DaemonJobSummary {
            job_id: DaemonJobId::new(job_id).expect("job id"),
            kind: DaemonJobKind::EnclosurePreparation,
            state,
            progress: DaemonJobProgress {
                stage: "accepted".to_string(),
                work_bytes_done: 0,
                work_bytes_total: 1,
                work_units_done: 0,
                work_units_total: 1,
                message: Some("accepted".to_string()),
            },
            submitted_at_utc: "2026-07-08T20:19:00Z".to_string(),
            updated_at_utc: updated_at_utc.to_string(),
            actor: Some("operator".to_string()),
            failure_message: None,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-admin-jobs-{label}-{}",
            std::process::id()
        ))
    }

    fn cleanup(root: &PathBuf) {
        let _ = fs::remove_dir_all(root);
    }
}
