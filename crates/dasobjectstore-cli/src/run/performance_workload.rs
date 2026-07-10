use super::performance_plan::{PerformancePayload, PerformanceWorkload, PerformanceWorkloadKind};
use super::*;
use super::{PerformanceFileOrder, PerformanceFileSelection};

pub(super) fn plan_performance_workload(
    args: &PerformanceTestArgs,
) -> Result<PerformanceWorkload, CliError> {
    let cap_bytes = args.cap().map(parse_binary_size).transpose()?;
    let primary_file_order = args
        .file_orders()
        .first()
        .copied()
        .unwrap_or(PerformanceFileOrder::SizeDesc);
    match (args.source(), args.file_size(), args.file_count()) {
        (Some(source), None, None) => {
            source_performance_workload(source, cap_bytes, args.file_select(), primary_file_order)
        }
        (None, Some(file_size), Some(file_count)) => {
            if cap_bytes.is_some() {
                return Err(CliError::CommandFailed(
                    "performance-test --cap can only be used with --source".to_string(),
                ));
            }
            if file_count == 0 {
                return Err(CliError::CommandFailed(
                    "performance-test requires --file_count greater than 0".to_string(),
                ));
            }
            let size_bytes = parse_binary_size(file_size)?;
            let payloads = (0..file_count)
                .map(|file_index| PerformancePayload {
                    file_index,
                    relative_path: PathBuf::from(format!("generated-{file_index:05}.bin")),
                    source_path: None,
                    size_bytes,
                    modified_unix_nanos: u128::from(file_index),
                })
                .collect::<Vec<_>>();
            let mut payloads = payloads;
            apply_performance_file_order(&mut payloads, primary_file_order);
            assign_performance_file_indexes(&mut payloads);
            Ok(PerformanceWorkload {
                kind: PerformanceWorkloadKind::Generated,
                source_path: None,
                source_cap_bytes: None,
                file_selection: args.file_select(),
                file_order: primary_file_order,
                discovered_file_count: file_count,
                discovered_total_bytes: size_bytes.saturating_mul(u64::from(file_count)),
                payloads,
            })
        }
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err(CliError::CommandFailed(
            "performance-test accepts either --source or --file_size/--file_count, not both"
                .to_string(),
        )),
        (None, _, _) => Err(CliError::CommandFailed(
            "performance-test requires either --source <DIR> or both --file_size and --file_count"
                .to_string(),
        )),
    }
}

pub(super) fn source_performance_workload(
    source: &Path,
    cap_bytes: Option<u64>,
    file_selection: PerformanceFileSelection,
    file_order: PerformanceFileOrder,
) -> Result<PerformanceWorkload, CliError> {
    if !source.is_dir() {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} is not a directory",
            source.display()
        )));
    }
    let mut files = Vec::new();
    collect_performance_source_files(source, source, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let discovered_file_count = files.len();
    let discovered_total_bytes = files.iter().map(|payload| payload.size_bytes).sum::<u64>();
    if files.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} contains no files",
            source.display()
        )));
    }
    if files.len() > u32::MAX as usize {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} contains more than {} files",
            source.display(),
            u32::MAX
        )));
    }
    if let Some(cap_bytes) = cap_bytes {
        files = select_performance_source_files(files, cap_bytes, file_selection, source)?;
    }
    apply_performance_file_order(&mut files, file_order);
    assign_performance_file_indexes(&mut files);
    Ok(PerformanceWorkload {
        kind: PerformanceWorkloadKind::SourceFolder,
        source_path: Some(source.to_path_buf()),
        source_cap_bytes: cap_bytes,
        file_selection,
        file_order,
        discovered_file_count: discovered_file_count as u32,
        discovered_total_bytes,
        payloads: files,
    })
}

pub(super) fn select_performance_source_files(
    mut files: Vec<PerformancePayload>,
    cap_bytes: u64,
    file_selection: PerformanceFileSelection,
    source: &Path,
) -> Result<Vec<PerformancePayload>, CliError> {
    match file_selection {
        PerformanceFileSelection::Random => shuffle_performance_payloads(&mut files),
        PerformanceFileSelection::Smaller => files.sort_by(|left, right| {
            left.size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileSelection::Larger => files.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
    }
    let mut selected = Vec::new();
    let mut selected_bytes = 0_u64;
    for payload in files {
        let next_bytes = selected_bytes.saturating_add(payload.size_bytes);
        if next_bytes <= cap_bytes {
            selected_bytes = next_bytes;
            selected.push(payload);
        }
    }
    if selected.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test --cap {} is smaller than every selectable source file in {}",
            format_bytes(cap_bytes as f64),
            source.display()
        )));
    }
    Ok(selected)
}

pub(super) fn ordered_performance_workload(
    workload: &PerformanceWorkload,
    file_order: PerformanceFileOrder,
) -> PerformanceWorkload {
    let mut ordered = workload.clone();
    ordered.file_order = file_order;
    apply_performance_file_order(&mut ordered.payloads, file_order);
    assign_performance_file_indexes(&mut ordered.payloads);
    ordered
}

pub(super) fn apply_performance_file_order(
    files: &mut [PerformancePayload],
    file_order: PerformanceFileOrder,
) {
    match file_order {
        PerformanceFileOrder::Fifo => {
            files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path))
        }
        PerformanceFileOrder::SizeAsc => files.sort_by(|left, right| {
            left.size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::SizeDesc => files.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::TimeAsc => files.sort_by(|left, right| {
            left.modified_unix_nanos
                .cmp(&right.modified_unix_nanos)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::TimeDesc => files.sort_by(|left, right| {
            right
                .modified_unix_nanos
                .cmp(&left.modified_unix_nanos)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
    }
}

pub(super) fn assign_performance_file_indexes(files: &mut [PerformancePayload]) {
    for (index, payload) in files.iter_mut().enumerate() {
        payload.file_index = index as u32;
    }
}

pub(super) fn shuffle_performance_payloads(files: &mut [PerformancePayload]) {
    let mut rng = OsRng;
    for index in (1..files.len()).rev() {
        let swap_index = (rng.next_u64() % (index as u64 + 1)) as usize;
        files.swap(index, swap_index);
    }
}

pub(super) fn collect_performance_source_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PerformancePayload>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_performance_source_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative_path = path
                .strip_prefix(root)
                .map_err(|err| CliError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(PerformancePayload {
                file_index: 0,
                relative_path,
                source_path: Some(path),
                size_bytes: metadata.len(),
                modified_unix_nanos: metadata_modified_unix_nanos(&metadata),
            });
        }
    }
    Ok(())
}

pub(super) fn metadata_modified_unix_nanos(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
