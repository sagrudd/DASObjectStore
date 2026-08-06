use super::ObjectBrowserDelegatedActor;
use dasobjectstore_metadata::{
    WorkspaceCleanupPlan, WorkspaceExpiryCandidate, WorkspaceReservationSnapshot,
};
use serde::{Deserialize, Serialize};

pub const WORKSPACE_CONTROL_SCHEMA_VERSION: &str = "dasobjectstore.workspace_control.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WorkspaceControlAction {
    List,
    Inspect {
        workspace_id: String,
    },
    Close {
        workspace_id: String,
        request_id: String,
        request_digest: String,
    },
    ExpiryReport,
    ApplyExpiry {
        workspace_id: String,
    },
    CleanupPlan {
        workspace_id: String,
    },
    RequestCleanup {
        workspace_id: String,
        operation_id: String,
        request_id: String,
        request_digest: String,
        confirmation_phrase: String,
    },
    CancelCleanup {
        workspace_id: String,
        operation_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceControlRequest {
    pub action: WorkspaceControlAction,
    /// Audience-bound actor asserted by the trusted local Web/API adapter.
    /// The daemon accepts delegation only from its dedicated service account;
    /// normal Unix peers, including root, cannot select another actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_actor: Option<ObjectBrowserDelegatedActor>,
}

impl WorkspaceControlRequest {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(actor) = &self.delegated_actor {
            actor.validate().map_err(|error| error.to_string())?;
        }
        match &self.action {
            WorkspaceControlAction::List | WorkspaceControlAction::ExpiryReport => Ok(()),
            WorkspaceControlAction::Inspect { workspace_id }
            | WorkspaceControlAction::ApplyExpiry { workspace_id }
            | WorkspaceControlAction::CleanupPlan { workspace_id } => identity(workspace_id),
            WorkspaceControlAction::Close {
                workspace_id,
                request_id,
                request_digest,
            } => {
                identity(workspace_id)?;
                identity(request_id)?;
                digest(request_digest)
            }
            WorkspaceControlAction::RequestCleanup {
                workspace_id,
                operation_id,
                request_id,
                request_digest,
                confirmation_phrase,
            } => {
                identity(workspace_id)?;
                identity(operation_id)?;
                identity(request_id)?;
                digest(request_digest)?;
                if confirmation_phrase != &format!("CLEAN WORKSPACE {workspace_id}") {
                    return Err("cleanup confirmation phrase does not match workspace".to_string());
                }
                Ok(())
            }
            WorkspaceControlAction::CancelCleanup {
                workspace_id,
                operation_id,
            } => {
                identity(workspace_id)?;
                identity(operation_id)
            }
        }
    }

    pub fn requires_administrator(&self) -> bool {
        !matches!(
            self.action,
            WorkspaceControlAction::List
                | WorkspaceControlAction::Inspect { .. }
                | WorkspaceControlAction::ExpiryReport
                | WorkspaceControlAction::CleanupPlan { .. }
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceControlResponse {
    pub schema_version: String,
    pub action: String,
    pub actor: String,
    pub workspaces: Vec<WorkspaceReservationSnapshot>,
    pub cleanup_plan: Option<WorkspaceCleanupPlan>,
    pub expiry_candidates: Vec<WorkspaceExpiryCandidate>,
}

fn identity(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err("identifier is not conservative".to_string());
    }
    Ok(())
}

fn digest(value: &str) -> Result<(), String> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("request_digest must be SHA-256".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceControlAction, WorkspaceControlRequest};

    #[test]
    fn read_only_actions_do_not_claim_administrator_authority() {
        for action in [
            WorkspaceControlAction::List,
            WorkspaceControlAction::Inspect {
                workspace_id: "analysis-a".to_string(),
            },
            WorkspaceControlAction::ExpiryReport,
            WorkspaceControlAction::CleanupPlan {
                workspace_id: "analysis-a".to_string(),
            },
        ] {
            let request = WorkspaceControlRequest {
                action,
                delegated_actor: None,
            };
            assert!(!request.requires_administrator());
            request.validate().expect("read-only request validates");
        }
    }

    #[test]
    fn mutation_actions_require_administrator_authority() {
        let request = WorkspaceControlRequest {
            action: WorkspaceControlAction::Close {
                workspace_id: "analysis-a".to_string(),
                request_id: "close-a".to_string(),
                request_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
            },
            delegated_actor: None,
        };
        assert!(request.requires_administrator());
        request.validate().expect("mutation validates");
    }

    #[test]
    fn cleanup_rejects_inexact_confirmation_before_dispatch() {
        let request = WorkspaceControlRequest {
            action: WorkspaceControlAction::RequestCleanup {
                workspace_id: "analysis-a".to_string(),
                operation_id: "cleanup-a".to_string(),
                request_id: "request-a".to_string(),
                request_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                confirmation_phrase: "CLEAN WORKSPACE analysis-b".to_string(),
            },
            delegated_actor: None,
        };
        assert_eq!(
            request.validate().expect_err("mismatch rejected"),
            "cleanup confirmation phrase does not match workspace"
        );
    }
}
