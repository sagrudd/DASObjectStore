//! Ingest queue inspection and daemon-owned drain handlers.

use super::*;

pub(super) fn run_ingest_queue(
    args: &IngestQueueArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let live_sqlite_path = super::metadata_paths::resolve_live_sqlite_path(args.live_sqlite_path());
    let snapshot = read_ingest_queue_for_store(&live_sqlite_path, args.store_id())?;
    let durable_destage =
        dasobjectstore_metadata::list_destage_queue(&live_sqlite_path, Some(args.store_id()))
            .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let durable_destage_diagnostics =
        dasobjectstore_metadata::destage_queue_diagnostics(&live_sqlite_path)
            .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &snapshot)?;
        writer.write_all(b"\n")?;
    } else {
        write_ingest_queue_summary(&snapshot, writer)?;
        writeln!(
            writer,
            "Durable HDD destage: {} object(s)",
            durable_destage.len()
        )?;
        writeln!(
            writer,
            "Queued bytes: {}; active bytes: {}; failed/review: {}",
            durable_destage_diagnostics.queued_bytes,
            durable_destage_diagnostics.active_bytes,
            durable_destage_diagnostics.failed_object_count
        )?;
        for record in durable_destage {
            writeln!(
                writer,
                "- {} state={:?} copies={}/{} attempts={}/{} retry={} error={}",
                record.object_id,
                record.state,
                record.verified_copy_count,
                record.required_copy_count,
                record.attempt_count,
                record.max_attempts,
                record.next_retry_at_utc.as_deref().unwrap_or("none"),
                record.last_error.as_deref().unwrap_or("none")
            )?;
        }
    }

    Ok(())
}

pub(super) fn run_ingest_drain_queue(
    args: &IngestDrainQueueArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    super::store_write::require_admin_for_destructive_store_action(args.dry_run())?;
    if !args.dry_run() {
        RiskGate::new(RiskPolicy {
            allow_ingest_queue_drain: args.allow_ingest_queue_drain(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::IngestQueueDrain,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }

    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.ingest_queue_drain(DaemonIngestQueueDrainRequest {
        store_id: args.store_id().to_string(),
        reason: args.reason().to_string(),
        dry_run: args.dry_run(),
        allow_ingest_queue_drain: args.allow_ingest_queue_drain(),
        confirmation_marker: args.confirm().to_string(),
    })?;
    let report = response.report;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_ingest_queue_drain_report(&report, writer)?;
    }

    Ok(())
}

fn write_ingest_queue_summary(
    snapshot: &IngestQueueSnapshot,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Ingest queue")?;
    writeln!(
        writer,
        "Live metadata: {}",
        snapshot.live_sqlite_path.display()
    )?;
    writeln!(writer, "Jobs: {}", snapshot.jobs.len())?;
    for job in &snapshot.jobs {
        writeln!(
            writer,
            "- {} store={} state={} object_type={} received={} expected={}",
            job.ingest_job_id,
            job.store_id,
            job.state,
            job.object_type,
            job.received_bytes,
            job.expected_size_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )?;
    }
    Ok(())
}

fn write_ingest_queue_drain_report(
    report: &IngestQueueDrainReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    let action = if report.dry_run {
        "would cancel"
    } else {
        "cancelled"
    };
    writeln!(writer, "Ingest queue drain")?;
    writeln!(writer, "Store: {}", report.store_id)?;
    writeln!(
        writer,
        "Live metadata: {}",
        report.live_sqlite_path.display()
    )?;
    writeln!(writer, "Jobs {action}: {}", report.jobs_cancelled)?;
    for job_id in &report.cancelled_job_ids {
        writeln!(writer, "- {job_id}")?;
    }
    Ok(())
}
