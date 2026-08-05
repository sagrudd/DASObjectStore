use super::*;

pub(super) fn run_authenticate(
    cli: &RemoteCli,
    args: &AuthenticateArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    run_authenticate_with_identity_policy(cli, args, writer, false)
}

pub(super) fn run_authenticate_with_identity_policy(
    cli: &RemoteCli,
    args: &AuthenticateArgs,
    writer: &mut impl Write,
    allow_confirmed_identity_replacement: bool,
) -> Result<(), RemoteRunError> {
    if legacy_password_transport_is_retired() {
        return Err(RemoteAuthenticateError::RetiredLocalPassword.into());
    }
    let initial_trust = crate::trust::load_trust(args.host_or_ip(), args.https_port())?;
    let username = args
        .username()
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("USER").ok())
        .ok_or_else(|| {
            RemoteRunError::UploadRouting("username is required; pass --username".to_string())
        })?;
    let trust_result = prepare_appliance_trust(
        args.host_or_ip(),
        args.https_port(),
        args.ca_cert(),
        args.tls_server_name(),
        args.trust_fingerprint(),
        |certificate, appliance_id, confirmation_required| {
            eprintln!(
                "{}",
                crate::trust::format_certificate_details(
                    args.host_or_ip(),
                    args.https_port(),
                    certificate,
                    appliance_id,
                )
            );
            if !confirmation_required {
                return Ok(true);
            }
            if !std::io::stdin().is_terminal() {
                return Ok(false);
            }
            eprint!("Trust this DASObjectStore appliance certificate? [y/N] ");
            std::io::stderr().flush()?;
            let mut response = String::new();
            std::io::stdin().read_line(&mut response)?;
            Ok(matches!(
                response.trim().to_ascii_lowercase().as_str(),
                "y" | "yes"
            ))
        },
    );
    let mut trust = match trust_result {
        Ok(trust) => trust,
        Err(error) => {
            if let (Ok(Some(existing)), Ok(presented)) = (
                crate::trust::load_trust(args.host_or_ip(), args.https_port()),
                crate::trust::probe_certificate(args.host_or_ip(), args.https_port()),
            ) {
                if existing.fingerprint_sha256 != presented.fingerprint_sha256 {
                    return Err(RemoteRunError::Config(RemoteConfigError::Integrity {
                        code: "certificate_binding_mismatch",
                        message: format!(
                            "{}\nOld SHA-256: {}\nNew SHA-256: {}\n{}",
                            crate::trust::format_certificate_details(
                                args.host_or_ip(),
                                args.https_port(),
                                &presented,
                                Some(&existing.appliance_id),
                            ),
                            existing.fingerprint_sha256,
                            presented.fingerprint_sha256,
                            error
                        ),
                        remediation: format!(
                            "independently verify with `dasobjectstore trust identity --json` on the appliance, then run `dasobjectstore-remote trust repair {} --username {} --store {}{}`",
                            args.host_or_ip(),
                            username,
                            args.object_store(),
                            if args.set_s3_config() { " --set-s3-config" } else { "" }
                        ),
                    }));
                }
            }
            return Err(error.into());
        }
    };
    if trust.newly_enrolled {
        eprintln!(
            "Enrolled appliance identity: {} ({})",
            trust.appliance_id.as_deref().unwrap_or("<endpoint-bound>"),
            if trust.legacy_fingerprint_pinned {
                "legacy fingerprint-pinned TLS name"
            } else {
                "certificate address verified"
            }
        );
    }
    let password = rpassword::prompt_password("DASObjectStore password: ")?;
    let context = authenticate(
        args.host_or_ip(),
        args.https_port(),
        Some(&trust.certificate_pem),
        trust.tls_server_name.as_deref(),
        &username,
        &password,
        args.object_store(),
        args.session_lifetime_seconds(),
    )?;
    if trust
        .appliance_id
        .as_deref()
        .is_some_and(|appliance_id| appliance_id != context.appliance_id)
    {
        if !allow_confirmed_identity_replacement {
            let record = crate::trust::load_trust(args.host_or_ip(), args.https_port())?
                .ok_or_else(|| {
                    RemoteRunError::UploadRouting(
                        "certificate_binding_mismatch: enrolled trust disappeared".to_string(),
                    )
                })?;
            return Err(RemoteRunError::Config(RemoteConfigError::Integrity {
                code: "certificate_binding_mismatch",
                message: format!(
                    "authenticated appliance identity {} does not match enrolled identity {}",
                    context.appliance_id, record.appliance_id
                ),
                remediation: format!(
                    "verify locally with `dasobjectstore trust identity --json`, then run `dasobjectstore-remote trust repair {} --username {} --store {}{}`",
                    args.host_or_ip(),
                    username,
                    args.object_store(),
                    if args.set_s3_config() { " --set-s3-config" } else { "" }
                ),
            }));
        }
        let existing =
            crate::trust::load_trust(args.host_or_ip(), args.https_port())?.ok_or_else(|| {
                RemoteRunError::UploadRouting(
                    "certificate_binding_mismatch: enrolled trust disappeared".to_string(),
                )
            })?;
        let mut replacement = existing.clone();
        replacement.appliance_id = context.appliance_id.clone();
        let path = crate::trust::trust_record_path(args.host_or_ip(), args.https_port())?;
        crate::trust::replace_verified_identity_trust_if_current(&path, &existing, &replacement)?;
        trust.appliance_id = Some(context.appliance_id.clone());
    }
    let mut prior_trust_record = crate::trust::load_trust(args.host_or_ip(), args.https_port())?;
    if prior_trust_record.is_none() {
        if let Some(mut enrollment) = trust.pending_replacement.take() {
            enrollment.appliance_id = context.appliance_id.clone();
            crate::trust::persist_trust(&enrollment)?;
            trust.appliance_id = Some(context.appliance_id.clone());
            prior_trust_record = Some(enrollment);
        }
    }
    let mut rotated_trust = false;
    if let (Some(existing), Some(replacement)) = (
        prior_trust_record.as_ref(),
        trust.pending_replacement.as_ref(),
    ) {
        let path = crate::trust::trust_record_path(args.host_or_ip(), args.https_port())?;
        crate::trust::replace_trust_if_current(&path, existing, replacement)?;
        rotated_trust = true;
        eprintln!(
            "Verified CA-backed certificate renewal for appliance {}\nOld SHA-256: {}\nNew SHA-256: {}",
            replacement.appliance_id,
            existing.fingerprint_sha256,
            replacement.fingerprint_sha256
        );
    }
    let profile_name = args
        .s3_profile()
        .map(str::to_string)
        .map(Ok)
        .unwrap_or_else(|| default_profile_name(&context.object_store))?;
    let path = config_path(cli)?;
    // Trigger a supported legacy migration before taking the transaction lock;
    // repair an identity-replacement ambiguity against enrolled endpoint trust,
    // then reload under the lock so concurrent authentication cannot publish
    // from a stale base generation. The same authenticate invocation continues
    // after repair and publishes the newly authenticated binding.
    if let Err(RemoteConfigError::Integrity { code, .. }) = read_optional_config(&path) {
        if matches!(
            code,
            "ambiguous_session_state" | "profile_association_mismatch"
        ) {
            repair_config(&path, true)?;
        } else {
            let _ = read_optional_config(&path)?;
        }
    }
    let _ = read_optional_config(&path)?;
    let transaction_lock = acquire_config_transaction(&path)?;
    let prior_config = read_optional_config(&path)?.unwrap_or_else(empty_config);
    let existing = prior_config
        .s3_profiles
        .iter()
        .find(|entry| entry.profile == profile_name)
        .cloned();
    let aws_backup = args
        .set_s3_config()
        .then(snapshot_profile_state)
        .transpose()?;
    let transaction = (|| -> Result<bool, RemoteRunError> {
        let (association, verified) = if args.set_s3_config() {
            let (association, verified) = install_profile(
                &context,
                &profile_name,
                existing.as_ref(),
                args.force(),
                args.verify_s3(),
            )?;
            (Some(association), verified)
        } else {
            (None, false)
        };
        let trust_record = crate::trust::load_trust(args.host_or_ip(), args.https_port())?
            .ok_or_else(|| {
                RemoteRunError::UploadRouting(
                    "certificate_binding_mismatch: enrolled appliance trust disappeared during authentication"
                        .to_string(),
                )
            })?;
        let candidate = authenticated_context_config(
            prior_config,
            &username,
            &context,
            args.https_port(),
            association,
            &trust_record.fingerprint_sha256,
            &trust_record.spki_sha256,
        )?;
        let (control, _) =
            RemoteControlClient::for_store(&candidate, &context.object_store, false)?;
        let readiness = control.readiness(&context.object_store)?;
        if readiness
            .get("catalogue_ready")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            return Err(RemoteRunError::UploadRouting(
                "new HTTPS control session failed readiness verification; the previous generation remains authoritative"
                    .to_string(),
            ));
        }
        write_config_locked(&path, &candidate, &transaction_lock)?;
        Ok(verified)
    })();
    let verified = match transaction {
        Ok(verified) => verified,
        Err(error) => {
            if let Some(backup) = &aws_backup {
                restore_profile_state(backup)?;
            }
            if rotated_trust {
                if let (Some(previous), Some(current)) = (
                    prior_trust_record.as_ref(),
                    trust.pending_replacement.as_ref(),
                ) {
                    let path =
                        crate::trust::trust_record_path(args.host_or_ip(), args.https_port())?;
                    crate::trust::replace_trust_if_current(&path, current, previous)?;
                }
            }
            restore_initial_trust(args.host_or_ip(), args.https_port(), initial_trust.as_ref())?;
            return Err(error);
        }
    };
    let safe = serde_json::json!({
        "authenticated": true,
        "server": context.appliance_host,
        "store_id": context.object_store,
        "s3": {
            "configured": args.set_s3_config(),
            "profile": if args.set_s3_config() { Some(profile_name.as_str()) } else { None },
            "endpoint_url": context.endpoint_url,
            "bucket": context.bucket,
            "region": context.region,
            "addressing_style": context.addressing_style,
            "temporary_credentials": context.session_token.is_some(),
            "expires_at": context.expires_at_utc,
            "verified": verified
        }
    });
    serde_json::to_writer_pretty(&mut *writer, &safe)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn legacy_password_transport_is_retired() -> bool {
    true
}

fn restore_initial_trust(
    host: &str,
    port: u16,
    initial: Option<&crate::trust::ApplianceTrustRecord>,
) -> Result<(), RemoteRunError> {
    let current = crate::trust::load_trust(host, port)?;
    match (initial, current.as_ref()) {
        (Some(initial), Some(current)) if initial != current => {
            let path = crate::trust::trust_record_path(host, port)?;
            crate::trust::replace_verified_identity_trust_if_current(&path, current, initial)?;
        }
        (None, Some(current)) => {
            let path = crate::trust::trust_record_path(host, port)?;
            crate::trust::remove_trust_if_current(&path, current)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn authenticated_context_config(
    mut config: RemoteConfig,
    username: &str,
    context: &RemoteConnectionContext,
    https_port: u16,
    association: Option<AwsProfileAssociation>,
    trust_fingerprint_sha256: &str,
    trust_spki_sha256: &str,
) -> Result<RemoteConfig, RemoteRunError> {
    config.schema_version = REMOTE_CONFIG_SCHEMA_VERSION.to_string();
    config.endpoint_url = context.endpoint_url.clone();
    config.region = context.region.clone();
    config.auth_authority = RemoteAuthAuthority::LocalPassword;
    config.username = Some(username.to_string());
    let profile = association
        .as_ref()
        .map(|association| association.profile.clone());
    if let Some(association) = association {
        config
            .s3_profiles
            .retain(|entry| entry.store_id != context.object_store);
        config.s3_profiles.push(association);
    } else {
        config
            .s3_profiles
            .retain(|entry| entry.store_id != context.object_store);
    }
    let appliance_id = context.appliance_id.clone();
    let control_base_url = format!("https://{}:{}", context.appliance_host, https_port);
    config.default_appliance_id = Some(appliance_id.clone());
    let session = RemoteUploadSession {
        session_id: context.session_id.clone(),
        issued_at: context.issued_at_utc.clone(),
        expires_at: context.expires_at_utc.clone(),
        credentials: RemoteSessionCredentials {
            access_key_id: context.access_key_id.clone(),
            secret_access_key: context.secret_access_key.clone(),
            session_token: context.session_token.clone(),
        },
        renewal: Some(RemoteSessionRenewalMetadata {
            renew_url: context.renew_url.clone(),
            renew_after: context.renew_after_utc.clone(),
            renewal_token: Some(context.renewal_token.clone()),
            last_renewed_at: None,
        }),
    };
    let grant = RemoteObjectStoreGrant {
        object_store: context.object_store.clone(),
        bucket: context.bucket.clone(),
        can_read: true,
        can_write: true,
        writer_group: None,
        object_type: "store_scoped_session".to_string(),
    };
    // Host aliases, historical endpoint spellings, and replaced appliance IDs
    // cannot create a second logical session. ObjectStore identity is the sole
    // replacement key; authenticated appliance identity becomes its new owner.
    config
        .session_bindings
        .retain(|binding| binding.store_id != context.object_store);
    config.session_bindings.push(RemoteSessionBinding {
        appliance_id: appliance_id.clone(),
        store_id: context.object_store.clone(),
        control_base_url: control_base_url.clone(),
        s3_endpoint_url: context.endpoint_url.clone(),
        bucket: context.bucket.clone(),
        region: context.region.clone(),
        addressing_style: context.addressing_style.clone(),
        s3_profile: profile,
        trust_fingerprint_sha256: trust_fingerprint_sha256.to_string(),
        trust_spki_sha256: trust_spki_sha256.to_string(),
        session: session.clone(),
    });
    if let Some(existing) = config
        .paired_appliances
        .iter_mut()
        .find(|appliance| appliance.appliance_id == appliance_id)
    {
        existing.appliance_base_url = format!("https://{}:{}", context.appliance_host, https_port);
        existing.discovery_url = format!(
            "{}/products/dasobjectstore/api/v1/remote/easyconnect/discovery",
            existing.appliance_base_url
        );
        existing.auth_authority = RemoteAuthAuthority::LocalPassword;
        existing.paired_actor = Some(username.to_string());
        existing.default_object_store = Some(context.object_store.clone());
        existing.session = None;
        existing
            .object_stores
            .retain(|entry| entry.object_store != context.object_store);
        existing.object_stores.push(grant);
    } else {
        let appliance_base_url = format!("https://{}:{}", context.appliance_host, https_port);
        config.paired_appliances.push(RemotePairedAppliance {
            appliance_id,
            display_name: format!("DASObjectStore {}", context.appliance_host),
            discovery_url: format!(
                "{appliance_base_url}/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
            ),
            appliance_base_url,
            auth_authority: RemoteAuthAuthority::LocalPassword,
            paired_actor: Some(username.to_string()),
            default_object_store: Some(context.object_store.clone()),
            session: None,
            object_stores: vec![grant],
        });
    }
    config.validate_session_integrity()?;
    Ok(config)
}

pub(super) fn run_s3_status(
    cli: &RemoteCli,
    store: &str,
    requested_profile: Option<&str>,
    _json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let config = read_optional_config(&config_path(cli)?)?.unwrap_or_else(empty_config);
    let binding = config.session_binding(store)?;
    let associations = config
        .s3_profiles
        .iter()
        .filter(|entry| {
            entry.store_id == store
                && requested_profile.is_none_or(|profile| profile == entry.profile)
        })
        .collect::<Vec<_>>();
    let association = match associations.as_slice() {
        [association] => *association,
        [] => {
            return Err(RemoteRunError::UploadRouting(format!(
                "no DASObjectStore-managed AWS profile is associated with ObjectStore {store}; authenticate with --set-s3-config"
            )))
        }
        _ => {
            return Err(RemoteRunError::UploadRouting(
                "profile_association_mismatch: multiple AWS profiles match the requested ObjectStore"
                    .to_string(),
            ))
        }
    };
    if binding.s3_profile.as_deref() != Some(association.profile.as_str())
        || binding.s3_endpoint_url != association.endpoint_url
        || binding.bucket != association.bucket
        || binding.session.expires_at != association.expires_at.clone().unwrap_or_default()
    {
        return Err(RemoteRunError::UploadRouting(
            "s3_control_generation_mismatch: AWS and HTTPS control state are not from the same committed generation; run `dasobjectstore-remote config repair --dry-run --json`"
                .to_string(),
        ));
    }
    serde_json::to_writer_pretty(&mut *writer, &s3_profile_status(association, true)?)?;
    writer.write_all(b"\n")?;
    Ok(())
}
