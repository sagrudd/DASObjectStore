//! Garage S3 reconciliation transfer orchestration.

use super::capacity_provider::CapacityAdmissionProvider;
use super::reconciliation::{
    plan_reconciliation, ReconciliationAction, ReconciliationEntryState, ReconciliationManifest,
    ReconciliationManifestError, ReconciliationObject,
};
use super::service::{DaemonServiceRuntimeError, GarageServiceRuntimeConfig, ServiceCommandRunner};
use crate::api::{
    DaemonIngestConflictPolicy, DaemonIngestResourceGate, DaemonIngressOrigin,
    StoreRepairS3Reconciliation, SubmitIngestFilesRequest,
};
use crate::runtime::ingest_files::resource_gate::submit_ingest_files_with_resource_gate;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_object_service::{
    bucket_name_for_definition, default_garage_credential_registry_path,
    default_store_registry_path, read_managed_credential_registry, read_store_registry,
};
use serde_json::Value;

pub(super) fn reconcile_store_s3<R: ServiceCommandRunner>(
    config: &GarageServiceRuntimeConfig,
    runner: &R,
    store_id: StoreId,
    prefix: Option<String>,
    dry_run: bool,
    accepted_at_utc: &str,
    is_cancelled: &dyn Fn() -> bool,
    capacity_provider: Option<std::sync::Arc<dyn CapacityAdmissionProvider>>,
    resource_gate: Option<std::sync::Arc<DaemonIngestResourceGate>>,
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
) -> Result<StoreRepairS3Reconciliation, DaemonServiceRuntimeError> {
    config.validate()?;
    let definitions = read_store_registry(default_store_registry_path())?;
    let definition = definitions
        .iter()
        .find(|definition| definition.store_id == store_id)
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("S3 reconciliation store {} is not registered", store_id),
        })?;
    let bucket_name = bucket_name_for_definition(definition)?;
    let stage_name = accepted_at_utc
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let staging_path = crate::runtime::default_ssd_root()
        .join(".dasobjectstore")
        .join("remote-s3-reconcile")
        .join(store_id.as_str())
        .join(stage_name);
    let manifest_path = staging_path
        .join(".dasobjectstore")
        .join("reconciliation-manifest.json");
    if dry_run {
        return Ok(StoreRepairS3Reconciliation {
            bucket_name,
            prefix,
            staging_path: staging_path.display().to_string(),
            manifest_path: Some(manifest_path.display().to_string()),
            ingest_job_id: None,
            dry_run: true,
        });
    }

    let credential_registry = read_managed_credential_registry(
        default_garage_credential_registry_path(),
        accepted_at_utc,
    )?;
    let credential = credential_registry
        .credentials
        .iter()
        .find(|credential| credential.store_id == store_id && credential.bucket_name == bucket_name)
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "S3 reconciliation requires provisioned Garage credentials for {}",
                store_id
            ),
        })?;
    std::fs::create_dir_all(&staging_path).map_err(|error| {
        DaemonServiceRuntimeError::CommandIo {
            program: "create remote S3 staging directory".to_string(),
            message: error.to_string(),
        }
    })?;
    let environment = vec![
        (
            "AWS_ACCESS_KEY_ID".to_string(),
            credential.access_key_id.clone(),
        ),
        (
            "AWS_SECRET_ACCESS_KEY".to_string(),
            credential.secret_access_key.clone(),
        ),
        ("AWS_DEFAULT_REGION".to_string(), "garage".to_string()),
    ];
    let mut manifest = if manifest_path.exists() {
        ReconciliationManifest::load(&manifest_path).map_err(reconciliation_manifest_error)?
    } else {
        ReconciliationManifest::new(store_id.as_str(), prefix.clone())
    };
    if manifest.store_id != store_id.as_str() || manifest.prefix != prefix {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation manifest identity mismatch at {}",
                manifest_path.display()
            ),
        });
    }
    let objects = list_garage_objects(
        runner,
        &config.endpoint,
        &bucket_name,
        prefix.as_deref(),
        &environment,
    )?;
    let plan = plan_reconciliation(&mut manifest, &objects);
    if let Some(action) = plan.actions.iter().find(|action| {
        matches!(
            action,
            ReconciliationAction::InvalidKey { .. } | ReconciliationAction::Collision { .. }
        )
    }) {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("reconciliation key rejected: {action:?}"),
        });
    }
    manifest
        .save_atomic(&manifest_path)
        .map_err(reconciliation_manifest_error)?;
    let total = plan.actions.len();
    for (index, action) in plan.actions.iter().enumerate() {
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
                let size_bytes = size_bytes.unwrap_or_default();
                manifest
                    .checkpoint(
                        &manifest_path,
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
                    std::fs::create_dir_all(parent).map_err(|error| {
                        DaemonServiceRuntimeError::CommandIo {
                            program: "create reconciliation object directory".to_string(),
                            message: error.to_string(),
                        }
                    })?;
                }
                let args = vec![
                    "--endpoint-url".to_string(),
                    config.endpoint.clone(),
                    "s3".to_string(),
                    "cp".to_string(),
                    format!("s3://{bucket_name}/{key}"),
                    destination.display().to_string(),
                    "--no-progress".to_string(),
                ];
                if let Err(error) =
                    runner.run_with_display_args_and_env("aws", &args, &args, &environment)
                {
                    manifest
                        .checkpoint(
                            &manifest_path,
                            key,
                            ReconciliationEntryState::Failed,
                            Some(error.to_string()),
                            0,
                        )
                        .map_err(reconciliation_manifest_error)?;
                    return Err(error);
                }
                manifest
                    .checkpoint(
                        &manifest_path,
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
    let ingest = submit_ingest_files_with_resource_gate(
        SubmitIngestFilesRequest {
            endpoint: store_id,
            source_path: staging_path.clone(),
            object_type: ObjectType::Naive,
            copies: None,
            hdd_workers: None,
            ingress_origin: DaemonIngressOrigin::RemoteS3,
            conflict_policy: DaemonIngestConflictPolicy::Lazy,
            dry_run: false,
            client_request_id: Some(format!("garage-reconcile-{accepted_at_utc}")),
        },
        accepted_at_utc,
        emit_progress,
        capacity_provider,
        resource_gate,
    )
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("S3 reconciliation ingest failed: {error}"),
    })?;
    Ok(StoreRepairS3Reconciliation {
        bucket_name,
        prefix,
        staging_path: staging_path.display().to_string(),
        manifest_path: Some(manifest_path.display().to_string()),
        ingest_job_id: Some(ingest.job_id.to_string()),
        dry_run: false,
    })
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
