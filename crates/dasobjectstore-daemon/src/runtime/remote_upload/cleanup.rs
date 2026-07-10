use super::super::service::ServiceCommandRunner;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupRequest {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
    pub staged_object_prefix: Option<String>,
    pub multipart_upload_id: Option<String>,
    pub session_id: Option<String>,
    pub pairing_id: Option<String>,
    pub browser_handoff_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupPlan {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
    pub resumable: bool,
    pub actions: Vec<RemoteUploadCancellationCleanupAction>,
}

impl RemoteUploadCancellationCleanupPlan {
    pub fn requires_work(&self) -> bool {
        !self.actions.is_empty()
    }

    pub fn requires_multipart_abort(&self) -> bool {
        self.actions.iter().any(|action| {
            action.scope == RemoteUploadCancellationCleanupScope::FailedMultipartUpload
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupRunReport {
    pub plan: RemoteUploadCancellationCleanupPlan,
    pub action_reports: Vec<RemoteUploadCancellationCleanupActionReport>,
}

impl RemoteUploadCancellationCleanupRunReport {
    pub fn completed(&self) -> bool {
        self.action_reports
            .iter()
            .all(|report| report.state == RemoteUploadCancellationCleanupActionState::Complete)
    }

    pub fn failed_actions(&self) -> Vec<&RemoteUploadCancellationCleanupActionReport> {
        self.action_reports
            .iter()
            .filter(|report| report.state == RemoteUploadCancellationCleanupActionState::Failed)
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupActionReport {
    pub action: RemoteUploadCancellationCleanupAction,
    pub state: RemoteUploadCancellationCleanupActionState,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteUploadCancellationCleanupActionState {
    Complete,
    Failed,
}

pub trait RemoteUploadCancellationCleanupWorker {
    fn cleanup(
        &self,
        action: &RemoteUploadCancellationCleanupAction,
    ) -> Result<(), RemoteUploadCancellationCleanupError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupRuntimeConfig {
    pub ssd_stage_root: PathBuf,
    pub session_state_root: PathBuf,
    pub pairing_state_root: PathBuf,
    pub browser_handoff_state_root: PathBuf,
    pub multipart_abort: Option<RemoteUploadMultipartAbortConfig>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadMultipartAbortConfig {
    pub program: String,
    pub endpoint_url: String,
    pub bucket: String,
    pub object_key: String,
    pub environment: Vec<(String, String)>,
}

pub struct RemoteUploadCancellationCleanupRuntime<'a> {
    config: RemoteUploadCancellationCleanupRuntimeConfig,
    runner: &'a dyn ServiceCommandRunner,
}

impl<'a> RemoteUploadCancellationCleanupRuntime<'a> {
    pub fn new(
        config: RemoteUploadCancellationCleanupRuntimeConfig,
        runner: &'a dyn ServiceCommandRunner,
    ) -> Self {
        Self { config, runner }
    }

    fn remove_managed_path(
        root: &Path,
        identifier: &str,
    ) -> Result<(), RemoteUploadCancellationCleanupError> {
        let path = safe_child_path(root, identifier)?;
        if !path.exists() {
            return Ok(());
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            RemoteUploadCancellationCleanupError::new(format!(
                "inspect cleanup target {} failed: {error}",
                path.display()
            ))
        })?;
        if metadata.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .map_err(|error| {
            RemoteUploadCancellationCleanupError::new(format!(
                "remove cleanup target {} failed: {error}",
                path.display()
            ))
        })
    }

    fn abort_multipart_upload(
        &self,
        upload_id: &str,
    ) -> Result<(), RemoteUploadCancellationCleanupError> {
        let multipart = self.config.multipart_abort.as_ref().ok_or_else(|| {
            RemoteUploadCancellationCleanupError::new(
                "remote upload multipart cleanup is not configured",
            )
        })?;
        require_cleanup_field("multipart upload id", upload_id)?;
        require_cleanup_field("multipart cleanup program", &multipart.program)?;
        require_cleanup_field("multipart cleanup endpoint URL", &multipart.endpoint_url)?;
        require_cleanup_field("multipart cleanup bucket", &multipart.bucket)?;
        require_cleanup_field("multipart cleanup object key", &multipart.object_key)?;

        let args = vec![
            "--endpoint-url".to_string(),
            multipart.endpoint_url.clone(),
            "s3api".to_string(),
            "abort-multipart-upload".to_string(),
            "--bucket".to_string(),
            multipart.bucket.clone(),
            "--key".to_string(),
            multipart.object_key.clone(),
            "--upload-id".to_string(),
            upload_id.to_string(),
        ];
        self.runner
            .run_with_display_args_and_env(&multipart.program, &args, &args, &multipart.environment)
            .map(|_| ())
            .map_err(|error| RemoteUploadCancellationCleanupError::new(error.to_string()))
    }
}

impl RemoteUploadCancellationCleanupWorker for RemoteUploadCancellationCleanupRuntime<'_> {
    fn cleanup(
        &self,
        action: &RemoteUploadCancellationCleanupAction,
    ) -> Result<(), RemoteUploadCancellationCleanupError> {
        match action.scope {
            RemoteUploadCancellationCleanupScope::PartialSsdStage => {
                Self::remove_managed_path(&self.config.ssd_stage_root, &action.identifier)
            }
            RemoteUploadCancellationCleanupScope::FailedMultipartUpload => {
                self.abort_multipart_upload(&action.identifier)
            }
            RemoteUploadCancellationCleanupScope::AbandonedSession => {
                Self::remove_managed_path(&self.config.session_state_root, &action.identifier)
            }
            RemoteUploadCancellationCleanupScope::ExpiredPairing => {
                Self::remove_managed_path(&self.config.pairing_state_root, &action.identifier)
            }
            RemoteUploadCancellationCleanupScope::InterruptedBrowserHandoff => {
                Self::remove_managed_path(
                    &self.config.browser_handoff_state_root,
                    &action.identifier,
                )
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupError {
    message: String,
}

impl RemoteUploadCancellationCleanupError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for RemoteUploadCancellationCleanupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteUploadCancellationCleanupError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCancellationCleanupAction {
    pub scope: RemoteUploadCancellationCleanupScope,
    pub identifier: String,
    pub required: bool,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteUploadCancellationCleanupScope {
    PartialSsdStage,
    FailedMultipartUpload,
    AbandonedSession,
    ExpiredPairing,
    InterruptedBrowserHandoff,
}

pub fn plan_remote_upload_cancellation_cleanup(
    request: RemoteUploadCancellationCleanupRequest,
) -> RemoteUploadCancellationCleanupPlan {
    let reason = request
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("remote upload was cancelled or interrupted")
        .to_string();
    let mut actions = Vec::new();

    if let Some(prefix) = non_blank(request.staged_object_prefix.as_deref()) {
        actions.push(RemoteUploadCancellationCleanupAction {
            scope: RemoteUploadCancellationCleanupScope::PartialSsdStage,
            identifier: prefix.to_string(),
            required: true,
            reason: format!("remove partial SSD-staged objects because {reason}"),
        });
    }
    if let Some(upload_id) = non_blank(request.multipart_upload_id.as_deref()) {
        actions.push(RemoteUploadCancellationCleanupAction {
            scope: RemoteUploadCancellationCleanupScope::FailedMultipartUpload,
            identifier: upload_id.to_string(),
            required: true,
            reason: format!("abort incomplete S3 multipart upload because {reason}"),
        });
    }
    if let Some(session_id) = non_blank(request.session_id.as_deref()) {
        actions.push(RemoteUploadCancellationCleanupAction {
            scope: RemoteUploadCancellationCleanupScope::AbandonedSession,
            identifier: session_id.to_string(),
            required: false,
            reason: format!(
                "revoke remote upload session if no active job owns it because {reason}"
            ),
        });
    }
    if let Some(pairing_id) = non_blank(request.pairing_id.as_deref()) {
        actions.push(RemoteUploadCancellationCleanupAction {
            scope: RemoteUploadCancellationCleanupScope::ExpiredPairing,
            identifier: pairing_id.to_string(),
            required: false,
            reason: format!("expire unused easyconnect pairing because {reason}"),
        });
    }
    if let Some(handoff_id) = non_blank(request.browser_handoff_id.as_deref()) {
        actions.push(RemoteUploadCancellationCleanupAction {
            scope: RemoteUploadCancellationCleanupScope::InterruptedBrowserHandoff,
            identifier: handoff_id.to_string(),
            required: false,
            reason: format!("close browser handoff state because {reason}"),
        });
    }

    RemoteUploadCancellationCleanupPlan {
        job_id: request.job_id,
        object_store: request.object_store,
        source_bytes: request.source_bytes,
        resumable: actions.iter().all(|action| !action.required),
        actions,
    }
}

pub fn run_remote_upload_cancellation_cleanup(
    plan: RemoteUploadCancellationCleanupPlan,
    worker: &dyn RemoteUploadCancellationCleanupWorker,
) -> RemoteUploadCancellationCleanupRunReport {
    let action_reports = plan
        .actions
        .iter()
        .cloned()
        .map(|action| match worker.cleanup(&action) {
            Ok(()) => RemoteUploadCancellationCleanupActionReport {
                action,
                state: RemoteUploadCancellationCleanupActionState::Complete,
                error: None,
            },
            Err(error) => RemoteUploadCancellationCleanupActionReport {
                action,
                state: RemoteUploadCancellationCleanupActionState::Failed,
                error: Some(error.to_string()),
            },
        })
        .collect();

    RemoteUploadCancellationCleanupRunReport {
        plan,
        action_reports,
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn require_cleanup_field(
    field: &str,
    value: &str,
) -> Result<(), RemoteUploadCancellationCleanupError> {
    if value.trim().is_empty() {
        return Err(RemoteUploadCancellationCleanupError::new(format!(
            "{field} must not be blank"
        )));
    }
    Ok(())
}

fn safe_child_path(
    root: &Path,
    identifier: &str,
) -> Result<PathBuf, RemoteUploadCancellationCleanupError> {
    require_cleanup_field("cleanup identifier", identifier)?;
    let relative = Path::new(identifier);
    let mut child = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => child.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(RemoteUploadCancellationCleanupError::new(format!(
                    "cleanup identifier must stay under managed root: {identifier}"
                )));
            }
        }
    }
    if child.as_os_str().is_empty() {
        return Err(RemoteUploadCancellationCleanupError::new(
            "cleanup identifier must not be empty",
        ));
    }
    Ok(root.join(child))
}
