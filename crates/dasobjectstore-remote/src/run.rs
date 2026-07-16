use crate::auth::{
    request_s3_credentials, RemoteAuthAuthority, RemoteAuthError, RemoteS3Credentials,
};
use crate::authenticate::{authenticate, RemoteAuthenticateError, RemoteConnectionContext};
use crate::cli::{
    AuthenticateArgs, ConfigCommand, EasyconnectArgs, RemoteCli, RemoteCommand, StoreListArgs,
    StoresCommand, UploadArgs,
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
use dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds as parse_rfc3339_utc_seconds;
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonClientTransport, DaemonJobEvent, DaemonJobSummary,
    RemoteEasyconnectAwsCliEnvironmentVariable, RemoteEasyconnectSubmitAwsCliUploadRequest,
    RemoteEasyconnectSubmitAwsCliUploadResponse, RemoteEasyconnectUploadProgressTelemetry,
    UnixSocketDaemonTransport, DEFAULT_DAEMON_SOCKET_FILE_NAME, LINUX_DAEMON_RUNTIME_DIR,
};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn run(cli: &RemoteCli, writer: &mut impl Write) -> Result<(), RemoteRunError> {
    match cli.command() {
        RemoteCommand::Authenticate(args) => run_authenticate(args, writer),
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

fn run_authenticate(
    args: &AuthenticateArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let username = args
        .username()
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("USER").ok())
        .ok_or_else(|| {
            RemoteRunError::UploadRouting("username is required; pass --username".to_string())
        })?;
    let password = rpassword::prompt_password("DASObjectStore password: ")?;
    let context = authenticate(
        args.host_or_ip(),
        args.https_port(),
        args.ca_cert(),
        args.tls_server_name(),
        &username,
        &password,
        args.object_store(),
        args.session_lifetime_seconds(),
    )?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &context)?;
        writer.write_all(b"\n")?;
    } else {
        write_redacted_connection_context(&context, writer)?;
    }
    Ok(())
}

fn write_redacted_connection_context(
    context: &RemoteConnectionContext,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    serde_json::to_writer_pretty(&mut *writer, &context.redacted())?;
    writer.write_all(b"\n")?;
    writeln!(
        writer,
        "Credentials are redacted; rerun with --json only when a process must consume the temporary S3 context."
    )?;
    Ok(())
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
        args.content_type(),
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
    if args.submit_to_daemon() {
        let source_inventory = source_inventory(args.source())?;
        let socket_path = args
            .daemon_socket()
            .map(PathBuf::from)
            .unwrap_or_else(default_daemon_socket_path);
        let client = DaemonClient::new(UnixSocketDaemonTransport::new(socket_path));
        let response = submit_upload_plan_to_daemon(
            &client,
            &route,
            &plan,
            &config.region,
            credentials.as_ref(),
            args.source(),
            source_inventory,
        )?;
        write_daemon_upload_response(&response, args.progress(), writer)?;
        return Ok(());
    }
    let output = execute_aws_plan(&plan, credentials.as_ref())?;
    if !output.trim().is_empty() {
        writer.write_all(output.as_bytes())?;
    }
    writeln!(writer, "Upload complete")?;
    Ok(())
}

fn submit_upload_plan_to_daemon<T: DaemonClientTransport>(
    client: &DaemonClient<T>,
    route: &RemoteUploadRoute,
    plan: &crate::s3::AwsS3CommandPlan,
    region: &str,
    credentials: Option<&RemoteS3Credentials>,
    source: &Path,
    source_inventory: RemoteSourceInventory,
) -> Result<RemoteEasyconnectSubmitAwsCliUploadResponse, RemoteRunError> {
    client
        .remote_easyconnect_submit_aws_cli_upload(build_daemon_upload_request(
            generated_upload_job_id(),
            route,
            plan,
            region,
            credentials,
            source,
            source_inventory,
        ))
        .map_err(RemoteRunError::Daemon)
}

fn build_daemon_upload_request(
    job_id: String,
    route: &RemoteUploadRoute,
    plan: &crate::s3::AwsS3CommandPlan,
    region: &str,
    credentials: Option<&RemoteS3Credentials>,
    source: &Path,
    source_inventory: RemoteSourceInventory,
) -> RemoteEasyconnectSubmitAwsCliUploadRequest {
    let completion = source_inventory.sha256.as_ref().and_then(|checksum| {
        let crate::s3::AwsS3Operation::UploadFile { destination, .. } = &plan.operation else {
            return None;
        };
        let object_key = destination
            .strip_prefix(&format!("s3://{}/", route.bucket))?
            .to_string();
        let endpoint_url = plan
            .args
            .windows(2)
            .find(|args| args[0] == "--endpoint-url")
            .map(|args| args[1].clone())?;
        let object_version = completion_object_version(checksum);
        Some(dasobjectstore_daemon::RemoteEasyconnectUploadCompletion {
            upload_id: job_id.clone(),
            provider: "garage".to_string(),
            bucket: route.bucket.clone(),
            object_id: object_key.clone(),
            object_version,
            object_key,
            expected_checksum: format!("sha256:{checksum}"),
            endpoint_url,
        })
    });
    let mut upload_args = plan.args.clone();
    if let Some(checksum) = &source_inventory.sha256 {
        let insertion = upload_args.len().saturating_sub(2);
        upload_args.splice(
            insertion..insertion,
            [
                "--metadata".to_string(),
                format!("dasobjectstore-sha256={checksum}"),
            ],
        );
    }
    RemoteEasyconnectSubmitAwsCliUploadRequest {
        job_id,
        object_store: route.object_store.clone(),
        source_bytes: source_inventory.total_bytes,
        policy: plan.backpressure_policy,
        ssd_pressure: dasobjectstore_daemon::DaemonSsdPressure::AcceptingWrites,
        program: plan.program.clone(),
        args: upload_args,
        display_args: redacted_upload_display_args(plan, source),
        environment: daemon_upload_environment(credentials, region),
        progress_telemetry: Some(RemoteEasyconnectUploadProgressTelemetry {
            source_scan_count: Some(source_inventory.file_count),
            staged_bytes: Some(source_inventory.total_bytes),
            session_renewal_status: route.session_renewal_status.clone(),
            ..RemoteEasyconnectUploadProgressTelemetry::default()
        }),
        progress_message: Some(format!(
            "easyconnect upload submitted {} bytes",
            source_inventory.total_bytes
        )),
        completion,
    }
}

fn completion_object_version(checksum: &str) -> u64 {
    (u64::from_str_radix(&checksum[..16], 16).unwrap_or(1) & i64::MAX as u64).max(1)
}

fn daemon_upload_environment(
    credentials: Option<&RemoteS3Credentials>,
    region: &str,
) -> Vec<RemoteEasyconnectAwsCliEnvironmentVariable> {
    let mut environment = vec![RemoteEasyconnectAwsCliEnvironmentVariable {
        name: "AWS_DEFAULT_REGION".to_string(),
        value: region.to_string(),
    }];
    let Some(credentials) = credentials else {
        return environment;
    };
    environment.extend([
        RemoteEasyconnectAwsCliEnvironmentVariable {
            name: "AWS_ACCESS_KEY_ID".to_string(),
            value: credentials.access_key_id.clone(),
        },
        RemoteEasyconnectAwsCliEnvironmentVariable {
            name: "AWS_SECRET_ACCESS_KEY".to_string(),
            value: credentials.secret_access_key.clone(),
        },
    ]);
    if let Some(session_token) = &credentials.session_token {
        environment.push(RemoteEasyconnectAwsCliEnvironmentVariable {
            name: "AWS_SESSION_TOKEN".to_string(),
            value: session_token.clone(),
        });
    }
    environment
}

fn redacted_upload_display_args(plan: &crate::s3::AwsS3CommandPlan, source: &Path) -> Vec<String> {
    let source_arg = source.display().to_string();
    plan.args
        .iter()
        .map(|arg| {
            if arg == &source_arg {
                "<source-redacted>".to_string()
            } else {
                arg.clone()
            }
        })
        .collect()
}

fn write_daemon_upload_response(
    response: &RemoteEasyconnectSubmitAwsCliUploadResponse,
    render_progress: bool,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    writeln!(writer, "Daemon remote upload job submitted")?;
    if render_progress {
        if let Some(event) = &response.running_event {
            write_daemon_job_event("Running", event, writer)?;
        }
    }
    if render_progress {
        for event in &response.progress_events {
            write_daemon_job_event("Progress", event, writer)?;
        }
    }
    write_daemon_job_event("Final", &response.final_event, writer)?;
    Ok(())
}

fn write_daemon_job_event(
    label: &str,
    event: &DaemonJobEvent,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    match event {
        DaemonJobEvent::Progress(job)
        | DaemonJobEvent::Complete(job)
        | DaemonJobEvent::Failed(job) => {
            writeln!(writer, "{label}: {}", daemon_job_progress_line(job))
        }
        DaemonJobEvent::Accepted(job) => {
            writeln!(writer, "{label}: {} accepted", job.job_id.as_str())
        }
        DaemonJobEvent::Cancelled(job) => {
            writeln!(writer, "{label}: {} cancelled", job.job_id.as_str())
        }
    }
}

fn daemon_job_progress_line(job: &DaemonJobSummary) -> String {
    let percent = job
        .progress
        .percent_complete()
        .map(|value| format!("{value:>3}%"))
        .unwrap_or_else(|| " n/a".to_string());
    let units = if job.progress.work_units_total > 0 {
        format!(
            " units={}/{}",
            job.progress.work_units_done, job.progress.work_units_total
        )
    } else {
        String::new()
    };
    let stage = if job.progress.stage.trim().is_empty() {
        "stage=unknown".to_string()
    } else {
        format!("stage={}", job.progress.stage)
    };
    let message = job
        .failure_message
        .as_ref()
        .or(job.progress.message.as_ref())
        .map(|message| format!(" message={message:?}"))
        .unwrap_or_default();

    format!(
        "{} state={:?} {} bytes={}/{}{} {}{}",
        job.job_id.as_str(),
        job.state,
        percent,
        job.progress.work_bytes_done,
        job.progress.work_bytes_total,
        units,
        stage,
        message
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteSourceInventory {
    total_bytes: u64,
    file_count: u64,
    sha256: Option<String>,
}

fn source_inventory(path: &Path) -> Result<RemoteSourceInventory, RemoteRunError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.is_file() {
        return Ok(RemoteSourceInventory {
            total_bytes: metadata.len(),
            file_count: 1,
            sha256: Some(sha256_file(path)?),
        });
    }
    if !metadata.is_dir() {
        return Err(RemoteRunError::UploadRouting(format!(
            "{} is neither a regular file nor a directory",
            path.display()
        )));
    }
    let mut inventory = RemoteSourceInventory {
        total_bytes: 0,
        file_count: 0,
        sha256: None,
    };
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let (child_bytes, child_files) = source_inventory_totals(&entry.path())?;
        inventory.total_bytes = inventory.total_bytes.saturating_add(child_bytes);
        inventory.file_count = inventory.file_count.saturating_add(child_files);
    }
    Ok(inventory)
}

fn source_inventory_totals(path: &Path) -> Result<(u64, u64), RemoteRunError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.is_file() {
        return Ok((metadata.len(), 1));
    }
    if !metadata.is_dir() {
        return Err(RemoteRunError::UploadRouting(format!(
            "{} is neither a regular file nor a directory",
            path.display()
        )));
    }
    let mut bytes = 0_u64;
    let mut files = 0_u64;
    for entry in std::fs::read_dir(path)? {
        let (child_bytes, child_files) = source_inventory_totals(&entry?.path())?;
        bytes = bytes.saturating_add(child_bytes);
        files = files.saturating_add(child_files);
    }
    Ok((bytes, files))
}

fn sha256_file(path: &Path) -> Result<String, RemoteRunError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn default_daemon_socket_path() -> PathBuf {
    PathBuf::from(LINUX_DAEMON_RUNTIME_DIR).join(DEFAULT_DAEMON_SOCKET_FILE_NAME)
}

fn generated_upload_job_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("remote-upload-{}-{nanos}", std::process::id())
}

#[derive(Clone, Debug)]
struct RemoteUploadRoute {
    object_store: String,
    bucket: String,
    credentials: Option<RemoteS3Credentials>,
    credential_source: AwsS3CredentialSource,
    session_renewal_status: Option<String>,
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
            session_renewal_status: None,
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
    reject_expired_session(requested_object_store, session, SystemTime::now())?;

    Ok(RemoteUploadRoute {
        object_store: grant.object_store.clone(),
        bucket: grant.bucket.clone(),
        credentials: Some(session_credentials(session)),
        credential_source: AwsS3CredentialSource::Environment,
        session_renewal_status: Some(session_renewal_status(session).to_string()),
    })
}

fn reject_expired_session(
    requested_object_store: &str,
    session: &RemoteUploadSession,
    now: SystemTime,
) -> Result<(), RemoteRunError> {
    if remote_upload_session_expired(session, now)? {
        return Err(RemoteRunError::UploadRouting(format!(
            "ObjectStore {requested_object_store} has an expired remote upload session; run dasobjectstore-remote easyconnect again"
        )));
    }
    Ok(())
}

fn remote_upload_session_expired(
    session: &RemoteUploadSession,
    now: SystemTime,
) -> Result<bool, RemoteRunError> {
    let expires_at = parse_rfc3339_utc_seconds(&session.expires_at).ok_or_else(|| {
        RemoteRunError::UploadRouting(format!(
            "remote upload session {} has an invalid expires_at timestamp; run dasobjectstore-remote easyconnect again",
            session.redacted_session_id()
        ))
    })?;
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_err(|err| RemoteRunError::Clock(err.to_string()))?
        .as_secs() as i64;

    Ok(expires_at <= now)
}

fn session_credentials(session: &RemoteUploadSession) -> RemoteS3Credentials {
    RemoteS3Credentials {
        access_key_id: session.credentials.access_key_id.clone(),
        secret_access_key: session.credentials.secret_access_key.clone(),
        session_token: session.credentials.session_token.clone(),
    }
}

fn session_renewal_status(session: &RemoteUploadSession) -> &'static str {
    let Some(renewal) = &session.renewal else {
        return "renewal_not_configured";
    };
    if renewal.renewal_token.is_some() {
        "renewal_configured"
    } else {
        "renewal_token_missing"
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
    Authenticate(RemoteAuthenticateError),
    S3(RemoteS3Error),
    Daemon(DaemonClientError),
    Clock(String),
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
            Self::Authenticate(error) => write!(formatter, "{error}"),
            Self::S3(error) => write!(formatter, "{error}"),
            Self::Daemon(error) => write!(formatter, "{error}"),
            Self::Clock(error) => write!(formatter, "{error}"),
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

impl From<RemoteAuthenticateError> for RemoteRunError {
    fn from(error: RemoteAuthenticateError) -> Self {
        Self::Authenticate(error)
    }
}

impl From<RemoteS3Error> for RemoteRunError {
    fn from(error: RemoteS3Error) -> Self {
        Self::S3(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        completion_object_version, daemon_job_progress_line, parse_rfc3339_utc_seconds,
        plan_upload_with_credentials, remote_upload_session_expired, resolve_upload_route, run,
        session_renewal_status, source_inventory, submit_upload_plan_to_daemon,
        write_daemon_upload_response,
    };
    use crate::auth::RemoteAuthAuthority;
    use crate::cli::RemoteCli;
    use crate::config::{
        read_optional_config, write_config, RemoteConfig, RemoteObjectStoreGrant,
        RemotePairedAppliance, RemoteSessionCredentials, RemoteSessionRenewalMetadata,
        RemoteUploadSession,
    };
    use clap::Parser;
    use dasobjectstore_daemon::{
        DaemonApiRequest, DaemonApiResponse, DaemonClient, DaemonJobEvent, DaemonJobId,
        DaemonJobKind, DaemonJobProgress, DaemonJobState, DaemonJobSummary,
        InProcessDaemonTransport, RemoteEasyconnectSubmitAwsCliUploadResponse,
    };
    use std::cell::RefCell;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn completion_object_version_preserves_sqlite_integer_range() {
        assert_eq!(completion_object_version(&format!("{:016x}", 42)), 42);
        assert_eq!(
            completion_object_version("ffffffffffffffff"),
            i64::MAX as u64
        );
        assert_eq!(completion_object_version("0000000000000000"), 1);
    }

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

    #[test]
    fn paired_upload_rejects_missing_session_before_using_credentials() {
        let mut config = paired_config();
        config.paired_appliances[0].session = None;

        let err = resolve_upload_route(&config, "zymo_fecal_2025.05")
            .expect_err("missing session rejected");

        assert!(err.to_string().contains("no active remote upload session"));
    }

    #[test]
    fn paired_upload_rejects_expired_session_before_using_credentials() {
        let mut config = paired_config();
        let session = config.paired_appliances[0]
            .session
            .as_mut()
            .expect("session");
        session.expires_at = "2000-01-01T00:00:00Z".to_string();

        let err = resolve_upload_route(&config, "zymo_fecal_2025.05").expect_err("expiry rejected");

        assert!(err.to_string().contains("expired remote upload session"));
        assert!(!err.to_string().contains("super-secret"));
        assert!(!err.to_string().contains("temporary-token"));
    }

    #[test]
    fn remote_session_expiry_uses_utc_timestamp_contract() {
        let mut config = paired_config();
        let session = config.paired_appliances[0]
            .session
            .as_mut()
            .expect("session");
        session.expires_at = "2026-07-09T19:30:00Z".to_string();
        let before_expiry = UNIX_EPOCH
            + Duration::from_secs(parse_rfc3339_utc_seconds("2026-07-09T19:29:59Z").unwrap() as u64);
        let at_expiry = UNIX_EPOCH
            + Duration::from_secs(parse_rfc3339_utc_seconds("2026-07-09T19:30:00Z").unwrap() as u64);

        assert!(
            !remote_upload_session_expired(session, before_expiry).expect("expiry check succeeds")
        );
        assert!(remote_upload_session_expired(session, at_expiry).expect("expiry check succeeds"));
    }

    #[test]
    fn session_renewal_status_reports_configured_missing_and_not_configured() {
        let config = paired_config_with_renewal();
        let session = config.paired_appliances[0]
            .session
            .as_ref()
            .expect("session");
        assert_eq!(session_renewal_status(session), "renewal_configured");

        let mut missing = paired_config_with_renewal();
        let session = missing.paired_appliances[0]
            .session
            .as_mut()
            .expect("session");
        session.renewal.as_mut().expect("renewal").renewal_token = None;
        assert_eq!(session_renewal_status(session), "renewal_token_missing");

        let config = paired_config();
        let session = config.paired_appliances[0]
            .session
            .as_ref()
            .expect("session");
        assert_eq!(session_renewal_status(session), "renewal_not_configured");
    }

    #[test]
    fn paired_upload_can_submit_aws_plan_to_daemon_with_session_environment() {
        let config = paired_config_with_renewal();
        let root = temp_source_root("upload-daemon-submit");
        std::fs::create_dir_all(&root).expect("create source");
        let source = root.join("reads.fastq.gz");
        std::fs::write(&source, b"ACGT").expect("write source");
        let route =
            resolve_upload_route(&config, "zymo_fecal_2025.05").expect("paired route resolves");
        let credentials = route.credentials.clone();
        let plan = plan_upload_with_credentials(
            &config,
            &route.bucket,
            &source,
            Some("raw/PAW10254"),
            None,
            None,
            false,
            true,
            route.credential_source,
        )
        .expect("upload plan");
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::RemoteEasyconnectSubmitAwsCliUpload(
                daemon_upload_response(),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = submit_upload_plan_to_daemon(
            &client,
            &route,
            &plan,
            "garage",
            credentials.as_ref(),
            &source,
            source_inventory(&source).expect("source inventory"),
        )
        .expect("daemon submit succeeds");
        let mut rendered = Vec::new();
        write_daemon_upload_response(&response, true, &mut rendered).expect("render response");

        let seen_requests = seen.borrow();
        let [DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(request)] =
            seen_requests.as_slice()
        else {
            panic!("expected daemon upload submit request");
        };
        assert_eq!(request.object_store, "zymo_fecal_2025.05");
        assert_eq!(request.source_bytes, 4);
        assert_eq!(
            request
                .progress_telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.source_scan_count),
            Some(1)
        );
        assert_eq!(
            request
                .progress_telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.staged_bytes),
            Some(4)
        );
        assert_eq!(
            request
                .progress_telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.session_renewal_status.as_deref()),
            Some("renewal_configured")
        );
        assert!(request
            .display_args
            .iter()
            .any(|arg| arg == "<source-redacted>"));
        let completion = request.completion.as_ref().expect("completion contract");
        assert_eq!(completion.provider, "garage");
        assert_eq!(completion.bucket, "dos-zymo-fecal-2025-05");
        assert_eq!(completion.object_key, "raw/PAW10254/reads.fastq.gz");
        assert_eq!(completion.expected_checksum.len(), 71);
        assert!(request.args.windows(2).any(|args| {
            args[0] == "--metadata"
                && args[1]
                    == format!(
                        "dasobjectstore-sha256={}",
                        &completion.expected_checksum[7..]
                    )
        }));
        assert_eq!(request.environment.len(), 4);
        assert!(request
            .environment
            .iter()
            .any(|variable| variable.name == "AWS_DEFAULT_REGION" && variable.value == "garage"));
        assert!(request
            .environment
            .iter()
            .any(|variable| variable.name == "AWS_SECRET_ACCESS_KEY"
                && variable.value == "super-secret"));
        let rendered = String::from_utf8(rendered).expect("utf8 output");
        assert!(rendered.contains("Daemon remote upload job submitted"));
        assert!(rendered.contains("remote-upload-test-1"));
        assert!(rendered.contains("Progress: remote-upload-test-1 state=Running  50% bytes=2/4"));
        assert!(rendered.contains("units=1/2"));
        assert!(rendered.contains("stage=remote_s3_transfer_running"));
        assert!(rendered.contains("message=\"copied 2 bytes\""));
        std::fs::remove_dir_all(root).expect("cleanup source");
    }

    #[test]
    fn daemon_upload_progress_renderer_reports_stage_percent_units_and_message() {
        let line = daemon_job_progress_line(&daemon_job(
            DaemonJobState::Running,
            "remote_s3_transfer_running",
            512,
            1024,
            3,
            9,
            Some("remote upload copied 512 bytes"),
            None,
        ));

        assert_eq!(
            line,
            "remote-upload-test-1 state=Running  50% bytes=512/1024 units=3/9 stage=remote_s3_transfer_running message=\"remote upload copied 512 bytes\""
        );
    }

    #[test]
    fn daemon_upload_response_can_suppress_intermediate_progress_rows() {
        let response = daemon_upload_response();
        let mut rendered = Vec::new();

        write_daemon_upload_response(&response, false, &mut rendered).expect("render response");

        let rendered = String::from_utf8(rendered).expect("utf8 output");
        assert!(rendered.contains("Daemon remote upload job submitted"));
        assert!(!rendered.contains("Running:"));
        assert!(!rendered.contains("Progress:"));
        assert!(rendered.contains("Final: remote-upload-test-1 state=Complete 100% bytes=4/4"));
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
                    issued_at: "2099-07-09T11:30:00Z".to_string(),
                    expires_at: "2099-07-09T19:30:00Z".to_string(),
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

    fn daemon_upload_response() -> RemoteEasyconnectSubmitAwsCliUploadResponse {
        RemoteEasyconnectSubmitAwsCliUploadResponse {
            running_event: Some(DaemonJobEvent::Progress(daemon_job(
                DaemonJobState::Running,
                "remote_s3_transfer_running",
                0,
                4,
                0,
                2,
                Some("remote upload started"),
                None,
            ))),
            progress_events: vec![DaemonJobEvent::Progress(daemon_job(
                DaemonJobState::Running,
                "remote_s3_transfer_running",
                2,
                4,
                1,
                2,
                Some("copied 2 bytes"),
                None,
            ))],
            final_event: DaemonJobEvent::Complete(daemon_job(
                DaemonJobState::Complete,
                "remote_s3_transfer_complete",
                4,
                4,
                2,
                2,
                None,
                None,
            )),
        }
    }

    fn daemon_job(
        state: DaemonJobState,
        stage: &str,
        work_bytes_done: u64,
        work_bytes_total: u64,
        work_units_done: u64,
        work_units_total: u64,
        message: Option<&str>,
        failure_message: Option<&str>,
    ) -> DaemonJobSummary {
        DaemonJobSummary {
            job_id: DaemonJobId::new("remote-upload-test-1").expect("job id"),
            kind: DaemonJobKind::RemoteUpload,
            state,
            progress: DaemonJobProgress {
                stage: stage.to_string(),
                work_bytes_done,
                work_bytes_total,
                work_units_done,
                work_units_total,
                message: message.map(str::to_string),
            },
            submitted_at_utc: "2026-07-09T14:52:00Z".to_string(),
            updated_at_utc: "2026-07-09T14:52:01Z".to_string(),
            actor: Some("stephen".to_string()),
            failure_message: failure_message.map(str::to_string),
        }
    }

    fn paired_config_with_renewal() -> RemoteConfig {
        let mut config = paired_config();
        let session = config.paired_appliances[0]
            .session
            .as_mut()
            .expect("paired session");
        session.renewal = Some(RemoteSessionRenewalMetadata {
            renew_url: "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/sessions/SESSIONREFERENCE7890/renew".to_string(),
            renew_after: "2026-07-09T18:30:00Z".to_string(),
            renewal_token: Some("renewal-token-secret".to_string()),
            last_renewed_at: None,
        });
        config
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
