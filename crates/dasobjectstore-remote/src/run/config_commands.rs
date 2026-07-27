use super::*;

pub(super) fn run_easyconnect(
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

pub(super) fn write_easyconnect_pairing_ready(
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

pub(super) fn write_easyconnect_pairing(
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
        "Status: contract defined; run without --contract/--json to launch browser pairing. Session exchange API is not implemented in this build."
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
