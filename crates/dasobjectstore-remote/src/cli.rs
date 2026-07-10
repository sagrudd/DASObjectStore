use crate::auth::RemoteAuthAuthority;
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "dasobjectstore-remote",
    version = dasobjectstore_core::VERSION,
    about = "Remote DASObjectStore S3 upload client"
)]
pub struct RemoteCli {
    /// Remote client config path.
    #[arg(long)]
    config: Option<PathBuf>,
    /// DASObjectStore S3 endpoint URL, for example http://192.168.1.192:3900.
    #[arg(long)]
    endpoint_url: Option<String>,
    /// S3 region used by the object service.
    #[arg(long)]
    region: Option<String>,
    /// AWS CLI profile to use when no credential helper is configured.
    #[arg(long)]
    profile: Option<String>,
    /// Authentication authority for credential discovery.
    #[arg(long)]
    auth: Option<RemoteAuthAuthority>,
    /// Remote username for local-password authentication.
    #[arg(long)]
    username: Option<String>,
    /// External command that emits S3 credentials as JSON.
    #[arg(long)]
    credential_helper: Option<String>,
    /// Prompt for a password without echo and pass it only to the credential helper.
    #[arg(long)]
    prompt_password: bool,
    #[command(subcommand)]
    command: RemoteCommand,
}

impl RemoteCli {
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub fn endpoint_url(&self) -> Option<&str> {
        self.endpoint_url.as_deref()
    }

    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    pub fn auth(&self) -> Option<RemoteAuthAuthority> {
        self.auth
    }

    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    pub fn credential_helper(&self) -> Option<&str> {
        self.credential_helper.as_deref()
    }

    pub fn prompt_password(&self) -> bool {
        self.prompt_password
    }

    pub fn command(&self) -> &RemoteCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Authenticate to one appliance ObjectStore and emit an 8-hour S3 context.
    Authenticate(AuthenticateArgs),
    /// Define the browser-approved easyconnect pairing flow for a DAS appliance.
    Easyconnect(EasyconnectArgs),
    /// Configure this remote client.
    Config(ConfigArgs),
    /// List object stores accessible through the configured S3 endpoint.
    Stores(StoresArgs),
    /// Upload a file or folder to an accessible object store.
    Upload(UploadArgs),
}

#[derive(Debug, Args)]
pub struct AuthenticateArgs {
    /// DAS appliance host name or IP address, without a URL path.
    host_or_ip: String,
    /// ObjectStore identifier to authorize.
    object_store: String,
    /// HTTPS port for the standalone DASObjectStore Web API.
    #[arg(long, default_value_t = crate::authenticate::DEFAULT_APPLIANCE_HTTPS_PORT)]
    https_port: u16,
    /// Username; defaults to the current local username.
    #[arg(long)]
    username: Option<String>,
    /// PEM CA certificate used to verify the appliance HTTPS certificate.
    #[arg(long)]
    ca_cert: Option<PathBuf>,
    /// TLS certificate name when the appliance certificate is not issued to its IP.
    #[arg(long)]
    tls_server_name: Option<String>,
    /// Requested session lifetime; defaults to the appliance policy (8 hours).
    #[arg(long)]
    session_lifetime_seconds: Option<u64>,
    /// Emit the full connection context, including temporary S3 credentials.
    #[arg(long)]
    json: bool,
}

impl AuthenticateArgs {
    pub fn host_or_ip(&self) -> &str {
        &self.host_or_ip
    }
    pub fn object_store(&self) -> &str {
        &self.object_store
    }
    pub fn https_port(&self) -> u16 {
        self.https_port
    }
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }
    pub fn ca_cert(&self) -> Option<&Path> {
        self.ca_cert.as_deref()
    }
    pub fn tls_server_name(&self) -> Option<&str> {
        self.tls_server_name.as_deref()
    }
    pub fn session_lifetime_seconds(&self) -> Option<u64> {
        self.session_lifetime_seconds
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct EasyconnectArgs {
    /// DAS appliance host name or IP address, without a URL scheme.
    host_or_ip: String,
    /// HTTPS port for the standalone DASObjectStore Web application.
    #[arg(long, default_value_t = crate::easyconnect::DEFAULT_APPLIANCE_HTTPS_PORT)]
    https_port: u16,
    /// Fixed local callback port; omit to let the remote client choose one.
    #[arg(long)]
    callback_port: Option<u16>,
    /// Print the easyconnect contract without launching the browser.
    #[arg(long)]
    contract: bool,
    /// Print the browser URL and wait for callback without opening a browser.
    #[arg(long)]
    no_browser: bool,
    /// Seconds to wait for browser-approved pairing callback.
    #[arg(long, default_value_t = crate::easyconnect::DEFAULT_PAIRING_TIMEOUT_SECS)]
    timeout_seconds: u64,
    /// Emit the contract as JSON without launching the browser.
    #[arg(long)]
    json: bool,
}

impl EasyconnectArgs {
    pub fn host_or_ip(&self) -> &str {
        &self.host_or_ip
    }

    pub fn https_port(&self) -> u16 {
        self.https_port
    }

    pub fn callback_port(&self) -> Option<u16> {
        self.callback_port
    }

    pub fn contract(&self) -> bool {
        self.contract
    }

    pub fn no_browser(&self) -> bool {
        self.no_browser
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

impl ConfigArgs {
    pub fn command(&self) -> &ConfigCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Write remote client configuration.
    Set(ConfigSetArgs),
    /// Show the resolved remote client configuration.
    Show(ConfigShowArgs),
}

#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    /// DASObjectStore S3 endpoint URL.
    #[arg(long)]
    endpoint_url: String,
    /// S3 region used by the object service.
    #[arg(long, default_value = crate::config::DEFAULT_REGION)]
    region: String,
    /// AWS CLI profile name.
    #[arg(long, default_value = crate::config::DEFAULT_PROFILE)]
    profile: String,
    /// Authentication authority for credential discovery.
    #[arg(long, default_value_t = RemoteAuthAuthority::AwsProfile)]
    auth: RemoteAuthAuthority,
    /// Remote username for local-password authentication.
    #[arg(long)]
    username: Option<String>,
    /// External command that emits S3 credentials as JSON.
    #[arg(long)]
    credential_helper: Option<String>,
}

impl ConfigSetArgs {
    pub fn endpoint_url(&self) -> &str {
        &self.endpoint_url
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn auth(&self) -> RemoteAuthAuthority {
        self.auth
    }

    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    pub fn credential_helper(&self) -> Option<&str> {
        self.credential_helper.as_deref()
    }
}

#[derive(Debug, Args)]
pub struct ConfigShowArgs {
    /// Emit JSON.
    #[arg(long)]
    json: bool,
}

impl ConfigShowArgs {
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct StoresArgs {
    #[command(subcommand)]
    command: StoresCommand,
}

impl StoresArgs {
    pub fn command(&self) -> &StoresCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum StoresCommand {
    /// List object stores visible to the configured S3 credentials.
    List(StoreListArgs),
}

#[derive(Debug, Args)]
pub struct StoreListArgs {
    /// Emit JSON.
    #[arg(long)]
    json: bool,
    /// Print the AWS command without executing it.
    #[arg(long)]
    dry_run: bool,
}

impl StoreListArgs {
    pub fn json(&self) -> bool {
        self.json
    }

    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[derive(Debug, Args)]
pub struct UploadArgs {
    /// ObjectStore name receiving the upload. Paired easyconnect sessions derive the S3 bucket.
    store: String,
    /// Local file or folder to upload.
    #[arg(long)]
    source: PathBuf,
    /// Object prefix for uploaded content.
    #[arg(long)]
    prefix: Option<String>,
    /// Exact object key; valid only for single-file uploads.
    #[arg(long)]
    key: Option<String>,
    /// Print the AWS command without executing it.
    #[arg(long)]
    dry_run: bool,
    /// Suppress AWS progress output.
    #[arg(long)]
    no_progress: bool,
    /// Submit the AWS upload job to a local dasobjectstored daemon instead of executing AWS locally.
    #[arg(long)]
    submit_to_daemon: bool,
    /// Local daemon socket used with --submit-to-daemon.
    #[arg(long)]
    daemon_socket: Option<PathBuf>,
}

impl UploadArgs {
    pub fn store(&self) -> &str {
        &self.store
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub fn progress(&self) -> bool {
        !self.no_progress
    }

    pub fn submit_to_daemon(&self) -> bool {
        self.submit_to_daemon
    }

    pub fn daemon_socket(&self) -> Option<&Path> {
        self.daemon_socket.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::{RemoteCli, RemoteCommand, StoresCommand};
    use crate::auth::RemoteAuthAuthority;
    use clap::Parser;

    #[test]
    fn parses_easyconnect_contract_command() {
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "easyconnect",
            "192.168.1.192",
            "--callback-port",
            "49321",
            "--json",
            "--timeout-seconds",
            "10",
        ])
        .expect("cli parses");

        let RemoteCommand::Easyconnect(args) = cli.command() else {
            panic!("expected easyconnect command");
        };
        assert_eq!(args.host_or_ip(), "192.168.1.192");
        assert_eq!(args.https_port(), 8448);
        assert_eq!(args.callback_port(), Some(49321));
        assert_eq!(args.timeout_seconds(), 10);
        assert!(!args.no_browser());
        assert!(!args.contract());
        assert!(args.json());
    }

    #[test]
    fn parses_authenticate_command() {
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "authenticate",
            "192.168.1.192",
            "porkchop",
            "--username",
            "stephen",
            "--ca-cert",
            "/etc/dasobjectstore/ca.pem",
            "--json",
        ])
        .expect("authenticate parses");
        let RemoteCommand::Authenticate(args) = cli.command() else {
            panic!("expected authenticate command");
        };
        assert_eq!(args.host_or_ip(), "192.168.1.192");
        assert_eq!(args.object_store(), "porkchop");
        assert_eq!(args.username(), Some("stephen"));
        assert_eq!(
            args.ca_cert().and_then(|path| path.to_str()),
            Some("/etc/dasobjectstore/ca.pem")
        );
        assert!(args.json());
    }

    #[test]
    fn parses_store_list() {
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "--endpoint-url",
            "http://192.168.1.192:3900",
            "stores",
            "list",
            "--json",
        ])
        .expect("cli parses");

        let RemoteCommand::Stores(stores) = cli.command() else {
            panic!("expected stores command");
        };
        let StoresCommand::List(args) = stores.command();
        assert!(args.json());
    }

    #[test]
    fn parses_upload_with_auth_overrides() {
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "--endpoint-url",
            "https://dos.example:3900",
            "--auth",
            "local-password",
            "--username",
            "alice",
            "--credential-helper",
            "dasobjectstore-credential-helper",
            "upload",
            "dos-generated",
            "--source",
            "/data/run-001",
            "--prefix",
            "runs/001",
        ])
        .expect("cli parses");

        assert_eq!(cli.auth(), Some(RemoteAuthAuthority::LocalPassword));
        assert_eq!(cli.username(), Some("alice"));
        assert_eq!(
            cli.credential_helper(),
            Some("dasobjectstore-credential-helper")
        );
        let RemoteCommand::Upload(args) = cli.command() else {
            panic!("expected upload command");
        };
        assert_eq!(args.store(), "dos-generated");
        assert_eq!(args.prefix(), Some("runs/001"));
    }
}
