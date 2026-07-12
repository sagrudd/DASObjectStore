use super::*;

pub(super) fn run_ingest_files_with_client<T>(
    args: &IngestFilesArgs,
    client: &DaemonClient<T>,
    writer: &mut impl Write,
) -> Result<(), CliError>
where
    T: DaemonClientTransport,
{
    let request = build_daemon_ingest_files_request(args);
    if args.tui() {
        return run_ingest_submission_with_tui(
            client,
            request,
            writer,
            UploadTuiContext {
                endpoint: args.endpoint().as_str().to_string(),
                source_path: args.source().to_path_buf(),
                object_type: args.object_type().to_string(),
                conflict_policy: args.conflict_policy().to_string(),
                dry_run: args.dry_run(),
            },
        );
    }
    let started_at = Instant::now();
    let response = client.submit_ingest_files_with_progress_and_heartbeat(
        request,
        |event| {
            write_daemon_ingest_progress(writer, &event, started_at)
                .map_err(|err| DaemonClientError::Transport(err.to_string()))
        },
        || Ok(()),
    )?;
    write_daemon_ingest_submission(
        args.endpoint(),
        args.source(),
        args.object_type(),
        args.copies(),
        args.conflict_policy(),
        args.dry_run(),
        &response,
        writer,
    )?;

    Ok(())
}

fn run_ingest_submission_with_tui<T>(
    client: &DaemonClient<T>,
    request: SubmitIngestFilesRequest,
    writer: &mut impl Write,
    context: UploadTuiContext,
) -> Result<(), CliError>
where
    T: DaemonClientTransport,
{
    let interrupt_guard = UploadInterruptGuard::install();
    let tui = start_upload_tui(writer, context)?;
    let tui = RefCell::new(tui);
    let response = match client.submit_ingest_files_with_progress_and_heartbeat(
        request,
        |event| {
            interrupt_guard.check_cancelled()?;
            tui.borrow_mut()
                .render_progress(event)
                .map_err(|err| DaemonClientError::Transport(err.to_string()))?;
            Ok(())
        },
        || {
            interrupt_guard.check_cancelled()?;
            tui.borrow_mut()
                .render_heartbeat()
                .map_err(|err| DaemonClientError::Transport(err.to_string()))?;
            Ok(())
        },
    ) {
        Ok(response) => response,
        Err(err) => {
            if matches!(err, DaemonClientError::Cancelled(_)) {
                let _ = tui.into_inner().cancel(&err);
                return Ok(());
            }
            let _ = tui.into_inner().fail(&err);
            return Err(err.into());
        }
    };
    tui.into_inner().finish(&response)?;

    Ok(())
}

#[cfg(not(test))]
fn start_upload_tui<W: Write>(
    writer: &mut W,
    context: UploadTuiContext,
) -> io::Result<UploadTui<'_, W>> {
    UploadTui::start(writer, context)
}

#[cfg(test)]
fn start_upload_tui<W: Write>(
    writer: &mut W,
    context: UploadTuiContext,
) -> io::Result<UploadTui<'_, W>> {
    UploadTui::start_with_fixed_viewport(writer, context, ratatui::layout::Rect::new(0, 0, 100, 28))
}

fn build_daemon_ingest_files_request(args: &IngestFilesArgs) -> SubmitIngestFilesRequest {
    SubmitIngestFilesRequest {
        endpoint: args.endpoint().clone(),
        source_path: args.source().to_path_buf(),
        object_type: args.object_type(),
        copies: args.copies(),
        hdd_workers: args.hdd_workers(),
        // The daemon verifies the source mount and device topology before it
        // honours this local-server hint. It fails closed to SSD-first for
        // removable, network, FUSE, and unknown sources.
        ingress_origin: DaemonIngressOrigin::LocalServerSsdFirst,
        conflict_policy: args.conflict_policy(),
        dry_run: args.dry_run(),
        client_request_id: None,
    }
}

fn build_daemon_direct_import_request(args: &IngestDirectImportArgs) -> SubmitIngestFilesRequest {
    SubmitIngestFilesRequest {
        endpoint: args.endpoint().clone(),
        source_path: args.source().to_path_buf(),
        object_type: args.object_type(),
        copies: args.copies(),
        hdd_workers: args.hdd_workers(),
        ingress_origin: DaemonIngressOrigin::LocalServerDirectImport,
        conflict_policy: args.conflict_policy(),
        dry_run: args.dry_run(),
        client_request_id: None,
    }
}

fn write_daemon_ingest_submission(
    endpoint: &StoreId,
    source: &Path,
    object_type: dasobjectstore_core::object_type::ObjectType,
    copies: Option<u8>,
    conflict_policy: DaemonIngestConflictPolicy,
    dry_run: bool,
    response: &SubmitIngestFilesResponse,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Daemon ingest job submitted")?;
    writeln!(writer, "Endpoint: {endpoint}")?;
    writeln!(writer, "Source: {}", source.to_string_lossy())?;
    writeln!(writer, "Object type: {object_type}")?;
    if let Some(copies) = copies {
        writeln!(writer, "Copies override: {copies}")?;
    }
    writeln!(writer, "Conflict policy: {conflict_policy}")?;
    writeln!(writer, "Dry run: {dry_run}")?;
    writeln!(writer, "Job: {}", response.job_id)?;
    writeln!(writer, "Accepted at UTC: {}", response.accepted_at_utc)
}

pub(super) fn run_ingest_direct_import_with_client<T>(
    args: &IngestDirectImportArgs,
    client: &DaemonClient<T>,
    writer: &mut impl Write,
) -> Result<(), CliError>
where
    T: DaemonClientTransport,
{
    let request = build_daemon_direct_import_request(args);
    if args.tui() {
        return run_ingest_submission_with_tui(
            client,
            request,
            writer,
            UploadTuiContext {
                endpoint: args.endpoint().as_str().to_string(),
                source_path: args.source().to_path_buf(),
                object_type: args.object_type().to_string(),
                conflict_policy: args.conflict_policy().to_string(),
                dry_run: args.dry_run(),
            },
        );
    }

    let started_at = Instant::now();
    let response = client.submit_ingest_files_with_progress_and_heartbeat(
        request,
        |event| {
            write_daemon_ingest_progress(writer, &event, started_at)
                .map_err(|err| DaemonClientError::Transport(err.to_string()))
        },
        || Ok(()),
    )?;
    write_daemon_ingest_submission(
        args.endpoint(),
        args.source(),
        args.object_type(),
        args.copies(),
        args.conflict_policy(),
        args.dry_run(),
        &response,
        writer,
    )?;

    Ok(())
}
