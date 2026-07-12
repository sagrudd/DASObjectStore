//! Transitional local-direct ingest fallback used by explicit developer flags.

use super::*;

#[derive(Clone, Debug)]
pub(super) struct FileIngestEntry {
    source_path: PathBuf,
    pub(super) relative_path: PathBuf,
    object_id: ObjectId,
    size_bytes: u64,
}

#[derive(Clone, Debug)]
struct ResolvedIngestEndpoint {
    endpoint_name: String,
    endpoint_kind: &'static str,
    store: StoreServiceDefinition,
    object_prefix: String,
}

pub(super) fn run_ingest_files_local_direct(
    args: &IngestFilesArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let ssd_root = args
        .ssd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&ssd_root)?;
    let hdd_root = args
        .hdd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_hdd_root);
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let subobject_registry_path = args
        .subobject_registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let endpoint =
        resolve_ingest_endpoint(args.endpoint(), &registry_path, &subobject_registry_path)?;
    authorize_store_write(&endpoint.store)?;
    let managed_disk_roots = discover_managed_hdd_roots(&hdd_root)?;
    let copies = args.copies().unwrap_or(endpoint.store.policy.copies);
    if copies == 0 || managed_disk_roots.len() < copies as usize {
        return Err(CliError::CommandFailed(format!(
            "ingest files requires at least {copies} managed HDD root(s), got {}",
            managed_disk_roots.len()
        )));
    }
    let files = collect_ingest_files(args.source(), &endpoint.object_prefix)?;
    let total_source_bytes = files.iter().map(|entry| entry.size_bytes).sum::<u64>();
    let total_work_bytes = total_source_bytes.saturating_mul(u64::from(copies) + 1);

    writeln!(writer, "File ingest plan")?;
    writeln!(writer, "Endpoint: {}", endpoint.endpoint_name)?;
    writeln!(writer, "Endpoint kind: {}", endpoint.endpoint_kind)?;
    writeln!(writer, "Store: {}", endpoint.store.store_id)?;
    writeln!(writer, "Object prefix: {}", endpoint.object_prefix)?;
    writeln!(writer, "Class: {}", endpoint.store.policy.class.name())?;
    writeln!(writer, "Source: {}", args.source().to_string_lossy())?;
    writeln!(writer, "Object type: {}", args.object_type())?;
    writeln!(writer, "SSD root: {}", ssd_root.to_string_lossy())?;
    writeln!(writer, "Managed HDD roots: {}", managed_disk_roots.len())?;
    writeln!(writer, "Files: {}", files.len())?;
    writeln!(writer, "Source bytes: {total_source_bytes}")?;
    writeln!(writer, "Copies: {copies}")?;
    writeln!(writer, "Conflict policy: {}", args.conflict_policy())?;
    writeln!(writer, "TUI: {}", args.tui())?;
    writeln!(writer, "Work bytes: {total_work_bytes}")?;

    if args.dry_run() {
        writeln!(writer, "Dry run: no files imported")?;
        for entry in &files {
            writeln!(
                writer,
                "- {} bytes={} object={}",
                entry.relative_path.to_string_lossy(),
                entry.size_bytes,
                entry.object_id
            )?;
        }
        return Ok(());
    }

    let mut completed_files = 0_usize;
    let mut completed_work_bytes = 0_u64;
    let started_at = Instant::now();
    let capacity_policy = SsdCapacityPolicy::default();

    for entry in &files {
        match read_ssd_stress(&ssd_root, &capacity_policy) {
            Ok(stress) => writeln!(writer, "SSD stress before file: {stress}")?,
            Err(err) => writeln!(writer, "SSD stress before file: unavailable ({err})")?,
        }
        writeln!(
            writer,
            "Importing {} as {}",
            entry.relative_path.to_string_lossy(),
            entry.object_id
        )?;

        let request = ObjectPutRequest::new(
            entry.object_id.clone(),
            &entry.source_path,
            &ssd_root,
            plan_disk_roots_for_entry(&managed_disk_roots, entry, &endpoint.store.policy, copies)?,
            copies,
        )
        .with_object_type(args.object_type());
        let mut stage_key = String::new();
        let mut stage_offset_bytes = 0_u64;
        let mut last_emit = Instant::now();
        let mut progress_write_error = None;
        let report = put_object_ssd_first_with_progress(&request, |progress| {
            let key = progress_stage_key(&progress);
            if key != stage_key {
                stage_key = key;
                stage_offset_bytes = 0;
                last_emit = Instant::now();
            }
            let delta = progress.bytes_written.saturating_sub(stage_offset_bytes);
            stage_offset_bytes = progress.bytes_written;
            completed_work_bytes = completed_work_bytes.saturating_add(delta);
            if last_emit.elapsed().as_secs() == 0 && progress.bytes_written < entry.size_bytes {
                return;
            }
            last_emit = Instant::now();
            if progress_write_error.is_none() {
                progress_write_error = write_file_ingest_progress(
                    writer,
                    completed_work_bytes,
                    total_work_bytes,
                    completed_files,
                    files.len(),
                    &progress,
                    started_at,
                    &ssd_root,
                    &capacity_policy,
                )
                .err();
            }
        })?;
        if let Some(err) = progress_write_error {
            return Err(CliError::Io(err));
        }

        completed_files += 1;
        writeln!(
            writer,
            "File complete: {} bytes={} hash={}:{} copies={}",
            entry.relative_path.to_string_lossy(),
            report.bytes_staged,
            report.content_hash_algorithm,
            report.content_hash,
            report.placements.len()
        )?;
    }

    writeln!(writer, "File ingest complete")?;
    writeln!(writer, "Files imported: {completed_files}")?;
    writeln!(writer, "Source bytes imported: {total_source_bytes}")?;
    writeln!(
        writer,
        "Elapsed seconds: {:.3}",
        started_at.elapsed().as_secs_f64()
    )?;

    Ok(())
}

fn resolve_ingest_endpoint(
    endpoint: &StoreId,
    store_registry_path: &Path,
    subobject_registry_path: &Path,
) -> Result<ResolvedIngestEndpoint, CliError> {
    let stores = read_store_registry(store_registry_path)?;
    let store_match = stores
        .iter()
        .find(|definition| definition.store_id == *endpoint);
    let subobjects = read_subobject_registry(subobject_registry_path)?;
    let subobject_match = subobjects
        .iter()
        .find(|definition| definition.name == endpoint.as_str());

    if store_match.is_some() && subobject_match.is_some() {
        return Err(CliError::CommandFailed(format!(
            "ingest endpoint {endpoint} is ambiguous; both an object store and a SubObject use that name"
        )));
    }

    if let Some(store) = store_match {
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: endpoint.as_str().to_string(),
            endpoint_kind: "object_store",
            store: store.clone(),
            object_prefix: store.store_id.as_str().to_string(),
        });
    }

    if let Some(subobject) = subobject_match {
        let store = stores
            .iter()
            .find(|definition| definition.store_id == subobject.store_id)
            .ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "SubObject {} references missing store {} in {}",
                    subobject.name,
                    subobject.store_id,
                    store_registry_path.display()
                ))
            })?;
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: subobject.name.clone(),
            endpoint_kind: "subobject",
            store: store.clone(),
            object_prefix: subobject.object_prefix(),
        });
    }

    Err(CliError::CommandFailed(format!(
        "ingest endpoint {endpoint} was not found in {} or {}",
        store_registry_path.display(),
        subobject_registry_path.display()
    )))
}

fn authorize_store_write(store: &StoreServiceDefinition) -> Result<(), CliError> {
    let Some(writer_group) = &store.writer_group else {
        return Err(CliError::CommandFailed(format!(
            "store {} has no writer group configured; ask an administrator to set --writer-group",
            store.store_id
        )));
    };

    if super::current_user_is_root()? {
        return Ok(());
    }

    let groups = super::current_user_group_names()?;
    if groups.iter().any(|group| group == writer_group) {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "current user is not allowed to write store {}; required group: {}",
        store.store_id, writer_group
    )))
}

fn plan_disk_roots_for_entry(
    roots: &[DiskCopyRoot],
    entry: &FileIngestEntry,
    policy: &StorePolicy,
    copies: u8,
) -> Result<Vec<DiskCopyRoot>, CliError> {
    let root_by_disk = roots
        .iter()
        .map(|root| (root.disk_id.clone(), root.clone()))
        .collect::<BTreeMap<_, _>>();
    let candidates = placement_candidates_for_entry(roots, entry)?;
    let request = if copies > 1 {
        PlacementRequest::protected(entry.size_bytes)
    } else {
        PlacementRequest::cache(entry.size_bytes)
    };
    let plan = plan_copy_count_for_store(&candidates, &request, policy, copies)
        .map_err(|err| CliError::CommandFailed(format!("copy placement failed: {err:?}")))?;
    if !plan.is_complete() {
        return Err(CliError::CommandFailed(format!(
            "copy placement for {} planned {} of {} required copy/copies",
            entry.object_id,
            plan.planned_copies.len(),
            copies
        )));
    }

    plan.planned_copies
        .into_iter()
        .map(|copy| {
            root_by_disk.get(&copy.disk_id).cloned().ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "copy placement selected unknown disk {}",
                    copy.disk_id
                ))
            })
        })
        .collect()
}

fn placement_candidates_for_entry(
    roots: &[DiskCopyRoot],
    entry: &FileIngestEntry,
) -> Result<Vec<PlacementCandidate>, CliError> {
    roots
        .iter()
        .map(|root| {
            let capacity = measure_ssd_capacity(&root.root_path)?;
            Ok(PlacementCandidate::new(
                root.disk_id.clone(),
                None,
                capacity.available_bytes,
                dasobjectstore_core::lifecycle::HealthState::Healthy,
                PerformanceClass::Unknown,
                deterministic_write_load(&entry.object_id, &root.disk_id),
            ))
        })
        .collect()
}

fn deterministic_write_load(object_id: &ObjectId, disk_id: &DiskId) -> WriteLoad {
    let mut hasher = DefaultHasher::new();
    object_id.as_str().hash(&mut hasher);
    disk_id.as_str().hash(&mut hasher);
    match hasher.finish() % 4 {
        0 => WriteLoad::Idle,
        1 => WriteLoad::Light,
        2 => WriteLoad::Busy,
        _ => WriteLoad::Saturated,
    }
}

pub(super) fn collect_ingest_files(
    root: &Path,
    object_prefix: &str,
) -> Result<Vec<FileIngestEntry>, CliError> {
    if !root.is_dir() {
        return Err(CliError::CommandFailed(format!(
            "ingest source must be a directory: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    collect_ingest_files_into(root, root, object_prefix, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(files)
}

fn collect_ingest_files_into(
    root: &Path,
    current: &Path,
    object_prefix: &str,
    files: &mut Vec<FileIngestEntry>,
) -> Result<(), CliError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        if is_hidden_entry_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_ingest_files_into(root, &path, object_prefix, files)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            let relative_path = path
                .strip_prefix(root)
                .map_err(|err| CliError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(FileIngestEntry {
                object_id: object_id_for_ingested_file(object_prefix, &relative_path)?,
                source_path: path,
                relative_path,
                size_bytes: metadata.len(),
            });
        }
    }

    Ok(())
}

fn is_hidden_entry_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn object_id_for_ingested_file(
    object_prefix: &str,
    relative_path: &Path,
) -> Result<ObjectId, CliError> {
    let relative = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    ObjectId::new(format!("{object_prefix}/{relative}"))
        .map_err(|err| CliError::CommandFailed(err.to_string()))
}

pub(super) fn progress_stage_key(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest | ObjectPutProgressStage::SsdFlush => {
            "ssd-ingest".to_string()
        }
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy-{disk_id}-{copy_number}"),
        ObjectPutProgressStage::HddFsync {
            disk_id,
            copy_number,
            ..
        } => format!("hdd-fsync-{disk_id}-{copy_number}"),
        ObjectPutProgressStage::HddRename {
            disk_id,
            copy_number,
            ..
        } => format!("hdd-rename-{disk_id}-{copy_number}"),
    }
}

pub(super) fn progress_stage_label(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest => "ssd-ingest".to_string(),
        ObjectPutProgressStage::SsdFlush => "ssd-flush".to_string(),
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy:{disk_id}:{copy_number}"),
        ObjectPutProgressStage::HddFsync {
            disk_id,
            copy_number,
            duration_millis,
        } => hdd_finalization_stage_label("hdd-fsync", disk_id, *copy_number, *duration_millis),
        ObjectPutProgressStage::HddRename {
            disk_id,
            copy_number,
            duration_millis,
        } => hdd_finalization_stage_label("hdd-rename", disk_id, *copy_number, *duration_millis),
    }
}

fn hdd_finalization_stage_label(
    stage: &str,
    disk_id: &str,
    copy_number: u8,
    duration_millis: Option<u64>,
) -> String {
    let label = format!("{stage}:{disk_id}:{copy_number}");
    match duration_millis {
        Some(duration_millis) => format!("{label}:{duration_millis}ms"),
        None => label,
    }
}

fn write_file_ingest_progress(
    writer: &mut impl Write,
    completed_work_bytes: u64,
    total_work_bytes: u64,
    completed_files: usize,
    total_files: usize,
    progress: &ObjectPutProgress,
    started_at: Instant,
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
) -> Result<(), io::Error> {
    let percent = if total_work_bytes == 0 {
        100.0
    } else {
        completed_work_bytes as f64 * 100.0 / total_work_bytes as f64
    };
    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
    let rate = completed_work_bytes as f64 / elapsed;
    let active_files = (completed_files + 1).min(total_files);
    let remaining_files = total_files.saturating_sub(active_files);
    let ssd_stress = match read_ssd_stress(ssd_root, capacity_policy) {
        Ok(stress) => stress,
        Err(_) => "unknown".to_string(),
    };

    writeln!(
        writer,
        "{:>12} {:>6.2}% {:>12}/s files={}/{} remaining={} stage={} stage_bytes={} ssd={}",
        completed_work_bytes,
        percent,
        super::format_bytes(rate),
        active_files,
        total_files,
        remaining_files,
        progress_stage_label(progress),
        progress.bytes_written,
        ssd_stress
    )
}

fn read_ssd_stress(
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
) -> Result<String, CliError> {
    let capacity = measure_ssd_capacity(ssd_root)?;
    let pressure = capacity_policy.evaluate(&capacity)?;

    Ok(format!(
        "pressure={pressure:?} used={}%",
        capacity.used_percent_floor()
    ))
}
