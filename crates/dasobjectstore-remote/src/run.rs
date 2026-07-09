use crate::auth::{
    request_s3_credentials, RemoteAuthAuthority, RemoteAuthError, RemoteS3Credentials,
};
use crate::cli::{
    ConfigCommand, EasyconnectArgs, RemoteCli, RemoteCommand, StoreListArgs, StoresCommand,
    UploadArgs,
};
use crate::config::{
    default_config_path, read_optional_config, write_config, RemoteConfig, RemoteConfigError,
    RemoteConfigOverrides, RemoteUploadSession, DEFAULT_PROFILE, DEFAULT_REGION,
};
use crate::easyconnect::{
    define_easyconnect_contract, run_easyconnect_pairing_with_ready, RemoteEasyconnectContract,
    RemoteEasyconnectContractError, RemoteEasyconnectContractRequest,
    RemoteEasyconnectPairingError, RemoteEasyconnectPairingOptions,
    RemoteEasyconnectPairingOutcome, SystemBrowserLauncher,
};
use crate::s3::{
    execute_aws_plan, parse_list_buckets, plan_list_stores, plan_upload_with_credentials,
    AwsS3CredentialSource, RemoteS3Error,
};
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

pub fn run(cli: &RemoteCli, writer: &mut impl Write) -> Result<(), RemoteRunError> {
    match cli.command() {
        RemoteCommand::Easyconnect(args) => run_easyconnect(args, writer),
        RemoteCommand::Config(args) => match args.command() {
            ConfigCommand::Set(args) => run_config_set(cli, args, writer),
            ConfigCommand::Show(args) => run_config_show(cli, args.json(), writer),
        },
        RemoteCommand::Stores(args) => match args.command() {
            StoresCommand::List(args) => run_store_list(cli, args, writer),
        },
        RemoteCommand::Upload(args) => run_upload(cli, args, writer),
    }
}

fn run_easyconnect(args: &EasyconnectArgs, writer: &mut impl Write) -> Result<(), RemoteRunError> {
    let contract = define_easyconnect_contract(RemoteEasyconnectContractRequest {
        host_or_ip: args.host_or_ip().to_string(),
        https_port: args.https_port(),
        callback_port: args.callback_port(),
    })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &contract)?;
        writer.write_all(b"\n")?;
    } else if args.contract() {
        write_easyconnect_contract(&contract, writer)?;
    } else {
        let options = RemoteEasyconnectPairingOptions {
            host_or_ip: args.host_or_ip().to_string(),
            https_port: args.https_port(),
            callback_port: args.callback_port(),
            timeout: Duration::from_secs(args.timeout_seconds()),
            open_browser: !args.no_browser(),
        };
        let open_browser = !args.no_browser();
        let outcome =
            run_easyconnect_pairing_with_ready(options, &SystemBrowserLauncher, |contract| {
                write_easyconnect_pairing_ready(contract, open_browser, writer)?;
                writer.flush()?;
                Ok(())
            })?;
        write_easyconnect_pairing(&outcome, writer)?;
    }
    Ok(())
}

fn write_easyconnect_pairing_ready(
    contract: &RemoteEasyconnectContract,
    open_browser: bool,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    writeln!(writer, "Remote easyconnect pairing")?;
    writeln!(writer, "Appliance: {}", contract.appliance_base_url)?;
    writeln!(
        writer,
        "Local callback bind: {}",
        contract.local_callback_bind
    )?;
    if open_browser {
        writeln!(writer, "Browser launch: requested")?;
    } else {
        writeln!(writer, "Open browser URL: {}", contract.browser_login_url)?;
    }
    writeln!(writer, "Waiting for browser-approved pairing callback...")?;
    Ok(())
}

fn write_easyconnect_pairing(
    outcome: &RemoteEasyconnectPairingOutcome,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    writeln!(writer, "Pairing result: received")?;
    writeln!(writer, "Pairing ID: {}", outcome.result.pairing_id)?;
    writeln!(
        writer,
        "Exchange code: {}",
        outcome.result.redacted_exchange_code()
    )?;
    writeln!(
        writer,
        "Status: browser-approved pairing callback received; session exchange API is not implemented in this build."
    )?;
    Ok(())
}

fn write_easyconnect_contract(
    contract: &RemoteEasyconnectContract,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    writeln!(writer, "Remote easyconnect contract")?;
    writeln!(writer, "Appliance: {}", contract.appliance_base_url)?;
    writeln!(writer, "Discovery URL: {}", contract.discovery_url)?;
    writeln!(writer, "Browser login URL: {}", contract.browser_login_url)?;
    writeln!(
        writer,
        "Local callback bind: {}",
        contract.local_callback_bind
    )?;
    writeln!(
        writer,
        "Polling URL template: {}",
        contract.polling_url_template
    )?;
    writeln!(
        writer,
        "Default session lifetime: {} seconds",
        contract.default_session_lifetime_seconds
    )?;
    writeln!(
        writer,
        "Renewal lead time: {} seconds before expiry",
        contract.session_renewal_lead_seconds
    )?;
    writeln!(writer, "Lifecycle:")?;
    for step in &contract.lifecycle {
        writeln!(
            writer,
            "- {} [{}]: {}",
            step.state, step.actor, step.message
        )?;
    }
    writeln!(writer, "Failure states:")?;
    for failure in &contract.failure_states {
        writeln!(
            writer,
            "- {} (retryable={}): {}",
            failure.code, failure.retryable, failure.message
        )?;
    }
    writeln!(
        writer,
        "Status: contract defined; run without --contract/--json to launch browser pairing. Session exchange API is not implemented in this build."
    )?;
    Ok(())
}

fn run_config_set(
    cli: &RemoteCli,
    args: &crate::cli::ConfigSetArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let path = config_path(cli)?;
    let mut config = read_optional_config(&path)?.unwrap_or_else(empty_config);
    config.endpoint_url = args.endpoint_url().to_string();
    config.region = args.region().to_string();
    config.profile = args.profile().to_string();
    config.auth_authority = args.auth();
    config.username = args.username().map(ToOwned::to_owned);
    config.credential_helper = args.credential_helper().map(ToOwned::to_owned);
    config.validate_for_command()?;
    write_config(&path, &config)?;
    writeln!(writer, "Wrote {}", path.display())?;
    Ok(())
}

fn run_config_show(
    cli: &RemoteCli,
    json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = resolved_config(cli)?;
    config.validate_for_command()?;
    if json {
        serde_json::to_writer_pretty(&mut *writer, &config.redacted())?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Endpoint: {}", config.endpoint_url)?;
        writeln!(writer, "Region: {}", config.region)?;
        writeln!(writer, "AWS profile: {}", config.profile)?;
        writeln!(writer, "Auth authority: {}", config.auth_authority)?;
        if let Some(username) = &config.username {
            writeln!(writer, "Username: {username}")?;
        }
        if config.credential_helper.is_some() {
            writeln!(writer, "Credential helper: configured")?;
        }
        if let Some(default_appliance_id) = &config.default_appliance_id {
            writeln!(writer, "Default appliance: {default_appliance_id}")?;
        }
        if !config.paired_appliances.is_empty() {
            writeln!(writer, "Paired appliances:")?;
            for appliance in &config.paired_appliances {
                writeln!(
                    writer,
                    "- {} ({})",
                    appliance.display_name, appliance.appliance_id
                )?;
                writeln!(writer, "  Base URL: {}", appliance.appliance_base_url)?;
                writeln!(writer, "  Auth authority: {}", appliance.auth_authority)?;
                if let Some(actor) = &appliance.paired_actor {
                    writeln!(writer, "  Paired actor: {actor}")?;
                }
                if let Some(store) = &appliance.default_object_store {
                    writeln!(writer, "  Default ObjectStore: {store}")?;
                }
                if let Some(session) = &appliance.session {
                    writeln!(writer, "  Session: {}", session.redacted_session_id())?;
                    writeln!(writer, "  Session expires: {}", session.expires_at)?;
                    if session.renewal.is_some() {
                        writeln!(writer, "  Renewal: configured")?;
                    }
                    writeln!(writer, "  Credentials: configured, redacted")?;
                }
            }
        }
    }
    Ok(())
}

fn run_store_list(
    cli: &RemoteCli,
    args: &StoreListArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = resolved_valid_config(cli)?;
    let credentials = resolve_credentials(cli, &config)?;
    let plan = plan_list_stores(&config);
    if args.dry_run() {
        writeln!(writer, "{}", plan.display_command())?;
        return Ok(());
    }
    let raw = execute_aws_plan(&plan, credentials.as_ref())?;
    let stores = parse_list_buckets(&raw)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &stores)?;
        writer.write_all(b"\n")?;
    } else if stores.is_empty() {
        writeln!(
            writer,
            "No accessible object stores reported by S3 endpoint"
        )?;
    } else {
        writeln!(writer, "Accessible object stores")?;
        for store in stores {
            match store.created_at {
                Some(created_at) => writeln!(writer, "- {} ({created_at})", store.bucket)?,
                None => writeln!(writer, "- {}", store.bucket)?,
            }
        }
    }
    Ok(())
}

fn run_upload(
    cli: &RemoteCli,
    args: &UploadArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = resolved_valid_config(cli)?;
    let route = resolve_upload_route(&config, args.store())?;
    let credentials = match route.credentials.clone() {
        Some(credentials) => Some(credentials),
        None => resolve_credentials(cli, &config)?,
    };
    let plan = plan_upload_with_credentials(
        &config,
        &route.bucket,
        args.source(),
        args.prefix(),
        args.key(),
        args.dry_run(),
        args.progress(),
        route.credential_source,
    )?;
    if args.dry_run() {
        writeln!(
            writer,
            "ObjectStore: {} -> bucket {}",
            route.object_store, route.bucket
        )?;
        writeln!(
            writer,
            "Remote upload S3 concurrency: {}",
            plan.backpressure_policy.max_s3_transfer_concurrency
        )?;
        writeln!(
            writer,
            "SSD high pressure action: {}",
            plan.backpressure_policy.ssd_high_pressure_action
        )?;
        writeln!(writer, "{}", plan.display_command())?;
        return Ok(());
    }
    let output = execute_aws_plan(&plan, credentials.as_ref())?;
    if !output.trim().is_empty() {
        writer.write_all(output.as_bytes())?;
    }
    writeln!(writer, "Upload complete")?;
    Ok(())
}

#[derive(Clone, Debug)]
struct RemoteUploadRoute {
    object_store: String,
    bucket: String,
    credentials: Option<RemoteS3Credentials>,
    credential_source: AwsS3CredentialSource,
}

fn resolve_upload_route(
    config: &RemoteConfig,
    requested_object_store: &str,
) -> Result<RemoteUploadRoute, RemoteRunError> {
    if config.paired_appliances.is_empty() {
        return Ok(RemoteUploadRoute {
            object_store: requested_object_store.to_string(),
            bucket: requested_object_store.to_string(),
            credentials: None,
            credential_source: AwsS3CredentialSource::AwsProfile,
        });
    }

    let Some((appliance, grant)) = config.paired_appliances.iter().find_map(|appliance| {
        appliance
            .writable_object_store(requested_object_store)
            .map(|grant| (appliance, grant))
    }) else {
        return Err(RemoteRunError::UploadRouting(format!(
            "ObjectStore {requested_object_store} is not writable in the paired appliance grants; run easyconnect again or choose a writable ObjectStore name"
        )));
    };
    let session = appliance.session.as_ref().ok_or_else(|| {
        RemoteRunError::UploadRouting(format!(
            "ObjectStore {requested_object_store} is paired but has no active remote upload session; run dasobjectstore-remote easyconnect"
        ))
    })?;

    Ok(RemoteUploadRoute {
        object_store: grant.object_store.clone(),
        bucket: grant.bucket.clone(),
        credentials: Some(session_credentials(session)),
        credential_source: AwsS3CredentialSource::Environment,
    })
}

fn session_credentials(session: &RemoteUploadSession) -> RemoteS3Credentials {
    RemoteS3Credentials {
        access_key_id: session.credentials.access_key_id.clone(),
        secret_access_key: session.credentials.secret_access_key.clone(),
        session_token: session.credentials.session_token.clone(),
    }
}

fn resolved_valid_config(cli: &RemoteCli) -> Result<RemoteConfig, RemoteRunError> {
    let config = resolved_config(cli)?;
    config.validate_for_command()?;
    Ok(config)
}

fn resolved_config(cli: &RemoteCli) -> Result<RemoteConfig, RemoteRunError> {
    let path = config_path(cli)?;
    let base = read_optional_config(&path)?.unwrap_or_else(empty_config);
    Ok(base.merged_with(RemoteConfigOverrides {
        endpoint_url: cli.endpoint_url(),
        region: cli.region(),
        profile: cli.profile(),
        auth_authority: cli.auth(),
        username: cli.username(),
        credential_helper: cli.credential_helper(),
    }))
}

fn empty_config() -> RemoteConfig {
    RemoteConfig {
        endpoint_url: String::new(),
        region: DEFAULT_REGION.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        auth_authority: RemoteAuthAuthority::AwsProfile,
        username: None,
        credential_helper: None,
        default_appliance_id: None,
        paired_appliances: Vec::new(),
    }
}

fn config_path(cli: &RemoteCli) -> Result<PathBuf, RemoteRunError> {
    cli.config()
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(default_config_path)
        .map_err(Into::into)
}

fn resolve_credentials(
    cli: &RemoteCli,
    config: &RemoteConfig,
) -> Result<Option<RemoteS3Credentials>, RemoteRunError> {
    let Some(helper) = &config.credential_helper else {
        return Ok(None);
    };
    let password =
        if cli.prompt_password() || config.auth_authority != RemoteAuthAuthority::AwsProfile {
            Some(rpassword::prompt_password("DASObjectStore password: ")?)
        } else {
            None
        };
    Ok(Some(request_s3_credentials(
        helper,
        config.auth_authority,
        &config.endpoint_url,
        config.username.as_deref(),
        password.as_deref(),
    )?))
}

#[derive(Debug)]
pub enum RemoteRunError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Config(RemoteConfigError),
    Easyconnect(RemoteEasyconnectContractError),
    EasyconnectPairing(RemoteEasyconnectPairingError),
    Auth(RemoteAuthError),
    S3(RemoteS3Error),
    UploadRouting(String),
}

impl fmt::Display for RemoteRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Easyconnect(error) => write!(formatter, "{error}"),
            Self::EasyconnectPairing(error) => write!(formatter, "{error}"),
            Self::Auth(error) => write!(formatter, "{error}"),
            Self::S3(error) => write!(formatter, "{error}"),
            Self::UploadRouting(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RemoteRunError {}

impl From<std::io::Error> for RemoteRunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RemoteRunError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<RemoteConfigError> for RemoteRunError {
    fn from(error: RemoteConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<RemoteEasyconnectContractError> for RemoteRunError {
    fn from(error: RemoteEasyconnectContractError) -> Self {
        Self::Easyconnect(error)
    }
}

impl From<RemoteEasyconnectPairingError> for RemoteRunError {
    fn from(error: RemoteEasyconnectPairingError) -> Self {
        Self::EasyconnectPairing(error)
    }
}

impl From<RemoteAuthError> for RemoteRunError {
    fn from(error: RemoteAuthError) -> Self {
        Self::Auth(error)
    }
}

impl From<RemoteS3Error> for RemoteRunError {
    fn from(error: RemoteS3Error) -> Self {
        Self::S3(error)
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::auth::RemoteAuthAuthority;
    use crate::cli::RemoteCli;
    use crate::config::{
        read_optional_config, write_config, RemoteConfig, RemoteObjectStoreGrant,
        RemotePairedAppliance, RemoteSessionCredentials, RemoteUploadSession,
    };
    use clap::Parser;

    #[test]
    fn config_show_json_redacts_paired_session_credentials() {
        let path = temp_config_path("show-redacts");
        write_config(&path, &paired_config()).expect("write config");
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "--config",
            path.to_str().expect("utf8 path"),
            "config",
            "show",
            "--json",
        ])
        .expect("cli parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("config show succeeds");

        let rendered = String::from_utf8(output).expect("utf8 output");
        assert!(rendered.contains("DOSR...1234"));
        assert!(rendered.contains("SESS...7890"));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("SESSIONREFERENCE7890"));
        assert!(!rendered.contains("super-secret"));
        assert!(!rendered.contains("temporary-token"));
    }

    #[test]
    fn config_set_preserves_paired_appliance_storage() {
        let path = temp_config_path("set-preserves");
        write_config(&path, &paired_config()).expect("write config");
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "--config",
            path.to_str().expect("utf8 path"),
            "config",
            "set",
            "--endpoint-url",
            "https://new.example:3900",
            "--region",
            "garage",
            "--profile",
            "new-profile",
        ])
        .expect("cli parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("config set succeeds");

        let config = read_optional_config(&path)
            .expect("read config")
            .expect("config exists");
        assert_eq!(config.endpoint_url, "https://new.example:3900");
        assert_eq!(config.profile, "new-profile");
        assert_eq!(config.default_appliance_id.as_deref(), Some("appliance-1"));
        assert_eq!(config.paired_appliances.len(), 1);
        assert_eq!(
            config.paired_appliances[0].default_object_store.as_deref(),
            Some("zymo_fecal_2025.05")
        );
    }

    #[test]
    fn upload_dry_run_routes_object_store_through_paired_bucket_and_session() {
        let path = temp_config_path("upload-routes");
        let root = temp_source_root("upload-routes-source");
        std::fs::create_dir_all(&root).expect("create source");
        let source = root.join("reads.fastq.gz");
        std::fs::write(&source, b"ACGT").expect("write source");
        write_config(&path, &paired_config()).expect("write config");
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "--config",
            path.to_str().expect("utf8 path"),
            "upload",
            "zymo_fecal_2025.05",
            "--source",
            source.to_str().expect("utf8 source"),
            "--prefix",
            "raw/PAW10254",
            "--dry-run",
        ])
        .expect("cli parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("dry run succeeds");

        let rendered = String::from_utf8(output).expect("utf8 output");
        assert!(
            rendered.contains("ObjectStore: zymo_fecal_2025.05 -> bucket dos-zymo-fecal-2025-05")
        );
        assert!(rendered.contains("Remote upload S3 concurrency: 2"));
        assert!(rendered.contains("SSD high pressure action: pause_new_transfers"));
        assert!(rendered.contains("s3://dos-zymo-fecal-2025-05/raw/PAW10254/reads.fastq.gz"));
        assert!(!rendered.contains("--profile"));
        assert!(!rendered.contains("s3://zymo_fecal_2025.05/"));
        std::fs::remove_dir_all(root).expect("cleanup source");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn paired_upload_rejects_ungranted_bucket_name() {
        let path = temp_config_path("upload-rejects-bucket");
        let root = temp_source_root("upload-rejects-bucket-source");
        std::fs::create_dir_all(&root).expect("create source");
        let source = root.join("reads.fastq.gz");
        std::fs::write(&source, b"ACGT").expect("write source");
        write_config(&path, &paired_config()).expect("write config");
        let cli = RemoteCli::try_parse_from([
            "dasobjectstore-remote",
            "--config",
            path.to_str().expect("utf8 path"),
            "upload",
            "dos-zymo-fecal-2025-05",
            "--source",
            source.to_str().expect("utf8 source"),
            "--dry-run",
        ])
        .expect("cli parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("bucket name rejected");

        assert!(err
            .to_string()
            .contains("choose a writable ObjectStore name"));
        std::fs::remove_dir_all(root).expect("cleanup source");
        let _ = std::fs::remove_file(path);
    }

    fn paired_config() -> RemoteConfig {
        RemoteConfig {
            endpoint_url: "https://192.168.1.192:3900".to_string(),
            region: "garage".to_string(),
            profile: "dasobjectstore".to_string(),
            auth_authority: RemoteAuthAuthority::LocalPassword,
            username: Some("stephen".to_string()),
            credential_helper: Some("helper".to_string()),
            default_appliance_id: Some("appliance-1".to_string()),
            paired_appliances: vec![RemotePairedAppliance {
                appliance_id: "appliance-1".to_string(),
                display_name: "QNAP TL-D800C".to_string(),
                appliance_base_url: "https://192.168.1.192:8448".to_string(),
                discovery_url:
                    "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
                        .to_string(),
                auth_authority: RemoteAuthAuthority::LocalPassword,
                paired_actor: Some("stephen".to_string()),
                default_object_store: Some("zymo_fecal_2025.05".to_string()),
                object_stores: vec![RemoteObjectStoreGrant {
                    object_store: "zymo_fecal_2025.05".to_string(),
                    bucket: "dos-zymo-fecal-2025-05".to_string(),
                    can_read: true,
                    can_write: true,
                    writer_group: Some("mnemosyne".to_string()),
                    object_type: "metagenomics".to_string(),
                }],
                session: Some(RemoteUploadSession {
                    session_id: "SESSIONREFERENCE7890".to_string(),
                    issued_at: "2026-07-09T11:30:00Z".to_string(),
                    expires_at: "2026-07-09T19:30:00Z".to_string(),
                    credentials: RemoteSessionCredentials {
                        access_key_id: "DOSREMOTEACCESSKEY1234".to_string(),
                        secret_access_key: "super-secret".to_string(),
                        session_token: Some("temporary-token".to_string()),
                    },
                    renewal: None,
                }),
            }],
        }
    }

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-remote-{name}-{}-{nanos}.json",
            std::process::id()
        ))
    }

    fn temp_source_root(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-remote-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
