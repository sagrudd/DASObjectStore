//! Endpoint resolution and deterministic source-file discovery for file ingest.

use super::DaemonIngestFilesRuntimeError;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use dasobjectstore_object_service::{
    read_store_registry, read_subobject_registry, StoreServiceDefinition,
};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FileIngestEntry {
    pub(super) source_path: PathBuf,
    pub(super) relative_path: PathBuf,
    pub(super) object_id: ObjectId,
    pub(super) size_bytes: u64,
    pub(super) file_index: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResolvedIngestEndpoint {
    pub(super) endpoint_name: String,
    pub(super) endpoint_kind: &'static str,
    pub(super) subobject_name: Option<String>,
    pub(super) store: StoreServiceDefinition,
    pub(super) object_prefix: String,
}

pub(super) fn resolve_ingest_endpoint(
    endpoint: &StoreId,
    store_registry_path: &Path,
    subobject_registry_path: &Path,
) -> Result<ResolvedIngestEndpoint, DaemonIngestFilesRuntimeError> {
    let stores = read_store_registry(store_registry_path)?;
    let store_match = stores
        .iter()
        .find(|definition| definition.store_id == *endpoint);
    let subobjects = read_subobject_registry(subobject_registry_path)?;
    let subobject_match = subobjects
        .iter()
        .find(|definition| definition.name == endpoint.as_str());

    if store_match.is_some() && subobject_match.is_some() {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "ingest endpoint {} is ambiguous; both an object store and a SubObject use that name",
            endpoint
        )));
    }

    if let Some(store) = store_match {
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: endpoint.as_str().to_string(),
            endpoint_kind: "object_store",
            subobject_name: None,
            store: store.clone(),
            object_prefix: store.store_id.as_str().to_string(),
        });
    }

    if let Some(subobject) = subobject_match {
        let store = stores
            .iter()
            .find(|definition| definition.store_id == subobject.store_id)
            .ok_or_else(|| {
                DaemonIngestFilesRuntimeError::CommandFailed(format!(
                    "SubObject {} references missing store {} in {}",
                    subobject.name,
                    subobject.store_id,
                    store_registry_path.display()
                ))
            })?;
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: subobject.name.clone(),
            endpoint_kind: "subobject",
            subobject_name: Some(subobject.name.clone()),
            store: store.clone(),
            object_prefix: subobject.object_prefix(),
        });
    }

    Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
        "ingest endpoint {} was not found in {} or {}",
        endpoint,
        store_registry_path.display(),
        subobject_registry_path.display()
    )))
}

pub(super) fn collect_ingest_files(
    root: &Path,
    object_prefix: &str,
) -> Result<Vec<FileIngestEntry>, DaemonIngestFilesRuntimeError> {
    if !root.is_dir() {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "ingest source must be a directory: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    collect_ingest_files_into(root, root, object_prefix, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    for (index, file) in files.iter_mut().enumerate() {
        file.file_index = index as u64 + 1;
    }

    Ok(files)
}

fn collect_ingest_files_into(
    root: &Path,
    current: &Path,
    object_prefix: &str,
    files: &mut Vec<FileIngestEntry>,
) -> Result<(), DaemonIngestFilesRuntimeError> {
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
                .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(FileIngestEntry {
                object_id: object_id_for_ingested_file(object_prefix, &relative_path)?,
                source_path: path,
                relative_path,
                size_bytes: metadata.len(),
                file_index: 0,
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
) -> Result<ObjectId, DaemonIngestFilesRuntimeError> {
    let relative = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    ObjectId::new(format!("{object_prefix}/{relative}"))
        .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))
}
