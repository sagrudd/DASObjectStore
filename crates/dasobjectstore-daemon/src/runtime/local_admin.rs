use crate::api::{DaemonJobId, DaemonLocalAdminAcceptedResponse, DaemonLocalAdminCommand};
use std::fmt::{self, Display};
use std::process::Command;

pub const LOCAL_ADMIN_CONFIRMATION_MARKER: &str = "confirm local group administration";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalGroupAdministrationRequest {
    pub operation: LocalGroupAdministrationOperation,
    pub group_name: String,
    pub username: Option<String>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_confirmation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalGroupAdministrationOperation {
    CreateGroup,
    AssignUserToGroup,
    Unsupported(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalGroupAdministrationResponse {
    pub accepted: DaemonLocalAdminAcceptedResponse,
    pub operation: LocalGroupAdministrationOperation,
    pub group_name: String,
    pub username: Option<String>,
    pub command: LocalAdminCommandPlan,
    pub executed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAdminCommandPlan {
    pub program: String,
    pub args: Vec<String>,
    pub display_args: Vec<String>,
}

impl LocalAdminCommandPlan {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        let args = args;
        Self {
            program: program.into(),
            display_args: args.clone(),
            args,
        }
    }
}

pub struct LocalGroupAdminController<P, R> {
    planner: P,
    runner: R,
}

impl<R> LocalGroupAdminController<SystemLocalGroupCommandPlanner, R>
where
    R: LocalAdminCommandRunner,
{
    pub fn new(runner: R) -> Self {
        Self {
            planner: SystemLocalGroupCommandPlanner,
            runner,
        }
    }
}

impl<P, R> LocalGroupAdminController<P, R>
where
    P: LocalGroupCommandPlanner,
    R: LocalAdminCommandRunner,
{
    pub fn with_planner(planner: P, runner: R) -> Self {
        Self { planner, runner }
    }

    pub fn execute(
        &self,
        request: LocalGroupAdministrationRequest,
        accepted_at_utc: impl AsRef<str>,
    ) -> Result<LocalGroupAdministrationResponse, LocalAdminRuntimeError> {
        let command = self.planner.plan(&request)?;
        let accepted_at_utc = accepted_at_utc.as_ref();

        if !request.dry_run {
            require_administrator_confirmation(request.administrator_confirmation.as_deref())?;
            self.runner.run_with_display_args(
                &command.program,
                &command.args,
                &command.display_args,
            )?;
        }

        Ok(LocalGroupAdministrationResponse {
            accepted: DaemonLocalAdminAcceptedResponse {
                job_id: local_admin_job_id(&request.operation, accepted_at_utc)?,
                command: local_admin_command(&request.operation)?,
                accepted_at_utc: accepted_at_utc.to_string(),
                dry_run: request.dry_run,
                client_request_id: request.client_request_id.clone(),
            },
            operation: request.operation,
            group_name: request.group_name,
            username: request.username,
            command,
            executed: !request.dry_run,
        })
    }
}

pub trait LocalGroupCommandPlanner {
    fn plan(
        &self,
        request: &LocalGroupAdministrationRequest,
    ) -> Result<LocalAdminCommandPlan, LocalAdminRuntimeError>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemLocalGroupCommandPlanner;

impl LocalGroupCommandPlanner for SystemLocalGroupCommandPlanner {
    fn plan(
        &self,
        request: &LocalGroupAdministrationRequest,
    ) -> Result<LocalAdminCommandPlan, LocalAdminRuntimeError> {
        match &request.operation {
            LocalGroupAdministrationOperation::CreateGroup => {
                let group_name = validated_account_name("group_name", &request.group_name)?;
                Ok(LocalAdminCommandPlan::new(
                    "groupadd",
                    vec!["--".to_string(), group_name.to_string()],
                ))
            }
            LocalGroupAdministrationOperation::AssignUserToGroup => {
                let group_name = validated_account_name("group_name", &request.group_name)?;
                let username = request
                    .username
                    .as_deref()
                    .ok_or(LocalAdminRuntimeError::MissingField { field: "username" })?;
                let username = validated_account_name("username", username)?;
                Ok(LocalAdminCommandPlan::new(
                    "usermod",
                    vec![
                        "-a".to_string(),
                        "-G".to_string(),
                        group_name.to_string(),
                        "--".to_string(),
                        username.to_string(),
                    ],
                ))
            }
            LocalGroupAdministrationOperation::Unsupported(operation) => {
                Err(LocalAdminRuntimeError::UnsupportedOperation {
                    operation: operation.clone(),
                })
            }
        }
    }
}

pub trait LocalAdminCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<LocalAdminCommandOutput, LocalAdminRuntimeError>;

    fn run_with_display_args(
        &self,
        program: &str,
        args: &[String],
        _display_args: &[String],
    ) -> Result<LocalAdminCommandOutput, LocalAdminRuntimeError> {
        self.run(program, args)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAdminCommandOutput {
    pub stdout: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemLocalAdminCommandRunner;

impl LocalAdminCommandRunner for SystemLocalAdminCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<LocalAdminCommandOutput, LocalAdminRuntimeError> {
        self.run_with_display_args(program, args, args)
    }

    fn run_with_display_args(
        &self,
        program: &str,
        args: &[String],
        display_args: &[String],
    ) -> Result<LocalAdminCommandOutput, LocalAdminRuntimeError> {
        let output = Command::new(program).args(args).output().map_err(|error| {
            LocalAdminRuntimeError::CommandIo {
                program: program.to_string(),
                message: error.to_string(),
            }
        })?;
        if !output.status.success() {
            return Err(LocalAdminRuntimeError::CommandFailed {
                program: program.to_string(),
                args: display_args.to_vec(),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(LocalAdminCommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalAdminRuntimeError {
    BlankField {
        field: &'static str,
    },
    MissingField {
        field: &'static str,
    },
    UnsafeAccountName {
        field: &'static str,
        value: String,
    },
    UnsupportedOperation {
        operation: String,
    },
    MissingAdministratorConfirmation {
        required_marker: &'static str,
    },
    AdministratorConfirmationMismatch {
        required_marker: &'static str,
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
    InvalidJobId(String),
}

impl Display for LocalAdminRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::MissingField { field } => write!(formatter, "{field} is required"),
            Self::UnsafeAccountName { field, value } => {
                write!(
                    formatter,
                    "{field} is not a safe local account name: {value}"
                )
            }
            Self::UnsupportedOperation { operation } => {
                write!(formatter, "unsupported local group operation: {operation}")
            }
            Self::MissingAdministratorConfirmation { required_marker } => write!(
                formatter,
                "missing administrator confirmation; pass `{required_marker}`"
            ),
            Self::AdministratorConfirmationMismatch { required_marker } => write!(
                formatter,
                "administrator confirmation mismatch; pass `{required_marker}`"
            ),
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
            Self::InvalidJobId(value) => write!(formatter, "invalid local admin job id: {value}"),
        }
    }
}

impl std::error::Error for LocalAdminRuntimeError {}

fn require_administrator_confirmation(
    confirmation: Option<&str>,
) -> Result<(), LocalAdminRuntimeError> {
    let provided = confirmation
        .ok_or(LocalAdminRuntimeError::MissingAdministratorConfirmation {
            required_marker: LOCAL_ADMIN_CONFIRMATION_MARKER,
        })?
        .trim();
    if provided != LOCAL_ADMIN_CONFIRMATION_MARKER {
        return Err(LocalAdminRuntimeError::AdministratorConfirmationMismatch {
            required_marker: LOCAL_ADMIN_CONFIRMATION_MARKER,
        });
    }
    Ok(())
}

fn validated_account_name<'a>(
    field: &'static str,
    value: &'a str,
) -> Result<&'a str, LocalAdminRuntimeError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(LocalAdminRuntimeError::BlankField { field });
    }
    if value.starts_with('-')
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(LocalAdminRuntimeError::UnsafeAccountName {
            field,
            value: value.to_string(),
        });
    }
    Ok(value)
}

fn local_admin_job_id(
    operation: &LocalGroupAdministrationOperation,
    accepted_at_utc: &str,
) -> Result<DaemonJobId, LocalAdminRuntimeError> {
    let operation = match operation {
        LocalGroupAdministrationOperation::CreateGroup => "create-group",
        LocalGroupAdministrationOperation::AssignUserToGroup => "assign-user-to-group",
        LocalGroupAdministrationOperation::Unsupported(operation) => operation,
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
    let value = format!("local-group-{operation}-{timestamp}");
    DaemonJobId::new(value.clone()).map_err(|_| LocalAdminRuntimeError::InvalidJobId(value))
}

fn local_admin_command(
    operation: &LocalGroupAdministrationOperation,
) -> Result<DaemonLocalAdminCommand, LocalAdminRuntimeError> {
    match operation {
        LocalGroupAdministrationOperation::CreateGroup => {
            Ok(DaemonLocalAdminCommand::CreateLocalGroup)
        }
        LocalGroupAdministrationOperation::AssignUserToGroup => {
            Ok(DaemonLocalAdminCommand::AssignLocalUserToLocalGroup)
        }
        LocalGroupAdministrationOperation::Unsupported(operation) => {
            Err(LocalAdminRuntimeError::UnsupportedOperation {
                operation: operation.clone(),
            })
        }
    }
}

#[cfg(test)]
struct FakeLocalAdminRunner {
    calls: std::cell::RefCell<Vec<(String, Vec<String>, Vec<String>)>>,
}

#[cfg(test)]
impl FakeLocalAdminRunner {
    fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl LocalAdminCommandRunner for FakeLocalAdminRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<LocalAdminCommandOutput, LocalAdminRuntimeError> {
        self.run_with_display_args(program, args, args)
    }

    fn run_with_display_args(
        &self,
        program: &str,
        args: &[String],
        display_args: &[String],
    ) -> Result<LocalAdminCommandOutput, LocalAdminRuntimeError> {
        self.calls
            .borrow_mut()
            .push((program.to_string(), args.to_vec(), display_args.to_vec()));
        Ok(LocalAdminCommandOutput {
            stdout: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FakeLocalAdminRunner, LocalAdminRuntimeError, LocalGroupAdminController,
        LocalGroupAdministrationOperation, LocalGroupAdministrationRequest,
        LOCAL_ADMIN_CONFIRMATION_MARKER,
    };

    #[test]
    fn plans_create_group_command() {
        let planner = super::SystemLocalGroupCommandPlanner;
        let request = create_group_request("daswriters", true);

        let plan = super::LocalGroupCommandPlanner::plan(&planner, &request).expect("planned");

        assert_eq!(plan.program, "groupadd");
        assert_eq!(plan.args, ["--", "daswriters"]);
        assert_eq!(plan.display_args, plan.args);
    }

    #[test]
    fn plans_assign_user_to_group_command() {
        let planner = super::SystemLocalGroupCommandPlanner;
        let request = assign_user_request("stephen", "daswriters", true);

        let plan = super::LocalGroupCommandPlanner::plan(&planner, &request).expect("planned");

        assert_eq!(plan.program, "usermod");
        assert_eq!(plan.args, ["-a", "-G", "daswriters", "--", "stephen"]);
        assert_eq!(plan.display_args, plan.args);
    }

    #[test]
    fn dry_run_does_not_execute_command() {
        let runner = FakeLocalAdminRunner::new();
        let controller = LocalGroupAdminController::new(runner);

        let response = controller
            .execute(
                create_group_request("daswriters", true),
                "2026-07-07T12:25:00Z",
            )
            .expect("dry run accepted");

        assert!(response.accepted.dry_run);
        assert!(!response.executed);
        assert_eq!(response.command.program, "groupadd");
        assert!(controller.runner.calls.borrow().is_empty());
    }

    #[test]
    fn non_dry_run_requires_administrator_confirmation() {
        let runner = FakeLocalAdminRunner::new();
        let controller = LocalGroupAdminController::new(runner);

        let error = controller
            .execute(
                create_group_request("daswriters", false),
                "2026-07-07T12:25:00Z",
            )
            .expect_err("confirmation required");

        assert!(matches!(
            error,
            LocalAdminRuntimeError::MissingAdministratorConfirmation { .. }
        ));
        assert!(controller.runner.calls.borrow().is_empty());
    }

    #[test]
    fn non_dry_run_executes_after_administrator_confirmation() {
        let runner = FakeLocalAdminRunner::new();
        let controller = LocalGroupAdminController::new(runner);
        let mut request = assign_user_request("stephen", "daswriters", false);
        request.administrator_confirmation = Some(LOCAL_ADMIN_CONFIRMATION_MARKER.to_string());

        let response = controller
            .execute(request, "2026-07-07T12:25:00Z")
            .expect("mutation accepted");

        assert!(response.executed);
        assert!(!response.accepted.dry_run);
        assert_eq!(
            controller.runner.calls.borrow().as_slice(),
            &[(
                "usermod".to_string(),
                vec![
                    "-a".to_string(),
                    "-G".to_string(),
                    "daswriters".to_string(),
                    "--".to_string(),
                    "stephen".to_string(),
                ],
                vec![
                    "-a".to_string(),
                    "-G".to_string(),
                    "daswriters".to_string(),
                    "--".to_string(),
                    "stephen".to_string(),
                ],
            )]
        );
    }

    #[test]
    fn invalid_account_name_surfaces_as_runtime_error() {
        let runner = FakeLocalAdminRunner::new();
        let controller = LocalGroupAdminController::new(runner);

        let error = controller
            .execute(create_group_request("-bad", true), "2026-07-07T12:25:00Z")
            .expect_err("unsafe group rejected");

        assert!(matches!(
            error,
            LocalAdminRuntimeError::UnsafeAccountName {
                field: "group_name",
                ..
            }
        ));
    }

    #[test]
    fn unsupported_operation_surfaces_as_runtime_error() {
        let runner = FakeLocalAdminRunner::new();
        let controller = LocalGroupAdminController::new(runner);

        let error = controller
            .execute(
                LocalGroupAdministrationRequest {
                    operation: LocalGroupAdministrationOperation::Unsupported(
                        "delete_group".to_string(),
                    ),
                    group_name: "daswriters".to_string(),
                    username: None,
                    dry_run: true,
                    client_request_id: None,
                    administrator_confirmation: None,
                },
                "2026-07-07T12:25:00Z",
            )
            .expect_err("unsupported operation rejected");

        assert_eq!(
            error,
            LocalAdminRuntimeError::UnsupportedOperation {
                operation: "delete_group".to_string()
            }
        );
    }

    fn create_group_request(group_name: &str, dry_run: bool) -> LocalGroupAdministrationRequest {
        LocalGroupAdministrationRequest {
            operation: LocalGroupAdministrationOperation::CreateGroup,
            group_name: group_name.to_string(),
            username: None,
            dry_run,
            client_request_id: None,
            administrator_confirmation: None,
        }
    }

    fn assign_user_request(
        username: &str,
        group_name: &str,
        dry_run: bool,
    ) -> LocalGroupAdministrationRequest {
        LocalGroupAdministrationRequest {
            operation: LocalGroupAdministrationOperation::AssignUserToGroup,
            group_name: group_name.to_string(),
            username: Some(username.to_string()),
            dry_run,
            client_request_id: None,
            administrator_confirmation: None,
        }
    }
}
