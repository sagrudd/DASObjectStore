use super::{CliError, DaemonClient, DaemonRuntimeConfig, UnixSocketDaemonTransport};
use crate::cli::{
    WorkspaceArgs, WorkspaceCleanupCancelArgs, WorkspaceCleanupRequestArgs, WorkspaceCommand,
    WorkspaceIdentityArgs, WorkspaceMutationArgs,
};
use dasobjectstore_daemon::api::{
    WorkspaceControlAction, WorkspaceControlRequest, WorkspaceControlResponse,
};
use sha2::{Digest, Sha256};
use std::io::Write;

pub(super) fn run_workspace(args: &WorkspaceArgs, writer: &mut impl Write) -> Result<(), CliError> {
    match args.command() {
        WorkspaceCommand::List(args) => submit(WorkspaceControlAction::List, args.json(), writer),
        WorkspaceCommand::Inspect(args) => submit_identity(
            args,
            WorkspaceControlAction::Inspect {
                workspace_id: args.workspace_id().to_string(),
            },
            writer,
        ),
        WorkspaceCommand::Close(args) => close(args, writer),
        WorkspaceCommand::ExpiryReport(args) => {
            submit(WorkspaceControlAction::ExpiryReport, args.json(), writer)
        }
        WorkspaceCommand::ExpiryApply(args) => submit_identity(
            args,
            WorkspaceControlAction::ApplyExpiry {
                workspace_id: args.workspace_id().to_string(),
            },
            writer,
        ),
        WorkspaceCommand::CleanupPlan(args) => submit_identity(
            args,
            WorkspaceControlAction::CleanupPlan {
                workspace_id: args.workspace_id().to_string(),
            },
            writer,
        ),
        WorkspaceCommand::CleanupRequest(args) => cleanup_request(args, writer),
        WorkspaceCommand::CleanupCancel(args) => cleanup_cancel(args, writer),
    }
}

fn close(args: &WorkspaceMutationArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let digest = request_digest(&["close", args.workspace_id(), args.request_id()]);
    submit(
        WorkspaceControlAction::Close {
            workspace_id: args.workspace_id().to_string(),
            request_id: args.request_id().to_string(),
            request_digest: digest,
        },
        args.json(),
        writer,
    )
}

fn cleanup_request(
    args: &WorkspaceCleanupRequestArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let digest = request_digest(&[
        "request_cleanup",
        args.workspace_id(),
        args.operation_id(),
        args.request_id(),
    ]);
    submit(
        WorkspaceControlAction::RequestCleanup {
            workspace_id: args.workspace_id().to_string(),
            operation_id: args.operation_id().to_string(),
            request_id: args.request_id().to_string(),
            request_digest: digest,
            confirmation_phrase: args.confirmation().to_string(),
        },
        args.json(),
        writer,
    )
}

fn cleanup_cancel(
    args: &WorkspaceCleanupCancelArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    submit(
        WorkspaceControlAction::CancelCleanup {
            workspace_id: args.workspace_id().to_string(),
            operation_id: args.operation_id().to_string(),
        },
        args.json(),
        writer,
    )
}

fn submit_identity(
    args: &WorkspaceIdentityArgs,
    action: WorkspaceControlAction,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    submit(action, args.json(), writer)
}

fn submit(
    action: WorkspaceControlAction,
    json: bool,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.workspace_control(WorkspaceControlRequest {
        action,
        delegated_actor: None,
    })?;
    render(response, json, writer)
}

fn render(
    response: WorkspaceControlResponse,
    json: bool,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if json {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writeln!(writer)?;
        return Ok(());
    }
    writeln!(writer, "Workspace operation: {}", response.action)?;
    writeln!(writer, "Actor: {}", response.actor)?;
    for workspace in response.workspaces {
        writeln!(
            writer,
            "{}  {:?}  owner={}  reserved={}  expires={}",
            workspace.workspace_id,
            workspace.state,
            workspace.owner,
            workspace.reserved_capacity_bytes,
            workspace.expires_at_utc
        )?;
    }
    for candidate in response.expiry_candidates {
        writeln!(
            writer,
            "{}  {}  expires={}  action={}",
            candidate.workspace_id, candidate.state, candidate.expires_at_utc, candidate.action
        )?;
    }
    if let Some(plan) = response.cleanup_plan {
        writeln!(
            writer,
            "{}  state={}  cleanup_eligible={}",
            plan.workspace_id, plan.state, plan.eligible
        )?;
        for blocker in plan.blockers {
            writeln!(writer, "blocker: {blocker}")?;
        }
    }
    Ok(())
}

fn request_digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::request_digest;

    #[test]
    fn lifecycle_request_digest_is_deterministic_and_domain_separated() {
        let first = request_digest(&["close", "workspace-a", "request-a"]);
        assert_eq!(
            first,
            request_digest(&["close", "workspace-a", "request-a"])
        );
        assert_ne!(
            first,
            request_digest(&["request_cleanup", "workspace-a", "request-a"])
        );
        assert_eq!(first.len(), 71);
    }
}
