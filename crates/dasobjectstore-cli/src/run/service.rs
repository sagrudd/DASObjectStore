use super::CliError;
use crate::cli::{ServiceComposeArgs, ServiceProvisionArgs, ServiceStatusArgs};
use dasobjectstore_daemon::{
    DaemonClient, DaemonRuntimeConfig, DaemonServiceProvisionRequest, UnixSocketDaemonTransport,
};
use std::io::Write;
use std::path::Path;
use std::process::Command as ProcessCommand;

pub(super) fn run_service_up(
    args: &ServiceComposeArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    run_docker_compose(args, ["up", "-d"], writer)?;
    if args.dry_run() {
        return Ok(());
    }
    writeln!(writer, "Object service started")?;
    Ok(())
}

pub(super) fn run_service_provision(
    args: &ServiceProvisionArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path.clone()));
    let response = client.service_provision(DaemonServiceProvisionRequest {
        provider_id: args.provider(),
        dry_run: args.dry_run(),
        rotate_credentials: args.rotate_credentials(),
        client_request_id: None,
    })?;

    writeln!(writer, "Object service provisioning submitted")?;
    writeln!(writer, "Provider: {}", response.provider_id)?;
    writeln!(writer, "Registry: {}", response.registry_path)?;
    writeln!(
        writer,
        "Credential registry: {}",
        response.credential_registry_path
    )?;
    writeln!(writer, "Stores: {}", response.stores)?;
    writeln!(writer, "Buckets: {}", response.buckets)?;
    writeln!(writer, "Garage commands: {}", response.commands)?;
    writeln!(
        writer,
        "Credentials issued: {}",
        response.credentials_issued
    )?;
    writeln!(
        writer,
        "Credentials reused: {}",
        response.credentials_reused
    )?;
    writeln!(
        writer,
        "Credentials rotated: {}",
        response.credentials_rotated
    )?;
    writeln!(writer, "Dry run: {}", response.accepted.dry_run)?;
    writeln!(writer, "Job: {}", response.accepted.job_id)?;
    writeln!(
        writer,
        "Accepted at UTC: {}",
        response.accepted.accepted_at_utc
    )?;
    writeln!(writer, "Daemon socket: {}", config.socket_path.display())?;
    Ok(())
}

pub(super) fn run_service_down(
    args: &ServiceComposeArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    run_docker_compose(args, ["down"], writer)?;
    if args.dry_run() {
        return Ok(());
    }
    writeln!(writer, "Object service stopped")?;
    Ok(())
}

pub(super) fn run_service_status(
    args: &ServiceStatusArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.json() {
        return Err(CliError::UnsupportedServiceStatusFormat);
    }

    let command = docker_compose_args(
        args.compose_file(),
        args.project_directory(),
        ["ps", "--format", "json"],
    );
    if args.dry_run() {
        let mut dry_run_command = vec!["docker".to_string()];
        dry_run_command.extend(command);
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({"dry_run": true, "command": dry_run_command}),
        )?;
        writer.write_all(b"\n")?;
        return Ok(());
    }

    let output = ProcessCommand::new("docker").args(&command).output()?;
    if !output.status.success() {
        return Err(CliError::CommandFailed(format!(
            "docker {} exited with status {}",
            command.join(" "),
            output.status
        )));
    }
    writer.write_all(&output.stdout)?;
    if !output.stdout.ends_with(b"\n") {
        writer.write_all(b"\n")?;
    }
    Ok(())
}

fn run_docker_compose(
    args: &ServiceComposeArgs,
    action_args: impl IntoIterator<Item = &'static str>,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let command = docker_compose_args(args.compose_file(), args.project_directory(), action_args);
    if args.dry_run() {
        writeln!(writer, "docker {}", command.join(" "))?;
        return Ok(());
    }
    let status = ProcessCommand::new("docker").args(&command).status()?;
    if !status.success() {
        return Err(CliError::CommandFailed(format!(
            "docker {} exited with status {}",
            command.join(" "),
            status
        )));
    }
    Ok(())
}

fn docker_compose_args(
    compose_file: &Path,
    project_directory: Option<&Path>,
    action_args: impl IntoIterator<Item = &'static str>,
) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "-f".to_string(),
        compose_file.to_string_lossy().to_string(),
    ];
    if let Some(project_directory) = project_directory {
        args.push("--project-directory".to_string());
        args.push(project_directory.to_string_lossy().to_string());
    }
    args.extend(action_args.into_iter().map(String::from));
    args
}
