use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PistisGrantArgs {
    #[command(subcommand)]
    command: PistisGrantCommand,
}

impl PistisGrantArgs {
    pub(crate) fn command(&self) -> &PistisGrantCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum PistisGrantCommand {
    /// Inspect the current immutable grant registry.
    Inspect(PistisGrantInspectArgs),
    /// Grant one resolved Prosopikon principal access to one exact ObjectStore.
    Grant(PistisGrantMutationArgs),
    /// Revoke one exact principal/ObjectStore grant.
    Revoke(PistisGrantRevokeArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PistisGrantInspectArgs {
    /// Absolute deployment-owned grant registry path.
    #[arg(long)]
    grant_registry: PathBuf,
}

impl PistisGrantInspectArgs {
    pub(crate) fn grant_registry(&self) -> &Path {
        &self.grant_registry
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PistisGrantMutationArgs {
    /// Private canonical Prosopikon SQLite authority.
    #[arg(long)]
    authority: PathBuf,
    /// Provisioning selector; never persisted as authorization authority.
    #[arg(long)]
    email: String,
    /// Absolute deployment-owned grant registry path.
    #[arg(long)]
    grant_registry: PathBuf,
    /// Current DASObjectStore service registry used to validate the exact store.
    #[arg(long)]
    store_registry: PathBuf,
    /// Exact revision observed with `pistis-grant inspect`; use 0 for a new registry.
    #[arg(long)]
    expected_revision: u64,
    /// Exact existing DASObjectStore identifier.
    #[arg(long)]
    object_store: String,
    /// Permit read access.
    #[arg(long)]
    read: bool,
    /// Permit write access.
    #[arg(long)]
    write: bool,
    /// Optional exact allowed key prefix; repeat to add more than one.
    #[arg(long)]
    allowed_prefix: Vec<String>,
}

impl PistisGrantMutationArgs {
    pub(crate) fn authority(&self) -> &Path {
        &self.authority
    }

    pub(crate) fn email(&self) -> &str {
        &self.email
    }

    pub(crate) fn grant_registry(&self) -> &Path {
        &self.grant_registry
    }

    pub(crate) fn store_registry(&self) -> &Path {
        &self.store_registry
    }

    pub(crate) fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub(crate) fn object_store(&self) -> &str {
        &self.object_store
    }

    pub(crate) fn read(&self) -> bool {
        self.read
    }

    pub(crate) fn write(&self) -> bool {
        self.write
    }

    pub(crate) fn allowed_prefixes(&self) -> Vec<String> {
        self.allowed_prefix.clone()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PistisGrantRevokeArgs {
    /// Private canonical Prosopikon SQLite authority.
    #[arg(long)]
    authority: PathBuf,
    /// Provisioning selector; never persisted as authorization authority.
    #[arg(long)]
    email: String,
    /// Absolute deployment-owned grant registry path.
    #[arg(long)]
    grant_registry: PathBuf,
    /// Exact revision observed with `pistis-grant inspect`.
    #[arg(long)]
    expected_revision: u64,
    /// Exact existing DASObjectStore identifier.
    #[arg(long)]
    object_store: String,
}

impl PistisGrantRevokeArgs {
    pub(crate) fn authority(&self) -> &Path {
        &self.authority
    }

    pub(crate) fn email(&self) -> &str {
        &self.email
    }

    pub(crate) fn grant_registry(&self) -> &Path {
        &self.grant_registry
    }

    pub(crate) fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub(crate) fn object_store(&self) -> &str {
        &self.object_store
    }
}
