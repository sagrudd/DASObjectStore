use crate::api::DaemonJobId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateLocalGroupRequest {
    pub group_name: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl CreateLocalGroupRequest {
    pub fn validate(&self) -> Result<(), DaemonLocalAdminValidationError> {
        validate_local_name("group_name", &self.group_name)?;
        validate_client_request_id(self.client_request_id.as_deref())?;
        validate_administrator_actor(self.administrator_actor.as_deref())?;
        validate_confirmation_marker(&self.confirmation_marker)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateLocalGroupResponse {
    pub accepted: DaemonLocalAdminAcceptedResponse,
    pub group_name: String,
    pub administrator_actor: Option<String>,
}

impl CreateLocalGroupResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        dry_run: bool,
        client_request_id: Option<String>,
        group_name: impl Into<String>,
        administrator_actor: Option<String>,
    ) -> Self {
        Self {
            accepted: DaemonLocalAdminAcceptedResponse {
                job_id,
                command: DaemonLocalAdminCommand::CreateLocalGroup,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run,
                client_request_id,
            },
            group_name: group_name.into(),
            administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignLocalUserToLocalGroupRequest {
    pub username: String,
    pub group_name: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl AssignLocalUserToLocalGroupRequest {
    pub fn validate(&self) -> Result<(), DaemonLocalAdminValidationError> {
        validate_local_name("username", &self.username)?;
        validate_local_name("group_name", &self.group_name)?;
        validate_client_request_id(self.client_request_id.as_deref())?;
        validate_administrator_actor(self.administrator_actor.as_deref())?;
        validate_confirmation_marker(&self.confirmation_marker)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignLocalUserToLocalGroupResponse {
    pub accepted: DaemonLocalAdminAcceptedResponse,
    pub username: String,
    pub group_name: String,
    pub administrator_actor: Option<String>,
}

impl AssignLocalUserToLocalGroupResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        dry_run: bool,
        client_request_id: Option<String>,
        username: impl Into<String>,
        group_name: impl Into<String>,
        administrator_actor: Option<String>,
    ) -> Self {
        Self {
            accepted: DaemonLocalAdminAcceptedResponse {
                job_id,
                command: DaemonLocalAdminCommand::AssignLocalUserToLocalGroup,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run,
                client_request_id,
            },
            username: username.into(),
            group_name: group_name.into(),
            administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonLocalAdminAcceptedResponse {
    pub job_id: DaemonJobId,
    pub command: DaemonLocalAdminCommand,
    pub accepted_at_utc: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonLocalAdminCommand {
    CreateLocalGroup,
    AssignLocalUserToLocalGroup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonLocalAdminValidationError {
    BlankName { field: &'static str },
    UnsafeName { field: &'static str, value: String },
    BlankClientRequestId,
    BlankAdministratorActor,
    BlankConfirmationMarker,
}

impl Display for DaemonLocalAdminValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankName { field } => write!(formatter, "{field} must not be blank"),
            Self::UnsafeName { field, value } => write!(
                formatter,
                "{field} must be a conservative POSIX-style local name: {value}"
            ),
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
            Self::BlankAdministratorActor => {
                formatter.write_str("administrator_actor must not be blank")
            }
            Self::BlankConfirmationMarker => {
                formatter.write_str("confirmation_marker must not be blank")
            }
        }
    }
}

impl std::error::Error for DaemonLocalAdminValidationError {}

fn validate_local_name(
    field: &'static str,
    value: &str,
) -> Result<(), DaemonLocalAdminValidationError> {
    if value.trim().is_empty() {
        return Err(DaemonLocalAdminValidationError::BlankName { field });
    }
    if !is_safe_posixish_name(value) {
        return Err(DaemonLocalAdminValidationError::UnsafeName {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_client_request_id(value: Option<&str>) -> Result<(), DaemonLocalAdminValidationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(DaemonLocalAdminValidationError::BlankClientRequestId);
    }
    Ok(())
}

fn validate_administrator_actor(
    value: Option<&str>,
) -> Result<(), DaemonLocalAdminValidationError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() {
        return Err(DaemonLocalAdminValidationError::BlankAdministratorActor);
    }
    if !is_safe_posixish_name(value) {
        return Err(DaemonLocalAdminValidationError::UnsafeName {
            field: "administrator_actor",
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_confirmation_marker(value: &str) -> Result<(), DaemonLocalAdminValidationError> {
    if value.trim().is_empty() {
        return Err(DaemonLocalAdminValidationError::BlankConfirmationMarker);
    }
    Ok(())
}

fn is_safe_posixish_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 || !value.is_ascii() {
        return false;
    }

    let first = bytes[0];
    if !(first == b'_' || first.is_ascii_lowercase()) {
        return false;
    }

    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
        CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonLocalAdminCommand,
        DaemonLocalAdminValidationError,
    };
    use crate::api::DaemonJobId;

    #[test]
    fn create_group_request_accepts_safe_group_name() {
        let request = CreateLocalGroupRequest {
            group_name: "dasobjectstore-admin".to_string(),
            dry_run: true,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: "confirm create local group".to_string(),
        };

        request.validate().expect("safe group request");
    }

    #[test]
    fn create_group_request_rejects_blank_group_name() {
        let request = CreateLocalGroupRequest {
            group_name: " ".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: "confirm create local group".to_string(),
        };

        let err = request.validate().expect_err("blank group rejected");

        assert_eq!(
            err,
            DaemonLocalAdminValidationError::BlankName {
                field: "group_name"
            }
        );
    }

    #[test]
    fn create_group_request_rejects_unsafe_group_name() {
        let request = CreateLocalGroupRequest {
            group_name: "../admin".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: "confirm create local group".to_string(),
        };

        let err = request.validate().expect_err("unsafe group rejected");

        assert_eq!(
            err,
            DaemonLocalAdminValidationError::UnsafeName {
                field: "group_name",
                value: "../admin".to_string()
            }
        );
    }

    #[test]
    fn create_group_request_rejects_blank_client_request_id() {
        let request = CreateLocalGroupRequest {
            group_name: "mnemosyne".to_string(),
            dry_run: true,
            client_request_id: Some(" ".to_string()),
            administrator_actor: None,
            confirmation_marker: "confirm create local group".to_string(),
        };

        let err = request
            .validate()
            .expect_err("blank client request id rejected");

        assert_eq!(err, DaemonLocalAdminValidationError::BlankClientRequestId);
    }

    #[test]
    fn create_group_request_rejects_blank_confirmation_marker() {
        let request = CreateLocalGroupRequest {
            group_name: "mnemosyne".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: " ".to_string(),
        };

        let err = request
            .validate()
            .expect_err("blank confirmation marker rejected");

        assert_eq!(
            err,
            DaemonLocalAdminValidationError::BlankConfirmationMarker
        );
    }

    #[test]
    fn assignment_request_rejects_blank_username() {
        let request = AssignLocalUserToLocalGroupRequest {
            username: " ".to_string(),
            group_name: "mnemosyne".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: "confirm assign local user".to_string(),
        };

        let err = request.validate().expect_err("blank username rejected");

        assert_eq!(
            err,
            DaemonLocalAdminValidationError::BlankName { field: "username" }
        );
    }

    #[test]
    fn assignment_request_rejects_unsafe_username() {
        let request = AssignLocalUserToLocalGroupRequest {
            username: "Stephen".to_string(),
            group_name: "mnemosyne".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: "confirm assign local user".to_string(),
        };

        let err = request.validate().expect_err("unsafe username rejected");

        assert_eq!(
            err,
            DaemonLocalAdminValidationError::UnsafeName {
                field: "username",
                value: "Stephen".to_string()
            }
        );
    }

    #[test]
    fn create_group_response_records_accepted_command() {
        let response = CreateLocalGroupResponse::accepted(
            DaemonJobId::new("local-admin-1").expect("job id"),
            "2026-07-07T12:05:42Z",
            true,
            Some("request-1".to_string()),
            "mnemosyne",
            Some("operator".to_string()),
        );

        assert_eq!(
            response.accepted.command,
            DaemonLocalAdminCommand::CreateLocalGroup
        );
        assert!(response.accepted.dry_run);
        assert_eq!(
            response.accepted.client_request_id.as_deref(),
            Some("request-1")
        );
    }

    #[test]
    fn assignment_response_records_accepted_command() {
        let response = AssignLocalUserToLocalGroupResponse::accepted(
            DaemonJobId::new("local-admin-2").expect("job id"),
            "2026-07-07T12:06:42Z",
            true,
            Some("request-2".to_string()),
            "stephen",
            "mnemosyne",
            Some("operator".to_string()),
        );

        assert_eq!(
            response.accepted.command,
            DaemonLocalAdminCommand::AssignLocalUserToLocalGroup
        );
        assert_eq!(response.username, "stephen");
        assert_eq!(response.group_name, "mnemosyne");
    }
}
