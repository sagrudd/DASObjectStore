use super::*;

pub(super) fn run_upload(
    cli: &RemoteCli,
    args: &UploadArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = resolved_valid_config(cli)?;
    let route = resolve_upload_route(&config, args.store(), args.bucket())?;
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

pub(super) fn submit_upload_plan_to_daemon<T: DaemonClientTransport>(
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

pub(super) fn build_daemon_upload_request(
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

pub(super) fn completion_object_version(checksum: &str) -> u64 {
    (u64::from_str_radix(&checksum[..16], 16).unwrap_or(1) & i64::MAX as u64).max(1)
}

pub(super) fn daemon_upload_environment(
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

pub(super) fn redacted_upload_display_args(
    plan: &crate::s3::AwsS3CommandPlan,
    source: &Path,
) -> Vec<String> {
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

pub(super) fn write_daemon_upload_response(
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

pub(super) fn write_daemon_job_event(
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

pub(super) fn daemon_job_progress_line(job: &DaemonJobSummary) -> String {
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
pub(super) struct RemoteSourceInventory {
    total_bytes: u64,
    file_count: u64,
    sha256: Option<String>,
}

pub(super) fn source_inventory(path: &Path) -> Result<RemoteSourceInventory, RemoteRunError> {
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

pub(super) fn source_inventory_totals(path: &Path) -> Result<(u64, u64), RemoteRunError> {
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

pub(super) fn sha256_file(path: &Path) -> Result<String, RemoteRunError> {
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

pub(super) fn default_daemon_socket_path() -> PathBuf {
    PathBuf::from(LINUX_DAEMON_RUNTIME_DIR).join(DEFAULT_DAEMON_SOCKET_FILE_NAME)
}

pub(super) fn generated_upload_job_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("remote-upload-{}-{nanos}", std::process::id())
}

#[derive(Clone, Debug)]
pub(super) struct RemoteUploadRoute {
    pub(super) object_store: String,
    pub(super) bucket: String,
    pub(super) credentials: Option<RemoteS3Credentials>,
    pub(super) credential_source: AwsS3CredentialSource,
    pub(super) session_renewal_status: Option<String>,
}

pub(super) fn resolve_upload_route(
    config: &RemoteConfig,
    requested_object_store: &str,
    reviewed_bucket: Option<&str>,
) -> Result<RemoteUploadRoute, RemoteRunError> {
    if config.session_bindings.is_empty() && config.paired_appliances.is_empty() {
        return Ok(RemoteUploadRoute {
            object_store: requested_object_store.to_string(),
            bucket: reviewed_bucket
                .unwrap_or(requested_object_store)
                .to_string(),
            credentials: None,
            credential_source: AwsS3CredentialSource::AwsProfile,
            session_renewal_status: None,
        });
    }
    if config.paired_appliances.iter().any(|appliance| {
        appliance
            .object_stores
            .iter()
            .any(|grant| grant.bucket == requested_object_store)
    }) {
        return Err(RemoteRunError::UploadRouting(format!(
            "{requested_object_store} is an S3 bucket name; choose a writable ObjectStore name"
        )));
    }

    let binding = config
        .session_binding(requested_object_store)
        .map_err(RemoteRunError::Config)?;
    let Some(grant) = config
        .paired_appliances
        .iter()
        .filter(|appliance| appliance.appliance_id == binding.appliance_id)
        .find_map(|appliance| appliance.writable_object_store(requested_object_store))
    else {
        return Err(RemoteRunError::UploadRouting(format!(
            "ObjectStore {requested_object_store} is not writable in the paired appliance grants; run easyconnect again or choose a writable ObjectStore name"
        )));
    };
    let session = &binding.session;
    reject_expired_session(requested_object_store, session, SystemTime::now())?;

    Ok(RemoteUploadRoute {
        object_store: grant.object_store.clone(),
        bucket: grant.bucket.clone(),
        credentials: Some(session_credentials(session)),
        credential_source: AwsS3CredentialSource::Environment,
        session_renewal_status: Some(session_renewal_status(session).to_string()),
    })
}

pub(super) fn reject_expired_session(
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

pub(super) fn remote_upload_session_expired(
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

pub(super) fn session_credentials(session: &RemoteUploadSession) -> RemoteS3Credentials {
    RemoteS3Credentials {
        access_key_id: session.credentials.access_key_id.clone(),
        secret_access_key: session.credentials.secret_access_key.clone(),
        session_token: session.credentials.session_token.clone(),
    }
}

pub(super) fn session_renewal_status(session: &RemoteUploadSession) -> &'static str {
    let Some(renewal) = &session.renewal else {
        return "renewal_not_configured";
    };
    if renewal.renewal_token.is_some() {
        "renewal_configured"
    } else {
        "renewal_token_missing"
    }
}

pub(super) fn resolved_valid_config(cli: &RemoteCli) -> Result<RemoteConfig, RemoteRunError> {
    let config = resolved_config(cli)?;
    config.validate_for_command()?;
    Ok(config)
}

pub(super) fn resolved_config(cli: &RemoteCli) -> Result<RemoteConfig, RemoteRunError> {
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

pub(super) fn empty_config() -> RemoteConfig {
    RemoteConfig {
        schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
        generation: 0,
        endpoint_url: String::new(),
        region: DEFAULT_REGION.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        auth_authority: RemoteAuthAuthority::AwsProfile,
        username: None,
        credential_helper: None,
        default_appliance_id: None,
        paired_appliances: Vec::new(),
        s3_profiles: Vec::new(),
        session_bindings: Vec::new(),
    }
}

pub(super) fn config_path(cli: &RemoteCli) -> Result<PathBuf, RemoteRunError> {
    cli.config()
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(default_config_path)
        .map_err(Into::into)
}

pub(super) fn resolve_credentials(
    _cli: &RemoteCli,
    config: &RemoteConfig,
) -> Result<Option<RemoteS3Credentials>, RemoteRunError> {
    let Some(helper) = &config.credential_helper else {
        return Ok(None);
    };
    Ok(Some(request_s3_credentials(
        helper,
        config.auth_authority,
        &config.endpoint_url,
        config.username.as_deref(),
    )?))
}
