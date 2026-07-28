use clap::{Args, Subcommand};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct TrustArgs {
    #[command(subcommand)]
    command: TrustCommand,
}

impl TrustArgs {
    pub(crate) fn command(&self) -> &TrustCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum TrustCommand {
    /// Print the authoritative appliance identity and public TLS certificate evidence.
    Identity(TrustIdentityArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct TrustIdentityArgs {
    /// Emit the identity evidence as JSON.
    #[arg(long)]
    json: bool,
}

impl TrustIdentityArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}
