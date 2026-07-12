//! Portable registry mirroring and writer-group access helpers.

use super::*;

pub(super) fn upsert_portable_store_definition(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    definition: &StoreServiceDefinition,
) -> Result<Option<StoreRegistryUpdateReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? else {
        return Ok(None);
    };
    let registry_path = portable_store_registry_path(&ssd_root);
    let report = upsert_store_definition(&registry_path, definition.clone())?;

    Ok(Some(report))
}

pub(super) fn known_ssd_root_for_optional_mirror(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
) -> Result<Option<PathBuf>, CliError> {
    match ssd_root {
        Some(path) => {
            validate_known_ssd_root(path)?;
            Ok(Some(path.to_path_buf()))
        }
        None => {
            if !allow_default_ssd {
                return Ok(None);
            }
            let path = default_ssd_root();
            if is_known_ssd_root(&path) {
                Ok(Some(path))
            } else {
                Ok(None)
            }
        }
    }
}

pub(super) fn grant_store_writer_group_access(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    writer_group: &str,
) -> Result<(), CliError> {
    #[cfg(target_os = "linux")]
    {
        ensure_group_exists(writer_group)?;
        let mut roots = Vec::new();
        if let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? {
            roots.push(ssd_root);
        }
        roots.extend(
            discover_managed_hdd_roots(&default_hdd_root())?
                .into_iter()
                .map(|root| root.root_path),
        );
        for root in roots {
            grant_group_acl(&root, writer_group)?;
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = ssd_root;
        let _ = allow_default_ssd;
        let _ = writer_group;
    }

    Ok(())
}

pub(super) fn grant_subobject_writer_group_registry_access(
    args: &SubobjectCreateArgs,
    definition: &SubObjectDefinition,
    registry_path: &Path,
) -> Result<(), CliError> {
    let stores_registry_path = args
        .stores_registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let stores = read_store_registry(&stores_registry_path)?;
    let Some(store) = stores
        .iter()
        .find(|store| store.store_id == definition.store_id)
    else {
        return Ok(());
    };
    let Some(writer_group) = &store.writer_group else {
        return Ok(());
    };

    grant_writer_group_registry_access(registry_path, writer_group)
}

pub(super) fn grant_writer_group_registry_access(
    registry_path: &Path,
    writer_group: &str,
) -> Result<(), CliError> {
    #[cfg(target_os = "linux")]
    {
        ensure_group_exists(writer_group)?;
        if let Some(parent) = registry_path.parent() {
            grant_group_read_dir_acl(parent, writer_group)?;
        }
        if registry_path.is_file() {
            grant_group_read_file_acl(registry_path, writer_group)?;
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = registry_path;
        let _ = writer_group;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn ensure_group_exists(group: &str) -> Result<(), CliError> {
    let status = ProcessCommand::new("getent")
        .args(["group", group])
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "writer group does not exist: {group}"
    )))
}

#[cfg(target_os = "linux")]
fn grant_group_acl(root: &Path, group: &str) -> Result<(), CliError> {
    let acl = format!("g:{group}:rwx");
    let default_acl = format!("d:g:{group}:rwx");
    let status = ProcessCommand::new("setfacl")
        .args(["-R", "-m", &acl, "-m", &default_acl])
        .arg(root)
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "setfacl failed for {} with status {}",
        root.display(),
        status
    )))
}

#[cfg(target_os = "linux")]
fn grant_group_read_dir_acl(path: &Path, group: &str) -> Result<(), CliError> {
    let acl = format!("g:{group}:rx");
    let default_acl = format!("d:g:{group}:rx");
    let status = ProcessCommand::new("setfacl")
        .args(["-m", &acl, "-m", &default_acl])
        .arg(path)
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "setfacl failed for {} with status {}",
        path.display(),
        status
    )))
}

#[cfg(target_os = "linux")]
fn grant_group_read_file_acl(path: &Path, group: &str) -> Result<(), CliError> {
    let acl = format!("g:{group}:r");
    let status = ProcessCommand::new("setfacl")
        .args(["-m", &acl])
        .arg(path)
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "setfacl failed for {} with status {}",
        path.display(),
        status
    )))
}

pub(super) fn known_ssd_root_for_adopt(ssd_root: Option<&Path>) -> Result<PathBuf, CliError> {
    let path = ssd_root
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&path)?;

    Ok(path)
}
