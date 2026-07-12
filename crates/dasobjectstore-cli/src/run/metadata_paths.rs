//! Shared managed-metadata path resolution for CLI read-only commands.

use super::*;

pub(super) fn resolve_live_sqlite_path(override_path: Option<&Path>) -> PathBuf {
    override_path.map(Path::to_path_buf).unwrap_or_else(|| {
        default_ssd_root()
            .join(METADATA_DIR_NAME)
            .join(LIVE_SQLITE_FILE_NAME)
    })
}

pub(super) fn resolve_store_live_sqlite_path(
    store_id: &StoreId,
    override_path: Option<&Path>,
    registry_path: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if override_path.is_none() {
        let registry_path = registry_path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_store_registry_path);
        let store_exists = read_store_registry(&registry_path)?
            .iter()
            .any(|definition| &definition.store_id == store_id);
        if !store_exists {
            return Err(CliError::CommandFailed(format!(
                "store `{store_id}` is not defined in {}",
                registry_path.display()
            )));
        }
    }

    Ok(resolve_live_sqlite_path(override_path))
}
