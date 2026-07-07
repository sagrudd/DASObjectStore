use super::{CliError, DiskHealthSummary, HealthReport, HostConnectionStatus};
use dasobjectstore_core::lifecycle::{HealthState, PoolState};
use dasobjectstore_metadata::{
    DestagePriorityPolicy, DirectHddImportReport, DiskDrainAction, DiskDrainObjectSummary,
    DiskDrainPlanSummary, DiskReplacementPlanSummary, DiskRetirementReport, ObjectExportReport,
    ObjectInspectSummary, ObjectPutReport, PoolInspectSummary, ReadOnlyAttachReport, SsdCapacity,
    SsdCapacityPolicy, SsdPressure,
};
use dasobjectstore_mnemosyne::{
    MneionDasObjectStoreEndpointLocation, ValidatedNasNfsEndpointDefinition,
};
use dasobjectstore_platform::{ObservedDisk, ObservedEnclosure, ProbeReport};
use std::io::{self, Write};

pub(super) fn write_ingest_status(
    capacity: &SsdCapacity,
    policy: &SsdCapacityPolicy,
    pressure: SsdPressure,
    destage_policy: &DestagePriorityPolicy,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(
        writer,
        "SSD ingest root: {}",
        capacity.path.to_string_lossy()
    )?;
    writeln!(writer, "Pressure: {pressure:?}")?;
    writeln!(
        writer,
        "Destage urgency: {:?}",
        destage_policy.urgency(pressure)
    )?;
    writeln!(
        writer,
        "Destage prioritized: {}",
        destage_policy.prioritizes_destage(pressure)
    )?;
    writeln!(writer, "Total bytes: {}", capacity.total_bytes)?;
    writeln!(writer, "Available bytes: {}", capacity.available_bytes)?;
    writeln!(writer, "Used bytes: {}", capacity.used_bytes())?;
    writeln!(writer, "Used percent: {}", capacity.used_percent_floor())?;
    writeln!(
        writer,
        "High watermark percent: {}",
        policy.high_watermark_percent
    )?;
    writeln!(
        writer,
        "Critical watermark percent: {}",
        policy.critical_watermark_percent
    )?;
    writeln!(writer, "Minimum free bytes: {}", policy.minimum_free_bytes)
}

pub(super) fn write_ingest_direct_import_report(
    report: &DirectHddImportReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Direct-to-HDD import complete")?;
    writeln!(writer, "Object: {}", report.object_id)?;
    writeln!(writer, "Disk: {}", report.disk_id)?;
    writeln!(writer, "Source: {}", report.source_path.to_string_lossy())?;
    if let Some(source_uri) = &report.source_uri {
        writeln!(writer, "Source URI: {source_uri}")?;
    }
    writeln!(
        writer,
        "Destination: {}",
        report.destination_path.to_string_lossy()
    )?;
    writeln!(writer, "Bytes written: {}", report.bytes_written)?;
    writeln!(
        writer,
        "Content hash: {}:{}",
        report.content_hash_algorithm, report.content_hash
    )?;
    writeln!(writer, "Warning: {}", report.warning)
}

pub(super) fn write_pool_inspect_summary(
    summary: &PoolInspectSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Pool: {}", summary.pool_id)?;
    writeln!(writer, "State: {:?}", summary.state)?;
    writeln!(writer, "Created: {}", summary.created_at_utc)?;
    writeln!(writer, "Updated: {}", summary.updated_at_utc)?;
    writeln!(writer, "Disks: {}", summary.disk_count)?;
    writeln!(
        writer,
        "Metadata path: {}",
        summary.metadata_path.to_string_lossy()
    )
}

pub(super) fn write_pool_import_report(
    report: &ReadOnlyAttachReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Pool: {}", report.pool_id)?;
    writeln!(writer, "Mode: read-only")?;
    writeln!(writer, "Disks: {}", report.recovered_disk_count)?;
    writeln!(
        writer,
        "Live metadata: {}",
        report.recovered_live_sqlite_path.to_string_lossy()
    )
}

pub(super) fn write_pool_repair_dry_run(
    summary: &PoolInspectSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Pool repair dry run")?;
    writeln!(writer, "Pool: {}", summary.pool_id)?;
    writeln!(writer, "State: {:?}", summary.state)?;
    writeln!(writer, "Disks: {}", summary.disk_count)?;
    writeln!(
        writer,
        "Metadata path: {}",
        summary.metadata_path.to_string_lossy()
    )?;
    writeln!(
        writer,
        "Planned action: {}",
        pool_repair_planned_action(summary.state)
    )
}

fn pool_repair_planned_action(state: PoolState) -> &'static str {
    match state {
        PoolState::Clean => "no repair required; use pool import --read-only for local attach",
        PoolState::Dirty => {
            "read-only recovery import; repair marker requires a future non-dry-run flow"
        }
        PoolState::ReadOnly => {
            "already read-only; inspect recovered metadata before further repair"
        }
        PoolState::Repairing => "repair already in progress; manual review required",
        PoolState::Degraded => "degraded pool; run disk health and drain planning before repair",
        PoolState::New => "new pool metadata; no portable repair action planned",
    }
}

pub(super) fn write_health_summary(
    report: &HealthReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Platform: {:?}", report.platform)?;
    writeln!(writer, "Disks: {}", report.disks.len())?;
    for state in ["Healthy", "Watch", "Suspect", "Failed"] {
        let count = report
            .disks
            .iter()
            .filter(|disk| health_state_name(disk) == state)
            .count();
        writeln!(writer, "{state}: {count}")?;
    }
    writeln!(writer, "Warnings: {}", health_warning_count(report))?;
    for disk in &report.disks {
        writeln!(
            writer,
            "- {} state={} score={} smart={} warnings={}",
            disk.device_path.as_deref().unwrap_or("<unknown>"),
            health_state_name(disk),
            disk.score.value,
            smart_status_name(disk.smart_passed),
            disk.warnings.len()
        )?;
    }

    Ok(())
}

pub(super) fn write_health_verbose(
    report: &HealthReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    write_health_summary(report, writer)?;
    for disk in &report.disks {
        writeln!(
            writer,
            "Disk {}",
            disk.device_path.as_deref().unwrap_or("<unknown>")
        )?;
        writeln!(
            writer,
            "  Model: {}",
            disk.model_hint.as_deref().unwrap_or("<unknown>")
        )?;
        writeln!(
            writer,
            "  Serial: {}",
            disk.serial_hint.as_deref().unwrap_or("<unknown>")
        )?;
        writeln!(
            writer,
            "  Size bytes: {}",
            disk.size_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        )?;
        writeln!(writer, "  Transport: {:?}", disk.transport)?;
        writeln!(writer, "  SMART: {}", smart_status_name(disk.smart_passed))?;
        writeln!(writer, "  Smart warnings: {}", disk.signals.smart_warnings)?;
        writeln!(writer, "  IO errors: {}", disk.signals.io_errors)?;
        writeln!(
            writer,
            "  Checksum failures: {}",
            disk.signals.checksum_failures
        )?;
        writeln!(writer, "  USB resets: {}", disk.signals.usb_resets)?;
        writeln!(
            writer,
            "  Temperature C: {}",
            disk.signals
                .temperature_celsius
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        )?;
        writeln!(
            writer,
            "  Benchmark drift percent: {}",
            disk.signals
                .benchmark_drift_percent
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        )?;
        for warning in &disk.warnings {
            writeln!(writer, "  Warning: {warning}")?;
        }
    }
    for warning in &report.warnings {
        writeln!(writer, "Report warning: {warning}")?;
    }

    Ok(())
}

pub(super) fn write_health_json(
    report: &HealthReport,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    serde_json::to_writer_pretty(
        &mut *writer,
        &serde_json::json!({
            "platform": format!("{:?}", report.platform),
            "disk_count": report.disks.len(),
            "warning_count": health_warning_count(report),
            "warnings": report.warnings.clone(),
            "disks": report.disks.iter().map(health_disk_json).collect::<Vec<_>>(),
        }),
    )?;
    writer.write_all(b"\n")?;

    Ok(())
}

pub(super) fn write_host_connection_status(
    report: &HostConnectionStatus,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Platform: {:?}", report.platform)?;
    writeln!(writer, "Disks: {}", report.disks.len())?;
    writeln!(
        writer,
        "Connection warnings: {}",
        connection_warning_count(report)
    )?;
    for disk in &report.disks {
        writeln!(
            writer,
            "- {} transport={:?} assessment={} direct_attached={} removable={} size_bytes={}",
            disk.device_path.as_deref().unwrap_or("<unknown>"),
            disk.transport,
            disk.assessment.as_str(),
            optional_bool(disk.direct_attached_hint),
            optional_bool(disk.removable_hint),
            disk.size_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        )?;
        writeln!(
            writer,
            "  Model: {}",
            disk.model_hint.as_deref().unwrap_or("<unknown>")
        )?;
        writeln!(
            writer,
            "  Enclosure topology: {}",
            disk.enclosure_topology_path
                .as_deref()
                .unwrap_or("<unknown>")
        )?;
        for warning in &disk.warnings {
            writeln!(writer, "  Warning: {warning}")?;
        }
        if let Some(recommendation) = &disk.recommendation {
            writeln!(writer, "  Recommendation: {recommendation}")?;
        }
    }
    for warning in &report.warnings {
        writeln!(writer, "Report warning: {warning}")?;
    }

    Ok(())
}

fn health_disk_json(disk: &DiskHealthSummary) -> serde_json::Value {
    serde_json::json!({
        "device_path": disk.device_path.clone(),
        "model_hint": disk.model_hint.clone(),
        "serial_hint": disk.serial_hint.clone(),
        "size_bytes": disk.size_bytes,
        "transport": format!("{:?}", disk.transport),
        "smart_passed": disk.smart_passed,
        "score": {
            "value": disk.score.value,
            "state": health_state_name(disk),
        },
        "signals": disk.signals,
        "warnings": disk.warnings.clone(),
    })
}

fn health_warning_count(report: &HealthReport) -> usize {
    report.warnings.len()
        + report
            .disks
            .iter()
            .map(|disk| disk.warnings.len())
            .sum::<usize>()
}

fn health_state_name(disk: &DiskHealthSummary) -> &'static str {
    match disk.score.state {
        HealthState::Healthy => "Healthy",
        HealthState::Watch => "Watch",
        HealthState::Suspect => "Suspect",
        HealthState::Draining => "Draining",
        HealthState::Retired => "Retired",
        HealthState::Failed => "Failed",
    }
}

fn smart_status_name(smart_passed: Option<bool>) -> &'static str {
    match smart_passed {
        Some(true) => "passed",
        Some(false) => "failing",
        None => "unknown",
    }
}

fn optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn connection_warning_count(report: &HostConnectionStatus) -> usize {
    report.warnings.len()
        + report
            .disks
            .iter()
            .map(|disk| disk.warnings.len())
            .sum::<usize>()
}

pub(super) fn write_disk_retirement_report(
    report: &DiskRetirementReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Disk retirement requested: {}", report.disk_id)?;
    writeln!(writer, "Previous state: {}", report.previous_state)?;
    writeln!(writer, "Next state: {:?}", report.next_state)?;
    writeln!(writer, "Updated: {}", report.updated_at_utc)?;
    writeln!(
        writer,
        "Live metadata: {}",
        report.live_sqlite_path.to_string_lossy()
    )
}

pub(super) fn write_disk_force_retirement_report(
    report: &DiskRetirementReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Disk force-retired: {}", report.disk_id)?;
    writeln!(writer, "Previous state: {}", report.previous_state)?;
    writeln!(writer, "Next state: {:?}", report.next_state)?;
    writeln!(writer, "Updated: {}", report.updated_at_utc)?;
    writeln!(
        writer,
        "Live metadata: {}",
        report.live_sqlite_path.to_string_lossy()
    )
}

pub(super) fn write_disk_drain_plan(
    plan: &DiskDrainPlanSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Disk drain plan: {}", plan.disk_id)?;
    writeln!(
        writer,
        "Protected copy tasks: {}",
        plan.protected_copy_tasks
    )?;
    writeln!(
        writer,
        "Protected blocked objects: {}",
        plan.protected_blocked_objects
    )?;
    writeln!(writer, "Cache copy tasks: {}", plan.cache_copy_tasks)?;
    writeln!(
        writer,
        "Cache redownload-required objects: {}",
        plan.cache_redownload_required_objects
    )?;
    write_disk_plan_objects(&plan.affected_objects, writer)?;
    writeln!(
        writer,
        "Live metadata: {}",
        plan.live_sqlite_path.to_string_lossy()
    )
}

pub(super) fn write_disk_replacement_plan(
    plan: &DiskReplacementPlanSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(
        writer,
        "Disk replacement plan: {} -> {}",
        plan.old_disk_id, plan.new_disk_id
    )?;
    writeln!(
        writer,
        "Protected copy tasks: {}",
        plan.protected_copy_tasks
    )?;
    writeln!(
        writer,
        "Protected blocked objects: {}",
        plan.protected_blocked_objects
    )?;
    writeln!(writer, "Cache copy tasks: {}", plan.cache_copy_tasks)?;
    writeln!(
        writer,
        "Cache redownload-required objects: {}",
        plan.cache_redownload_required_objects
    )?;
    write_disk_plan_objects(&plan.affected_objects, writer)?;
    writeln!(
        writer,
        "Live metadata: {}",
        plan.live_sqlite_path.to_string_lossy()
    )
}

fn write_disk_plan_objects(
    affected_objects: &[DiskDrainObjectSummary],
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Affected objects: {}", affected_objects.len())?;
    for object in affected_objects {
        let destinations = if object.destination_disk_ids.is_empty() {
            "<none>".to_string()
        } else {
            object
                .destination_disk_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        writeln!(
            writer,
            "- {} store={} action={} destinations={}",
            object.object_id,
            object.store_id,
            disk_drain_action_name(object.action),
            destinations
        )?;
    }

    Ok(())
}

fn disk_drain_action_name(action: DiskDrainAction) -> &'static str {
    match action {
        DiskDrainAction::CopyPlanned => "copy_planned",
        DiskDrainAction::Blocked => "blocked",
        DiskDrainAction::RedownloadRequired => "redownload_required",
    }
}

pub(super) fn write_object_inspect_summary(
    summary: &ObjectInspectSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Object: {}", summary.object_id)?;
    writeln!(writer, "Store: {}", summary.store_id)?;
    writeln!(writer, "Store class: {}", summary.store_class)?;
    writeln!(writer, "State: {}", summary.state)?;
    writeln!(
        writer,
        "Size bytes: {}",
        summary
            .size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    )?;
    writeln!(
        writer,
        "Content hash: {}",
        summary.content_hash.as_deref().unwrap_or("<unknown>")
    )?;
    writeln!(writer, "Placements: {}", summary.placements.len())?;
    for placement in &summary.placements {
        writeln!(
            writer,
            "- {} disk={} path={} verified_at={}",
            placement.placement_id,
            placement.disk_id,
            placement.relative_path,
            placement
                .verified_at_utc
                .as_deref()
                .unwrap_or("<unverified>")
        )?;
    }
    writeln!(
        writer,
        "Live metadata: {}",
        summary.live_sqlite_path.to_string_lossy()
    )
}

pub(super) fn write_object_export_report(
    report: &ObjectExportReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Object: {}", report.object_id)?;
    writeln!(writer, "Source disk: {}", report.source_disk_id)?;
    writeln!(writer, "Source: {}", report.source_path.to_string_lossy())?;
    writeln!(
        writer,
        "Destination: {}",
        report.destination_path.to_string_lossy()
    )?;
    writeln!(writer, "Bytes written: {}", report.bytes_written)?;
    writeln!(writer, "Content hash: {}", report.content_hash)
}

pub(super) fn write_object_put_report(
    report: &ObjectPutReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Object put complete")?;
    writeln!(writer, "Object: {}", report.object_id)?;
    writeln!(writer, "Source: {}", report.source_path.to_string_lossy())?;
    writeln!(
        writer,
        "Staged payload: {}",
        report.staged_payload_path.to_string_lossy()
    )?;
    writeln!(writer, "Bytes staged: {}", report.bytes_staged)?;
    writeln!(
        writer,
        "Content hash: {}:{}",
        report.content_hash_algorithm, report.content_hash
    )?;
    writeln!(writer, "Settled copies: {}", report.placements.len())?;
    for placement in &report.placements {
        writeln!(
            writer,
            "- copy {} disk={} bytes={} path={}",
            placement.copy_number,
            placement.disk_id,
            placement.bytes_written,
            placement.destination_path.to_string_lossy()
        )?;
    }

    Ok(())
}

pub(super) fn write_nas_nfs_endpoint_validation_report(
    validated: &ValidatedNasNfsEndpointDefinition,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "NAS/NFS endpoint definition is valid")?;
    writeln!(writer, "Endpoint: {}", validated.definition.identifier)?;
    writeln!(
        writer,
        "Display name: {}",
        validated.definition.display_name
    )?;
    writeln!(writer, "Status: {:?}", validated.definition.status)?;
    writeln!(
        writer,
        "Object service endpoint: {}",
        validated.definition.object_service_endpoint
    )?;
    writeln!(
        writer,
        "Mneion endpoint kind: {:?}",
        validated.mneion_endpoint.endpoint_kind
    )?;
    match &validated.mneion_endpoint.location {
        MneionDasObjectStoreEndpointLocation::Nfs {
            export_id,
            service_endpoint,
        } => {
            writeln!(writer, "Mneion NFS export ID: {export_id}")?;
            writeln!(writer, "Mneion service endpoint: {service_endpoint}")?;
        }
        MneionDasObjectStoreEndpointLocation::Das {
            pool_id,
            service_endpoint,
        } => {
            writeln!(writer, "Mneion DAS pool ID: {pool_id}")?;
            writeln!(writer, "Mneion service endpoint: {service_endpoint}")?;
        }
        MneionDasObjectStoreEndpointLocation::S3Compatible { endpoint, .. } => {
            writeln!(writer, "Mneion S3 endpoint: {endpoint}")?;
        }
    }
    writeln!(
        writer,
        "Tenant-facing contract: {:?}",
        validated.mneion_endpoint.object_contract
    )
}

pub(super) fn write_pretty_report(
    report: &ProbeReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Platform: {:?}", report.platform)?;
    writeln!(writer, "Disks: {}", report.disks.len())?;
    for disk in &report.disks {
        write_disk(disk, writer)?;
    }

    writeln!(writer, "Enclosures: {}", report.enclosures.len())?;
    for enclosure in &report.enclosures {
        write_enclosure(enclosure, writer)?;
    }

    if !report.warnings.is_empty() {
        writeln!(writer, "Warnings: {}", report.warnings.len())?;
        for warning in &report.warnings {
            writeln!(writer, "- {}: {}", warning.code, warning.message)?;
        }
    }

    Ok(())
}

fn write_disk(disk: &ObservedDisk, writer: &mut impl Write) -> Result<(), io::Error> {
    let device_path = disk.device_path.as_deref().unwrap_or("<unknown>");
    let size = disk
        .size_bytes
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown-size".to_string());
    let serial = disk.serial_hint.as_deref().unwrap_or("unknown-serial");

    writeln!(
        writer,
        "- {device_path} size={size} transport={:?} serial={serial}",
        disk.transport
    )
}

fn write_enclosure(
    enclosure: &ObservedEnclosure,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    let topology = enclosure
        .identity
        .usb_topology_path
        .as_deref()
        .unwrap_or("<unknown>");
    writeln!(
        writer,
        "- topology={topology} disks={}",
        enclosure.disk_device_paths.join(",")
    )
}
