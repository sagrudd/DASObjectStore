fn execute_reconciliation_plan<P: ReconciliationProvider>(
    provider: &P,
    manifest: &mut ReconciliationManifest,
    manifest_path: &Path,
    staging_path: &Path,
    store_id: &StoreId,
    actions: &[ReconciliationAction],
    is_cancelled: &dyn Fn() -> bool,
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
) -> Result<(), DaemonServiceRuntimeError> {
    let total = actions.len();
    for (index, action) in actions.iter().enumerate() {
        if is_cancelled() {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "S3 reconciliation cancelled by administrator".to_string(),
            });
        }
        match action {
            ReconciliationAction::SkipComplete { .. } => {}
            ReconciliationAction::Download {
                key,
                relative_path,
                size_bytes,
            }
            | ReconciliationAction::Resume {
                key,
                relative_path,
                size_bytes,
                ..
            } => {
                let resume_offset = match action {
                    ReconciliationAction::Resume {
                        downloaded_bytes, ..
                    } => Some(*downloaded_bytes),
                    _ => None,
                };
                let declared_size = *size_bytes;
                if let (Some(offset), Some(size)) = (resume_offset, declared_size) {
                    if offset > size {
                        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: format!(
                                "reconciliation checkpoint offset {offset} exceeds declared size {size} for {key}"
                            ),
                        });
                    }
                } else if resume_offset.is_some() {
                    return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: format!(
                            "reconciliation resume requires a declared size for {key}"
                        ),
                    });
                }
                let size_bytes = declared_size.unwrap_or_default();
                manifest
                    .checkpoint(
                        manifest_path,
                        key,
                        ReconciliationEntryState::InProgress,
                        Some("provider download in progress".to_string()),
                        manifest
                            .entries
                            .get(key)
                            .map(|entry| entry.downloaded_bytes)
                            .unwrap_or_default(),
                    )
                    .map_err(reconciliation_manifest_error)?;
                emit_reconciliation_key_progress(
                    emit_progress,
                    store_id.clone(),
                    index,
                    total,
                    0,
                    size_bytes,
                    key,
                    "provider download started",
                )?;
                let destination = staging_path.join(relative_path);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        DaemonServiceRuntimeError::CommandIo {
                            program: "create reconciliation object directory".to_string(),
                            message: error.to_string(),
                        }
                    })?;
                }
                if let Some(offset) = resume_offset.filter(|offset| *offset > 0) {
                    if let Err(error) = validate_partial_offset(&destination, offset, key) {
                        manifest
                            .checkpoint(
                                manifest_path,
                                key,
                                ReconciliationEntryState::Failed,
                                Some(error.to_string()),
                                offset,
                            )
                            .map_err(reconciliation_manifest_error)?;
                        return Err(error);
                    }
                }
                let temporary_range_path = resume_offset.filter(|offset| *offset > 0).map(|_| {
                    destination.with_file_name(format!(
                        ".{}.resume-{}",
                        destination
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("object"),
                        reconciliation_temp_suffix()
                    ))
                });
                if let Err(error) = provider.download(ReconciliationDownloadRequest {
                    key,
                    destination: &destination,
                    resume_offset,
                    range_destination: temporary_range_path.as_deref(),
                    is_cancelled,
                }) {
                    if let Some(path) = &temporary_range_path {
                        let _ = fs::remove_file(path);
                    }
                    manifest
                        .checkpoint(
                            manifest_path,
                            key,
                            ReconciliationEntryState::Failed,
                            Some(error.to_string()),
                            resume_offset.unwrap_or_default(),
                        )
                        .map_err(reconciliation_manifest_error)?;
                    return Err(error);
                }
                if let Some(offset) = resume_offset.filter(|offset| *offset > 0) {
                    let partial = temporary_range_path.as_deref().ok_or_else(|| {
                        DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: format!("missing range staging path for {key}"),
                        }
                    })?;
                    if let Err(error) =
                        append_range_download(&destination, partial, offset, size_bytes)
                    {
                        let _ = fs::remove_file(partial);
                        manifest
                            .checkpoint(
                                manifest_path,
                                key,
                                ReconciliationEntryState::Failed,
                                Some(error.to_string()),
                                offset,
                            )
                            .map_err(reconciliation_manifest_error)?;
                        return Err(error);
                    }
                } else if let Some(size) = declared_size {
                    if let Err(error) = validate_downloaded_size(&destination, size, key) {
                        manifest
                            .checkpoint(
                                manifest_path,
                                key,
                                ReconciliationEntryState::Failed,
                                Some(error.to_string()),
                                resume_offset.unwrap_or_default(),
                            )
                            .map_err(reconciliation_manifest_error)?;
                        return Err(error);
                    }
                }
                manifest
                    .checkpoint(
                        manifest_path,
                        key,
                        ReconciliationEntryState::Complete,
                        None,
                        size_bytes,
                    )
                    .map_err(reconciliation_manifest_error)?;
                emit_reconciliation_key_progress(
                    emit_progress,
                    store_id.clone(),
                    index + 1,
                    total,
                    size_bytes,
                    size_bytes,
                    key,
                    "provider download complete",
                )?;
            }
            ReconciliationAction::InvalidKey { .. } | ReconciliationAction::Collision { .. } => {
                unreachable!("rejected before transfer")
            }
        }
    }
    Ok(())
}

/// Provider-neutral listing and transfer seam used by reconciliation. Garage
/// currently supplies the AWS CLI implementation; other providers can
/// implement the same listing, range/resume, and cancellation contract without
/// changing manifest or checkpoint logic.
pub(crate) struct ReconciliationListRequest<'a> {
    pub(crate) prefix: Option<&'a str>,
}

pub(crate) struct ReconciliationDownloadRequest<'a> {
    pub(crate) key: &'a str,
    pub(crate) destination: &'a Path,
    pub(crate) resume_offset: Option<u64>,
    pub(crate) range_destination: Option<&'a Path>,
    pub(crate) is_cancelled: &'a dyn Fn() -> bool,
}

pub(crate) trait ReconciliationProvider {
    fn list_objects(
        &self,
        request: ReconciliationListRequest<'_>,
    ) -> Result<Vec<ReconciliationObject>, DaemonServiceRuntimeError>;

    fn download(
        &self,
        request: ReconciliationDownloadRequest<'_>,
    ) -> Result<(), DaemonServiceRuntimeError>;
}

struct GarageReconciliationProvider<'a, R> {
    runner: &'a R,
    endpoint: &'a str,
    bucket_name: &'a str,
    environment: &'a [(String, String)],
}

impl<R: ServiceCommandRunner> ReconciliationProvider for GarageReconciliationProvider<'_, R> {
    fn list_objects(
        &self,
        request: ReconciliationListRequest<'_>,
    ) -> Result<Vec<ReconciliationObject>, DaemonServiceRuntimeError> {
        list_garage_objects(
            self.runner,
            self.endpoint,
            self.bucket_name,
            request.prefix,
            self.environment,
        )
    }

    fn download(
        &self,
        request: ReconciliationDownloadRequest<'_>,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let args = reconciliation_download_args(
            self.endpoint,
            self.bucket_name,
            request.key,
            request.destination,
            request.resume_offset,
            request.range_destination,
        );
        self.runner
            .run_with_display_args_and_env_cancellable(
                "aws",
                &args,
                &args,
                self.environment,
                request.is_cancelled,
            )
            .map(|_| ())
    }
}

fn reconciliation_download_args(
    endpoint: &str,
    bucket_name: &str,
    key: &str,
    destination: &Path,
    resume_offset: Option<u64>,
    range_destination: Option<&Path>,
) -> Vec<String> {
    match resume_offset.filter(|offset| *offset > 0) {
        Some(offset) => vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "s3api".to_string(),
            "get-object".to_string(),
            "--bucket".to_string(),
            bucket_name.to_string(),
            "--key".to_string(),
            key.to_string(),
            "--range".to_string(),
            format!("bytes={offset}-"),
            range_destination
                .expect("range destination is required for a non-zero resume")
                .display()
                .to_string(),
        ],
        _ => vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "s3".to_string(),
            "cp".to_string(),
            format!("s3://{bucket_name}/{key}"),
            destination.display().to_string(),
            "--no-progress".to_string(),
        ],
    }
}

fn reconciliation_temp_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn append_range_download(
    destination: &Path,
    partial: &Path,
    offset: u64,
    expected_size: u64,
) -> Result<(), DaemonServiceRuntimeError> {
    let destination_label = destination.display().to_string();
    validate_partial_offset(destination, offset, &destination_label)?;
    let partial_size = fs::metadata(partial)
        .map_err(|error| reconciliation_file_error(partial, error))?
        .len();
    let expected_suffix = expected_size.checked_sub(offset).ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation range offset exceeds size for {}",
                destination.display()
            ),
        }
    })?;
    if partial_size != expected_suffix {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation range size {partial_size} does not match expected suffix {expected_suffix} for {}",
                destination.display()
            ),
        });
    }
    let mut output = OpenOptions::new()
        .append(true)
        .open(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?;
    let mut input =
        fs::File::open(partial).map_err(|error| reconciliation_file_error(partial, error))?;
    io::copy(&mut input, &mut output)
        .map_err(|error| reconciliation_file_error(destination, error))?;
    output
        .sync_all()
        .map_err(|error| reconciliation_file_error(destination, error))?;
    let final_size = fs::metadata(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?
        .len();
    if final_size != expected_size {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation destination size {final_size} does not match expected size {expected_size} for {}",
                destination.display()
            ),
        });
    }
    fs::remove_file(partial).map_err(|error| reconciliation_file_error(partial, error))
}

fn validate_partial_offset(
    destination: &Path,
    offset: u64,
    key: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    let destination_size = fs::metadata(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?
        .len();
    if destination_size != offset {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation partial size {destination_size} does not match checkpoint offset {offset} for {key}"
            ),
        });
    }
    Ok(())
}

fn validate_downloaded_size(
    destination: &Path,
    expected_size: u64,
    key: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    let actual = fs::metadata(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?
        .len();
    if actual != expected_size {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation download size {actual} does not match expected size {expected_size} for {key}"
            ),
        });
    }
    Ok(())
}

fn reconciliation_file_error(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::CommandIo {
        program: "reconciliation file".to_string(),
        message: format!("{}: {error}", path.display()),
    }
}

pub(super) fn list_garage_objects<R: ServiceCommandRunner>(
    runner: &R,
    endpoint: &str,
    bucket_name: &str,
    prefix: Option<&str>,
    environment: &[(String, String)],
) -> Result<Vec<ReconciliationObject>, DaemonServiceRuntimeError> {
    let mut objects = Vec::new();
    let mut continuation_token: Option<String> = None;
    loop {
        let mut args = vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "s3api".to_string(),
            "list-objects-v2".to_string(),
            "--bucket".to_string(),
            bucket_name.to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];
        if let Some(prefix) = prefix.filter(|prefix| !prefix.trim().is_empty()) {
            args.extend(["--prefix".to_string(), prefix.trim_matches('/').to_string()]);
        }
        if let Some(token) = continuation_token.as_deref() {
            args.extend(["--continuation-token".to_string(), token.to_string()]);
        }
        let output = runner.run_with_display_args_and_env("aws", &args, &args, environment)?;
        let value: Value = serde_json::from_str(&output.stdout).map_err(|error| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("Garage object listing returned invalid JSON: {error}"),
            }
        })?;
        if let Some(contents) = value.get("Contents").and_then(Value::as_array) {
            for object in contents {
                let Some(key) = object.get("Key").and_then(Value::as_str) else {
                    return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: "Garage object listing contained an entry without Key"
                            .to_string(),
                    });
                };
                objects.push(ReconciliationObject {
                    key: key.to_string(),
                    size_bytes: object.get("Size").and_then(Value::as_u64),
                    source_revision: object
                        .get("ETag")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
            }
        }
        let truncated = value
            .get("IsTruncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !truncated {
            break;
        }
        continuation_token = value
            .get("NextContinuationToken")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if continuation_token.is_none() {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "Garage object listing was truncated without a continuation token"
                    .to_string(),
            });
        }
    }
    Ok(objects)
}

fn reconciliation_manifest_error(error: ReconciliationManifestError) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: error.to_string(),
    }
}

fn emit_reconciliation_key_progress(
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
    endpoint: StoreId,
    files_done: usize,
    files_total: usize,
    work_bytes_done: u64,
    work_bytes_total: u64,
    key: &str,
    message: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    use dasobjectstore_core::ids::IngestJobId;
    emit_progress(crate::api::DaemonIngestProgressEvent {
        job_id: IngestJobId::new("store-repair-s3-reconcile").expect("static job id"),
        endpoint,
        stage: crate::api::DaemonIngestStage::SsdIngest,
        pipeline_stage: Some(crate::api::DaemonIngestPipelineStage::SsdStage),
        work_bytes_done,
        work_bytes_total: Some(work_bytes_total),
        source_bytes_done: Some(work_bytes_done),
        source_bytes_total: Some(work_bytes_total),
        stage_bytes_done: Some(work_bytes_done),
        stage_bytes_total: Some(work_bytes_total),
        files_done: files_done as u64,
        files_total: Some(files_total as u64),
        current_object_id: None,
        ssd_pressure: None,
        telemetry: None,
        active_hdd_transfers: Vec::new(),
        resource_policy: None,
        message: Some(format!("{message}: {key}")),
    })
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("reconciliation progress delivery failed: {error}"),
    })
}
