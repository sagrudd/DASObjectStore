//! Pool and managed-disk lifecycle command handlers.

use super::*;

pub(super) fn run_pool_inspect(
    args: &PoolInspectArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let summary = inspect_pool_metadata(args.metadata_path())?;
    write_pool_inspect_summary(&summary, writer)?;
    Ok(())
}

pub(super) fn run_pool_import(
    args: &PoolImportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.read_only() {
        return Err(CliError::UnsupportedPoolImportMode);
    }
    let summary = inspect_pool_metadata(args.source_path())?;
    let options = ReadOnlyAttachOptions::new(
        args.source_path(),
        args.recovery_metadata_dir(),
        args.recorded_at_utc().to_string(),
    );
    let report = match summary.state {
        PoolState::Clean => attach_clean_pool_read_only(&options)?,
        PoolState::Dirty => import_dirty_pool_read_only(&options)?,
        state => return Err(CliError::UnsupportedPoolImportState { state }),
    };
    write_pool_import_report(&report, writer)?;
    Ok(())
}

pub(super) fn run_pool_repair(
    args: &PoolRepairArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.dry_run() {
        return Err(CliError::UnsupportedPoolRepairMode);
    }
    let summary = inspect_pool_metadata(args.source_path())?;
    write_pool_repair_dry_run(&summary, writer)?;
    Ok(())
}

pub(super) fn run_disk_retire(
    args: &DiskRetireArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.disk_retire(DaemonDiskRetireRequest {
        disk_id: args.disk_id().to_string(),
    })?;
    write_disk_retirement_report(&response.report, writer)?;
    Ok(())
}

pub(super) fn run_disk_force_retire(
    args: &DiskForceRetireArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.disk_force_retire(DaemonDiskForceRetireRequest {
        disk_id: args.disk_id().to_string(),
        allow_force_retire: args.allow_force_retire(),
        confirmation_marker: args.confirm().to_string(),
    })?;
    write_disk_force_retirement_report(&response.report, writer)?;
    Ok(())
}

pub(super) fn run_disk_lockdown_das(
    args: &DiskLockdownDasArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.dry_run() && args.confirm() != LOCKDOWN_CONFIRMATION {
        return Err(CliError::CommandFailed(format!(
            "action confirmation mismatch; pass `{LOCKDOWN_CONFIRMATION}`"
        )));
    }
    let config = DaemonRuntimeConfig::default_packaged();
    let response = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path))
        .disk_lockdown(DaemonDiskLockdownRequest {
            mount_root: args.mount_root().to_path_buf(),
            service_user: args.service_user().to_string(),
            service_group: args.service_group().to_string(),
            create_service_user: args.create_service_user(),
            dry_run: args.dry_run(),
            confirmation_marker: args.confirm().to_string(),
        })?;
    write_lockdown_das_report(&response, writer)?;
    Ok(())
}

pub(super) fn run_disk_prepare_das(
    args: &DiskPrepareDasArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.dry_run() {
        if !args.acknowledge_existing_data() {
            return Err(CliError::CommandFailed(
                "existing data acknowledgement is required; pass `--acknowledge-existing-data`"
                    .to_string(),
            ));
        }
        RiskGate::new(RiskPolicy {
            allow_prepare_das: args.allow_format(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::PrepareDas,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.prepare_enclosure(prepare_enclosure_request(args)?)?;
    write_prepare_das_report(&response, writer)?;
    Ok(())
}

fn prepare_enclosure_request(
    args: &DiskPrepareDasArgs,
) -> Result<DaemonPrepareEnclosureRequest, CliError> {
    let mut hdd_devices = Vec::with_capacity(args.hdd_devices().len());
    for value in args.hdd_devices() {
        let (disk_id, device_path) =
            value
                .split_once('=')
                .ok_or_else(|| CliError::InvalidDeviceMapping {
                    value: value.clone(),
                })?;
        let disk_id = DiskId::new(disk_id).map_err(|_| CliError::InvalidDeviceMapping {
            value: value.clone(),
        })?;
        if device_path.is_empty() {
            return Err(CliError::InvalidDeviceMapping {
                value: value.clone(),
            });
        }
        hdd_devices.push(DaemonPrepareEnclosureHddDevice {
            disk_id: disk_id.as_str().to_string(),
            device_path: Path::new(device_path).to_path_buf(),
        });
    }

    Ok(DaemonPrepareEnclosureRequest {
        ssd_device: args.ssd_device().to_path_buf(),
        hdd_devices,
        mount_root: args.mount_root().to_path_buf(),
        filesystem: match args.filesystem() {
            DiskPrepareFilesystem::Ext4 => DaemonPrepareEnclosureFilesystem::Ext4,
        },
        owner: args.owner().map(ToOwned::to_owned),
        dry_run: args.dry_run(),
        client_request_id: None,
        administrator_actor: None,
        allow_format: args.allow_format(),
        existing_data_acknowledged: args.acknowledge_existing_data(),
        confirmation_marker: args.confirm().to_string(),
    })
}

pub(super) fn run_disk_drain(
    args: &DiskDrainArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let plan = read_disk_drain_plan(args.live_sqlite_path(), args.disk_id())?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &plan)?;
        writer.write_all(b"\n")?;
    } else {
        write_disk_drain_plan(&plan, writer)?;
    }
    Ok(())
}

pub(super) fn run_disk_replace(
    args: &DiskReplaceArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let plan = read_disk_replacement_plan(
        args.live_sqlite_path(),
        args.old_disk_id(),
        args.new_disk_id(),
    )?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &plan)?;
        writer.write_all(b"\n")?;
    } else {
        write_disk_replacement_plan(&plan, writer)?;
    }
    Ok(())
}
