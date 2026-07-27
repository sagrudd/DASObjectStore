use super::*;
use crate::api::{
    WorkspaceControlAction, WorkspaceControlRequest, WorkspaceControlResponse,
    WORKSPACE_CONTROL_SCHEMA_VERSION,
};
use dasobjectstore_core::ids::WorkspaceId;
use dasobjectstore_metadata::{
    apply_workspace_expiry, cancel_workspace_cleanup, close_workspace, list_workspace_reservations,
    read_cleanup_plan, read_workspace_reservation, report_expired_workspaces,
    request_workspace_cleanup, CloseWorkspaceRequest, RequestWorkspaceCleanup,
    WorkspaceCleanupPlan, WorkspaceExpiryCandidate, WorkspaceReservationSnapshot,
};

pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: WorkspaceControlRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let delegated_actor =
        match handler.delegated_object_browser_actor(actor, request.delegated_actor.as_ref()) {
            Ok(actor) => actor,
            Err(error_value) => {
                return Ok(error(
                    "workspace_actor_delegation_rejected",
                    error_value.to_string(),
                ))
            }
        };
    let effective_actor = delegated_actor.as_ref().or(actor);
    let Some(actor) = effective_actor else {
        return Ok(error(
            "workspace_authentication_required",
            "workspace control requires an authenticated local operating-system user",
        ));
    };
    if request.requires_administrator() && !actor.is_administrator() {
        return Ok(error(
            "workspace_administrator_required",
            "this workspace lifecycle operation requires root, sudo, or dasobjectstore-admin authority",
        ));
    }

    let actor_name = actor.display_name();
    let now = handler.clock.now_utc();
    let response = match request.action {
        WorkspaceControlAction::List => {
            let workspaces = match list_workspace_reservations(&handler.live_sqlite_path) {
                Ok(workspaces) => visible_workspaces(workspaces, actor),
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            response("list", &actor_name, workspaces, None, Vec::new())
        }
        WorkspaceControlAction::Inspect { workspace_id } => {
            let workspace = match authorized_workspace(handler, actor, &workspace_id) {
                Ok(workspace) => workspace,
                Err(response) => return Ok(response),
            };
            response("inspect", &actor_name, vec![workspace], None, Vec::new())
        }
        WorkspaceControlAction::Close {
            workspace_id,
            request_id,
            request_digest,
        } => {
            let workspace_id = parsed_workspace_id(&workspace_id)?;
            let plan = match close_workspace(&CloseWorkspaceRequest {
                live_sqlite_path: handler.live_sqlite_path.clone(),
                workspace_id,
                actor_id: actor_name.clone(),
                application_id: None,
                request_id,
                request_digest,
                closed_at_utc: now,
            }) {
                Ok(plan) => plan,
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            response("close", &actor_name, Vec::new(), Some(plan), Vec::new())
        }
        WorkspaceControlAction::ExpiryReport => {
            let candidates = match report_expired_workspaces(&handler.live_sqlite_path, &now) {
                Ok(candidates) => candidates,
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            let candidates = if actor.is_administrator() {
                candidates
            } else {
                candidates
                    .into_iter()
                    .filter(|candidate| {
                        authorized_workspace(handler, actor, &candidate.workspace_id).is_ok()
                    })
                    .collect()
            };
            response("expiry_report", &actor_name, Vec::new(), None, candidates)
        }
        WorkspaceControlAction::ApplyExpiry { workspace_id } => {
            let workspace_id = parsed_workspace_id(&workspace_id)?;
            let plan = match apply_workspace_expiry(
                &handler.live_sqlite_path,
                &workspace_id,
                &actor_name,
                None,
                &now,
            ) {
                Ok(plan) => plan,
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            response(
                "apply_expiry",
                &actor_name,
                Vec::new(),
                Some(plan),
                Vec::new(),
            )
        }
        WorkspaceControlAction::CleanupPlan { workspace_id } => {
            if let Err(response) = authorized_workspace(handler, actor, &workspace_id) {
                return Ok(response);
            }
            let workspace_id = parsed_workspace_id(&workspace_id)?;
            let plan = match read_cleanup_plan(&handler.live_sqlite_path, &workspace_id) {
                Ok(plan) => plan,
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            response(
                "cleanup_plan",
                &actor_name,
                Vec::new(),
                Some(plan),
                Vec::new(),
            )
        }
        WorkspaceControlAction::RequestCleanup {
            workspace_id,
            operation_id,
            request_id,
            request_digest,
            confirmation_phrase,
        } => {
            let workspace_id = parsed_workspace_id(&workspace_id)?;
            let plan = match request_workspace_cleanup(&RequestWorkspaceCleanup {
                live_sqlite_path: handler.live_sqlite_path.clone(),
                workspace_id,
                operation_id,
                actor_id: actor_name.clone(),
                application_id: None,
                request_id,
                request_digest,
                confirmation_phrase,
                requested_at_utc: now,
            }) {
                Ok(plan) => plan,
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            response(
                "request_cleanup",
                &actor_name,
                Vec::new(),
                Some(plan),
                Vec::new(),
            )
        }
        WorkspaceControlAction::CancelCleanup {
            workspace_id,
            operation_id,
        } => {
            let workspace_id = parsed_workspace_id(&workspace_id)?;
            let plan = match cancel_workspace_cleanup(
                &handler.live_sqlite_path,
                &workspace_id,
                &operation_id,
                &actor_name,
                &now,
            ) {
                Ok(plan) => plan,
                Err(error_value) => return Ok(metadata_error(error_value.to_string())),
            };
            response(
                "cancel_cleanup",
                &actor_name,
                Vec::new(),
                Some(plan),
                Vec::new(),
            )
        }
    };
    Ok(DaemonApiResponse::WorkspaceControl(response))
}

fn parsed_workspace_id(value: &str) -> Result<WorkspaceId, DaemonRequestHandlerError> {
    Ok(WorkspaceId::new(value.to_string())
        .expect("workspace request identity was validated before dispatch"))
}

fn authorized_workspace<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    actor: &DaemonLocalActor,
    workspace_id: &str,
) -> Result<WorkspaceReservationSnapshot, DaemonApiResponse>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let workspace_id = WorkspaceId::new(workspace_id.to_string()).map_err(|_| {
        error(
            "workspace_invalid_identity",
            "workspace identity is not valid",
        )
    })?;
    let workspace = read_workspace_reservation(&handler.live_sqlite_path, &workspace_id)
        .map_err(|error_value| metadata_error(error_value.to_string()))?;
    if actor.is_administrator() || workspace.owner == actor.display_name() {
        Ok(workspace)
    } else {
        Err(error(
            "workspace_not_visible",
            "workspace does not exist or is not owned by the authenticated user",
        ))
    }
}

fn visible_workspaces(
    workspaces: Vec<WorkspaceReservationSnapshot>,
    actor: &DaemonLocalActor,
) -> Vec<WorkspaceReservationSnapshot> {
    if actor.is_administrator() {
        workspaces
    } else {
        let actor_name = actor.display_name();
        workspaces
            .into_iter()
            .filter(|workspace| workspace.owner == actor_name)
            .collect()
    }
}

fn response(
    action: &str,
    actor: &str,
    workspaces: Vec<WorkspaceReservationSnapshot>,
    cleanup_plan: Option<WorkspaceCleanupPlan>,
    expiry_candidates: Vec<WorkspaceExpiryCandidate>,
) -> WorkspaceControlResponse {
    WorkspaceControlResponse {
        schema_version: WORKSPACE_CONTROL_SCHEMA_VERSION.to_string(),
        action: action.to_string(),
        actor: actor.to_string(),
        workspaces,
        cleanup_plan,
        expiry_candidates,
    }
}

fn metadata_error(message: String) -> DaemonApiResponse {
    error("workspace_metadata_unavailable", message)
}

fn error(code: &str, message: impl Into<String>) -> DaemonApiResponse {
    DaemonApiResponse::Error(DaemonApiErrorResponse::new(code, message))
}
