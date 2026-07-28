use super::*;
use serde::Serialize;

const RESYNC_PROTOCOL: u32 = 1;
const RESYNC_CAPABILITY: &str = "remote_resync_v1";
const AUTHORITATIVE_S3_ENDPOINT_CAPABILITY: &str = "authoritative_s3_endpoint_v1";

#[derive(Debug, Serialize)]
struct ResyncReport {
    schema_version: &'static str,
    host: String,
    objectstore: String,
    appliance_id: String,
    server_version: String,
    component_builds: dasobjectstore_daemon::RemoteEasyconnectComponentBuilds,
    compatibility: String,
    proposed_actions: Vec<String>,
    warnings: Vec<String>,
    blockers: Vec<String>,
    applied: bool,
    state: String,
    readiness: Option<serde_json::Value>,
}

pub(super) fn run_resync(
    cli: &RemoteCli,
    args: &ResyncArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let presented = crate::trust::probe_certificate(args.host_or_ip(), args.https_port())?;
    if let Some(expected) = args.trust_fingerprint() {
        crate::trust::expected_fingerprint_matches(expected, &presented)?;
    }
    let existing = crate::trust::load_trust(args.host_or_ip(), args.https_port())?;
    let trust_action = match &existing {
        Some(record) if crate::trust::verify_presented_pin(record, &presented).is_ok() => {
            "trust_retained"
        }
        Some(record) if crate::trust::ca_validated_replacement(record, &presented).is_ok() => {
            "trust_rotated_ca_verified"
        }
        Some(_) => "trust_replacement_confirmation_required",
        None => "trust_enrollment_required",
    };
    let tls_name = presented.tls_server_name.as_deref().ok_or_else(|| {
        RemoteRunError::UploadRouting(
            "presented certificate has no usable endpoint identity".to_string(),
        )
    })?;
    let descriptor = crate::authenticate::discover_appliance_descriptor(
        args.host_or_ip(),
        args.https_port(),
        presented.certificate_pem.as_bytes(),
        tls_name,
    )?;
    let compatibility = negotiate_descriptor(&descriptor)?;
    let config_path = config_path(cli)?;
    let doctor = doctor_config(&config_path)?;
    let bindings = doctor
        .bindings
        .iter()
        .filter(|binding| binding.store_id == args.object_store())
        .collect::<Vec<_>>();
    let mut actions = vec![trust_action.to_string()];
    actions.extend(bindings.iter().map(|binding| {
        if binding.appliance_id == descriptor.appliance_id
            && binding.trust_identity_matches
            && !binding.expired
        {
            format!(
                "session_retained appliance_id={} store={}",
                binding.appliance_id, binding.store_id
            )
        } else {
            format!(
                "session_retired appliance_id={} store={}",
                binding.appliance_id, binding.store_id
            )
        }
    }));
    if !bindings.iter().any(|binding| {
        binding.appliance_id == descriptor.appliance_id
            && binding.trust_identity_matches
            && !binding.expired
    }) {
        actions.push("authentication_required".to_string());
    }
    actions.push(if args.set_s3_config() {
        let profile = args
            .s3_profile()
            .map(str::to_string)
            .map(Ok)
            .unwrap_or_else(|| default_profile_name(args.object_store()))?;
        if bindings
            .iter()
            .any(|binding| binding.s3_profile.as_deref() == Some(profile.as_str()))
        {
            format!("s3_profile_replaced_and_verified profile={profile}")
        } else {
            format!("s3_profile_created_and_verified profile={profile}")
        }
    } else {
        "s3_profile_unchanged".to_string()
    });
    let mut report = ResyncReport {
        schema_version: "dasobjectstore.remote_resync.v1",
        host: args.host_or_ip().to_string(),
        objectstore: args.object_store().to_string(),
        appliance_id: descriptor.appliance_id.clone(),
        server_version: descriptor.server_version.clone(),
        component_builds: descriptor.component_builds.clone(),
        compatibility,
        proposed_actions: actions,
        warnings: Vec::new(),
        blockers: Vec::new(),
        applied: false,
        state: "planned".to_string(),
        readiness: None,
    };
    if args.dry_run() {
        return write_report(&report, args.json(), writer);
    }

    let auth_args = args.as_authenticate_args();
    let mut ignored_auth_output = Vec::new();
    if let Some(existing) = existing.as_ref().filter(|record| {
        crate::trust::verify_presented_pin(record, &presented).is_err()
            && crate::trust::ca_validated_replacement(record, &presented).is_err()
    }) {
        let (trust_path, replacement) = confirm_and_apply_replacement(args, existing, &presented)?;
        if let Err(error) =
            run_authenticate_with_identity_policy(cli, &auth_args, &mut ignored_auth_output, true)
        {
            rollback_provisional_replacement(&trust_path, &replacement, existing).map_err(
                |rollback_error| {
                    RemoteRunError::UploadRouting(format!(
                        "resync authentication failed ({error}); trust rollback also failed: {rollback_error}"
                    ))
                },
            )?;
            return Err(error);
        }
    } else {
        run_authenticate_with_identity_policy(cli, &auth_args, &mut ignored_auth_output, false)?;
    }

    let config = read_optional_config(&config_path)?.ok_or_else(|| {
        RemoteRunError::UploadRouting(
            "resync authentication completed without a committed configuration".to_string(),
        )
    })?;
    let binding = config.session_binding(args.object_store())?;
    if binding.appliance_id != descriptor.appliance_id {
        return Err(RemoteRunError::UploadRouting(
            "resync committed a session for a different appliance identity".to_string(),
        ));
    }
    let (control, _) = RemoteControlClient::for_store(&config, args.object_store(), false)?;
    let readiness = control.readiness(args.object_store())?;
    let blocking = readiness_blockers(&readiness);
    report.warnings = readiness
        .get("reason_codes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect();
    report.applied = true;
    report.readiness = Some(readiness);
    if blocking.is_empty() {
        report.state = "ready".to_string();
    } else {
        report.state = "blocked".to_string();
        report.blockers = blocking;
    }
    write_report(&report, args.json(), writer)
}

fn rollback_provisional_replacement(
    path: &std::path::Path,
    provisional: &crate::trust::ApplianceTrustRecord,
    previous: &crate::trust::ApplianceTrustRecord,
) -> Result<(), RemoteRunError> {
    let current = crate::trust::load_trust(&previous.endpoint_host, previous.endpoint_port)?
        .ok_or_else(|| {
            RemoteRunError::UploadRouting(
                "provisional appliance trust disappeared before rollback".to_string(),
            )
        })?;
    if current.endpoint_host != provisional.endpoint_host
        || current.endpoint_port != provisional.endpoint_port
        || current.fingerprint_sha256 != provisional.fingerprint_sha256
        || current.spki_sha256 != provisional.spki_sha256
    {
        return Err(RemoteRunError::UploadRouting(
            "concurrent trust update prevented safe resync rollback".to_string(),
        ));
    }
    crate::trust::replace_trust_if_current(path, &current, previous)?;
    Ok(())
}

fn readiness_blockers(readiness: &serde_json::Value) -> Vec<String> {
    let mut blockers = [
        "catalogue_ready",
        "ssd_ingest_ready",
        "readable",
        "writable",
    ]
    .into_iter()
    .filter_map(
        |field| match readiness.get(field).and_then(serde_json::Value::as_bool) {
            Some(true) => None,
            Some(false) => Some(field.to_string()),
            None => Some(format!("{field}_unavailable")),
        },
    )
    .collect::<Vec<_>>();
    if !readiness
        .get("profile_binding")
        .is_some_and(serde_json::Value::is_object)
    {
        blockers.push("profile_binding_unavailable".to_string());
    }
    blockers
}

fn negotiate_descriptor(
    descriptor: &dasobjectstore_daemon::RemoteEasyconnectDiscoveryResponse,
) -> Result<String, RemoteRunError> {
    if RESYNC_PROTOCOL < descriptor.remote_client_protocol_min {
        return Err(RemoteRunError::UploadRouting(format!(
            "remote_client_too_old: appliance requires protocol {}; upgrade dasobjectstore-remote with the matching DASObjectStore package",
            descriptor.remote_client_protocol_min
        )));
    }
    if RESYNC_PROTOCOL > descriptor.remote_client_protocol_max {
        return Err(RemoteRunError::UploadRouting(format!(
            "appliance_too_old: appliance supports protocol {}; upgrade the appliance with `sudo apt-get install --reinstall dasobjectstore`",
            descriptor.remote_client_protocol_max
        )));
    }
    if !descriptor
        .capabilities
        .iter()
        .any(|capability| capability == RESYNC_CAPABILITY)
    {
        return Err(RemoteRunError::UploadRouting(
            "remote_resync_unsupported: upgrade the appliance with `sudo apt-get install --reinstall dasobjectstore`"
                .to_string(),
        ));
    }
    if !descriptor
        .capabilities
        .iter()
        .any(|capability| capability == AUTHORITATIVE_S3_ENDPOINT_CAPABILITY)
    {
        return Err(RemoteRunError::UploadRouting(
            "authoritative_s3_endpoint_unsupported: upgrade the appliance before resynchronizing AWS configuration"
                .to_string(),
        ));
    }
    Ok(format!("protocol_{RESYNC_PROTOCOL}_compatible"))
}

fn confirm_and_apply_replacement(
    args: &ResyncArgs,
    existing: &crate::trust::ApplianceTrustRecord,
    presented: &crate::trust::PresentedCertificate,
) -> Result<(std::path::PathBuf, crate::trust::ApplianceTrustRecord), RemoteRunError> {
    eprintln!(
        "Appliance replacement requires independent verification.\nEnrolled appliance ID: {}\nOld SHA-256: {}\nNew SHA-256: {}\nRun on the appliance: dasobjectstore trust identity --json",
        existing.appliance_id, existing.fingerprint_sha256, presented.fingerprint_sha256
    );
    if !args.accept_verified_appliance_replacement() {
        if !std::io::stdin().is_terminal() {
            return Err(RemoteRunError::UploadRouting(
                "replacement requires interactive confirmation or --accept-verified-appliance-replacement with --trust-fingerprint"
                    .to_string(),
            ));
        }
        eprint!("The appliance-local identity was independently verified. Continue? [y/N] ");
        std::io::stderr().flush()?;
        let mut response = String::new();
        std::io::stdin().read_line(&mut response)?;
        if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(RemoteRunError::UploadRouting(
                "appliance replacement was not confirmed".to_string(),
            ));
        }
    }
    let mut replacement = crate::trust::new_trust_record(
        args.host_or_ip(),
        args.https_port(),
        Some(&existing.appliance_id),
        presented,
    )?;
    replacement.appliance_id = existing.appliance_id.clone();
    let path = crate::trust::trust_record_path(args.host_or_ip(), args.https_port())?;
    crate::trust::replace_trust_if_current(&path, existing, &replacement)?;
    Ok((path, replacement))
}

fn write_report(
    report: &ResyncReport,
    json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    if json {
        serde_json::to_writer_pretty(&mut *writer, report)?;
        writer.write_all(b"\n")?;
    } else {
        for action in &report.proposed_actions {
            writeln!(writer, "{action}")?;
        }
        writeln!(
            writer,
            "objectstore={} state={}",
            report.objectstore, report.state
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(
        min: u32,
        max: u32,
        capabilities: &[&str],
    ) -> dasobjectstore_daemon::RemoteEasyconnectDiscoveryResponse {
        dasobjectstore_daemon::RemoteEasyconnectDiscoveryResponse {
            appliance_id: "das-appliance-test".to_string(),
            product_id: "dasobjectstore".to_string(),
            display_name: "test".to_string(),
            pairing_create_url: String::new(),
            pairing_exchange_url: String::new(),
            session_revoke_url_template: String::new(),
            session_renew_url_template: String::new(),
            default_session_lifetime_seconds: 28_800,
            session_policy: Default::default(),
            auth_providers: Vec::new(),
            descriptor_schema_version: "dasobjectstore.remote_descriptor.v1".to_string(),
            server_version: "99.0.0".to_string(),
            api_schema_versions: Vec::new(),
            capabilities: capabilities
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            remote_client_protocol_min: min,
            remote_client_protocol_max: max,
            component_builds: Default::default(),
        }
    }

    #[test]
    fn compatibility_uses_protocol_and_capability_not_semantic_version() {
        let supported = [RESYNC_CAPABILITY, AUTHORITATIVE_S3_ENDPOINT_CAPABILITY];
        assert!(negotiate_descriptor(&descriptor(1, 1, &supported)).is_ok());
        assert!(negotiate_descriptor(&descriptor(2, 2, &supported))
            .expect_err("client old")
            .to_string()
            .contains("remote_client_too_old"));
        assert!(negotiate_descriptor(&descriptor(0, 0, &supported))
            .expect_err("server old")
            .to_string()
            .contains("appliance_too_old"));
        assert!(
            negotiate_descriptor(&descriptor(1, 1, &["capability_added_later"]))
                .expect_err("missing resync")
                .to_string()
                .contains("remote_resync_unsupported")
        );
        assert!(
            negotiate_descriptor(&descriptor(1, 1, &[RESYNC_CAPABILITY]))
                .expect_err("missing endpoint authority")
                .to_string()
                .contains("authoritative_s3_endpoint_unsupported")
        );
    }

    #[test]
    fn readiness_is_fail_closed_for_false_or_missing_required_fields() {
        assert!(readiness_blockers(&serde_json::json!({
            "catalogue_ready": true,
            "ssd_ingest_ready": true,
            "readable": true,
            "writable": true,
            "profile_binding": {}
        }))
        .is_empty());
        assert_eq!(
            readiness_blockers(&serde_json::json!({
                "catalogue_ready": true,
                "ssd_ingest_ready": false,
                "readable": true,
                "profile_binding": {}
            })),
            vec!["ssd_ingest_ready", "writable_unavailable"]
        );
    }
}
