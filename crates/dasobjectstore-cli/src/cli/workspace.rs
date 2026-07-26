use clap::{Args, Subcommand};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommand,
}

impl WorkspaceArgs {
    pub(crate) fn command(&self) -> &WorkspaceCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum WorkspaceCommand {
    /// List workspaces visible to the authenticated operating-system user.
    List(WorkspaceOutputArgs),
    /// Inspect one owned workspace and its branch reservations.
    Inspect(WorkspaceIdentityArgs),
    /// Close a ready workspace after daemon-side blocker checks.
    Close(WorkspaceMutationArgs),
    /// Report workspaces whose declared expiry has elapsed.
    ExpiryReport(WorkspaceOutputArgs),
    /// Apply expiry after daemon-side closure evidence checks.
    ExpiryApply(WorkspaceIdentityArgs),
    /// Inspect cleanup eligibility and branch release scope without mutation.
    CleanupPlan(WorkspaceIdentityArgs),
    /// Queue explicitly confirmed, daemon-owned workspace cleanup.
    CleanupRequest(WorkspaceCleanupRequestArgs),
    /// Cancel cleanup before any branch has been released.
    CleanupCancel(WorkspaceCleanupCancelArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct WorkspaceOutputArgs {
    /// Emit the typed daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl WorkspaceOutputArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct WorkspaceIdentityArgs {
    /// Stable workspace identity.
    workspace_id: String,
    /// Emit the typed daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl WorkspaceIdentityArgs {
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct WorkspaceMutationArgs {
    /// Stable workspace identity.
    workspace_id: String,
    /// Idempotency identity for this lifecycle request.
    #[arg(long)]
    request_id: String,
    /// Emit the typed daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl WorkspaceMutationArgs {
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(crate) fn request_id(&self) -> &str {
        &self.request_id
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct WorkspaceCleanupRequestArgs {
    /// Stable workspace identity.
    workspace_id: String,
    /// Stable cleanup operation identity.
    #[arg(long)]
    operation_id: String,
    /// Idempotency identity for this cleanup request.
    #[arg(long)]
    request_id: String,
    /// Exact phrase `CLEAN WORKSPACE <workspace_id>`.
    #[arg(long)]
    confirmation: String,
    /// Emit the typed daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl WorkspaceCleanupRequestArgs {
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub(crate) fn request_id(&self) -> &str {
        &self.request_id
    }
    pub(crate) fn confirmation(&self) -> &str {
        &self.confirmation
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct WorkspaceCleanupCancelArgs {
    /// Stable workspace identity.
    workspace_id: String,
    /// Cleanup operation identity returned by cleanup-request.
    #[arg(long)]
    operation_id: String,
    /// Emit the typed daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl WorkspaceCleanupCancelArgs {
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[cfg(test)]
mod tests {
    use super::{super::Cli, WorkspaceCommand};
    use clap::Parser;

    #[test]
    fn cleanup_requires_explicit_identities_and_confirmation() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "workspace",
            "cleanup-request",
            "analysis-a",
            "--operation-id",
            "cleanup-a",
            "--request-id",
            "request-a",
            "--confirmation",
            "CLEAN WORKSPACE analysis-a",
            "--json",
        ])
        .expect("workspace cleanup parses");
        let Some(super::super::Command::Workspace(args)) = cli.command() else {
            panic!("expected workspace command");
        };
        assert!(matches!(
            args.command(),
            WorkspaceCommand::CleanupRequest(request)
                if request.workspace_id() == "analysis-a"
                    && request.confirmation() == "CLEAN WORKSPACE analysis-a"
                    && request.json()
        ));
    }
}
