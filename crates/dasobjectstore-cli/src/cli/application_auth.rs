use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ApplicationAuthArgs {
    #[command(subcommand)]
    pub(crate) command: ApplicationAuthCommand,
}

impl ApplicationAuthArgs {
    pub(crate) fn command(&self) -> &ApplicationAuthCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum ApplicationAuthCommand {
    /// Exchange a proof-bearing request for a daemon-authorized access token.
    Exchange(ApplicationAuthRequestArgs),
    /// Register a daemon-owned application identity from path-free JSON.
    RegisterIdentity(ApplicationAuthRequestArgs),
    /// Register or rotate a daemon-owned public key from path-free JSON.
    RegisterKey(ApplicationAuthRequestArgs),
    /// Revoke an application identity or public key from path-free JSON.
    Revoke(ApplicationAuthRequestArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ApplicationAuthRequestArgs {
    /// JSON request file. Private keys and bearer tokens are not accepted by
    /// the request contracts and must never be placed in this file.
    #[arg(long)]
    request: PathBuf,
    /// Emit the typed daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl ApplicationAuthRequestArgs {
    pub(crate) fn request(&self) -> &Path {
        &self.request
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{Cli, Command},
        ApplicationAuthCommand,
    };
    use clap::Parser;

    #[test]
    fn parses_path_based_application_auth_exchange_without_private_key_flags() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "application-auth",
            "exchange",
            "--request",
            "/tmp/exchange.json",
            "--json",
        ])
        .expect("application auth exchange parses");
        let Some(Command::ApplicationAuth(args)) = cli.command() else {
            panic!("expected application-auth command");
        };
        assert!(matches!(
            args.command(),
            ApplicationAuthCommand::Exchange(request)
                if request.json() && request.request().ends_with("exchange.json")
        ));
    }
}
