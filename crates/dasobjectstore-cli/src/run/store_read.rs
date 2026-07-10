//! Read-only ObjectStore inspection and policy-validation handlers.

use super::*;

pub(super) fn run_store_contents(
    args: &StoreContentsArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if args.du() && args.tree() {
        return Err(CliError::UnsupportedStoreContentsFormat);
    }
    let live_sqlite_path =
        resolve_store_live_sqlite_path(args.store_id(), args.live_sqlite_path(), None)?;
    let mut request = StoreContentsRequest::new(live_sqlite_path, args.store_id().clone());
    if let Some(prefix) = args.prefix() {
        request = request.with_prefix(prefix);
    }
    if let Some(filter) = args.filter() {
        request = request.with_filter(filter);
    }
    let snapshot = read_store_contents(&request)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &snapshot)?;
        writer.write_all(b"\n")?;
    } else if args.tree() {
        write_store_contents_tree(&snapshot, args.depth(), writer)?;
    } else {
        write_store_contents_du(&snapshot, args.depth(), writer)?;
    }

    Ok(())
}

fn write_store_contents_du(
    snapshot: &StoreContentsSnapshot,
    depth: usize,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "Store contents")?;
    writeln!(writer, "Store: {}", snapshot.store_id)?;
    writeln!(
        writer,
        "Live metadata: {}",
        snapshot.live_sqlite_path.display()
    )?;
    if let Some(filter) = &snapshot.filter {
        writeln!(writer, "Filter: {filter}")?;
    }
    if let Some(prefix) = &snapshot.prefix {
        writeln!(writer, "Path: {prefix}")?;
    }
    writeln!(writer, "Objects: {}", snapshot.objects.len())?;
    writeln!(
        writer,
        "Total: {}",
        format_bytes(snapshot.total_size_bytes() as f64)
    )?;
    writeln!(writer, "Mode: du depth={depth}")?;
    if snapshot.prefix.is_some() {
        let root_is_file = snapshot.objects.len() == 1 && snapshot.objects[0].path.is_empty();
        if root_is_file {
            writeln!(
                writer,
                "[FILE]\t{}\t.",
                format_bytes(snapshot.objects[0].size_bytes as f64)
            )?;
        } else {
            writeln!(
                writer,
                "[DIR]\t{}\t.",
                format_bytes(snapshot.total_size_bytes() as f64)
            )?;
            for object in &snapshot.objects {
                writeln!(
                    writer,
                    "[FILE]\t{}\t{}",
                    format_bytes(object.size_bytes as f64),
                    object.path
                )?;
            }
        }
        return Ok(());
    }
    let single_file = snapshot.objects.len() == 1 && snapshot.objects[0].path.is_empty();
    for (path, size_bytes) in store_contents_du_entries(&snapshot.objects, depth) {
        let kind = if single_file { "FILE" } else { "DIR" };
        writeln!(
            writer,
            "[{kind}]\t{}\t{path}",
            format_bytes(size_bytes as f64)
        )?;
    }
    Ok(())
}

fn write_store_contents_tree(
    snapshot: &StoreContentsSnapshot,
    depth: usize,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "Store contents")?;
    writeln!(writer, "Store: {}", snapshot.store_id)?;
    writeln!(
        writer,
        "Live metadata: {}",
        snapshot.live_sqlite_path.display()
    )?;
    if let Some(filter) = &snapshot.filter {
        writeln!(writer, "Filter: {filter}")?;
    }
    if let Some(prefix) = &snapshot.prefix {
        writeln!(writer, "Path: {prefix}")?;
    }
    writeln!(writer, "Objects: {}", snapshot.objects.len())?;
    writeln!(
        writer,
        "Total: {}",
        format_bytes(snapshot.total_size_bytes() as f64)
    )?;
    writeln!(writer, "Mode: tree depth={depth}")?;
    let tree = StoreContentsTreeNode::from_objects(&snapshot.objects);
    if tree.children.is_empty() && tree.file_size_bytes.is_some() {
        writeln!(writer, "[FILE] . {}", format_bytes(tree.size_bytes as f64))?;
    } else {
        writeln!(writer, "[DIR] . {}", format_bytes(tree.size_bytes as f64))?;
    }
    write_store_contents_tree_children(&tree, 1, depth, writer)
}

fn store_contents_du_entries(objects: &[StoreContentsObject], depth: usize) -> Vec<(String, u64)> {
    let mut entries = BTreeMap::<String, u64>::new();
    entries.insert(
        ".".to_string(),
        objects.iter().map(|object| object.size_bytes).sum(),
    );
    if depth == 0 {
        return entries.into_iter().collect();
    }
    for object in objects {
        let parts = store_contents_path_parts(&object.path);
        for prefix_depth in 1..=depth.min(parts.len()) {
            let prefix = parts[..prefix_depth].join("/");
            *entries.entry(prefix).or_insert(0) += object.size_bytes;
        }
    }
    entries.into_iter().collect()
}

#[derive(Default)]
struct StoreContentsTreeNode {
    size_bytes: u64,
    file_size_bytes: Option<u64>,
    children: BTreeMap<String, StoreContentsTreeNode>,
}

impl StoreContentsTreeNode {
    fn from_objects(objects: &[StoreContentsObject]) -> Self {
        let mut root = Self::default();
        for object in objects {
            root.insert(&store_contents_path_parts(&object.path), object.size_bytes);
        }
        root
    }

    fn insert(&mut self, parts: &[String], size_bytes: u64) {
        self.size_bytes = self.size_bytes.saturating_add(size_bytes);
        if let Some((head, tail)) = parts.split_first() {
            self.children
                .entry(head.clone())
                .or_default()
                .insert(tail, size_bytes);
        } else {
            self.file_size_bytes = Some(size_bytes);
        }
    }
}

fn write_store_contents_tree_children(
    node: &StoreContentsTreeNode,
    current_depth: usize,
    max_depth: usize,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if current_depth > max_depth {
        return Ok(());
    }
    for (name, child) in &node.children {
        let indent = "  ".repeat(current_depth.saturating_sub(1));
        if child.children.is_empty() {
            writeln!(
                writer,
                "{indent}[FILE] {name} {}",
                format_bytes(child.size_bytes as f64)
            )?;
        } else {
            writeln!(
                writer,
                "{indent}[DIR] {name}/ {}",
                format_bytes(child.size_bytes as f64)
            )?;
            write_store_contents_tree_children(child, current_depth + 1, max_depth, writer)?;
        }
    }
    Ok(())
}

fn store_contents_path_parts(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn run_store_list(
    args: &StoreListArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = if args.portable() {
        let ssd_root = known_ssd_root_for_adopt(args.ssd_root())?;
        portable_store_registry_path(ssd_root)
    } else {
        args.registry_path()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_store_registry_path)
    };
    let definitions = read_store_registry(&registry_path)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &definitions)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_list_report(&definitions, writer)?;
    }

    Ok(())
}

pub(super) fn run_store_defaults(
    args: &StoreDefaultsArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let policy = StorePolicy::defaults_for(args.class());

    serde_json::to_writer_pretty(&mut *writer, &policy)?;
    writer.write_all(b"\n")?;

    Ok(())
}

pub(super) fn run_store_s3_upload(
    args: &StoreS3UploadArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let (bucket_name, credential_reference) = match args.bucket() {
        Some(bucket_name) => (
            bucket_name.to_string(),
            credential_reference_for_store(args.store_id()),
        ),
        None => {
            let registry_path = args
                .registry_path()
                .map(Path::to_path_buf)
                .unwrap_or_else(default_store_registry_path);
            let definitions = read_store_registry(&registry_path)?;
            let definition = definitions
                .iter()
                .find(|definition| definition.store_id == *args.store_id())
                .cloned()
                .ok_or_else(|| {
                    CliError::CommandFailed(format!(
                        "store {} was not found in {}",
                        args.store_id(),
                        registry_path.display()
                    ))
                })?;
            let layout = plan_store_service_layout(&[definition])?;
            let binding = layout.bucket_bindings.into_iter().next().ok_or_else(|| {
                CliError::CommandFailed(format!("store {} is not S3-exported", args.store_id()))
            })?;
            (binding.bucket_name, binding.credential_reference)
        }
    };
    let profile_name = args
        .profile()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("dasobjectstore-{}", args.store_id()));
    let plan = plan_remote_s3_upload(RemoteS3UploadPlanRequest {
        store_id: args.store_id().clone(),
        bucket_name,
        endpoint_url: args.endpoint_url().to_string(),
        region: args.region().to_string(),
        profile_name,
        credential_reference,
        auth_authority: args.auth().into(),
        username: args.username().map(ToOwned::to_owned),
    })?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &plan)?;
        writer.write_all(b"\n")?;
    } else {
        write_remote_s3_upload_plan(&plan, writer)?;
    }

    Ok(())
}

pub(super) fn run_store_validate(
    args: &StoreValidateArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let file = File::open(args.policy_file())?;
    let policy: StorePolicy = serde_json::from_reader(file)?;

    policy.validate()?;
    writeln!(writer, "Store policy is valid: {}", policy.class.name())?;

    Ok(())
}
