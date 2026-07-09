use crate::auth::{
    request_s3_credentials, RemoteAuthAuthority, RemoteAuthError, RemoteS3Credentials,
};
use crate::cli::{
    ConfigCommand, EasyconnectArgs, RemoteCli, RemoteCommand, StoreListArgs, StoresCommand,
    UploadArgs,
};
use crate::config::{
    default_config_path, read_optional_config, write_config, RemoteConfig, RemoteConfigError,
    RemoteConfigOverrides, DEFAULT_PROFILE, DEFAULT_REGION,
};
use crate::easyconnect::{
    define_easyconnect_contract, RemoteEasyconnectContract, RemoteEasyconnectContractError,
    RemoteEasyconnectContractRequest,
};
use crate::s3::{
    execute_aws_plan, parse_list_buckets, plan_list_stores, plan_upload, RemoteS3Error,
};
use std::fmt;
use std::io::Write;
use std::path::PathBuf;

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
    } else {
        write_easyconnect_contract(&contract, writer)?;
    }
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
        "Status: contract defined; pairing execution is not implemented in this build."
    )?;
    Ok(())
}

fn run_config_set(
    cli: &RemoteCli,
    args: &crate::cli::ConfigSetArgs,
    writer: &mut impl Write,
) -> Result<(), RemoteRunError> {
    let path = config_path(cli)?;
    let config = RemoteConfig {
        endpoint_url: args.endpoint_url().to_string(),
        region: args.region().to_string(),
        profile: args.profile().to_string(),
        auth_authority: args.auth(),
        username: args.username().map(ToOwned::to_owned),
        credential_helper: args.credential_helper().map(ToOwned::to_owned),
    };
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
        serde_json::to_writer_pretty(&mut *writer, &config)?;
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
    let credentials = resolve_credentials(cli, &config)?;
    let plan = plan_upload(
        &config,
        args.store(),
        args.source(),
        args.prefix(),
        args.key(),
        args.dry_run(),
        args.progress(),
    )?;
    if args.dry_run() {
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

fn resolved_valid_config(cli: &RemoteCli) -> Result<RemoteConfig, RemoteRunError> {
    let config = resolved_config(cli)?;
    config.validate_for_command()?;
    Ok(config)
}

fn resolved_config(cli: &RemoteCli) -> Result<RemoteConfig, RemoteRunError> {
    let path = config_path(cli)?;
    let base = read_optional_config(&path)?.unwrap_or_else(|| RemoteConfig {
        endpoint_url: String::new(),
        region: DEFAULT_REGION.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        auth_authority: RemoteAuthAuthority::AwsProfile,
        username: None,
        credential_helper: None,
    });
    Ok(base.merged_with(RemoteConfigOverrides {
        endpoint_url: cli.endpoint_url(),
        region: cli.region(),
        profile: cli.profile(),
        auth_authority: cli.auth(),
        username: cli.username(),
        credential_helper: cli.credential_helper(),
    }))
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
    Auth(RemoteAuthError),
    S3(RemoteS3Error),
}

impl fmt::Display for RemoteRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Easyconnect(error) => write!(formatter, "{error}"),
            Self::Auth(error) => write!(formatter, "{error}"),
            Self::S3(error) => write!(formatter, "{error}"),
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
