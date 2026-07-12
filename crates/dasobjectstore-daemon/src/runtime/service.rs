use super::{
    admin_jobs::AdminJobRegistry,
    remote_upload::{
        run_remote_easyconnect_aws_cli_upload_job, RemoteEasyconnectAwsCliUploadJobRequest,
        RemoteUploadAdmissionGate, RemoteUploadS3TransferWorkerReport,
    },
};
use crate::api::{
    DaemonJobId, DaemonRequestValidationError, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceOperation, DaemonServiceStatusDetail,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse, StoreRepairS3Reconciliation,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_object_service::{
    default_garage_credential_registry_path, generate_per_store_credentials,
    plan_garage_provisioning, plan_store_service_layout, read_store_registry,
    resolve_managed_store_credentials, GarageProvisioningCommandKind, ObjectServiceError,
    ObjectServiceProviderId, ServiceState, StoreServiceCredential, SystemCredentialEntropy,
};
use serde_json::Value;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageServiceRuntimeConfig {
    pub compose_file: PathBuf,
    pub project_directory: Option<PathBuf>,
    pub compose_project: String,
    pub service_name: String,
    pub config_path: PathBuf,
    pub metadata_path: PathBuf,
    pub data_path: PathBuf,
    pub endpoint: String,
}

impl GarageServiceRuntimeConfig {
    pub fn validate(&self) -> Result<(), DaemonServiceRuntimeError> {
        require_absolute_path("compose_file", &self.compose_file)?;
        if let Some(project_directory) = &self.project_directory {
            require_absolute_path("project_directory", project_directory)?;
        }
        require_nonblank("compose_project", &self.compose_project)?;
        require_nonblank("service_name", &self.service_name)?;
        require_absolute_path("config_path", &self.config_path)?;
        require_absolute_path("metadata_path", &self.metadata_path)?;
        require_absolute_path("data_path", &self.data_path)?;
        require_nonblank("endpoint", &self.endpoint)?;
        Ok(())
    }

    fn status_detail(&self) -> DaemonServiceStatusDetail {
        DaemonServiceStatusDetail {
            compose_project: self.compose_project.clone(),
            service_name: self.service_name.clone(),
            config_path: self.config_path.to_string_lossy().to_string(),
            metadata_path: self.metadata_path.to_string_lossy().to_string(),
            data_path: self.data_path.to_string_lossy().to_string(),
        }
    }
}

pub struct GarageServiceController<R> {
    config: GarageServiceRuntimeConfig,
    runner: R,
}

impl<R> GarageServiceController<R>
where
    R: ServiceCommandRunner,
{
    pub fn new(config: GarageServiceRuntimeConfig, runner: R) -> Self {
        Self { config, runner }
    }

    pub fn status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        self.config.validate()?;
        let output = self.runner.run(
            "docker",
            &docker_compose_args(&self.config, ["ps", "--format", "json"]),
        )?;
        let state = parse_compose_service_state(&output.stdout, &self.config.service_name)?;

        Ok(DaemonServiceStatusResponse {
            provider_id: ObjectServiceProviderId::Garage,
            state,
            endpoint: Some(self.config.endpoint.clone()),
            message: None,
            detail: request.include_detail.then(|| self.config.status_detail()),
        })
    }

    pub fn lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
        accepted_at_utc: impl AsRef<str>,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        request.validate()?;
        self.config.validate()?;

        let accepted_at_utc = accepted_at_utc.as_ref();
        if !request.dry_run {
            self.runner.run(
                "docker",
                &docker_compose_args(&self.config, operation_args(request.operation)),
            )?;
        }

        Ok(DaemonServiceLifecycleResponse::accepted(
            service_job_id(request.operation, accepted_at_utc)?,
            accepted_at_utc,
            request.dry_run,
            request.operation,
            ObjectServiceProviderId::Garage,
        ))
    }

    pub fn prepare_enclosure(
        &self,
        request: crate::api::PrepareEnclosureRequest,
        accepted_at_utc: impl AsRef<str>,
    ) -> Result<crate::api::PrepareEnclosureResponse, DaemonServiceRuntimeError> {
        self.config.validate()?;
        super::enclosure_prepare::prepare_enclosure(&self.runner, request, accepted_at_utc.as_ref())
    }

    pub fn provision_buckets(
        &self,
        credentials: &[StoreServiceCredential],
    ) -> Result<GarageProvisioningSummary, DaemonServiceRuntimeError> {
        self.config.validate()?;
        let plan = plan_garage_provisioning(credentials)?;
        for command in &plan.commands {
            let raw_args = docker_compose_args(
                &self.config,
                garage_exec_args(&self.config.service_name, command.argv()),
            );
            let redacted_args = docker_compose_args(
                &self.config,
                garage_exec_args(&self.config.service_name, command.redacted_argv()),
            );
            if let Err(error) =
                self.runner
                    .run_with_display_args("docker", &raw_args, &redacted_args)
            {
                if is_idempotent_provisioning_conflict(command.kind, &error) {
                    continue;
                }
                return Err(error);
            }
        }

        Ok(GarageProvisioningSummary {
            stores: credentials.len(),
            buckets: plan.bucket_count(),
            commands: plan.commands.len(),
        })
    }

    pub fn remote_easyconnect_aws_cli_upload_job(
        &self,
        registry: &dyn AdminJobRegistry,
        gate: Arc<RemoteUploadAdmissionGate>,
        request: RemoteEasyconnectAwsCliUploadJobRequest,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError> {
        run_remote_easyconnect_aws_cli_upload_job(registry, gate, &self.runner, request)
    }

    /// Pulls a provisioned Garage bucket to a private SSD staging area and then
    /// delegates every byte to the normal RemoteS3 ingest pipeline.  The bucket
    /// is intentionally never treated as a metadata authority.
    pub fn reconcile_store_s3(
        &self,
        store_id: StoreId,
        prefix: Option<String>,
        dry_run: bool,
        accepted_at_utc: &str,
        emit_progress: &mut dyn FnMut(
            crate::api::DaemonIngestProgressEvent,
        )
            -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
    ) -> Result<StoreRepairS3Reconciliation, DaemonServiceRuntimeError> {
        self.reconcile_store_s3_cancellable(
            store_id,
            prefix,
            dry_run,
            accepted_at_utc,
            &|| false,
            emit_progress,
        )
    }

    pub fn reconcile_store_s3_cancellable(
        &self,
        store_id: StoreId,
        prefix: Option<String>,
        dry_run: bool,
        accepted_at_utc: &str,
        is_cancelled: &dyn Fn() -> bool,
        emit_progress: &mut dyn FnMut(
            crate::api::DaemonIngestProgressEvent,
        )
            -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
    ) -> Result<StoreRepairS3Reconciliation, DaemonServiceRuntimeError> {
        super::service_reconciliation::reconcile_store_s3(
            &self.config,
            &self.runner,
            store_id,
            prefix,
            dry_run,
            accepted_at_utc,
            is_cancelled,
            emit_progress,
        )
    }
}

fn is_idempotent_provisioning_conflict(
    command_kind: GarageProvisioningCommandKind,
    error: &DaemonServiceRuntimeError,
) -> bool {
    let DaemonServiceRuntimeError::CommandFailed { stderr, .. } = error else {
        return false;
    };
    match command_kind {
        GarageProvisioningCommandKind::ImportKey => stderr.contains("KeyAlreadyExists"),
        GarageProvisioningCommandKind::CreateBucket => {
            stderr.contains("BucketAlreadyExists") || stderr.contains("bucket already exists")
        }
        GarageProvisioningCommandKind::AllowBucket => false,
    }
}

pub trait ServiceCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError>;

    fn run_with_display_args(
        &self,
        program: &str,
        args: &[String],
        _display_args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        self.run(program, args)
    }

    fn run_with_display_args_and_env(
        &self,
        program: &str,
        args: &[String],
        display_args: &[String],
        _environment: &[(String, String)],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        self.run_with_display_args(program, args, display_args)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceCommandOutput {
    pub stdout: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageProvisioningSummary {
    pub stores: usize,
    pub buckets: usize,
    pub commands: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GarageStoreRegistryProvisioningSummary {
    pub registry_path: PathBuf,
    pub credential_registry_path: PathBuf,
    pub stores: usize,
    pub buckets: usize,
    pub commands: usize,
    pub credentials_issued: usize,
    pub credentials_reused: usize,
    pub credentials_rotated: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemServiceCommandRunner;

impl ServiceCommandRunner for SystemServiceCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        self.run_with_display_args(program, args, args)
    }

    fn run_with_display_args(
        &self,
        program: &str,
        args: &[String],
        display_args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        self.run_with_display_args_and_env(program, args, display_args, &[])
    }

    fn run_with_display_args_and_env(
        &self,
        program: &str,
        args: &[String],
        display_args: &[String],
        environment: &[(String, String)],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        let output = Command::new(program)
            .args(args)
            .envs(environment.iter().map(|(name, value)| (name, value)))
            .output()
            .map_err(|error| DaemonServiceRuntimeError::CommandIo {
                program: program.to_string(),
                message: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(DaemonServiceRuntimeError::CommandFailed {
                program: program.to_string(),
                args: display_args.to_vec(),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(ServiceCommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonServiceRuntimeError {
    BlankField {
        field: &'static str,
    },
    RelativePath {
        field: &'static str,
        path: PathBuf,
    },
    CommandIo {
        program: String,
        message: String,
    },
    CommandFailed {
        program: String,
        args: Vec<String>,
        status: String,
        stderr: String,
    },
    InvalidStatusJson(String),
    ServiceNotPresent {
        service_name: String,
    },
    InvalidJobId(String),
    JobNotFound {
        job_id: String,
    },
    JobRegistryIo {
        path: PathBuf,
        message: String,
    },
    InvalidJobRegistryJson {
        path: PathBuf,
        message: String,
    },
    EndpointRegistryIo {
        path: PathBuf,
        message: String,
    },
    InvalidEndpointRegistryJson {
        path: PathBuf,
        message: String,
    },
    UnsupportedOperation {
        operation: String,
    },
    Validation(DaemonRequestValidationError),
    ObjectService(ObjectServiceError),
}

impl Display for DaemonServiceRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::CommandIo { program, message } => {
                write!(formatter, "failed to run {program}: {message}")
            }
            Self::CommandFailed {
                program,
                args,
                status,
                stderr,
            } => write!(
                formatter,
                "{} {} exited with status {}: {}",
                program,
                args.join(" "),
                status,
                stderr.trim()
            ),
            Self::InvalidStatusJson(message) => {
                write!(formatter, "invalid Docker Compose status JSON: {message}")
            }
            Self::ServiceNotPresent { service_name } => {
                write!(
                    formatter,
                    "Docker Compose service is not present: {service_name}"
                )
            }
            Self::InvalidJobId(value) => write!(formatter, "invalid service job id: {value}"),
            Self::JobNotFound { job_id } => write!(formatter, "daemon job not found: {job_id}"),
            Self::JobRegistryIo { path, message } => write!(
                formatter,
                "failed to access daemon job registry {}: {message}",
                path.display()
            ),
            Self::InvalidJobRegistryJson { path, message } => write!(
                formatter,
                "invalid daemon job registry JSON at {}: {message}",
                path.display()
            ),
            Self::EndpointRegistryIo { path, message } => write!(
                formatter,
                "failed to access endpoint inventory registry {}: {message}",
                path.display()
            ),
            Self::InvalidEndpointRegistryJson { path, message } => write!(
                formatter,
                "invalid endpoint inventory registry JSON at {}: {message}",
                path.display()
            ),
            Self::UnsupportedOperation { operation } => {
                write!(
                    formatter,
                    "unsupported daemon service operation: {operation}"
                )
            }
            Self::Validation(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for DaemonServiceRuntimeError {}

impl From<DaemonRequestValidationError> for DaemonServiceRuntimeError {
    fn from(error: DaemonRequestValidationError) -> Self {
        Self::Validation(error)
    }
}

impl From<ObjectServiceError> for DaemonServiceRuntimeError {
    fn from(error: ObjectServiceError) -> Self {
        Self::ObjectService(error)
    }
}

pub fn provision_garage_store_registry<R>(
    controller: &GarageServiceController<R>,
    registry_path: impl Into<PathBuf>,
    dry_run: bool,
    rotate_credentials: bool,
    accepted_at_utc: &str,
) -> Result<GarageStoreRegistryProvisioningSummary, DaemonServiceRuntimeError>
where
    R: ServiceCommandRunner,
{
    provision_garage_store_registry_with_credentials_path(
        controller,
        registry_path,
        default_garage_credential_registry_path(),
        dry_run,
        rotate_credentials,
        accepted_at_utc,
    )
}

fn provision_garage_store_registry_with_credentials_path<R>(
    controller: &GarageServiceController<R>,
    registry_path: impl Into<PathBuf>,
    credential_registry_path: impl Into<PathBuf>,
    dry_run: bool,
    rotate_credentials: bool,
    accepted_at_utc: &str,
) -> Result<GarageStoreRegistryProvisioningSummary, DaemonServiceRuntimeError>
where
    R: ServiceCommandRunner,
{
    let registry_path = registry_path.into();
    let credential_registry_path = credential_registry_path.into();
    let definitions = read_store_registry(&registry_path)?;
    let layout = plan_store_service_layout(&definitions)?;
    let mut entropy = SystemCredentialEntropy;
    let (credentials, credentials_issued, credentials_reused, credentials_rotated) = if dry_run {
        let credentials =
            generate_per_store_credentials(&layout.credential_requests, &mut entropy)?;
        (credentials, 0, 0, 0)
    } else {
        let resolution = resolve_managed_store_credentials(
            &credential_registry_path,
            &layout.credential_requests,
            accepted_at_utc,
            rotate_credentials,
            &mut entropy,
        )?;
        (
            resolution.credentials,
            resolution.issued,
            resolution.reused,
            resolution.rotated,
        )
    };
    let summary = if dry_run {
        let plan = plan_garage_provisioning(&credentials)?;
        GarageProvisioningSummary {
            stores: credentials.len(),
            buckets: plan.bucket_count(),
            commands: plan.commands.len(),
        }
    } else {
        controller.provision_buckets(&credentials)?
    };

    Ok(GarageStoreRegistryProvisioningSummary {
        registry_path,
        credential_registry_path,
        stores: summary.stores,
        buckets: summary.buckets,
        commands: summary.commands,
        credentials_issued,
        credentials_reused,
        credentials_rotated,
    })
}

fn docker_compose_args(
    config: &GarageServiceRuntimeConfig,
    action_args: impl IntoIterator<Item = impl Into<String>>,
) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "-p".to_string(),
        config.compose_project.clone(),
        "-f".to_string(),
        config.compose_file.to_string_lossy().to_string(),
    ];
    if let Some(project_directory) = &config.project_directory {
        args.push("--project-directory".to_string());
        args.push(project_directory.to_string_lossy().to_string());
    }
    args.extend(action_args.into_iter().map(Into::into));
    args
}

fn garage_exec_args(service_name: &str, garage_args: Vec<String>) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        "-T".to_string(),
        service_name.to_string(),
        "/garage".to_string(),
    ];
    args.extend(garage_args);
    args
}

fn operation_args(operation: DaemonServiceOperation) -> Vec<&'static str> {
    match operation {
        DaemonServiceOperation::Start => vec!["up", "-d"],
        DaemonServiceOperation::Stop => vec!["down"],
        DaemonServiceOperation::Restart => vec!["restart"],
    }
}

fn parse_compose_service_state(
    stdout: &str,
    service_name: &str,
) -> Result<ServiceState, DaemonServiceRuntimeError> {
    let value: Value = serde_json::from_str(stdout)
        .map_err(|error| DaemonServiceRuntimeError::InvalidStatusJson(error.to_string()))?;
    let services = value.as_array().ok_or_else(|| {
        DaemonServiceRuntimeError::InvalidStatusJson("expected array".to_string())
    })?;

    let service = services
        .iter()
        .find(|entry| {
            field_eq(entry, "Service", service_name) || field_eq(entry, "Name", service_name)
        })
        .ok_or_else(|| DaemonServiceRuntimeError::ServiceNotPresent {
            service_name: service_name.to_string(),
        })?;
    let state = service
        .get("State")
        .and_then(Value::as_str)
        .or_else(|| service.get("Status").and_then(Value::as_str))
        .unwrap_or("unknown");

    Ok(match state.to_ascii_lowercase().as_str() {
        "running" => ServiceState::Running,
        "created" | "restarting" => ServiceState::Starting,
        "paused" => ServiceState::Degraded,
        "exited" | "dead" | "removing" => ServiceState::Stopped,
        "unknown" => ServiceState::Unknown,
        _ => ServiceState::Failed,
    })
}

fn field_eq(value: &Value, field: &str, expected: &str) -> bool {
    value.get(field).and_then(Value::as_str) == Some(expected)
}

fn service_job_id(
    operation: DaemonServiceOperation,
    accepted_at_utc: &str,
) -> Result<DaemonJobId, DaemonServiceRuntimeError> {
    let operation = match operation {
        DaemonServiceOperation::Start => "start",
        DaemonServiceOperation::Stop => "stop",
        DaemonServiceOperation::Restart => "restart",
    };
    let timestamp = accepted_at_utc
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    let value = format!("service-{operation}-{timestamp}");
    DaemonJobId::new(value.clone()).map_err(|_| DaemonServiceRuntimeError::InvalidJobId(value))
}

fn require_nonblank(field: &'static str, value: &str) -> Result<(), DaemonServiceRuntimeError> {
    if value.trim().is_empty() {
        return Err(DaemonServiceRuntimeError::BlankField { field });
    }
    Ok(())
}

fn require_absolute_path(
    field: &'static str,
    path: &Path,
) -> Result<(), DaemonServiceRuntimeError> {
    if !path.is_absolute() {
        return Err(DaemonServiceRuntimeError::RelativePath {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
struct FakeRunner {
    output: std::cell::RefCell<ServiceCommandOutput>,
    calls: std::cell::RefCell<Vec<(String, Vec<String>)>>,
    display_calls: std::cell::RefCell<Vec<(String, Vec<String>)>>,
    fail_with_display_args: bool,
    bucket_already_exists: bool,
}

#[cfg(test)]
impl FakeRunner {
    fn with_stdout(stdout: impl Into<String>) -> Self {
        Self {
            output: std::cell::RefCell::new(ServiceCommandOutput {
                stdout: stdout.into(),
            }),
            calls: std::cell::RefCell::new(Vec::new()),
            display_calls: std::cell::RefCell::new(Vec::new()),
            fail_with_display_args: false,
            bucket_already_exists: false,
        }
    }

    fn failing() -> Self {
        Self {
            fail_with_display_args: true,
            ..Self::with_stdout("")
        }
    }

    fn bucket_already_exists() -> Self {
        Self {
            bucket_already_exists: true,
            ..Self::with_stdout("")
        }
    }
}

#[cfg(test)]
impl ServiceCommandRunner for FakeRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        self.calls
            .borrow_mut()
            .push((program.to_string(), args.to_vec()));
        Ok(self.output.borrow().clone())
    }

    fn run_with_display_args(
        &self,
        program: &str,
        args: &[String],
        display_args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        self.calls
            .borrow_mut()
            .push((program.to_string(), args.to_vec()));
        self.display_calls
            .borrow_mut()
            .push((program.to_string(), display_args.to_vec()));
        if self.fail_with_display_args {
            return Err(DaemonServiceRuntimeError::CommandFailed {
                program: program.to_string(),
                args: display_args.to_vec(),
                status: "exit status: 1".to_string(),
                stderr: "failed".to_string(),
            });
        }
        if self.bucket_already_exists && args.iter().any(|arg| arg == "create") {
            return Err(DaemonServiceRuntimeError::CommandFailed {
                program: program.to_string(),
                args: display_args.to_vec(),
                status: "exit status: 1".to_string(),
                stderr: "Error: CreateBucket returned BucketAlreadyExists (409)".to_string(),
            });
        }
        if self.bucket_already_exists && args.iter().any(|arg| arg == "import") {
            return Err(DaemonServiceRuntimeError::CommandFailed {
                program: program.to_string(),
                args: display_args.to_vec(),
                status: "exit status: 1".to_string(),
                stderr: "Error: ImportKey returned KeyAlreadyExists (409)".to_string(),
            });
        }
        Ok(self.output.borrow().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{GarageServiceController, GarageServiceRuntimeConfig};
    use crate::api::{
        DaemonServiceLifecycleRequest, DaemonServiceOperation, DaemonServiceStatusRequest,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use dasobjectstore_object_service::{
        generate_per_store_credentials, CredentialEntropy, ObjectServiceError,
        ObjectServiceProviderId, ServiceState, StoreCredentialRequest, StoreServiceCredential,
        StoreServiceDefinition,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn status_parses_running_compose_service() {
        let runner = super::FakeRunner::with_stdout(
            r#"[{"Name":"garage","Service":"garage","State":"running"}]"#,
        );
        let controller = GarageServiceController::new(config(), runner);

        let status = controller
            .status(DaemonServiceStatusRequest {
                include_detail: true,
            })
            .expect("status parsed");

        assert_eq!(status.provider_id, ObjectServiceProviderId::Garage);
        assert_eq!(status.state, ServiceState::Running);
        assert_eq!(status.endpoint.as_deref(), Some("http://127.0.0.1:3900"));
        assert_eq!(status.detail.expect("detail").service_name, "garage");
    }

    #[test]
    fn lifecycle_start_runs_compose_up() {
        let runner = super::FakeRunner::with_stdout("[]");
        let controller = GarageServiceController::new(config(), runner);

        let response = controller
            .lifecycle(
                DaemonServiceLifecycleRequest {
                    operation: DaemonServiceOperation::Start,
                    provider_id: ObjectServiceProviderId::Garage,
                    dry_run: false,
                    client_request_id: None,
                },
                "2026-07-07T11:42:12Z",
            )
            .expect("lifecycle accepted");

        assert_eq!(response.operation, DaemonServiceOperation::Start);
        assert!(!response.accepted.dry_run);
        let calls = controller.runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "docker");
        assert!(calls[0]
            .1
            .windows(2)
            .any(|args| args[0] == "-p" && args[1] == "dasobjectstore"));
        assert_eq!(calls[0].1.last().map(String::as_str), Some("-d"));
    }

    #[test]
    fn lifecycle_dry_run_does_not_execute_compose() {
        let runner = super::FakeRunner::with_stdout("[]");
        let controller = GarageServiceController::new(config(), runner);

        let response = controller
            .lifecycle(
                DaemonServiceLifecycleRequest {
                    operation: DaemonServiceOperation::Restart,
                    provider_id: ObjectServiceProviderId::Garage,
                    dry_run: true,
                    client_request_id: Some("request-a".to_string()),
                },
                "2026-07-07T11:42:12Z",
            )
            .expect("lifecycle accepted");

        assert!(response.accepted.dry_run);
        assert!(controller.runner.calls.borrow().is_empty());
    }

    #[test]
    fn provision_buckets_runs_garage_commands_through_compose() {
        let credentials = credentials();
        let runner = super::FakeRunner::with_stdout("");
        let controller = GarageServiceController::new(config(), runner);

        let summary = controller
            .provision_buckets(&credentials)
            .expect("buckets provisioned");

        assert_eq!(summary.buckets, 1);
        assert_eq!(summary.commands, 3);
        let calls = controller.runner.calls.borrow();
        assert_eq!(calls.len(), 3);
        assert!(calls[0]
            .1
            .windows(4)
            .any(|args| args == ["exec", "-T", "garage", "/garage"]));
        assert!(calls[0].1.contains(&"import".to_string()));
        assert!(calls[1].1.ends_with(&[
            "bucket".to_string(),
            "create".to_string(),
            "dos-generated".to_string()
        ]));
        assert!(calls[2].1.contains(&"--owner".to_string()));
    }

    #[test]
    fn provision_buckets_redacts_secret_from_failed_command_error() {
        let credentials = credentials();
        let secret = credentials[0].secret_access_key.expose_secret().to_string();
        let runner = super::FakeRunner::failing();
        let controller = GarageServiceController::new(config(), runner);

        let err = controller
            .provision_buckets(&credentials)
            .expect_err("failure returned");
        let message = err.to_string();

        assert!(!message.contains(&secret));
        assert!(message.contains("<redacted>"));
    }

    #[test]
    fn provision_buckets_treats_existing_key_and_bucket_as_idempotent() {
        let credentials = credentials();
        let runner = super::FakeRunner::bucket_already_exists();
        let controller = GarageServiceController::new(config(), runner);

        let summary = controller
            .provision_buckets(&credentials)
            .expect("existing bucket is safe to reuse");

        assert_eq!(summary.buckets, 1);
        assert_eq!(controller.runner.calls.borrow().len(), 3);
    }

    #[test]
    fn provision_store_registry_dry_run_counts_registry_bindings_without_commands() {
        let registry_path = write_store_registry();
        let credential_registry_path = temp_root().join("garage-credentials.json");
        let runner = super::FakeRunner::with_stdout("");
        let controller = GarageServiceController::new(config(), runner);

        let summary = super::provision_garage_store_registry_with_credentials_path(
            &controller,
            &registry_path,
            &credential_registry_path,
            true,
            false,
            "2026-07-09T10:00:00Z",
        )
        .expect("registry provision planned");

        assert_eq!(summary.registry_path, registry_path);
        assert_eq!(summary.credential_registry_path, credential_registry_path);
        assert_eq!(summary.stores, 1);
        assert_eq!(summary.buckets, 1);
        assert_eq!(summary.commands, 3);
        assert_eq!(summary.credentials_issued, 0);
        assert_eq!(summary.credentials_reused, 0);
        assert_eq!(summary.credentials_rotated, 0);
        assert!(controller.runner.calls.borrow().is_empty());
    }

    #[test]
    fn provision_store_registry_executes_commands_from_registry_bindings() {
        let registry_path = write_store_registry();
        let credential_registry_path = temp_root().join("garage-credentials.json");
        let runner = super::FakeRunner::with_stdout("");
        let controller = GarageServiceController::new(config(), runner);

        let summary = super::provision_garage_store_registry_with_credentials_path(
            &controller,
            &registry_path,
            &credential_registry_path,
            false,
            false,
            "2026-07-09T10:00:00Z",
        )
        .expect("registry provisioned");

        assert_eq!(summary.stores, 1);
        assert_eq!(summary.commands, 3);
        assert_eq!(summary.credentials_issued, 1);
        assert_eq!(summary.credentials_reused, 0);
        assert_eq!(summary.credentials_rotated, 0);
        assert_eq!(controller.runner.calls.borrow().len(), 3);
    }

    #[test]
    fn provision_store_registry_reuses_persisted_credentials_on_repeated_runs() {
        let registry_path = write_store_registry();
        let credential_registry_path = temp_root().join("garage-credentials.json");
        let runner = super::FakeRunner::with_stdout("");
        let controller = GarageServiceController::new(config(), runner);

        let first = super::provision_garage_store_registry_with_credentials_path(
            &controller,
            &registry_path,
            &credential_registry_path,
            false,
            false,
            "2026-07-09T10:00:00Z",
        )
        .expect("registry provisioned");
        let second = super::provision_garage_store_registry_with_credentials_path(
            &controller,
            &registry_path,
            &credential_registry_path,
            false,
            false,
            "2026-07-09T10:10:00Z",
        )
        .expect("registry provisioned again");

        assert_eq!(first.credentials_issued, 1);
        assert_eq!(first.credentials_reused, 0);
        assert_eq!(second.credentials_issued, 0);
        assert_eq!(second.credentials_reused, 1);
        assert_eq!(controller.runner.calls.borrow().len(), 6);
    }

    #[test]
    fn list_garage_objects_decodes_key_and_size() {
        let runner = super::FakeRunner::with_stdout(
            r#"{"Contents":[{"Key":"run-42/data.bin","Size":12}],"IsTruncated":false}"#,
        );
        let objects = crate::runtime::service_reconciliation::list_garage_objects(
            &runner,
            "http://127.0.0.1:3900",
            "dos-generated",
            Some("run-42"),
            &[],
        )
        .expect("object listing decoded");
        assert_eq!(
            objects,
            vec![crate::runtime::ReconciliationObject {
                key: "run-42/data.bin".to_string(),
                size_bytes: Some(12),
            }]
        );
        assert_eq!(runner.calls.borrow().len(), 1);
    }

    #[test]
    fn list_garage_objects_rejects_invalid_json() {
        let runner = super::FakeRunner::with_stdout("not-json");
        let error = crate::runtime::service_reconciliation::list_garage_objects(
            &runner,
            "http://127.0.0.1:3900",
            "dos-generated",
            None,
            &[],
        )
        .expect_err("invalid listing should fail closed");
        assert!(error.to_string().contains("invalid JSON"));
    }

    fn config() -> GarageServiceRuntimeConfig {
        GarageServiceRuntimeConfig {
            compose_file: PathBuf::from("/etc/dasobjectstore/garage.compose.yml"),
            project_directory: Some(PathBuf::from("/var/lib/dasobjectstore/garage")),
            compose_project: "dasobjectstore".to_string(),
            service_name: "garage".to_string(),
            config_path: PathBuf::from("/etc/dasobjectstore/garage.toml"),
            metadata_path: PathBuf::from("/var/lib/dasobjectstore/garage/meta"),
            data_path: PathBuf::from("/srv/dasobjectstore/hdd/garage"),
            endpoint: "http://127.0.0.1:3900".to_string(),
        }
    }

    fn credentials() -> Vec<StoreServiceCredential> {
        generate_per_store_credentials(
            &[StoreCredentialRequest {
                store_id: StoreId::new("generated").expect("store id"),
                bucket_name: "dos-generated".to_string(),
            }],
            &mut FixedEntropy::default(),
        )
        .expect("credentials generated")
    }

    fn write_store_registry() -> PathBuf {
        let path = temp_root().join("stores.json");
        let definitions = vec![StoreServiceDefinition {
            store_id: StoreId::new("generated").expect("store id"),
            policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
            bucket_name: Some("dos-generated".to_string()),
            reader_group: None,
            writer_group: Some("mnemosyne".to_string()),
            public: false,
        }];
        let parent = path.parent().expect("registry parent");
        fs::create_dir_all(parent).expect("registry dir");
        let file = fs::File::create(&path).expect("registry file");
        serde_json::to_writer_pretty(file, &definitions).expect("registry written");
        path
    }

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-daemon-service-{}-{now}",
            std::process::id()
        ))
    }

    #[derive(Default)]
    struct FixedEntropy {
        next: u8,
    }

    impl CredentialEntropy for FixedEntropy {
        fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError> {
            for byte in bytes {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
    }
}
