use super::{
    completion_object_version, daemon_job_progress_line, parse_rfc3339_utc_seconds,
    plan_upload_with_credentials, remote_upload_session_expired, resolve_upload_route, run,
    session_renewal_status, source_inventory, submit_upload_plan_to_daemon,
    write_daemon_upload_response,
};
use crate::auth::RemoteAuthAuthority;
use crate::cli::RemoteCli;
use crate::config::REMOTE_CONFIG_SCHEMA_VERSION;
use crate::config::{
    read_optional_config, write_config, RemoteConfig, RemoteObjectStoreGrant,
    RemotePairedAppliance, RemoteSessionBinding, RemoteSessionCredentials,
    RemoteSessionRenewalMetadata, RemoteUploadSession,
};
use clap::Parser;
use dasobjectstore_daemon::{
    DaemonApiRequest, DaemonApiResponse, DaemonClient, DaemonJobEvent, DaemonJobId, DaemonJobKind,
    DaemonJobProgress, DaemonJobState, DaemonJobSummary, InProcessDaemonTransport,
    RemoteEasyconnectSubmitAwsCliUploadResponse,
};
use std::cell::RefCell;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn retired_password_options_fail_before_any_remote_work() {
    let cli = RemoteCli::try_parse_from([
        "dasobjectstore-remote",
        "--auth",
        "local-password",
        "stores",
        "list",
    ])
    .expect("legacy CLI spelling remains parseable for explicit remediation");
    let error = run(&cli, &mut Vec::new()).expect_err("retired authority must fail closed");
    assert!(error
        .to_string()
        .contains("local-password authority is retired"));

    let cli = RemoteCli::try_parse_from([
        "dasobjectstore-remote",
        "--prompt-password",
        "stores",
        "list",
    ])
    .expect("legacy password option remains parseable for explicit remediation");
    let error = run(&cli, &mut Vec::new()).expect_err("retired option must fail closed");
    assert!(error.to_string().contains("--prompt-password is retired"));

    let cli = RemoteCli::try_parse_from([
        "dasobjectstore-remote",
        "authenticate",
        "unreachable.invalid",
        "example-store",
    ])
    .expect("legacy bootstrap command remains parseable for remediation");
    let error = run(&cli, &mut Vec::new()).expect_err("legacy bootstrap must fail closed");
    assert!(error
        .to_string()
        .contains("password authentication is retired"));
}

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
    assert!(rendered.contains("ObjectStore: zymo_fecal_2025.05 -> bucket dos-zymo-fecal-2025-05"));
    assert!(rendered.contains("Remote upload S3 concurrency: 2"));
    assert!(rendered.contains("SSD high pressure action: pause_new_transfers"));
    assert!(rendered.contains("s3://dos-zymo-fecal-2025-05/raw/PAW10254/reads.fastq.gz"));
    assert!(!rendered.contains("--profile"));
    assert!(!rendered.contains("s3://zymo_fecal_2025.05/"));
    std::fs::remove_dir_all(root).expect("cleanup source");
    let _ = std::fs::remove_file(path);
}

#[test]
fn unpaired_daemon_route_keeps_logical_store_and_reviewed_bucket_distinct() {
    let mut config = paired_config();
    config.paired_appliances.clear();
    config.session_bindings.clear();

    let route = resolve_upload_route(&config, "pinakotheke_media", Some("dos-pinakotheke-media"))
        .expect("reviewed daemon route resolves");

    assert_eq!(route.object_store, "pinakotheke_media");
    assert_eq!(route.bucket, "dos-pinakotheke-media");
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
    config.session_bindings.clear();

    let err = resolve_upload_route(&config, "zymo_fecal_2025.05", None)
        .expect_err("missing session rejected");

    assert!(err.to_string().contains("configuration_migration_required"));
}

#[test]
fn paired_upload_rejects_expired_session_before_using_credentials() {
    let mut config = paired_config();
    let session = &mut config.session_bindings[0].session;
    session.expires_at = "2000-01-01T00:00:00Z".to_string();

    let err =
        resolve_upload_route(&config, "zymo_fecal_2025.05", None).expect_err("expiry rejected");

    assert!(err.to_string().contains("expired remote upload session"));
    assert!(!err.to_string().contains("super-secret"));
    assert!(!err.to_string().contains("temporary-token"));
}

#[test]
fn remote_session_expiry_uses_utc_timestamp_contract() {
    let mut config = paired_config();
    let session = &mut config.session_bindings[0].session;
    session.expires_at = "2026-07-09T19:30:00Z".to_string();
    let before_expiry = UNIX_EPOCH
        + Duration::from_secs(parse_rfc3339_utc_seconds("2026-07-09T19:29:59Z").unwrap() as u64);
    let at_expiry = UNIX_EPOCH
        + Duration::from_secs(parse_rfc3339_utc_seconds("2026-07-09T19:30:00Z").unwrap() as u64);

    assert!(!remote_upload_session_expired(session, before_expiry).expect("expiry check succeeds"));
    assert!(remote_upload_session_expired(session, at_expiry).expect("expiry check succeeds"));
}

#[test]
fn session_renewal_status_reports_configured_missing_and_not_configured() {
    let config = paired_config_with_renewal();
    let session = &config.session_bindings[0].session;
    assert_eq!(session_renewal_status(session), "renewal_configured");

    let mut missing = paired_config_with_renewal();
    let session = &mut missing.session_bindings[0].session;
    session.renewal.as_mut().expect("renewal").renewal_token = None;
    assert_eq!(session_renewal_status(session), "renewal_token_missing");

    let config = paired_config();
    let session = &config.session_bindings[0].session;
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
        resolve_upload_route(&config, "zymo_fecal_2025.05", None).expect("paired route resolves");
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
        Some("/run/site-trust/monas.pem"),
        &source,
        source_inventory(&source).expect("source inventory"),
    )
    .expect("daemon submit succeeds");
    let mut rendered = Vec::new();
    write_daemon_upload_response(&response, true, &mut rendered).expect("render response");

    let seen_requests = seen.borrow();
    let [DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(request)] = seen_requests.as_slice()
    else {
        panic!("expected daemon upload submit request");
    };
    assert_eq!(request.object_store, "zymo_fecal_2025.05");
    assert_eq!(request.source_bytes, 4);
    assert!(request.environment.iter().any(|variable| {
        variable.name == "AWS_CA_BUNDLE" && variable.value == "/run/site-trust/monas.pem"
    }));
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
    assert_eq!(request.environment.len(), 5);
    assert!(request
        .environment
        .iter()
        .any(|variable| variable.name == "AWS_DEFAULT_REGION" && variable.value == "garage"));
    assert!(request.environment.iter().any(
        |variable| variable.name == "AWS_SECRET_ACCESS_KEY" && variable.value == "super-secret"
    ));
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
    let session = RemoteUploadSession {
        session_id: "SESSIONREFERENCE7890".to_string(),
        issued_at: "2099-07-09T11:30:00Z".to_string(),
        expires_at: "2099-07-09T19:30:00Z".to_string(),
        credentials: RemoteSessionCredentials {
            access_key_id: "DOSREMOTEACCESSKEY1234".to_string(),
            secret_access_key: "super-secret".to_string(),
            session_token: Some("temporary-token".to_string()),
        },
        renewal: None,
    };
    RemoteConfig {
            schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
            generation: 1,
            endpoint_url: "https://192.168.1.192:3900".to_string(),
            region: "garage".to_string(),
            profile: "dasobjectstore".to_string(),
            auth_authority: RemoteAuthAuthority::Pistis,
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
                auth_authority: RemoteAuthAuthority::Pistis,
                tls_trust: crate::config::RemoteTlsTrust::EnrolledCertificate,
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
                session: None,
            }],
            s3_profiles: Vec::new(),
            session_bindings: vec![RemoteSessionBinding {
                appliance_id: "appliance-1".to_string(),
                store_id: "zymo_fecal_2025.05".to_string(),
                control_base_url: "https://192.168.1.192:8448".to_string(),
                s3_endpoint_url: "https://192.168.1.192:3900".to_string(),
                bucket: "dos-zymo-fecal-2025-05".to_string(),
                region: "garage".to_string(),
                addressing_style: "path".to_string(),
                s3_profile: None,
                tls_trust: crate::config::RemoteTlsTrust::EnrolledCertificate,
                site_trust_bundle_path: None,
                trust_fingerprint_sha256: "test-fingerprint".to_string(),
                trust_spki_sha256: "test-spki".to_string(),
                session,
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
    let session = &mut config.session_bindings[0].session;
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
