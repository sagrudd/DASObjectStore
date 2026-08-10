use super::*;

pub(super) fn run_easyconnect(
    cli: &RemoteCli,
    args: &EasyconnectArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
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
            requested_object_store: args.object_store().map(str::to_string),
            callback_port: args.callback_port(),
            timeout: Duration::from_secs(args.timeout_seconds()),
            open_browser: !args.no_browser(),
        };
        let open_browser = !args.no_browser();
        let outcome = run_complete_easyconnect_pairing_with_ready(
            options,
            &SystemBrowserLauncher,
            |contract, pairing| {
                write_easyconnect_pairing_ready(contract, pairing, open_browser, writer)?;
                writer.flush()?;
                Ok(())
            },
        )?;
        install_easyconnect_result(cli, &outcome)?;
        if args.set_s3_config() {
            install_easyconnect_s3_profile(cli, args, &outcome)?;
        }
        write_easyconnect_pairing(&outcome, writer)?;
        if args.set_s3_config() {
            let profile = args
                .s3_profile()
                .map(str::to_string)
                .map(Ok)
                .unwrap_or_else(|| {
                    crate::aws_profile::default_profile_name(
                        args.object_store()
                            .expect("--set-s3-config requires ObjectStore"),
                    )
                })?;
            writeln!(writer, "AWS profile: {profile} (installed and verified)")?;
        }
    }
    Ok(())
}

pub(super) fn write_easyconnect_pairing_ready(
    contract: &RemoteEasyconnectContract,
    pairing: &dasobjectstore_daemon::RemoteEasyconnectCreatePairingResponse,
    open_browser: bool,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    writeln!(writer, "Remote easyconnect pairing")?;
    writeln!(writer, "Appliance: {}", contract.appliance_base_url)?;
    writeln!(
        writer,
        "ObjectStore verification code: {}",
        pairing.user_code
    )?;
    writeln!(
        writer,
        "Approval URL: {}",
        pairing.verification_uri_complete
    )?;
    writeln!(
        writer,
        "Local callback bind: {}",
        contract.local_callback_bind
    )?;
    if open_browser {
        writeln!(writer, "Browser launch: requested")?;
    } else {
        writeln!(
            writer,
            "Browser launch: disabled; open the approval URL above"
        )?;
    }
    writeln!(writer, "Waiting for browser-approved pairing callback...")?;
    Ok(())
}

pub(super) fn write_easyconnect_pairing(
    outcome: &crate::easyconnect::RemoteEasyconnectCompletedPairing,
    writer: &mut impl Write,
) -> Result<(), std::io::Error> {
    writeln!(writer, "Pairing result: received")?;
    writeln!(writer, "Pairing ID: {}", outcome.pairing.pairing_id)?;
    writeln!(writer, "Exchange code: <redacted>")?;
    writeln!(
        writer,
        "Approved principal: {}",
        outcome.exchange.exchange.approved_actor
    )?;
    writeln!(
        writer,
        "Session expires: {}",
        outcome.exchange.exchange.session.expires_at_utc
    )?;
    writeln!(
        writer,
        "Status: passwordless session and server-owned S3 connection descriptor committed."
    )?;
    Ok(())
}

fn install_easyconnect_result(
    cli: &RemoteCli,
    outcome: &crate::easyconnect::RemoteEasyconnectCompletedPairing,
) -> Result<(), RemoteRunError> {
    use dasobjectstore_daemon::RemoteEasyconnectAuthProvider;

    let path = config_path(cli)?;
    let mut config = read_optional_config(&path)?.unwrap_or_else(empty_config);
    let exchange = &outcome.exchange.exchange;
    let auth_authority = match exchange.auth_provider {
        RemoteEasyconnectAuthProvider::StandaloneLocalUser => RemoteAuthAuthority::LocalPassword,
        RemoteEasyconnectAuthProvider::Pistis => RemoteAuthAuthority::Pistis,
        RemoteEasyconnectAuthProvider::Synoptikon => RemoteAuthAuthority::Synoptikon,
        RemoteEasyconnectAuthProvider::Mneion => RemoteAuthAuthority::Mneion,
    };
    let session = RemoteUploadSession {
        session_id: exchange.session.session_id.clone(),
        issued_at: exchange.session.issued_at_utc.clone(),
        expires_at: exchange.session.expires_at_utc.clone(),
        credentials: RemoteSessionCredentials {
            access_key_id: exchange.session.credentials.access_key_id.clone(),
            secret_access_key: exchange.session.credentials.secret_access_key.clone(),
            session_token: exchange.session.credentials.session_token.clone(),
        },
        renewal: Some(RemoteSessionRenewalMetadata {
            renew_url: format!(
                "{}/products/dasobjectstore{}",
                outcome.contract.appliance_base_url, exchange.session.renewal.renew_url
            ),
            renew_after: exchange.session.renewal.renew_after_utc.clone(),
            renewal_token: Some(exchange.session.renewal.renewal_token.clone()),
            last_renewed_at: None,
        }),
    };
    let grants = exchange
        .object_stores
        .iter()
        .map(|grant| RemoteObjectStoreGrant {
            object_store: grant.object_store.clone(),
            bucket: grant.bucket.clone(),
            can_read: grant.can_read,
            can_write: grant.can_write,
            writer_group: grant.writer_group.clone(),
            object_type: grant.object_type.clone(),
        })
        .collect::<Vec<_>>();
    let default_object_store = if grants.len() == 1 {
        Some(grants[0].object_store.clone())
    } else {
        None
    };
    config
        .paired_appliances
        .retain(|appliance| appliance.appliance_id != exchange.appliance_id);
    config.paired_appliances.push(RemotePairedAppliance {
        appliance_id: exchange.appliance_id.clone(),
        display_name: outcome.discovery.display_name.clone(),
        appliance_base_url: outcome.contract.appliance_base_url.clone(),
        discovery_url: outcome.contract.discovery_url.clone(),
        auth_authority,
        paired_actor: Some(exchange.approved_actor.clone()),
        default_object_store: default_object_store.clone(),
        session: Some(session.clone()),
        object_stores: grants.clone(),
    });
    let trust = crate::trust::load_trust(&outcome.contract.host_or_ip, outcome.https_port)?
        .ok_or_else(|| {
            RemoteRunError::UploadRouting(
                "easyconnect appliance trust disappeared before config commit".to_string(),
            )
        })?;
    config.session_bindings.retain(|binding| {
        !grants
            .iter()
            .any(|grant| grant.object_store == binding.store_id)
    });
    for grant in &grants {
        config.session_bindings.push(RemoteSessionBinding {
            appliance_id: exchange.appliance_id.clone(),
            store_id: grant.object_store.clone(),
            control_base_url: outcome.contract.appliance_base_url.clone(),
            s3_endpoint_url: outcome.exchange.s3.endpoint_url.clone(),
            bucket: grant.bucket.clone(),
            region: outcome.exchange.s3.region.clone(),
            addressing_style: outcome.exchange.s3.addressing_style.clone(),
            s3_profile: None,
            trust_fingerprint_sha256: trust.fingerprint_sha256.clone(),
            trust_spki_sha256: trust.spki_sha256.clone(),
            session: session.clone(),
        });
    }
    config.default_appliance_id = Some(exchange.appliance_id.clone());
    config.endpoint_url = outcome.exchange.s3.endpoint_url.clone();
    config.region = outcome.exchange.s3.region.clone();
    config.auth_authority = auth_authority;
    write_config(&path, &config)?;
    Ok(())
}

fn install_easyconnect_s3_profile(
    cli: &RemoteCli,
    args: &EasyconnectArgs,
    outcome: &crate::easyconnect::RemoteEasyconnectCompletedPairing,
) -> Result<(), RemoteRunError> {
    let store = args
        .object_store()
        .expect("clap requires ObjectStore with --set-s3-config");
    let grant = outcome
        .exchange
        .exchange
        .object_stores
        .iter()
        .find(|grant| grant.object_store == store)
        .ok_or_else(|| {
            RemoteRunError::UploadRouting(format!(
                "Pistis exchange did not grant requested ObjectStore {store}"
            ))
        })?;
    let exchange = &outcome.exchange.exchange;
    let profile = args
        .s3_profile()
        .map(str::to_string)
        .map(Ok)
        .unwrap_or_else(|| crate::aws_profile::default_profile_name(store))?;
    let context = crate::authenticate::RemoteConnectionContext {
        schema_version: outcome.exchange.schema_version.clone(),
        appliance_id: exchange.appliance_id.clone(),
        appliance_host: outcome.contract.host_or_ip.clone(),
        endpoint_url: outcome.exchange.s3.endpoint_url.clone(),
        region: outcome.exchange.s3.region.clone(),
        addressing_style: outcome.exchange.s3.addressing_style.clone(),
        object_store: store.to_string(),
        bucket: grant.bucket.clone(),
        access_key_id: exchange.session.credentials.access_key_id.clone(),
        secret_access_key: exchange.session.credentials.secret_access_key.clone(),
        session_token: exchange.session.credentials.session_token.clone(),
        session_id: exchange.session.session_id.clone(),
        issued_at_utc: exchange.session.issued_at_utc.clone(),
        expires_at_utc: exchange.session.expires_at_utc.clone(),
        renew_url: exchange.session.renewal.renew_url.clone(),
        renew_after_utc: exchange.session.renewal.renew_after_utc.clone(),
        renewal_token: exchange.session.renewal.renewal_token.clone(),
    };
    let path = config_path(cli)?;
    let before = read_optional_config(&path)?.unwrap_or_else(empty_config);
    let existing = before
        .s3_profiles
        .iter()
        .find(|association| association.profile == profile);
    let aws_before = crate::aws_profile::snapshot_profile_state()?;
    let (association, _) =
        crate::aws_profile::install_profile(&context, &profile, existing, true, true)?;
    let mut config = read_optional_config(&path)?.unwrap_or_else(empty_config);
    config
        .s3_profiles
        .retain(|item| item.profile != profile && item.store_id != store);
    config.s3_profiles.push(association);
    if let Some(binding) = config
        .session_bindings
        .iter_mut()
        .find(|binding| binding.appliance_id == exchange.appliance_id && binding.store_id == store)
    {
        binding.s3_profile = Some(profile.clone());
    }
    config.profile = profile;
    if let Err(error) = write_config(&path, &config) {
        let rollback = crate::aws_profile::restore_profile_state(&aws_before);
        return Err(RemoteRunError::UploadRouting(format!(
            "Pistis session succeeded but AWS profile association commit failed: {error}; AWS rollback_succeeded={}",
            rollback.is_ok()
        )));
    }
    Ok(())
}

pub(super) fn write_easyconnect_contract(
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
        "Status: run without --contract/--json to create, approve, exchange, and commit a passwordless session."
    )?;
    Ok(())
}

pub(super) fn run_config_set(
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

pub(super) fn run_config_show(
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

pub(super) fn run_config_doctor(
    cli: &RemoteCli,
    _json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let report = doctor_config(&config_path(cli)?)?;
    serde_json::to_writer_pretty(&mut *writer, &report)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn run_config_repair(
    cli: &RemoteCli,
    apply: bool,
    _json: bool,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let report = repair_config(&config_path(cli)?, apply)?;
    serde_json::to_writer_pretty(&mut *writer, &report)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn run_store_list(
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
