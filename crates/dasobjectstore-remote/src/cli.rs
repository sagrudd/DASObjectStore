use crate::auth::RemoteAuthAuthority;
use clap::{Args, Parser, Subcommand, ValueEnum};
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
    /// Inspect locally configured S3 access for an authenticated ObjectStore.
    S3(S3Args),
    /// Define the browser-approved easyconnect pairing flow for a DAS appliance.
    Easyconnect(EasyconnectArgs),
    /// Configure this remote client.
    Config(ConfigArgs),
    /// List object stores accessible through the configured S3 endpoint.
    Stores(StoresArgs),
    /// Inspect and reconcile authoritative ObjectStore objects over HTTPS.
    Objects(ObjectsArgs),
    /// Inspect or wait for daemon-owned remote operations over HTTPS.
    Operations(OperationsArgs),
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
    /// Install the issued session into a standard AWS CLI profile.
    #[arg(long)]
    set_s3_config: bool,
    /// AWS profile name; defaults to dasobjectstore-<ObjectStore>.
    #[arg(long, requires = "set_s3_config")]
    s3_profile: Option<String>,
    /// Replace a conflicting DASObjectStore-managed AWS profile association.
    #[arg(long, requires = "set_s3_config")]
    force: bool,
    /// Skip the default authenticated S3 verification (diagnostics only).
    #[arg(long, requires = "set_s3_config")]
    no_verify_s3: bool,
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
    pub fn set_s3_config(&self) -> bool {
        self.set_s3_config
    }
    pub fn s3_profile(&self) -> Option<&str> {
        self.s3_profile.as_deref()
    }
    pub fn force(&self) -> bool {
        self.force
    }
    pub fn verify_s3(&self) -> bool {
        !self.no_verify_s3
    }
}

#[derive(Debug, Args)]
pub struct S3Args {
    #[command(subcommand)]
    command: S3Command,
}

impl S3Args {
    pub fn command(&self) -> &S3Command {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum S3Command {
    /// Compare a local AWS profile with its authenticated ObjectStore association.
    Status(S3StatusArgs),
}

#[derive(Debug, Args)]
pub struct S3StatusArgs {
    store: String,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    json: bool,
}

impl S3StatusArgs {
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
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
    /// Report whether a store is ready for remote S3 ingest and catalogue work.
    Readiness(StoreReadinessArgs),
}

#[derive(Debug, Args)]
pub struct StoreReadinessArgs {
    /// ObjectStore identifier.
    store: String,
    /// Emit the stable JSON response.
    #[arg(long)]
    json: bool,
}

impl StoreReadinessArgs {
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct ObjectsArgs {
    #[command(subcommand)]
    command: ObjectsCommand,
}

impl ObjectsArgs {
    pub fn command(&self) -> &ObjectsCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum ObjectsCommand {
    /// Fetch a bounded page of authoritative catalogue objects.
    Snapshot(ObjectSnapshotArgs),
    /// Inspect one payload and its manifest/checksum sidecars as a group.
    GroupStatus(ObjectGroupStatusArgs),
    /// Idempotently reconcile an S3-visible payload group into the catalogue.
    ReconcileS3(ObjectReconcileS3Args),
}

#[derive(Debug, Args)]
pub struct ObjectSnapshotArgs {
    store: String,
    /// Restrict results to this object-key prefix.
    #[arg(long, default_value = "")]
    prefix: String,
    /// Opaque continuation cursor returned by the preceding page.
    #[arg(long)]
    cursor: Option<String>,
    /// Maximum objects in this page; the appliance enforces its own upper bound.
    #[arg(long, default_value_t = 20_000)]
    limit: u32,
    #[arg(long)]
    json: bool,
}

impl ObjectSnapshotArgs {
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }
    pub fn limit(&self) -> u32 {
        self.limit
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct ObjectGroupStatusArgs {
    store: String,
    /// Exact payload object key; sidecar keys are derived by the appliance.
    #[arg(long)]
    key: String,
    #[arg(long)]
    json: bool,
}

impl ObjectGroupStatusArgs {
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ReconcileAckPolicy {
    AfterSsdIngest,
    AfterHddSettlement,
}

impl ReconcileAckPolicy {
    pub fn as_wire_name(self) -> &'static str {
        match self {
            Self::AfterSsdIngest => "after_ssd_ingest",
            Self::AfterHddSettlement => "after_hdd_settlement",
        }
    }
}

#[derive(Debug, Args)]
pub struct ObjectReconcileS3Args {
    store: String,
    #[arg(long)]
    key: String,
    #[arg(long)]
    expected_bytes: u64,
    /// Expected lowercase hexadecimal SHA-256 of the payload.
    #[arg(long)]
    expected_sha256: String,
    /// Stable caller-generated key used to deduplicate retries.
    #[arg(long)]
    idempotency_key: String,
    #[arg(long, value_enum, default_value_t = ReconcileAckPolicy::AfterSsdIngest)]
    ack_policy: ReconcileAckPolicy,
    #[arg(long)]
    json: bool,
}

impl ObjectReconcileS3Args {
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn expected_bytes(&self) -> u64 {
        self.expected_bytes
    }
    pub fn expected_sha256(&self) -> &str {
        &self.expected_sha256
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn ack_policy(&self) -> ReconcileAckPolicy {
        self.ack_policy
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct OperationsArgs {
    #[command(subcommand)]
    command: OperationsCommand,
}

impl OperationsArgs {
    pub fn command(&self) -> &OperationsCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum OperationsCommand {
    /// Fetch the current state of a remote operation.
    Status(OperationStatusArgs),
    /// Poll until an acknowledgement boundary or terminal state is reached.
    Wait(OperationWaitArgs),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OperationWaitUntil {
    SsdAcknowledged,
    HddSettled,
    Complete,
}

impl OperationWaitUntil {
    pub fn as_wire_name(self) -> &'static str {
        match self {
            Self::SsdAcknowledged => "ssd_acknowledged",
            Self::HddSettled => "hdd_settled",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Args)]
pub struct OperationStatusArgs {
    operation_id: String,
    #[arg(long)]
    json: bool,
}

impl OperationStatusArgs {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct OperationWaitArgs {
    operation_id: String,
    #[arg(long, value_enum, default_value_t = OperationWaitUntil::Complete)]
    until: OperationWaitUntil,
    /// Maximum wait, for example 90s, 10m, or 2h.
    #[arg(long, default_value = "10m")]
    timeout: String,
    #[arg(long)]
    json: bool,
}

impl OperationWaitArgs {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn until(&self) -> OperationWaitUntil {
        self.until
    }
    pub fn timeout(&self) -> &str {
        &self.timeout
    }
    pub fn json(&self) -> bool {
        self.json
    }
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
    /// Reviewed provider bucket binding for daemon submission without an easyconnect session.
    #[arg(long, requires = "submit_to_daemon")]
    bucket: Option<String>,
    /// Explicit MIME type for a single-file object (for example image/png).
    #[arg(long)]
    content_type: Option<String>,
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

    pub fn bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
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
    use super::{
        ObjectsCommand, OperationWaitUntil, OperationsCommand, RemoteCli, RemoteCommand,
        S3Command, StoresCommand,
    };
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
            "--set-s3-config",
            "--s3-profile",
            "dasobjectstore-porkchop",
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
        assert!(args.set_s3_config());
        assert_eq!(args.s3_profile(), Some("dasobjectstore-porkchop"));
        assert!(args.verify_s3());
    }

    #[test]
    fn parses_s3_status_command() {
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "s3",
            "status",
            "epic_collection",
            "--profile",
            "dasobjectstore-epic_collection",
            "--json",
        ])
        .expect("s3 status parses");
        let RemoteCommand::S3(args) = cli.command() else {
            panic!()
        };
        let S3Command::Status(args) = args.command();
        assert_eq!(args.store(), "epic_collection");
        assert_eq!(args.profile(), Some("dasobjectstore-epic_collection"));
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
        let StoresCommand::List(args) = stores.command() else {
            panic!("expected list")
        };
        assert!(args.json());
    }

    #[test]
    fn parses_remote_control_hierarchy() {
        let readiness = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "stores",
            "readiness",
            "epic_collection",
            "--json",
        ])
        .unwrap();
        let RemoteCommand::Stores(stores) = readiness.command() else {
            panic!()
        };
        let StoresCommand::Readiness(args) = stores.command() else {
            panic!()
        };
        assert_eq!(args.store(), "epic_collection");

        let reconcile = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "objects",
            "reconcile-s3",
            "epic_collection",
            "--key",
            "EPICv1/GSE224365_RAW.tar",
            "--expected-bytes",
            "10705582080",
            "--expected-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--idempotency-key",
            "epic-gse224365-v1",
            "--ack-policy",
            "after-ssd-ingest",
            "--json",
        ])
        .unwrap();
        let RemoteCommand::Objects(objects) = reconcile.command() else {
            panic!()
        };
        let ObjectsCommand::ReconcileS3(args) = objects.command() else {
            panic!()
        };
        assert_eq!(args.expected_bytes(), 10_705_582_080);

        let wait = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "operations",
            "wait",
            "op-1",
            "--until",
            "ssd-acknowledged",
            "--timeout",
            "10m",
            "--json",
        ])
        .unwrap();
        let RemoteCommand::Operations(operations) = wait.command() else {
            panic!()
        };
        let OperationsCommand::Wait(args) = operations.command() else {
            panic!()
        };
        assert_eq!(args.until(), OperationWaitUntil::SsdAcknowledged);
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
            "--content-type",
            "application/gzip",
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
        assert_eq!(args.content_type(), Some("application/gzip"));
    }
}
