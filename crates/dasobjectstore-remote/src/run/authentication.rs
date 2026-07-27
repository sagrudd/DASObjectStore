use super::*;

pub(super) fn run_authenticate(
    cli: &RemoteCli,
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
    let trust = prepare_appliance_trust(
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
    )?;
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
        return Err(RemoteRunError::Config(RemoteConfigError::Integrity {
            code: "certificate_binding_mismatch",
            message: "the authenticated appliance identity does not match enrolled TLS trust"
                .to_string(),
            remediation: format!(
                "dasobjectstore-remote trust inspect {} --https-port {} --json",
                args.host_or_ip(),
                args.https_port()
            ),
        }));
    }
    let profile_name = args
        .s3_profile()
        .map(str::to_string)
        .map(Ok)
        .unwrap_or_else(|| default_profile_name(&context.object_store))?;
    let path = config_path(cli)?;
    // Trigger a supported legacy migration before taking the transaction lock;
    // then reload under the lock so concurrent authentication cannot publish
    // from a stale base generation.
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
            .retain(|entry| entry.profile != association.profile);
        config.s3_profiles.push(association);
    }
    let appliance_id = context.appliance_id.clone();
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
    // Host aliases and historical endpoint spellings cannot create a second
    // logical session. The server-returned appliance/store identities are the
    // sole replacement key.
    config
        .session_bindings
        .retain(|binding| binding.store_id != context.object_store);
    config.session_bindings.push(RemoteSessionBinding {
        appliance_id: appliance_id.clone(),
        store_id: context.object_store.clone(),
        control_base_url: format!("https://{}:{}", context.appliance_host, https_port),
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
