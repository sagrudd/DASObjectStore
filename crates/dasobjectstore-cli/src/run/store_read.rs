//! Read-only ObjectStore inspection and policy-validation handlers.

use super::*;

pub(super) fn run_store_profile_inspection(
    args: &StoreProfileInspectionArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let store_id = StoreId::new(args.store_id())
        .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let config = DaemonRuntimeConfig::default_packaged();
    let response = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path))
        .profile_inspection(ProfileInspectionRequest { store_id })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Profile inspection")?;
        writeln!(writer, "Store: {}", response.store_id)?;
        writeln!(writer, "Profile: {}", response.deployment_profile.name())?;
        writeln!(writer, "Root state: {}", response.root_state.as_str())?;
        writeln!(
            writer,
            "Unmanaged entries: {}",
            response.unmanaged_path_count
        )?;
        writeln!(writer, "Unsafe entries: {}", response.unsafe_path_count)?;
        for warning in response.warnings {
            writeln!(writer, "Warning: {warning}")?;
        }
    }
    Ok(())
}

pub(super) fn run_store_profile_browser(
    args: &StoreProfileBrowserArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let store_id = StoreId::new(args.store_id())
        .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let config = DaemonRuntimeConfig::default_packaged();
    let response = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path))
        .profile_browser(ProfileBrowserRequest {
            store_id,
            prefix: args.prefix().map(str::to_owned),
            search: args.search().map(str::to_owned),
            offset: args.offset(),
            limit: args.limit(),
            delegated_actor: None,
        })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Profile browser")?;
        writeln!(writer, "Store: {}", response.store_id)?;
        writeln!(writer, "Profile: {}", response.profile.name())?;
        writeln!(writer, "Entries: {}", response.entries.len())?;
        writeln!(writer, "Total matches: {}", response.total_entries)?;
        if let Some(next_offset) = response.next_offset {
            writeln!(writer, "Next offset: {next_offset}")?;
        }
        for entry in response.entries {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}",
                entry.key.object_id, entry.key.version, entry.size_bytes, entry.checksum
            )?;
        }
    }
    Ok(())
}

pub(super) fn run_store_user_service_plan(
    args: &StoreUserServicePlanArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    use dasobjectstore_core::deployment::HostMode;
    use dasobjectstore_daemon::runtime::{folder_host_paths, user_service_plan};

    let home = args
        .home()
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| {
            CliError::CommandFailed("per-user service plan requires HOME or --home".to_string())
        })?;
    let state_home = args
        .state_home()
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(PathBuf::from));
    let runtime_home = args
        .runtime_home()
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from));
    let paths = folder_host_paths(
        HostMode::PerUser,
        Some(&home),
        state_home.as_deref(),
        runtime_home.as_deref(),
        Path::new("/var/lib/dasobjectstore"),
        Path::new("/run/dasobjectstore"),
    )
    .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let plan = user_service_plan(&paths, args.executable(), args.config(), args.label())
        .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let plist = plan
        .launchd_plist()
        .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    if args.json() {
        #[derive(serde::Serialize)]
        struct UserServicePlanResponse {
            label: String,
            executable: String,
            config_path: String,
            state_dir: String,
            plist: String,
        }
        serde_json::to_writer_pretty(
            &mut *writer,
            &UserServicePlanResponse {
                label: plan.label,
                executable: plan.executable.display().to_string(),
                config_path: plan.config_path.display().to_string(),
                state_dir: plan.state_dir.display().to_string(),
                plist,
            },
        )?;
        writer.write_all(b"\n")?;
    } else {
        writer.write_all(plist.as_bytes())?;
    }
    Ok(())
}

pub(super) fn run_store_contents(
    args: &StoreContentsArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if args.du() && args.tree() {
        return Err(CliError::UnsupportedStoreContentsFormat);
    }
    let live_sqlite_path = super::metadata_paths::resolve_store_live_sqlite_path(
        args.store_id(),
        args.live_sqlite_path(),
        None,
    )?;
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

#[cfg(test)]
mod tests {
    use super::super::run;
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn renders_user_service_plan_as_json_without_installing_launchd_service() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "user-service-plan",
            "--executable",
            "/Users/tester/bin/dasobjectstored",
            "--config",
            "/Users/tester/Library/Config/dasobjectstore.json",
            "--home",
            "/Users/tester",
            "--state-home",
            "/Users/tester/Library/State",
            "--runtime-home",
            "/tmp/tester-runtime",
            "--json",
        ])
        .expect("user service plan parses");
        let mut output = Vec::new();
        run(&cli, &mut output).expect("plan renders");
        let response: serde_json::Value = serde_json::from_slice(&output).expect("json output");
        assert_eq!(response["label"], "org.dasobjectstore.dasobjectstored");
        assert_eq!(
            response["state_dir"],
            "/Users/tester/Library/State/dasobjectstore"
        );
        assert!(response["plist"]
            .as_str()
            .unwrap()
            .contains("<key>RunAtLoad</key>"));
    }
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
        let ssd_root = super::registry_access::known_ssd_root_for_adopt(args.ssd_root())?;
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

pub(super) fn run_store_capabilities(
    args: &StoreCapabilitiesArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.profile_capabilities(ObjectStoreCapabilityDiscoveryRequest::default())?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "ObjectStore profile capabilities")?;
        writeln!(writer, "Schema: {}", response.schema_version)?;
        for profile in response.profiles {
            writeln!(
                writer,
                "- {}: availability={:?} host_modes={} bounded_capacity={} dedicated_ssd={} local_failure_domains={}",
                profile.profile,
                profile.availability,
                profile.host_modes.len(),
                profile.requirements.bounded_capacity_required,
                profile.requirements.dedicated_ssd_required,
                profile
                    .max_distinct_local_failure_domains
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unavailable".to_string())
            )?;
            if let Some(reason) = profile.unavailable_reason {
                writeln!(writer, "  reason: {reason}")?;
            }
        }
    }
    Ok(())
}

pub(super) fn run_store_capacity(
    args: &StoreCapacityArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.capacity_status(CapacityStatusRequest {
        store_id: args.store_id().as_str().to_string(),
    })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "ObjectStore capacity")?;
        writeln!(writer, "Store: {}", response.store_id)?;
        writeln!(writer, "Pressure: {:?}", response.pressure)?;
        writeln!(writer, "Logical limit: {:?}", response.logical_limit_bytes)?;
        writeln!(writer, "Used: {}", format_bytes(response.used_bytes as f64))?;
        writeln!(
            writer,
            "Reserved: {}",
            format_bytes(response.reserved_bytes as f64)
        )?;
        writeln!(
            writer,
            "Logical available: {:?}",
            response.logical_available_bytes
        )?;
        writeln!(
            writer,
            "Backend free: {}",
            format_bytes(response.backend_free_bytes as f64)
        )?;
        writeln!(
            writer,
            "Backend available: {}",
            format_bytes(response.backend_available_bytes as f64)
        )?;
        writeln!(writer, "SSD available: {:?}", response.ssd_available_bytes)?;
        writeln!(writer, "Copy count: {}", response.copy_count)?;
        writeln!(
            writer,
            "SSD staging required: {}",
            response.requires_ssd_staging
        )?;
        if let Some(reason) = response.admission_block_reason {
            writeln!(writer, "Admission blocked: {:?}", reason)?;
        }
    }
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
