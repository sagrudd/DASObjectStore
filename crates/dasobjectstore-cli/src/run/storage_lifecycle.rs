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
    let report = force_retire_disk(
        args.live_sqlite_path(),
        args.disk_id(),
        args.recorded_at_utc().to_string(),
        RiskPolicy {
            allow_force_retire: args.allow_force_retire(),
            ..RiskPolicy::default()
        },
        &ActionConfirmation::new(args.confirm()),
    )?;
    write_disk_force_retirement_report(&report, writer)?;
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
    let report = lockdown_das(&LockdownDasRequest {
        mount_root: args.mount_root().to_path_buf(),
        service_user: args.service_user().to_string(),
        service_group: args.service_group().to_string(),
        create_service_user: args.create_service_user(),
        dry_run: args.dry_run(),
    })?;
    write_lockdown_das_report(&report, writer)?;
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
    let request = PrepareDasRequest {
        devices: prepare_das_devices(args)?,
        mount_root: args.mount_root().to_path_buf(),
        filesystem: prepare_filesystem(args.filesystem()),
        owner: args.owner().map(ToOwned::to_owned),
        dry_run: args.dry_run(),
    };
    let report = prepare_das(&request)?;
    write_prepare_das_report(&report, writer)?;
    Ok(())
}

fn prepare_das_devices(args: &DiskPrepareDasArgs) -> Result<Vec<PrepareDasDevice>, CliError> {
    let mut devices = vec![PrepareDasDevice {
        role: PrepareDasRole::Ssd,
        device_path: args.ssd_device().to_path_buf(),
    }];
    for (index, value) in args.hdd_devices().iter().enumerate() {
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
        devices.push(PrepareDasDevice {
            role: PrepareDasRole::Hdd {
                disk_id,
                ordinal: index + 1,
            },
            device_path: Path::new(device_path).to_path_buf(),
        });
    }
    Ok(devices)
}

fn prepare_filesystem(filesystem: DiskPrepareFilesystem) -> PrepareFilesystem {
    match filesystem {
        DiskPrepareFilesystem::Ext4 => PrepareFilesystem::Ext4,
    }
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
