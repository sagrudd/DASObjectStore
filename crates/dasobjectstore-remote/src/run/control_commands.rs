use super::*;

pub(super) fn trust_summary(record: &crate::trust::ApplianceTrustRecord) -> serde_json::Value {
    serde_json::json!({
        "appliance_id": record.appliance_id,
        "endpoint": format!("{}:{}", record.endpoint_host, record.endpoint_port),
        "enrolled_at_utc": record.enrolled_at_utc,
        "subject": record.subject,
        "issuer": record.issuer,
        "subject_alt_names": record.subject_alt_names,
        "not_before": record.not_before,
        "not_after": record.not_after,
        "fingerprint_sha256": record.fingerprint_sha256,
        "spki_sha256": record.spki_sha256,
        "authority_fingerprint_sha256": record.authority_fingerprint_sha256,
        "address_matches_certificate": record.address_matches_certificate,
        "legacy_fingerprint_pinned": record.legacy_fingerprint_pinned,
        "tls_server_name": record.tls_server_name,
    })
}

pub(super) fn run_trust_inspect(
    host: &str,
    port: u16,
    _json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let record = crate::trust::load_trust(host, port)?.ok_or_else(|| {
        RemoteRunError::UploadRouting(format!("no appliance trust is enrolled for {host}:{port}"))
    })?;
    serde_json::to_writer_pretty(&mut *writer, &trust_summary(&record))?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn run_trust_list(_json: bool, writer: &mut impl Write) -> Result<(), RemoteRunError> {
    let records = crate::trust::list_trust()?
        .into_iter()
        .map(|(_, record)| trust_summary(&record))
        .collect::<Vec<_>>();
    serde_json::to_writer_pretty(&mut *writer, &records)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn run_trust_remove(
    appliance_id: &str,
    confirmed: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    if !confirmed {
        if !std::io::stdin().is_terminal() {
            return Err(RemoteRunError::UploadRouting(
                "trust removal requires --yes in non-interactive operation".to_string(),
            ));
        }
        eprint!("Remove TLS trust for appliance {appliance_id}? [y/N] ");
        std::io::stderr().flush()?;
        let mut response = String::new();
        std::io::stdin().read_line(&mut response)?;
        if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(RemoteRunError::UploadRouting(
                "appliance trust was not removed".to_string(),
            ));
        }
    }
    let path = crate::trust::remove_trust(appliance_id)?;
    writeln!(writer, "Removed appliance trust: {}", path.display())?;
    Ok(())
}

pub(super) fn run_trust_rotate(
    appliance_id: &str,
    expected_fingerprint: &str,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let record = crate::trust::rotate_trust(appliance_id, expected_fingerprint)?;
    serde_json::to_writer_pretty(&mut *writer, &trust_summary(&record))?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn run_trust_repair(
    cli: &RemoteCli,
    args: &TrustRepairArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let existing =
        crate::trust::load_trust(args.host_or_ip(), args.https_port())?.ok_or_else(|| {
            RemoteRunError::UploadRouting(format!(
                "no appliance trust is enrolled for {}:{}; use authenticate for first enrollment",
                args.host_or_ip(),
                args.https_port()
            ))
        })?;
    let username = args
        .username()
        .map(str::to_string)
        .or_else(|| std::env::var("USER").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            RemoteRunError::UploadRouting(
                "username is required to construct the trust-repair command".to_string(),
            )
        })?;
    let presented = crate::trust::probe_certificate(args.host_or_ip(), args.https_port())?;
    if crate::trust::verify_presented_pin(&existing, &presented).is_ok()
        || crate::trust::ca_validated_replacement(&existing, &presented).is_ok()
    {
        return run_authenticate_with_identity_policy(
            cli,
            &args.as_authenticate_args(),
            writer,
            false,
        );
    }

    eprintln!(
        "DASObjectStore exceptional trust repair\nEnrolled appliance ID: {}\nOld SHA-256: {}\nNew SHA-256: {}\n{}\nIndependent verification: log into the appliance and run `dasobjectstore trust identity --json`\nProposed command: dasobjectstore-remote trust repair {} --username {} --store {}{}",
        existing.appliance_id,
        existing.fingerprint_sha256,
        presented.fingerprint_sha256,
        crate::trust::format_certificate_details(
            args.host_or_ip(),
            args.https_port(),
            &presented,
            Some(&existing.appliance_id),
        ),
        args.host_or_ip(),
        username,
        args.store(),
        if args.as_authenticate_args().set_s3_config() {
            " --set-s3-config"
        } else {
            ""
        }
    );
    if !std::io::stdin().is_terminal() {
        return Err(RemoteRunError::UploadRouting(
            "identity continuity cannot be proven automatically; compare the appliance-local identity and rerun interactively"
                .to_string(),
        ));
    }
    eprint!(
        "The appliance-local identity has been independently verified. Continue trust repair? [y/N] "
    );
    std::io::stderr().flush()?;
    let mut response = String::new();
    std::io::stdin().read_line(&mut response)?;
    if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        return Err(RemoteRunError::UploadRouting(
            "trust repair was not confirmed".to_string(),
        ));
    }
    let path = crate::trust::trust_record_path(args.host_or_ip(), args.https_port())?;
    let mut replacement = crate::trust::new_trust_record(
        args.host_or_ip(),
        args.https_port(),
        Some(&existing.appliance_id),
        &presented,
    )?;
    replacement.appliance_id = existing.appliance_id.clone();
    crate::trust::replace_trust_if_current(&path, &existing, &replacement)?;
    let result =
        run_authenticate_with_identity_policy(cli, &args.as_authenticate_args(), writer, true);
    if let Err(error) = result {
        let current = crate::trust::load_trust(args.host_or_ip(), args.https_port())?;
        if let Some(current) = current {
            let _ = crate::trust::replace_trust_if_current(&path, &current, &existing);
        }
        return Err(error);
    }
    Ok(())
}

pub(super) fn run_store_readiness(
    cli: &RemoteCli,
    args: &StoreReadinessArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = resolved_control_config(cli, args.store())?;
    let (client, _) = RemoteControlClient::for_store(&config, args.store(), false)?;
    write_control_json(client.readiness(args.store())?, args.json(), writer)
}

pub(super) fn run_object_snapshot(
    cli: &RemoteCli,
    args: &ObjectSnapshotArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    if args.limit() == 0 || args.limit() > 20_000 {
        return Err(RemoteRunError::UploadRouting(
            "snapshot --limit must be between 1 and 20000".to_string(),
        ));
    }
    let config = resolved_control_config(cli, args.store())?;
    let (client, _) = RemoteControlClient::for_store(&config, args.store(), false)?;
    write_control_json(
        client.snapshot(args.store(), args.prefix(), args.cursor(), args.limit())?,
        args.json(),
        writer,
    )
}

pub(super) fn run_object_reconcile(
    cli: &RemoteCli,
    args: &ObjectReconcileS3Args,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    validate_sha256(args.expected_sha256())?;
    if args.idempotency_key().trim().is_empty() {
        return Err(RemoteRunError::UploadRouting(
            "--idempotency-key must not be blank".to_string(),
        ));
    }
    let config = resolved_control_config(cli, args.store())?;
    let (client, _) = RemoteControlClient::for_store(&config, args.store(), true)?;
    let request = ReconcileS3Request {
        key: args.key(),
        expected_bytes: args.expected_bytes(),
        expected_sha256: args.expected_sha256(),
        idempotency_key: args.idempotency_key(),
        ack_policy: args.ack_policy().as_wire_name(),
    };
    write_control_json(
        client.reconcile_s3(args.store(), &request)?,
        args.json(),
        writer,
    )
}

pub(super) fn run_operation_status(
    cli: &RemoteCli,
    args: &OperationStatusArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = resolved_control_config_for_operation(cli)?;
    let client = control_client_for_operation(&config)?;
    write_control_json(
        client.operation_status(args.operation_id())?,
        args.json(),
        writer,
    )
}

pub(super) fn run_operation_wait(
    cli: &RemoteCli,
    args: &OperationWaitArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let timeout = parse_human_duration(args.timeout())?;
    let config = resolved_control_config_for_operation(cli)?;
    let client = control_client_for_operation(&config)?;
    let started = std::time::Instant::now();
    loop {
        let response = client.operation_status(args.operation_id())?;
        if operation_reached(&response, args.until().as_wire_name())
            || operation_terminal(&response)
        {
            return write_control_json(response, args.json(), writer);
        }
        if started.elapsed() >= timeout {
            return Err(RemoteRunError::UploadRouting(format!(
                "operation {} did not reach {} within {}",
                args.operation_id(),
                args.until().as_wire_name(),
                args.timeout()
            )));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

pub(super) fn control_client_for_operation(
    config: &RemoteConfig,
) -> Result<RemoteControlClient, RemoteRunError> {
    let appliance_id = config.default_appliance_id.as_deref().ok_or_else(|| {
        RemoteRunError::UploadRouting(
            "no canonical default appliance is configured; authenticate first".to_string(),
        )
    })?;
    let stores = config
        .session_bindings
        .iter()
        .filter(|binding| binding.appliance_id == appliance_id)
        .map(|binding| binding.store_id.as_str())
        .collect::<Vec<_>>();
    let store =
        match stores.as_slice() {
            [store] => *store,
            [] => {
                return Err(RemoteRunError::UploadRouting(
                    "the canonical default appliance has no active session generation".to_string(),
                ))
            }
            _ => return Err(RemoteRunError::UploadRouting(
                "ambiguous_session_state: operation status requires an exact ObjectStore binding"
                    .to_string(),
            )),
        };
    config
        .session_binding(store)
        .map_err(RemoteRunError::Config)?;
    RemoteControlClient::for_store(config, store, false)
        .map(|(client, _)| client)
        .map_err(Into::into)
}

pub(super) fn operation_store(config: &RemoteConfig) -> Result<String, RemoteRunError> {
    let appliance_id = config.default_appliance_id.as_deref().ok_or_else(|| {
        RemoteRunError::UploadRouting(
            "no canonical default appliance is configured; authenticate first".to_string(),
        )
    })?;
    let stores = config
        .session_bindings
        .iter()
        .filter(|binding| binding.appliance_id == appliance_id)
        .map(|binding| binding.store_id.clone())
        .collect::<Vec<_>>();
    match stores.as_slice() {
        [store] => Ok(store.clone()),
        [] => Err(RemoteRunError::UploadRouting(
            "the canonical default appliance has no active session generation".to_string(),
        )),
        _ => Err(RemoteRunError::UploadRouting(
            "ambiguous_session_state: operation status requires an exact ObjectStore binding"
                .to_string(),
        )),
    }
}

pub(super) fn resolved_control_config(
    cli: &RemoteCli,
    store: &str,
) -> Result<RemoteConfig, RemoteRunError> {
    let path = config_path(cli)?;
    let _ = read_optional_config(&path)?;
    let transaction_lock = acquire_config_transaction(&path)?;
    let mut config = resolved_valid_config(cli)?;
    if renew_store_session_if_due(&mut config, store)? {
        let binding = config.session_binding(store)?.clone();
        if let Some(profile) = &binding.s3_profile {
            let backup = snapshot_profile_state()?;
            let context = connection_context_from_binding(&binding)?;
            let existing = config
                .s3_profiles
                .iter()
                .find(|association| association.profile == *profile)
                .cloned();
            let update = install_profile(&context, profile, existing.as_ref(), true, true);
            match update {
                Ok((association, _)) => {
                    config.s3_profiles.retain(|item| item.profile != *profile);
                    config.s3_profiles.push(association);
                }
                Err(error) => {
                    restore_profile_state(&backup)?;
                    return Err(error.into());
                }
            }
        }
        write_config_locked(&path, &config, &transaction_lock)?;
    }
    Ok(config)
}

pub(super) fn connection_context_from_binding(
    binding: &RemoteSessionBinding,
) -> Result<RemoteConnectionContext, RemoteRunError> {
    let renewal = binding.session.renewal.as_ref().ok_or_else(|| {
        RemoteRunError::UploadRouting(
            "session_expired_reauthentication_required: renewal metadata is unavailable"
                .to_string(),
        )
    })?;
    Ok(RemoteConnectionContext {
        schema_version: "dasobjectstore.remote_authenticate.v3".to_string(),
        appliance_id: binding.appliance_id.clone(),
        appliance_host: reqwest::Url::parse(&binding.control_base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .ok_or_else(|| {
                RemoteRunError::UploadRouting(
                    "configuration_migration_required: control endpoint is invalid".to_string(),
                )
            })?,
        endpoint_url: binding.s3_endpoint_url.clone(),
        region: binding.region.clone(),
        addressing_style: binding.addressing_style.clone(),
        object_store: binding.store_id.clone(),
        bucket: binding.bucket.clone(),
        access_key_id: binding.session.credentials.access_key_id.clone(),
        secret_access_key: binding.session.credentials.secret_access_key.clone(),
        session_token: binding.session.credentials.session_token.clone(),
        session_id: binding.session.session_id.clone(),
        issued_at_utc: binding.session.issued_at.clone(),
        expires_at_utc: binding.session.expires_at.clone(),
        renew_url: renewal.renew_url.clone(),
        renew_after_utc: renewal.renew_after.clone(),
        renewal_token: renewal.renewal_token.clone().unwrap_or_default(),
    })
}

pub(super) fn resolved_control_config_for_operation(
    cli: &RemoteCli,
) -> Result<RemoteConfig, RemoteRunError> {
    let config = resolved_valid_config(cli)?;
    let store = operation_store(&config)?;
    resolved_control_config(cli, &store)
}

pub(super) fn operation_reached(response: &serde_json::Value, until: &str) -> bool {
    match until {
        "ssd_acknowledged" => response
            .get("ssd_acknowledged")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "hdd_settled" => response
            .get("hdd_settled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        _ => operation_terminal(response),
    }
}

pub(super) fn operation_terminal(response: &serde_json::Value) -> bool {
    response
        .get("state")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|state| matches!(state, "complete" | "failed" | "cancelled"))
}

pub(super) fn parse_human_duration(value: &str) -> Result<Duration, RemoteRunError> {
    let (number, multiplier) = if let Some(number) = value.strip_suffix('s') {
        (number, 1)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3600)
    } else {
        return Err(RemoteRunError::UploadRouting(
            "--timeout must end in s, m, or h".to_string(),
        ));
    };
    let number = number.parse::<u64>().map_err(|_| {
        RemoteRunError::UploadRouting("--timeout must contain a positive integer".to_string())
    })?;
    if number == 0 {
        return Err(RemoteRunError::UploadRouting(
            "--timeout must be greater than zero".to_string(),
        ));
    }
    Ok(Duration::from_secs(number.saturating_mul(multiplier)))
}

pub(super) fn validate_sha256(value: &str) -> Result<(), RemoteRunError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RemoteRunError::UploadRouting(
            "--expected-sha256 must be exactly 64 hexadecimal characters".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn write_control_json(
    value: serde_json::Value,
    _json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    serde_json::to_writer_pretty(&mut *writer, &value)?;
    writer.write_all(b"\n")?;
    Ok(())
}
