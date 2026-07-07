use crate::api::{
    DaemonIngestPipelineStage, DaemonIngestProgressEvent, DaemonIngestStage,
    SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::HealthState;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::placement::{
    plan_copy_count_for_store, PerformanceClass, PlacementCandidate, PlacementRequest, WriteLoad,
};
use dasobjectstore_core::store::StorePolicy;
use dasobjectstore_metadata::{
    measure_ssd_capacity, put_object_ssd_first_with_progress, DiskCopyRoot, ObjectPutProgress,
    ObjectPutProgressStage, ObjectPutRequest,
};
use dasobjectstore_object_service::{
    default_store_registry_path, default_subobject_registry_path, read_store_registry,
    read_subobject_registry, ObjectServiceError, StoreServiceDefinition,
};
use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::fmt::{self, Display};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

const SSD_ROOT_ENV: &str = "DASOBJECTSTORE_SSD_ROOT";
const HDD_ROOT_ENV: &str = "DASOBJECTSTORE_HDD_ROOT";
const DEFAULT_SSD_ROOT: &str = "/srv/dasobjectstore/ssd";
const DEFAULT_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonFileIngestSummary {
    pub endpoint_name: String,
    pub endpoint_kind: &'static str,
    pub store_id: StoreId,
    pub object_prefix: String,
    pub files: usize,
    pub source_bytes: u64,
    pub copies: u8,
    pub object_type: ObjectType,
    pub dry_run: bool,
}

pub fn submit_ingest_files_to_local_store(
    request: SubmitIngestFilesRequest,
    accepted_at_utc: &str,
) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
    submit_ingest_files_to_local_store_with_progress(request, accepted_at_utc, |_| {})
}

pub fn submit_ingest_files_to_local_store_with_progress(
    request: SubmitIngestFilesRequest,
    accepted_at_utc: &str,
    progress: impl FnMut(DaemonIngestProgressEvent),
) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
    let executor = LocalFileIngestExecutor::from_environment();
    executor.submit(request, accepted_at_utc, progress)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIngestEntry {
    source_path: PathBuf,
    relative_path: PathBuf,
    object_id: ObjectId,
    size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedIngestEndpoint {
    endpoint_name: String,
    endpoint_kind: &'static str,
    store: StoreServiceDefinition,
    object_prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalFileIngestExecutor {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    store_registry_path: PathBuf,
    subobject_registry_path: PathBuf,
}

impl LocalFileIngestExecutor {
    fn from_environment() -> Self {
        Self {
            ssd_root: default_ssd_root(),
            hdd_root: default_hdd_root(),
            store_registry_path: default_store_registry_path(),
            subobject_registry_path: default_subobject_registry_path(),
        }
    }

    fn submit(
        &self,
        request: SubmitIngestFilesRequest,
        accepted_at_utc: &str,
        progress: impl FnMut(DaemonIngestProgressEvent),
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        let job_id = ingest_job_id(accepted_at_utc)?;
        let summary = self.execute(request, &job_id, progress)?;
        Ok(SubmitIngestFilesResponse {
            job_id,
            accepted_at_utc: accepted_at_utc.to_string(),
            dry_run: summary.dry_run,
        })
    }

    fn execute(
        &self,
        request: SubmitIngestFilesRequest,
        job_id: &IngestJobId,
        mut progress: impl FnMut(DaemonIngestProgressEvent),
    ) -> Result<DaemonFileIngestSummary, DaemonIngestFilesRuntimeError> {
        validate_known_ssd_root(&self.ssd_root)?;
        let endpoint = resolve_ingest_endpoint(
            &request.endpoint,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )?;
        let managed_disk_roots = discover_managed_hdd_roots(&self.hdd_root)?;
        let copies = request.copies.unwrap_or(endpoint.store.policy.copies);
        if copies == 0 || managed_disk_roots.len() < copies as usize {
            return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "ingest files requires at least {copies} managed HDD root(s), got {}",
                managed_disk_roots.len()
            )));
        }
        let files = collect_ingest_files(&request.source_path, &endpoint.object_prefix)?;
        let source_bytes = files.iter().map(|entry| entry.size_bytes).sum::<u64>();
        let total_work_bytes = source_bytes.saturating_mul(u64::from(copies) + 1);
        let summary = DaemonFileIngestSummary {
            endpoint_name: endpoint.endpoint_name.clone(),
            endpoint_kind: endpoint.endpoint_kind,
            store_id: endpoint.store.store_id.clone(),
            object_prefix: endpoint.object_prefix.clone(),
            files: files.len(),
            source_bytes,
            copies,
            object_type: request.object_type,
            dry_run: request.dry_run,
        };

        progress(DaemonIngestProgressEvent {
            job_id: job_id.clone(),
            endpoint: request.endpoint.clone(),
            stage: DaemonIngestStage::Queued,
            pipeline_stage: Some(DaemonIngestPipelineStage::Scan),
            work_bytes_done: 0,
            work_bytes_total: Some(total_work_bytes),
            files_done: 0,
            files_total: Some(files.len() as u64),
            current_object_id: None,
            ssd_pressure: None,
            telemetry: None,
            resource_policy: None,
            message: Some(format!(
                "planned {} file(s), {} source byte(s), {} copy/copies",
                files.len(),
                source_bytes,
                copies
            )),
        });

        if request.dry_run {
            progress(DaemonIngestProgressEvent {
                job_id: job_id.clone(),
                endpoint: request.endpoint.clone(),
                stage: DaemonIngestStage::Complete,
                pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
                work_bytes_done: 0,
                work_bytes_total: Some(total_work_bytes),
                files_done: files.len() as u64,
                files_total: Some(files.len() as u64),
                current_object_id: None,
                ssd_pressure: None,
                telemetry: None,
                resource_policy: None,
                message: Some("dry run complete; no files imported".to_string()),
            });
            return Ok(summary);
        }

        let mut completed_files = 0_u64;
        let mut completed_work_bytes = 0_u64;
        for entry in &files {
            let mut stage_key = String::new();
            let mut stage_offset_bytes = 0_u64;
            let put_request = ObjectPutRequest::new(
                entry.object_id.clone(),
                &entry.source_path,
                &self.ssd_root,
                plan_disk_roots_for_entry(
                    &managed_disk_roots,
                    entry,
                    &endpoint.store.policy,
                    copies,
                )?,
                copies,
            )
            .with_object_type(request.object_type);
            put_object_ssd_first_with_progress(&put_request, |object_progress| {
                let next_stage_key = object_progress_stage_key(&object_progress);
                if next_stage_key != stage_key {
                    stage_key = next_stage_key;
                    stage_offset_bytes = 0;
                }
                let delta = object_progress
                    .bytes_written
                    .saturating_sub(stage_offset_bytes);
                stage_offset_bytes = object_progress.bytes_written;
                completed_work_bytes = completed_work_bytes.saturating_add(delta);
                progress(object_progress_event(
                    job_id,
                    &request.endpoint,
                    entry,
                    completed_work_bytes,
                    total_work_bytes,
                    completed_files,
                    files.len() as u64,
                    &object_progress,
                ));
            })?;
            completed_files = completed_files.saturating_add(1);
            progress(DaemonIngestProgressEvent {
                job_id: job_id.clone(),
                endpoint: request.endpoint.clone(),
                stage: DaemonIngestStage::SsdIngest,
                pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
                work_bytes_done: completed_work_bytes,
                work_bytes_total: Some(total_work_bytes),
                files_done: completed_files,
                files_total: Some(files.len() as u64),
                current_object_id: Some(entry.object_id.clone()),
                ssd_pressure: None,
                telemetry: None,
                resource_policy: None,
                message: Some(format!(
                    "file complete: {}",
                    entry.relative_path.to_string_lossy()
                )),
            });
        }

        progress(DaemonIngestProgressEvent {
            job_id: job_id.clone(),
            endpoint: request.endpoint.clone(),
            stage: DaemonIngestStage::Complete,
            pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
            work_bytes_done: total_work_bytes,
            work_bytes_total: Some(total_work_bytes),
            files_done: files.len() as u64,
            files_total: Some(files.len() as u64),
            current_object_id: None,
            ssd_pressure: None,
            telemetry: None,
            resource_policy: None,
            message: Some("file ingest complete".to_string()),
        });

        Ok(summary)
    }
}

fn object_progress_event(
    job_id: &IngestJobId,
    endpoint: &StoreId,
    entry: &FileIngestEntry,
    completed_work_bytes: u64,
    total_work_bytes: u64,
    completed_files: u64,
    total_files: u64,
    progress: &ObjectPutProgress,
) -> DaemonIngestProgressEvent {
    DaemonIngestProgressEvent {
        job_id: job_id.clone(),
        endpoint: endpoint.clone(),
        stage: daemon_stage_for_object_progress(progress),
        pipeline_stage: Some(pipeline_stage_for_object_progress(progress)),
        work_bytes_done: completed_work_bytes,
        work_bytes_total: Some(total_work_bytes),
        files_done: completed_files,
        files_total: Some(total_files),
        current_object_id: Some(entry.object_id.clone()),
        ssd_pressure: None,
        telemetry: None,
        resource_policy: None,
        message: Some(entry.relative_path.to_string_lossy().to_string()),
    }
}

fn object_progress_stage_key(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest => "ssd-ingest".to_string(),
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy-{disk_id}-{copy_number}"),
    }
}

fn daemon_stage_for_object_progress(progress: &ObjectPutProgress) -> DaemonIngestStage {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest => DaemonIngestStage::SsdIngest,
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => DaemonIngestStage::HddCopy {
            disk_id: DiskId::new(disk_id).expect("object progress disk id is valid"),
            copy_number: *copy_number,
        },
    }
}

fn pipeline_stage_for_object_progress(progress: &ObjectPutProgress) -> DaemonIngestPipelineStage {
    match progress.stage {
        ObjectPutProgressStage::SsdIngest => DaemonIngestPipelineStage::SsdStage,
        ObjectPutProgressStage::HddCopy { .. } => DaemonIngestPipelineStage::HddWrite,
    }
}

fn default_ssd_root() -> PathBuf {
    std::env::var_os(SSD_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SSD_ROOT))
}

fn default_hdd_root() -> PathBuf {
    std::env::var_os(HDD_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_HDD_ROOT))
}

fn validate_known_ssd_root(path: &Path) -> Result<(), DaemonIngestFilesRuntimeError> {
    let marker = read_device_marker(path).map_err(|err| {
        DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "{} is not a known DASObjectStore SSD root: {err}",
            path.display()
        ))
    })?;
    if !marker.lines().any(|line| line == "role=ssd") {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "{} is not a DASObjectStore SSD root; expected role=ssd in .dasobjectstore/device.env",
            path.display()
        )));
    }

    Ok(())
}

fn read_device_marker(path: &Path) -> Result<String, io::Error> {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
}

fn discover_managed_hdd_roots(
    hdd_root: &Path,
) -> Result<Vec<DiskCopyRoot>, DaemonIngestFilesRuntimeError> {
    let mut roots = Vec::new();
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(roots),
        Err(err) => return Err(DaemonIngestFilesRuntimeError::Io(err)),
    };

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let root_path = entry.path();
        let marker = match read_device_marker(&root_path) {
            Ok(marker) => marker,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(DaemonIngestFilesRuntimeError::Io(err)),
        };
        let Some(disk_id) = hdd_disk_id_from_marker(&marker)? else {
            continue;
        };
        roots.push(DiskCopyRoot::new(disk_id, root_path));
    }

    roots.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(roots)
}

fn hdd_disk_id_from_marker(marker: &str) -> Result<Option<DiskId>, DaemonIngestFilesRuntimeError> {
    for line in marker.lines() {
        let Some(role) = line.strip_prefix("role=") else {
            continue;
        };
        let Some(disk_id) = role.strip_prefix("hdd:") else {
            return Ok(None);
        };
        return DiskId::new(disk_id)
            .map(Some)
            .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()));
    }

    Ok(None)
}

fn resolve_ingest_endpoint(
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

fn collect_ingest_files(
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
            });
        }
    }

    Ok(())
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

fn plan_disk_roots_for_entry(
    roots: &[DiskCopyRoot],
    entry: &FileIngestEntry,
    policy: &StorePolicy,
    copies: u8,
) -> Result<Vec<DiskCopyRoot>, DaemonIngestFilesRuntimeError> {
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
    let plan = plan_copy_count_for_store(&candidates, &request, policy, copies).map_err(|err| {
        DaemonIngestFilesRuntimeError::CommandFailed(format!("copy placement failed: {err:?}"))
    })?;
    if !plan.is_complete() {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
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
                DaemonIngestFilesRuntimeError::CommandFailed(format!(
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
) -> Result<Vec<PlacementCandidate>, DaemonIngestFilesRuntimeError> {
    roots
        .iter()
        .map(|root| {
            let capacity = measure_ssd_capacity(&root.root_path)?;
            Ok(PlacementCandidate::new(
                root.disk_id.clone(),
                None,
                capacity.available_bytes,
                HealthState::Healthy,
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

fn ingest_job_id(accepted_at_utc: &str) -> Result<IngestJobId, DaemonIngestFilesRuntimeError> {
    let job_id_value = format!(
        "ingest-files-{}",
        accepted_at_utc
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            })
            .collect::<String>()
            .trim_matches('-')
            .to_ascii_lowercase()
    );
    IngestJobId::new(job_id_value.clone())
        .map_err(|_| DaemonIngestFilesRuntimeError::InvalidJobId(job_id_value))
}

#[derive(Debug)]
pub enum DaemonIngestFilesRuntimeError {
    Io(io::Error),
    ObjectService(ObjectServiceError),
    ObjectPut(dasobjectstore_metadata::ObjectPutError),
    Capacity(dasobjectstore_metadata::SsdCapacityMeasurementError),
    InvalidJobId(String),
    CommandFailed(String),
}

impl Display for DaemonIngestFilesRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "file ingest IO failed: {err}"),
            Self::ObjectService(err) => Display::fmt(err, formatter),
            Self::ObjectPut(err) => Display::fmt(err, formatter),
            Self::Capacity(err) => Display::fmt(err, formatter),
            Self::InvalidJobId(job_id) => write!(formatter, "invalid ingest job id: {job_id}"),
            Self::CommandFailed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DaemonIngestFilesRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::ObjectService(err) => Some(err),
            Self::ObjectPut(err) => Some(err),
            Self::Capacity(err) => Some(err),
            Self::InvalidJobId(_) | Self::CommandFailed(_) => None,
        }
    }
}

impl From<io::Error> for DaemonIngestFilesRuntimeError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<ObjectServiceError> for DaemonIngestFilesRuntimeError {
    fn from(err: ObjectServiceError) -> Self {
        Self::ObjectService(err)
    }
}

impl From<dasobjectstore_metadata::ObjectPutError> for DaemonIngestFilesRuntimeError {
    fn from(err: dasobjectstore_metadata::ObjectPutError) -> Self {
        Self::ObjectPut(err)
    }
}

impl From<dasobjectstore_metadata::SsdCapacityMeasurementError> for DaemonIngestFilesRuntimeError {
    fn from(err: dasobjectstore_metadata::SsdCapacityMeasurementError) -> Self {
        Self::Capacity(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalFileIngestExecutor, SSD_ROOT_ENV};
    use crate::api::{DaemonIngestConflictPolicy, SubmitIngestFilesRequest};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_discovers_files_without_copying_payloads() {
        let root = temp_root("daemon-ingest-dry-run");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let source_root = root.join("source");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        write_device_marker(&ssd_root, "role=ssd");
        write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
        fs::create_dir_all(source_root.join("nested")).expect("source nested dir");
        fs::write(source_root.join("nested").join("sample.fastq.gz"), b"ACGT")
            .expect("source file");
        write_store_registry(&registry_path);
        fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

        let executor = LocalFileIngestExecutor {
            ssd_root: ssd_root.clone(),
            hdd_root: hdd_root.clone(),
            store_registry_path: registry_path,
            subobject_registry_path,
        };

        let response = executor
            .submit(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: source_root,
                    object_type: ObjectType::Fastq,
                    copies: Some(1),
                    conflict_policy: DaemonIngestConflictPolicy::Strict,
                    dry_run: true,
                    client_request_id: None,
                },
                "2026-07-07T10:27:12Z",
                |_| {},
            )
            .expect("dry run succeeds");

        assert_eq!(
            response.job_id.as_str(),
            "ingest-files-2026-07-07t10-27-12z"
        );
        assert!(response.dry_run);
        assert!(!ssd_root.join("ingest").exists());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn default_ssd_root_uses_environment_override() {
        let root = temp_root("daemon-ingest-env-root");
        std::env::set_var(SSD_ROOT_ENV, &root);
        assert_eq!(super::default_ssd_root(), root);
        std::env::remove_var(SSD_ROOT_ENV);
    }

    fn write_device_marker(root: &Path, marker: &str) {
        fs::create_dir_all(root.join(".dasobjectstore")).expect("device marker dir");
        fs::write(root.join(".dasobjectstore").join("device.env"), marker).expect("device marker");
    }

    fn write_store_registry(path: &Path) {
        let definition = StoreServiceDefinition {
            store_id: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
            bucket_name: Some("dos-zymo-fecal-2025-05".to_string()),
            writer_group: None,
        };
        let json = serde_json::to_string_pretty(&vec![definition]).expect("store registry json");
        fs::write(path, json).expect("store registry");
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{label}-{}-{nanos}", std::process::id()))
    }
}
