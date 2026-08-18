use super::*;
use crate::api::{
    DaemonApiErrorResponse, DaemonApiResponse, ProviderStreamChunkHeader,
    ProviderStreamMultipartPartUploadOpenRequest, ProviderStreamMultipartPartUploadResponse,
    ProviderStreamOpenRequest, ProviderStreamUploadOpenRequest, ProviderStreamUploadResponse,
    ProviderStreamVerifier,
};
use crate::server::unix_socket::UnixSocketDaemonServerError;
use dasobjectstore_core::backend::ObjectStoreBackend;
use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_core::store::AcknowledgementPolicy;
use dasobjectstore_metadata::{
    commit_verified_ssd_and_enqueue_with_capacity_claims, read_destage, DestageState,
    VerifiedSsdCommitRequest,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

const AFTER_HDD_ACK_DEADLINE: Duration = Duration::from_secs(300);
const AFTER_HDD_ACK_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn projection_runtime_error(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("Synoptikon projection denied: {}", message.into()),
    }
}

fn canonical_digest(value: &impl Serialize) -> Result<String, DaemonServiceRuntimeError> {
    let bytes = serde_jcs::to_vec(value)
        .map_err(|error| projection_runtime_error(format!("canonical encode failed: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(unix)]
fn hdd_target_has_capacity(path: &std::path::Path, bytes: u64) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return false;
    }
    let stats = unsafe { stats.assume_init() };
    (stats.f_bavail as u128).saturating_mul(stats.f_frsize as u128) >= bytes as u128
        && stats.f_bavail != 0
}

#[cfg(not(unix))]
fn hdd_target_has_capacity(_: &std::path::Path, _: u64) -> bool {
    false
}

fn probe_fixed_synoptikon_tls() -> Result<String, DaemonServiceRuntimeError> {
    use dasobjectstore_core::{
        SYNOPTIKON_PROJECTION_ENDPOINT, SYNOPTIKON_PROJECTION_TLS_CERTIFICATE_PATH,
        SYNOPTIKON_PROJECTION_TLS_EXPECTATION_PATH,
    };
    let expected =
        std::fs::read_to_string(SYNOPTIKON_PROJECTION_TLS_EXPECTATION_PATH).map_err(|error| {
            projection_runtime_error(format!("TLS expectation unavailable: {error}"))
        })?;
    let expected = expected.trim();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(projection_runtime_error("TLS expectation is invalid"));
    }
    let certificate =
        std::fs::read(SYNOPTIKON_PROJECTION_TLS_CERTIFICATE_PATH).map_err(|error| {
            projection_runtime_error(format!("TLS certificate unavailable: {error}"))
        })?;
    let certificate_sha256 = format!("{:x}", Sha256::digest(&certificate));
    if certificate_sha256 != expected {
        return Err(projection_runtime_error(
            "TLS certificate differs from protected expectation",
        ));
    }
    let certificate = reqwest::Certificate::from_pem(&certificate)
        .map_err(|error| projection_runtime_error(format!("TLS certificate invalid: {error}")))?;
    let client = reqwest::blocking::Client::builder()
        .add_root_certificate(certificate)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| projection_runtime_error(format!("TLS probe unavailable: {error}")))?;
    let response = client
        .get(format!(
            "{SYNOPTIKON_PROJECTION_ENDPOINT}/.well-known/dasobjectstore/appliance-ca.pem"
        ))
        .send()
        .map_err(|error| projection_runtime_error(format!("TLS endpoint unavailable: {error}")))?;
    if !response.status().is_success()
        || response
            .headers()
            .get("x-dasobjectstore-certificate-sha256")
            .and_then(|value| value.to_str().ok())
            != Some(expected)
    {
        return Err(projection_runtime_error(
            "TLS endpoint identity was not proven",
        ));
    }
    Ok(expected.to_ascii_lowercase())
}

#[derive(Clone)]
pub(super) struct MultipartCompletionWorkerContext {
    pub live_sqlite_path: std::path::PathBuf,
    pub hdd_root_path: std::path::PathBuf,
    pub accepted_at_utc: String,
}

pub(super) fn publish_profile_s3_catalogue_at(
    store_id: &StoreId,
    backend: &FolderBackend,
    live_sqlite_path: &std::path::Path,
    committed_at_utc: &str,
) -> Result<(), dasobjectstore_core::backend::BackendError> {
    let profile_namespace = format!("profile-s3:{}", store_id.as_str());
    crate::runtime::publish_profile_catalogue_with_metadata(
        store_id,
        backend,
        live_sqlite_path,
        backend
            .root()
            .join(".dasobjectstore/profile-catalogue-handoffs"),
        &profile_namespace,
        committed_at_utc,
    )
    .map(|_| ())
}

pub(super) fn commit_profile_s3_acceptance_at(
    live_sqlite_path: &std::path::Path,
    hdd_root_path: &std::path::Path,
    committed_at_utc: &str,
    definition: &dasobjectstore_object_service::StoreServiceDefinition,
    binding: &crate::runtime::BackendProfileBinding,
    backend: &FolderBackend,
    record: &dasobjectstore_core::backend::BackendObjectRecord,
    upload_id: &str,
) -> Result<(), String> {
    let object_id = match dasobjectstore_metadata::read_s3_object_binding(
        live_sqlite_path,
        &definition.store_id,
        &record.key.object_id,
        record.key.version,
    )
    .map_err(|error| error.to_string())?
    {
        Some(existing) => existing.object_id,
        None => ObjectId::new(format!(
            "{}/{}",
            definition.store_id.as_str(),
            record.key.object_id
        ))
        .map_err(|error| error.to_string())?,
    };
    let managed_ssd_root = binding
        .ssd_staging_root
        .as_deref()
        .unwrap_or(&binding.backend_root);
    let payload_path = backend
        .root()
        .join(".dasobjectstore/objects")
        .join(&record.key.object_id);
    let relative_path = payload_path
        .strip_prefix(managed_ssd_root)
        .map_err(|_| "direct S3 payload escaped its authoritative managed SSD root".to_string())?
        .to_string_lossy()
        .into_owned();
    let acknowledgement_policy = match definition.policy.acknowledgement_policy {
        AcknowledgementPolicy::AfterSsdIngest => "after_ssd_ingest",
        AcknowledgementPolicy::AfterHddPlacement => "after_hdd_placement",
    };
    let destage_job_id = format!("destage-direct-s3-{upload_id}");
    let capacity_request = crate::runtime::build_destage_capacity_claim(
        live_sqlite_path,
        hdd_root_path,
        &object_id,
        &destage_job_id,
        definition.policy.copies,
        record.size_bytes,
        &record.checksum,
        committed_at_utc,
    )?;
    commit_verified_ssd_and_enqueue_with_capacity_claims(
        live_sqlite_path,
        VerifiedSsdCommitRequest {
            destage_job_id: &destage_job_id,
            store_id: &definition.store_id,
            object_id: &object_id,
            object_type: dasobjectstore_core::object_type::ObjectType::Naive.name(),
            relative_path: &relative_path,
            size_bytes: record.size_bytes,
            content_hash_algorithm: "sha256",
            content_hash: record.checksum.trim_start_matches("sha256:"),
            acknowledgement_policy,
            required_copy_count: definition.policy.copies,
            max_attempts: 8,
            priority: 0,
            committed_at_utc,
            ingest_job_id: Some(&format!("ingest-direct-s3-{upload_id}")),
            ingress_origin: Some("remote_s3"),
            s3_key: Some(&record.key.object_id),
            s3_version: record.key.version,
        },
        &capacity_request,
    )
    .map_err(|error| error.to_string())?;
    publish_profile_s3_catalogue_at(
        &definition.store_id,
        backend,
        live_sqlite_path,
        committed_at_utc,
    )
    .map_err(|error| error.to_string())?;
    if definition.policy.acknowledgement_policy == AcknowledgementPolicy::AfterHddPlacement {
        let deadline = Instant::now() + AFTER_HDD_ACK_DEADLINE;
        loop {
            let state = read_destage(live_sqlite_path, &object_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "durable HDD acknowledgement job disappeared".to_string())?;
            match state.state {
                DestageState::HddCopyVerified
                    if state.verified_copy_count >= state.required_copy_count =>
                {
                    break;
                }
                DestageState::DestageFailed
                | DestageState::NeedsReview
                | DestageState::Cancelled => {
                    return Err(format!(
                        "HDD placement did not satisfy acknowledgement policy: {:?}: {}",
                        state.state,
                        state.last_error.as_deref().unwrap_or("no detail")
                    ));
                }
                _ if Instant::now() >= deadline => {
                    return Err(
                        "HDD placement acknowledgement exceeded its 300 second deadline"
                            .to_string(),
                    );
                }
                _ => std::thread::sleep(AFTER_HDD_ACK_POLL_INTERVAL),
            }
        }
    }
    Ok(())
}

pub(crate) struct ProviderStreamSource {
    pub reader: Box<dyn Read + Send>,
    pub expected_size_bytes: u64,
    pub expected_checksum: Option<String>,
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    pub(super) fn derive_synoptikon_projection_settlement(
        &self,
        request: &dasobjectstore_core::SynoptikonProjectionRequestV1,
        authority_sequence: u64,
        now: u64,
    ) -> Result<dasobjectstore_core::SynoptikonProjectionSettlementV1, DaemonServiceRuntimeError>
    {
        use dasobjectstore_core::{
            authenticate_das_owned_synoptikon_projection_readiness, settle_synoptikon_projection,
            verify_das_owned_synoptikon_projection_readiness, DasCatalogueMappingEvidenceV1,
            DasCatalogueObjectEvidenceV1, DasHddReplicaEvidenceV1,
            DasProviderGroupStatusEvidenceV1, DasUploadCompletionEvidenceV1,
            SynoptikonProjectionReadinessV1, SYNOPTIKON_PROJECTION_ENDPOINT,
            SYNOPTIKON_PROJECTION_READINESS_V1_SCHEMA,
        };
        let store_id = StoreId::new(request.object_store_id.clone())
            .map_err(|error| projection_runtime_error(error.to_string()))?;
        let binding = dasobjectstore_metadata::read_s3_object_binding(
            &self.live_sqlite_path,
            &store_id,
            &request.object_key,
            request.object_version,
        )
        .map_err(|error| projection_runtime_error(error.to_string()))?
        .ok_or_else(|| projection_runtime_error("catalogue binding is absent"))?;
        if binding.object_id.as_str() != request.object_id
            || binding.size_bytes != request.source_size_bytes
            || !binding
                .checksum
                .eq_ignore_ascii_case(&request.source_sha256)
        {
            return Err(projection_runtime_error(
                "catalogue binding differs from intent",
            ));
        }
        let destage = read_destage(&self.live_sqlite_path, &binding.object_id)
            .map_err(|error| projection_runtime_error(error.to_string()))?
            .ok_or_else(|| projection_runtime_error("destage authority is absent"))?;
        if destage.state != DestageState::HddCopyVerified
            || destage.verified_copy_count < destage.required_copy_count
        {
            return Err(projection_runtime_error("HDD settlement is incomplete"));
        }
        let connection = Connection::open_with_flags(
            &self.live_sqlite_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| projection_runtime_error(error.to_string()))?;
        let upload_id: String = connection
            .query_row(
                "SELECT ingest_job_id FROM ingest_jobs WHERE object_id=?1 AND state IN ('completed','hdd_copy_verified') ORDER BY rowid DESC LIMIT 1",
                [binding.object_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| projection_runtime_error(error.to_string()))?
            .ok_or_else(|| projection_runtime_error("SSD ingress receipt is absent"))?;
        let mut statement = connection
            .prepare("SELECT placement_id,disk_id,content_hash,verified_at_utc FROM placements WHERE object_id=?1 ORDER BY placement_id")
            .map_err(|error| projection_runtime_error(error.to_string()))?;
        let rows = statement
            .query_map([binding.object_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|error| projection_runtime_error(error.to_string()))?;
        let mut replicas = Vec::new();
        for row in rows {
            let (placement_id, disk_id, checksum, verified_at) =
                row.map_err(|error| projection_runtime_error(error.to_string()))?;
            if verified_at.is_none()
                || checksum.as_deref() != Some(request.source_sha256.as_str())
                || !hdd_target_has_capacity(
                    &self.hdd_root_path.join(&disk_id),
                    request.source_size_bytes,
                )
            {
                return Err(projection_runtime_error(
                    "HDD placement is unhealthy or full",
                ));
            }
            replicas.push(DasHddReplicaEvidenceV1 {
                replica_id: placement_id.clone(),
                placement_sha256: canonical_digest(&(
                    placement_id,
                    disk_id,
                    checksum,
                    verified_at,
                ))?,
                verified_size_bytes: request.source_size_bytes,
                verified_sha256: request.source_sha256.clone(),
                disposition: "hdd_verified".to_owned(),
            });
        }
        if replicas.is_empty() {
            return Err(projection_runtime_error("verified HDD placement is absent"));
        }
        let ambiguous: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM logical_identity_reviews WHERE state='needs_review'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| projection_runtime_error(error.to_string()))?;
        if ambiguous != 0 {
            return Err(projection_runtime_error(format!(
                "catalogue has {ambiguous} ambiguous unmapped objects"
            )));
        }
        let snapshot_sha256 = canonical_digest(&(
            &request.object_store_id,
            &request.object_id,
            request.object_version,
            &request.object_key,
            request.source_size_bytes,
            &request.source_sha256,
            &replicas,
            ambiguous,
        ))?;
        let tls_sha256 = probe_fixed_synoptikon_tls()?;
        let upload_receipt_sha256 = canonical_digest(&(
            upload_id.as_str(),
            binding.object_id.as_str(),
            request.source_size_bytes,
            request.source_sha256.as_str(),
        ))?;
        let provider_status_sha256 = canonical_digest(&(
            binding.object_id.as_str(),
            destage.required_copy_count,
            destage.verified_copy_count,
            "hdd_settled",
        ))?;
        let settlement_reference = canonical_digest(&(
            binding.object_id.as_str(),
            &replicas,
            provider_status_sha256.as_str(),
        ))?;
        let readiness = SynoptikonProjectionReadinessV1 {
            schema_version: SYNOPTIKON_PROJECTION_READINESS_V1_SCHEMA.to_owned(),
            projection_id: request.projection_id.clone(),
            generation: request.generation,
            source_sha256: request.source_sha256.clone(),
            nonce: request.nonce.clone(),
            authority_sequence,
            endpoint_url: SYNOPTIKON_PROJECTION_ENDPOINT.to_owned(),
            expected_tls_peer_certificate_sha256: tls_sha256.clone(),
            observed_tls_peer_certificate_sha256: tls_sha256,
            daemon_ready: true,
            s3_endpoint_ready: true,
            catalogue_current: true,
            upload_completion: DasUploadCompletionEvidenceV1 {
                receipt_id: upload_id.clone(),
                receipt_sha256: upload_receipt_sha256,
                upload_id,
                source_size_bytes: request.source_size_bytes,
                source_sha256: request.source_sha256.clone(),
                disposition: "committed".to_owned(),
            },
            catalogue_object: DasCatalogueObjectEvidenceV1 {
                snapshot_sha256: snapshot_sha256.clone(),
                object_store_id: request.object_store_id.clone(),
                object_id: request.object_id.clone(),
                object_version: request.object_version,
                object_key: request.object_key.clone(),
                source_size_bytes: request.source_size_bytes,
                source_sha256: request.source_sha256.clone(),
            },
            provider_group_status: DasProviderGroupStatusEvidenceV1 {
                status_sha256: provider_status_sha256,
                object_store_id: request.object_store_id.clone(),
                object_id: request.object_id.clone(),
                object_version: request.object_version,
                settled: true,
            },
            hdd_replicas: replicas,
            hdd_settlement_reference_sha256: settlement_reference,
            catalogue_mapping: DasCatalogueMappingEvidenceV1 {
                snapshot_sha256,
                ambiguous_unmapped_objects: 0,
                observed_at_unix_seconds: now,
            },
            mapping_exclusion: None,
            observed_at_unix_seconds: now,
            expires_at_unix_seconds: now.saturating_add(60).min(request.expires_at_unix_seconds),
        };
        let authenticated = authenticate_das_owned_synoptikon_projection_readiness(readiness)
            .map_err(|error| projection_runtime_error(error.to_string()))?;
        let verified = verify_das_owned_synoptikon_projection_readiness(&authenticated)
            .map_err(|error| projection_runtime_error(error.to_string()))?;
        settle_synoptikon_projection(request, &verified, now, None)
            .map(|outcome| outcome.settlement)
            .map_err(|error| projection_runtime_error(error.to_string()))
    }
    pub(super) fn publish_profile_s3_catalogue(
        &self,
        store_id: &StoreId,
        backend: &FolderBackend,
    ) -> Result<(), dasobjectstore_core::backend::BackendError> {
        publish_profile_s3_catalogue_at(
            store_id,
            backend,
            &self.live_sqlite_path,
            &self.clock.now_utc(),
        )
    }

    pub(super) fn commit_profile_s3_acceptance(
        &self,
        definition: &dasobjectstore_object_service::StoreServiceDefinition,
        binding: &crate::runtime::BackendProfileBinding,
        backend: &FolderBackend,
        record: &dasobjectstore_core::backend::BackendObjectRecord,
        upload_id: &str,
    ) -> Result<(), String> {
        commit_profile_s3_acceptance_at(
            &self.live_sqlite_path,
            &self.hdd_root_path,
            &self.clock.now_utc(),
            definition,
            binding,
            backend,
            record,
            upload_id,
        )
    }

    pub(super) fn multipart_completion_worker_context(&self) -> MultipartCompletionWorkerContext {
        MultipartCompletionWorkerContext {
            live_sqlite_path: self.live_sqlite_path.clone(),
            hdd_root_path: self.hdd_root_path.clone(),
            accepted_at_utc: self.clock.now_utc(),
        }
    }

    pub(crate) fn handle_provider_stream_multipart_part_upload_for_actor(
        &self,
        request: ProviderStreamMultipartPartUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let mut request = request;
        let authorized = match self.authorize_endpoint_write_scope(actor, &request.store_id) {
            Ok(authorized) => authorized,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )))
            }
        };
        request.object = authorized.qualify_object(&request.object);
        let store_id = authorized.store_id.clone();
        let binding =
            match read_profile_binding(&self.profile_binding_registry_path, store_id.as_str()) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        "multipart part staging requires a registered bounded folder profile",
                    )))
                }
            };
        let (backend_root, _) = match crate::runtime::direct_s3_profile_backend(&binding) {
            Ok(specification) => specification,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    error.to_string(),
                )))
            }
        };
        let mut journal = match crate::runtime::MultipartPartJournal::open(backend_root, &request) {
            Ok(journal) => journal,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_failed",
                    error.to_string(),
                )))
            }
        };
        let previous_reserved_bytes = journal.staged_bytes();
        let duplicate_part = journal.contains_part(request.part_number);
        let admitted = previous_reserved_bytes != 0;
        let requested_reservation_bytes = if duplicate_part {
            previous_reserved_bytes
        } else {
            match previous_reserved_bytes.checked_add(request.expected_size_bytes) {
                Some(bytes) => bytes,
                None => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_multipart_rejected",
                        "multipart reservation size overflow",
                    )))
                }
            }
        };
        if !admitted {
            let Some(provider) = self.service_orchestrator.capacity_provider() else {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_unavailable",
                    "multipart part staging requires daemon capacity admission",
                )));
            };
            let admission = match authorized.subobject.as_deref() {
                Some(subobject) => provider.admit_subobject_ingest(
                    store_id.as_str(),
                    subobject,
                    requested_reservation_bytes,
                    1,
                    crate::api::DaemonIngressOrigin::RemoteS3,
                    &request.reservation_id,
                ),
                None => provider.admit_remote_upload(
                    store_id.as_str(),
                    requested_reservation_bytes,
                    &request.reservation_id,
                ),
            };
            let admission = match admission {
                Ok(admission) => admission,
                Err(error) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_multipart_failed",
                        error.to_string(),
                    )))
                }
            };
            if admission.decision != crate::api::CapacityAdmissionDecision::Admitted {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_rejected",
                    admission
                        .message
                        .unwrap_or_else(|| "multipart capacity admission rejected".to_string()),
                )));
            }
        } else if !duplicate_part {
            let Some(provider) = self.service_orchestrator.capacity_provider() else {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_unavailable",
                    "multipart reservation growth requires daemon capacity admission",
                )));
            };
            if let Err(error) = provider.resize_remote_upload(
                &store_id,
                authorized.subobject.as_deref(),
                &request.reservation_id,
                requested_reservation_bytes,
            ) {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_rejected",
                    error.to_string(),
                )));
            }
        }
        if !duplicate_part {
            if let Err(error) = journal.resize_reservation(requested_reservation_bytes) {
                if let Some(provider) = self.service_orchestrator.capacity_provider() {
                    if admitted {
                        let _ = provider.resize_remote_upload(
                            &store_id,
                            authorized.subobject.as_deref(),
                            &request.reservation_id,
                            previous_reserved_bytes,
                        );
                    } else {
                        let _ = match authorized.subobject.as_deref() {
                            Some(subobject) => provider.release_subobject(
                                &store_id,
                                subobject,
                                &request.reservation_id,
                            ),
                            None => provider.release(&store_id, &request.reservation_id),
                        };
                    }
                }
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_failed",
                    error.to_string(),
                )));
            }
        }
        let part = match journal.stage_part(&request, &mut || {
            read_frame()
                .map_err(|error| crate::runtime::MultipartPartJournalError::Io(error.to_string()))
        }) {
            Ok(part) => part,
            Err(error) => {
                if !admitted {
                    if let Some(provider) = self.service_orchestrator.capacity_provider() {
                        let _ = match authorized.subobject.as_deref() {
                            Some(subobject) => provider.release_subobject(
                                &store_id,
                                subobject,
                                &request.reservation_id,
                            ),
                            None => provider.release(&store_id, &request.reservation_id),
                        };
                    }
                } else if !duplicate_part {
                    if let Some(provider) = self.service_orchestrator.capacity_provider() {
                        let _ = provider.resize_remote_upload(
                            &store_id,
                            authorized.subobject.as_deref(),
                            &request.reservation_id,
                            previous_reserved_bytes,
                        );
                    }
                    let _ = journal.resize_reservation(previous_reserved_bytes);
                }
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_failed",
                    error.to_string(),
                )));
            }
        };
        emit_response(DaemonApiResponse::ProviderStreamMultipartPartUpload(
            ProviderStreamMultipartPartUploadResponse {
                schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request.request_id,
                reservation_id: request.reservation_id,
                part_number: part.part_number,
                store_id,
                object: request.object,
                size_bytes: part.size_bytes,
                sha256: part.checksum,
            },
        ))
    }

    pub(crate) fn handle_provider_stream_upload_for_actor(
        &self,
        request: ProviderStreamUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let mut request = request;
        let authorized = match if request.retained_dossier.is_some() {
            self.authorize_expedition_retained_dossier_write(actor, &request)
        } else if request.synoptikon_projection.is_some() {
            self.authorize_synoptikon_projection_write(actor, &request)
        } else {
            self.authorize_endpoint_write_scope(actor, &request.store_id)
        } {
            Ok(authorized) => authorized,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )))
            }
        };
        request.object = authorized.qualify_object(&request.object);
        let store_id = authorized.store_id.clone();
        let binding =
            match read_profile_binding(&self.profile_binding_registry_path, store_id.as_str()) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        "provider stream upload requires a registered bounded folder profile",
                    )))
                }
            };
        let (backend_root, backend_manifest) =
            match crate::runtime::direct_s3_profile_backend(&binding) {
                Ok(specification) => specification,
                Err(error) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        error.to_string(),
                    )))
                }
            };
        let definition = match read_store_registry(&self.store_registry_path) {
            Ok(definitions) => definitions
                .into_iter()
                .find(|definition| definition.store_id == store_id),
            Err(_) => None,
        };
        let Some(definition) = definition else {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "profile capacity policy is unavailable",
            )));
        };
        match dasobjectstore_metadata::read_s3_object_binding(
            &self.live_sqlite_path,
            &store_id,
            &request.object.object_id,
            request.object.version,
        ) {
            Ok(Some(existing))
                if request.retained_dossier.is_none()
                    && existing.size_bytes == request.expected_size_bytes
                    && existing
                        .checksum
                        .eq_ignore_ascii_case(&request.expected_sha256) =>
            {
                if request.synoptikon_projection.is_some() {
                    if let Err(error) = crate::runtime::mark_projection_uploaded(
                        &self.synoptikon_projection_ledger_path,
                        &request.upload_id,
                    ) {
                        return emit_response(DaemonApiResponse::Error(
                            DaemonApiErrorResponse::new(
                                "projection_receipt_commit_failed",
                                error.to_string(),
                            ),
                        ));
                    }
                }
                return emit_response(DaemonApiResponse::ProviderStreamUpload(
                    ProviderStreamUploadResponse {
                        schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                        upload_id: request.upload_id,
                        store_id,
                        object: request.object,
                        size_bytes: existing.size_bytes,
                        sha256: existing.checksum,
                        retained_dossier: None,
                    },
                ));
            }
            Ok(Some(existing))
                if existing.size_bytes == request.expected_size_bytes
                    && existing
                        .checksum
                        .eq_ignore_ascii_case(&request.expected_sha256) => {}
            Ok(Some(_)) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_object_conflict",
                    "the authoritative S3 key already exists with different content",
                )));
            }
            Ok(None) => {}
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_preflight_failed",
                    error.to_string(),
                )));
            }
        }
        let capacity = crate::runtime::direct_s3_profile_capacity(
            &binding,
            definition.policy.capacity.clone(),
        );
        let mut backend = match FolderBackend::open(backend_root, backend_manifest, capacity, 0) {
            Ok(backend) => backend,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    error.to_string(),
                )))
            }
        };
        match crate::runtime::head_profile_object(&backend, &request.object) {
            Ok(existing)
                if existing.size_bytes == request.expected_size_bytes
                    && existing
                        .checksum
                        .eq_ignore_ascii_case(&request.expected_sha256) =>
            {
                let record = match backend.records().and_then(|records| {
                    records
                        .into_iter()
                        .find(|record| record.key == existing.key)
                        .ok_or_else(|| {
                            dasobjectstore_core::backend::BackendError::NotFound(
                                existing.key.object_id.clone(),
                            )
                        })
                }) {
                    Ok(record) => record,
                    Err(error) => {
                        return emit_response(DaemonApiResponse::Error(
                            DaemonApiErrorResponse::new(
                                "provider_stream_preflight_failed",
                                error.to_string(),
                            ),
                        ))
                    }
                };
                if let Err(error) = self.commit_profile_s3_acceptance(
                    &definition,
                    &binding,
                    &backend,
                    &record,
                    &request.upload_id,
                ) {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_destage_publication_failed",
                        error,
                    )));
                }
                let response = match retained_dossier_upload_response(
                    &request,
                    store_id,
                    &backend,
                    &record,
                    &self.clock.now_utc(),
                ) {
                    Ok(response) => response,
                    Err(response) => return emit_response(response),
                };
                if request.synoptikon_projection.is_some() {
                    if let Err(error) = crate::runtime::mark_projection_uploaded(
                        &self.synoptikon_projection_ledger_path,
                        &request.upload_id,
                    ) {
                        return emit_response(DaemonApiResponse::Error(
                            DaemonApiErrorResponse::new(
                                "projection_receipt_commit_failed",
                                error.to_string(),
                            ),
                        ));
                    }
                }
                return emit_response(DaemonApiResponse::ProviderStreamUpload(response));
            }
            Ok(_) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_object_conflict",
                    "an accepted object already exists with different content",
                )))
            }
            Err(dasobjectstore_core::backend::BackendError::NotFound(_)) => {}
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_preflight_failed",
                    error.to_string(),
                )))
            }
        }
        let Some(provider) = self.service_orchestrator.capacity_provider() else {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_upload_unavailable",
                "provider stream upload requires daemon capacity admission",
            )));
        };
        let mut source = ProviderUploadReader::new(&request, read_frame);
        let record = crate::runtime::put_profile_object_with_capacity_scope(
            provider.as_ref(),
            store_id.as_str(),
            authorized.subobject.as_deref(),
            &mut backend,
            &request.upload_id,
            &request.object,
            &mut source,
            request.expected_size_bytes,
        );
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_upload_failed",
                    error.to_string(),
                )))
            }
        };
        if let Err(error) = self.commit_profile_s3_acceptance(
            &definition,
            &binding,
            &backend,
            &record,
            &request.upload_id,
        ) {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_destage_publication_failed",
                error,
            )));
        }
        if request.synoptikon_projection.is_some() {
            if let Err(error) = crate::runtime::mark_projection_uploaded(
                &self.synoptikon_projection_ledger_path,
                &request.upload_id,
            ) {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "projection_receipt_commit_failed",
                    error.to_string(),
                )));
            }
        }
        let response = match retained_dossier_upload_response(
            &request,
            store_id,
            &backend,
            &record,
            &self.clock.now_utc(),
        ) {
            Ok(response) => response,
            Err(response) => return emit_response(response),
        };
        emit_response(DaemonApiResponse::ProviderStreamUpload(response))
    }

    /// Open a catalogue-authoritative profile object for the Unix-socket
    /// provider stream. The returned reader never exposes a backend path; the
    /// transport owns chunking and cumulative verification.
    pub(crate) fn open_provider_stream(
        &self,
        request: &ProviderStreamOpenRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<ProviderStreamSource, DaemonApiResponse> {
        let application_capability = request.application_capability.as_ref();
        let store_id = super::ergasterion::authorize_provider_store(self, request, actor)?;
        let native = dasobjectstore_metadata::read_s3_object_binding(
            &self.live_sqlite_path,
            &store_id,
            &request.object.object_id,
            request.object.version,
        )
        .map_err(|error| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                error.to_string(),
            ))
        })?;
        if let Some(native) = native {
            if let Some(capability) = application_capability {
                super::ergasterion::authorize_provider_read(
                    self,
                    capability,
                    store_id.as_str(),
                    &request.object.object_id,
                    native.size_bytes,
                )?;
            }
            if request
                .condition
                .if_match_sha256
                .as_deref()
                .is_some_and(|checksum| !checksum.eq_ignore_ascii_case(&native.checksum))
            {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_precondition_failed",
                    "if_match_sha256 does not match the catalogue checksum",
                )));
            }
            if request
                .condition
                .if_none_match_sha256
                .as_deref()
                .is_some_and(|checksum| checksum.eq_ignore_ascii_case(&native.checksum))
            {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_not_modified",
                    "if_none_match_sha256 matches the catalogue checksum",
                )));
            }
            let resolved = resolve_object_download_with_hdd_root(
                &self.live_sqlite_path,
                &self.hdd_root_path,
                &store_id,
                &ObjectDownloadRequest {
                    endpoint: store_id.clone(),
                    object_id: native.object_id,
                    delegated_actor: request.delegated_actor.clone(),
                    verified_subject: None,
                },
            )
            .map_err(|error| {
                DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    format!("catalogued object has no readable verified placement: {error}"),
                ))
            })?;
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let mut file = options.open(&resolved.source_path).map_err(|error| {
                DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    format!("verified placement could not be opened: {error}"),
                ))
            })?;
            let actual_size = file
                .metadata()
                .map_err(|error| {
                    DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        format!("verified placement could not be inspected: {error}"),
                    ))
                })?
                .len();
            if actual_size != native.size_bytes {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    "verified placement size no longer matches the authoritative catalogue",
                )));
            }
            if let Some(range) = request.range {
                if range.start > native.size_bytes {
                    return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_invalid_range",
                        "provider stream range starts beyond the catalogue object",
                    )));
                }
                let end = range
                    .end_exclusive
                    .unwrap_or(native.size_bytes)
                    .min(native.size_bytes);
                if end < range.start {
                    return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_invalid_range",
                        "provider stream range ends before it starts",
                    )));
                }
                file.seek(SeekFrom::Start(range.start)).map_err(|error| {
                    DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        format!("verified placement range could not be opened: {error}"),
                    ))
                })?;
                return Ok(ProviderStreamSource {
                    reader: Box::new(file.take(end - range.start)),
                    expected_size_bytes: end - range.start,
                    expected_checksum: None,
                });
            }
            return Ok(ProviderStreamSource {
                reader: Box::new(file),
                expected_size_bytes: native.size_bytes,
                expected_checksum: Some(native.checksum),
            });
        }
        let binding =
            match read_profile_binding(&self.profile_binding_registry_path, store_id.as_str()) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        "provider stream requires a registered bounded folder profile",
                    )))
                }
            };
        let (backend_root, backend_manifest) = crate::runtime::direct_s3_profile_backend(&binding)
            .map_err(|error| {
                DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    error.to_string(),
                ))
            })?;
        let capacity = match read_store_registry(&self.store_registry_path) {
            Ok(definitions) => definitions
                .into_iter()
                .find(|definition| definition.store_id == store_id)
                .map(|definition| definition.policy.capacity),
            Err(_) => None,
        };
        let Some(capacity) = capacity else {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "profile capacity policy is unavailable",
            )));
        };
        let capacity = crate::runtime::direct_s3_profile_capacity(&binding, capacity);
        let backend = match FolderBackend::open(backend_root, backend_manifest, capacity, 0) {
            Ok(backend) => backend,
            Err(error) => {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    error.to_string(),
                )))
            }
        };
        let object = match head_profile_object(&backend, &request.object) {
            Ok(object) => object,
            Err(error) => {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_head_failed",
                    error.to_string(),
                )))
            }
        };
        if let Some(capability) = application_capability {
            super::ergasterion::authorize_provider_read(
                self,
                capability,
                store_id.as_str(),
                &request.object.object_id,
                object.size_bytes,
            )?;
        }
        if request
            .condition
            .if_match_sha256
            .as_deref()
            .is_some_and(|checksum| !checksum.eq_ignore_ascii_case(&object.checksum))
        {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_precondition_failed",
                "if_match_sha256 does not match the catalogue checksum",
            )));
        }
        if request
            .condition
            .if_none_match_sha256
            .as_deref()
            .is_some_and(|checksum| checksum.eq_ignore_ascii_case(&object.checksum))
        {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_not_modified",
                "if_none_match_sha256 matches the catalogue checksum",
            )));
        }
        let (reader, expected_size_bytes, expected_checksum) = if let Some(range) = request.range {
            if range.start > object.size_bytes {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_invalid_range",
                    "provider stream range starts beyond the catalogue object",
                )));
            }
            let end = range
                .end_exclusive
                .unwrap_or(object.size_bytes)
                .min(object.size_bytes);
            if end < range.start {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_invalid_range",
                    "provider stream range ends before it starts",
                )));
            }
            let length = end - range.start;
            (
                backend
                    .read_range(&request.object, range.start, length)
                    .map_err(|error| {
                        DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "provider_stream_read_failed",
                            error.to_string(),
                        ))
                    })?,
                length,
                None,
            )
        } else {
            (
                backend.read(&request.object).map_err(|error| {
                    DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_read_failed",
                        error.to_string(),
                    ))
                })?,
                object.size_bytes,
                Some(object.checksum),
            )
        };
        Ok(ProviderStreamSource {
            reader,
            expected_size_bytes,
            expected_checksum,
        })
    }
}

fn retained_dossier_upload_response(
    request: &ProviderStreamUploadOpenRequest,
    store_id: StoreId,
    backend: &FolderBackend,
    record: &dasobjectstore_core::backend::BackendObjectRecord,
    observed_at_utc: &str,
) -> Result<ProviderStreamUploadResponse, DaemonApiResponse> {
    let mut response =
        ProviderStreamUploadResponse::from_record(request.upload_id.clone(), store_id, record);
    let Some(authority) = request.retained_dossier.as_ref() else {
        return Ok(response);
    };
    let projection = dasobjectstore_core::JenkinsDossierEvidenceProjectionV1 {
        schema: dasobjectstore_core::JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA.to_owned(),
        authority_scope: authority.authority_scope.clone(),
        store_id: response.store_id.as_str().to_owned(),
        object_id: response.object.object_id.clone(),
        object_version: response.object.version,
        size_bytes: response.size_bytes,
        content_sha256: response.sha256.trim_start_matches("sha256:").to_owned(),
        dossier_digest: authority.dossier_digest.clone(),
        evidence_revision: authority.evidence_revision,
    };
    let evidence = projection.project().map_err(|error| {
        DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "expedition_dossier_projection_failed",
            error.to_string(),
        ))
    })?;
    let mut reader = backend.read(&record.key).map_err(|error| {
        DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "expedition_dossier_readback_failed",
            error.to_string(),
        ))
    })?;
    let readback = dasobjectstore_core::verify_jenkins_dossier_readback(evidence, &mut reader)
        .map_err(|error| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "expedition_dossier_readback_failed",
                error.to_string(),
            ))
        })?;
    response.retained_dossier = Some(crate::api::JenkinsDossierEvidenceSettlementResponse {
        schema_version: crate::api::JENKINS_DOSSIER_EVIDENCE_SETTLEMENT_V1_SCHEMA.to_owned(),
        request_id: request.request_id.clone(),
        evidence: readback.evidence,
        size_bytes: readback.size_bytes,
        content_sha256: readback.content_sha256,
        observed_at_utc: observed_at_utc.to_owned(),
    });
    Ok(response)
}

struct ProviderUploadReader<'a> {
    expected_size_bytes: u64,
    expected_sha256: String,
    next_frame: &'a mut dyn FnMut() -> Result<
        (ProviderStreamChunkHeader, Vec<u8>),
        UnixSocketDaemonServerError,
    >,
    verifier: Option<ProviderStreamVerifier>,
    pending: Vec<u8>,
    pending_offset: usize,
    final_frame_seen: bool,
}

impl<'a> ProviderUploadReader<'a> {
    fn new(
        request: &ProviderStreamUploadOpenRequest,
        next_frame: &'a mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
    ) -> Self {
        Self {
            expected_size_bytes: request.expected_size_bytes,
            expected_sha256: request.expected_sha256.clone(),
            next_frame,
            verifier: Some(
                ProviderStreamVerifier::new(request.request_id.clone())
                    .expect("validated provider upload request has a request id"),
            ),
            pending: Vec::new(),
            pending_offset: 0,
            final_frame_seen: false,
        }
    }

    fn invalid(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.into())
    }
}

impl Read for ProviderUploadReader<'_> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if destination.is_empty() {
            return Ok(0);
        }
        loop {
            if self.pending_offset < self.pending.len() {
                let count = (self.pending.len() - self.pending_offset).min(destination.len());
                destination[..count].copy_from_slice(
                    &self.pending[self.pending_offset..self.pending_offset + count],
                );
                self.pending_offset += count;
                if self.pending_offset == self.pending.len() {
                    self.pending.clear();
                    self.pending_offset = 0;
                }
                return Ok(count);
            }
            if self.final_frame_seen {
                return Ok(0);
            }

            let (header, payload) = (self.next_frame)()
                .map_err(|error| io::Error::new(io::ErrorKind::UnexpectedEof, error.to_string()))?;
            let verifier = self
                .verifier
                .as_mut()
                .ok_or_else(|| Self::invalid("provider upload verifier already consumed"))?;
            if header.final_chunk {
                let total_size = self
                    .verifier
                    .take()
                    .expect("provider upload verifier present")
                    .finish(&header, &payload)
                    .map_err(|error| Self::invalid(error.to_string()))?;
                let checksum = header
                    .sha256
                    .as_deref()
                    .ok_or_else(|| Self::invalid("provider upload final checksum is missing"))?;
                if total_size != self.expected_size_bytes
                    || !checksum.eq_ignore_ascii_case(&self.expected_sha256)
                {
                    return Err(Self::invalid(
                        "provider upload differs from its declared size or checksum",
                    ));
                }
                self.pending = payload;
                self.final_frame_seen = true;
                continue;
            }
            verifier
                .push(&header, &payload)
                .map_err(|error| Self::invalid(error.to_string()))?;
            let end = header
                .offset
                .checked_add(payload.len() as u64)
                .ok_or_else(|| Self::invalid("provider upload size overflow"))?;
            if end > self.expected_size_bytes {
                return Err(Self::invalid("provider upload exceeds its declared size"));
            }
            self.pending = payload;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn request(expected_size_bytes: u64, expected_sha256: &str) -> ProviderStreamUploadOpenRequest {
        ProviderStreamUploadOpenRequest {
            schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "request-1".to_string(),
            upload_id: "upload-1".to_string(),
            store_id: "codex".parse().expect("store id"),
            object: dasobjectstore_core::backend::BackendObjectKey {
                object_id: "reads/example.txt".to_string(),
                version: 1,
            },
            expected_size_bytes,
            expected_sha256: expected_sha256.to_string(),
            chunk_size_bytes: 4,
            retained_dossier: None,
            synoptikon_projection: None,
        }
    }

    fn frame(
        request_id: &str,
        offset: u64,
        payload: &[u8],
        final_chunk: bool,
        total_size: Option<u64>,
        sha256: Option<&str>,
    ) -> (ProviderStreamChunkHeader, Vec<u8>) {
        (
            ProviderStreamChunkHeader {
                schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request_id.to_string(),
                offset,
                payload_len: payload.len() as u32,
                final_chunk,
                total_size,
                sha256: sha256.map(ToOwned::to_owned),
            },
            payload.to_vec(),
        )
    }

    #[test]
    fn reader_streams_frames_and_verifies_terminal_metadata() {
        let checksum = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let request = request(5, checksum);
        let frames = RefCell::new(vec![
            frame("request-1", 0, b"hello", false, None, None),
            frame("request-1", 5, b"", true, Some(5), Some(checksum)),
        ]);
        let mut next_frame = || {
            frames.borrow_mut().drain(..1).next().ok_or_else(|| {
                UnixSocketDaemonServerError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "test frame missing",
                ))
            })
        };
        let mut reader = ProviderUploadReader::new(&request, &mut next_frame);
        let mut payload = Vec::new();
        reader.read_to_end(&mut payload).expect("verified payload");
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn reader_rejects_declared_checksum_mismatch() {
        let request = request(
            5,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let frames = RefCell::new(vec![frame(
            "request-1",
            0,
            b"hello",
            true,
            Some(5),
            Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        )]);
        let mut next_frame = || {
            frames.borrow_mut().drain(..1).next().ok_or_else(|| {
                UnixSocketDaemonServerError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "test frame missing",
                ))
            })
        };
        let mut reader = ProviderUploadReader::new(&request, &mut next_frame);
        let mut payload = Vec::new();
        let error = reader
            .read_to_end(&mut payload)
            .expect_err("checksum mismatch");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
