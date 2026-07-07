use crate::api::{
    DaemonJobId, DaemonRequestValidationError, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceOperation, DaemonServiceStatusDetail,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse,
};
use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
use serde_json::Value;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

pub trait ServiceCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceCommandOutput {
    pub stdout: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemServiceCommandRunner;

impl ServiceCommandRunner for SystemServiceCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        let output = Command::new(program).args(args).output().map_err(|error| {
            DaemonServiceRuntimeError::CommandIo {
                program: program.to_string(),
                message: error.to_string(),
            }
        })?;
        if !output.status.success() {
            return Err(DaemonServiceRuntimeError::CommandFailed {
                program: program.to_string(),
                args: args.to_vec(),
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
    Validation(DaemonRequestValidationError),
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
            Self::Validation(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for DaemonServiceRuntimeError {}

impl From<DaemonRequestValidationError> for DaemonServiceRuntimeError {
    fn from(error: DaemonRequestValidationError) -> Self {
        Self::Validation(error)
    }
}

fn docker_compose_args(
    config: &GarageServiceRuntimeConfig,
    action_args: impl IntoIterator<Item = &'static str>,
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
    args.extend(action_args.into_iter().map(String::from));
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
}

#[cfg(test)]
impl FakeRunner {
    fn with_stdout(stdout: impl Into<String>) -> Self {
        Self {
            output: std::cell::RefCell::new(ServiceCommandOutput {
                stdout: stdout.into(),
            }),
            calls: std::cell::RefCell::new(Vec::new()),
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
}

#[cfg(test)]
mod tests {
    use super::{GarageServiceController, GarageServiceRuntimeConfig};
    use crate::api::{
        DaemonServiceLifecycleRequest, DaemonServiceOperation, DaemonServiceStatusRequest,
    };
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::path::PathBuf;

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
}
