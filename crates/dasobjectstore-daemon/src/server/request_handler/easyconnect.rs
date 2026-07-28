use super::*;
use crate::api::{
    ApplicationObjectDeleteOutcome, ApplicationObjectDeleteReason, ApplicationObjectDeleteRequest,
    ApplicationObjectDeleteResponse, CapacityAdmissionDecision,
    RemoteEasyconnectAwsCliEnvironmentVariable, APPLICATION_OBJECT_DELETE_SCHEMA_VERSION,
};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

fn lock_application_store_mutation(
    store_id: &StoreId,
) -> Result<Arc<Mutex<()>>, DaemonServiceRuntimeError> {
    static GUARDS: OnceLock<Mutex<BTreeMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut guards = GUARDS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| upload_error("application ObjectStore mutation guard is unavailable"))?;
    let guard = guards
        .entry(store_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    if guard.is_poisoned() {
        return Err(upload_error(
            "application ObjectStore mutation guard is unavailable",
        ));
    }
    Ok(guard)
}

fn reconcile_application_deletion_capacity(
    capacity_provider: &dyn crate::runtime::CapacityAdmissionProvider,
    store_id: &StoreId,
    outcome: crate::runtime::ApplicationObjectDeletionOutcome,
) -> Result<(), DaemonServiceRuntimeError> {
    let remaining_logical_bytes = match outcome {
        crate::runtime::ApplicationObjectDeletionOutcome::Deleted {
            remaining_logical_bytes,
        }
        | crate::runtime::ApplicationObjectDeletionOutcome::AlreadyAbsent {
            remaining_logical_bytes,
        } => remaining_logical_bytes,
    };
    capacity_provider.reconcile_used_bytes(store_id, remaining_logical_bytes)
}

fn random_capability_slug(prefix: &str) -> Result<String, DaemonServiceRuntimeError> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| upload_error("operating-system randomness is unavailable"))?;
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{prefix}-{encoded}"))
}

fn random_easyconnect_value(prefix: &str, byte_count: usize) -> Result<String, String> {
    let mut bytes = vec![0_u8; byte_count];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "operating-system randomness is unavailable".to_string())?;
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{prefix}{encoded}"))
}

fn temporary_s3_session_credentials() -> Result<RemoteEasyconnectSessionCredentials, String> {
    Ok(RemoteEasyconnectSessionCredentials {
        access_key_id: random_easyconnect_value("DOST", 16)?,
        secret_access_key: random_easyconnect_value("", 32)?,
        session_token: Some(random_easyconnect_value("dost_", 32)?),
    })
}

fn upload_error(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

fn resolve_daemon_custodied_upload_credentials(
    credential_registry_path: &std::path::Path,
    now_utc: &str,
    mut request: RemoteEasyconnectSubmitAwsCliUploadRequest,
) -> Result<RemoteEasyconnectSubmitAwsCliUploadRequest, DaemonServiceRuntimeError> {
    let Some(completion) = request.completion.as_ref() else {
        return Ok(request);
    };
    let credential = read_managed_credential_registry(credential_registry_path, now_utc)?
        .credentials
        .into_iter()
        .find(|credential| {
            credential.store_id.as_str() == request.object_store
                && credential.bucket_name == completion.bucket
        })
        .ok_or_else(|| {
            upload_error(
                "no daemon-managed credential matches the remote upload ObjectStore and bucket",
            )
        })?;
    request.environment.retain(|variable| {
        !matches!(
            variable.name.as_str(),
            "AWS_ACCESS_KEY_ID" | "AWS_SECRET_ACCESS_KEY" | "AWS_SESSION_TOKEN"
        )
    });
    let strip_profile = |args: &mut Vec<String>| {
        let mut index = 0;
        while index < args.len() {
            if args[index] == "--profile" {
                args.drain(index..args.len().min(index + 2));
            } else {
                index += 1;
            }
        }
    };
    strip_profile(&mut request.args);
    strip_profile(&mut request.display_args);
    request.environment.extend([
        RemoteEasyconnectAwsCliEnvironmentVariable {
            name: "AWS_ACCESS_KEY_ID".to_string(),
            value: credential.access_key_id,
        },
        RemoteEasyconnectAwsCliEnvironmentVariable {
            name: "AWS_SECRET_ACCESS_KEY".to_string(),
            value: credential.secret_access_key,
        },
    ]);
    Ok(request)
}

/// Handles Remote EasyConnect pairing, sessions, admission, and upload requests.
pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::RemoteEasyconnectCreatePairing(request) => {
            let created_at_utc = handler.clock.now_utc();
            match handler.create_remote_easyconnect_pairing(request, &created_at_utc) {
                Ok(response) => Ok(DaemonApiResponse::RemoteEasyconnectCreatePairing(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "remote_easyconnect_pairing_create_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::RemoteEasyconnectApprovePairing(request) => {
            match handler.approve_remote_easyconnect_pairing(request) {
                Ok(response) => Ok(DaemonApiResponse::RemoteEasyconnectApprovePairing(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "remote_easyconnect_pairing_approve_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::RemoteEasyconnectExchangePairing(request) => {
            let exchanged_at_utc = handler.clock.now_utc();
            match handler.exchange_remote_easyconnect_pairing(request, &exchanged_at_utc) {
                Ok(response) => Ok(DaemonApiResponse::RemoteEasyconnectExchangePairing(
                    response,
                )),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "remote_easyconnect_pairing_exchange_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::RemoteEasyconnectRevokeSession(request) => {
            let revoked_at_utc = handler.clock.now_utc();
            match handler.revoke_remote_easyconnect_session(&request.session_id, &revoked_at_utc) {
                Ok(revoked) => Ok(DaemonApiResponse::RemoteEasyconnectRevokeSession(
                    RemoteEasyconnectRevokeSessionResponse {
                        session_id: request.session_id,
                        revoked,
                        revoked_at_utc,
                    },
                )),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "remote_easyconnect_session_revoke_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::RemoteEasyconnectRenewSession(request) => {
            let renewed_at_utc = handler.clock.now_utc();
            match handler.renew_remote_easyconnect_session(request, &renewed_at_utc) {
                Ok(response) => Ok(DaemonApiResponse::RemoteEasyconnectRenewSession(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "remote_easyconnect_session_renew_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::RemoteEasyconnectUploadAdmission(request) => {
            Ok(DaemonApiResponse::RemoteEasyconnectUploadAdmission(
                handler
                    .remote_upload_admission_gate
                    .admission_decision_from_request(request),
            ))
        }
        DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(request) => {
            let Some(registry) = &handler.admin_job_registry else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "job_registry_unavailable",
                    "remote easyconnect uploads require the daemon job registry",
                )));
            };
            let accepted_at_utc = handler.clock.now_utc();
            let request = match resolve_daemon_custodied_upload_credentials(
                &handler.credential_registry_path,
                &accepted_at_utc,
                request,
            ) {
                Ok(request) => request,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "remote_easyconnect_upload_credential_failed",
                        error.to_string(),
                    )));
                }
            };
            match handler
                .service_orchestrator
                .remote_easyconnect_aws_cli_upload_job(
                    registry.as_ref(),
                    Arc::clone(&handler.remote_upload_admission_gate),
                    remote_easyconnect_aws_cli_upload_job_request(
                        request,
                        &accepted_at_utc,
                        actor.and_then(|actor| actor.username.clone()),
                        handler.live_sqlite_path.clone(),
                    ),
                ) {
                Ok(report) => Ok(DaemonApiResponse::RemoteEasyconnectSubmitAwsCliUpload(
                    RemoteEasyconnectSubmitAwsCliUploadResponse {
                        running_event: report.running_event,
                        progress_events: report.progress_events,
                        final_event: report.final_event,
                    },
                )),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "remote_easyconnect_upload_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::IssueApplicationUploadCapability(request) => {
            let now_utc = handler.clock.now_utc();
            match handler.issue_application_upload_capability(request, &now_utc) {
                Ok(response) => Ok(DaemonApiResponse::ApplicationUploadCapabilityIssued(
                    response,
                )),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "application_upload_capability_issue_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::CompleteApplicationUpload(request) => {
            let now_utc = handler.clock.now_utc();
            match handler.complete_application_upload(request, &now_utc) {
                Ok(response) => Ok(DaemonApiResponse::ApplicationUploadCompleted(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "application_upload_completion_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::DeleteApplicationObject(request) => {
            let now_utc = handler.clock.now_utc();
            match handler.delete_application_object(request, &now_utc) {
                Ok(response) => Ok(DaemonApiResponse::ApplicationObjectDeleted(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "application_object_delete_failed",
                    error.to_string(),
                ))),
            }
        }
        _ => unreachable!("easyconnect dispatcher received an unrelated request"),
    }
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    fn issue_application_upload_capability(
        &self,
        request: ApplicationUploadCapabilityIssueRequest,
        now_utc: &str,
    ) -> Result<ApplicationUploadCapabilityIssueResponse, DaemonServiceRuntimeError> {
        request.validate().map_err(upload_error)?;
        let now = parse_utc_timestamp_seconds(now_utc)
            .ok_or_else(|| upload_error("daemon clock is not a supported UTC timestamp"))?
            as u64;
        if request.provider != "garage"
            || self
                .service_orchestrator
                .application_upload_endpoint()
                .as_deref()
                != Some(request.endpoint_url.as_str())
        {
            return Err(upload_error(
                "only a configured Garage provider endpoint is supported",
            ));
        }
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
        let grant = session_store
            .authorize_completion(
                &request.session_id,
                &request.renewal_token,
                &request.object_store,
                now_utc,
            )
            .map_err(|error| upload_error(error.to_string()))?;
        if grant.bucket != request.bucket {
            return Err(upload_error(
                "requested bucket is outside the paired session grant",
            ));
        }
        let store_id = StoreId::new(request.object_store.clone())
            .map_err(|error| upload_error(error.to_string()))?;
        let identity = read_application_identity(
            &self.application_identity_registry_path,
            &request.application_id,
        )?
        .ok_or_else(|| upload_error("application identity is not registered"))?;
        identity
            .authorize_upload_completion(
                &store_id,
                &request.object_key,
                request.expected_size_bytes,
                now,
            )
            .map_err(|error| upload_error(error.to_string()))?;
        let ttl = request
            .requested_ttl_seconds
            .unwrap_or(MAX_UPLOAD_COMPLETION_TTL_SECONDS);
        if ttl == 0 || ttl > MAX_UPLOAD_COMPLETION_TTL_SECONDS {
            return Err(upload_error(
                "upload capability TTL must be between 1 and 900 seconds",
            ));
        }
        let expires = now
            .checked_add(ttl)
            .ok_or_else(|| upload_error("capability expiry overflow"))?;
        let session = session_store
            .get(&request.session_id)
            .map_err(|error| upload_error(error.to_string()))?
            .ok_or_else(|| upload_error("paired session disappeared during capability issuance"))?;
        if session.revoked_at_utc.is_some() || session.renewal_token != request.renewal_token {
            return Err(upload_error(
                "paired session changed during capability issuance",
            ));
        }
        let session_expires = parse_utc_timestamp_seconds(&session.expires_at_utc)
            .ok_or_else(|| upload_error("paired session expiry is not a supported UTC timestamp"))?
            as u64;
        if expires > session_expires {
            return Err(upload_error(
                "upload capability lifetime exceeds paired session",
            ));
        }
        if expires > identity.expires_at_unix_seconds {
            return Err(upload_error(
                "upload capability lifetime exceeds application identity",
            ));
        }
        let capability = UploadCompletionCapability {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            capability_id: random_capability_slug("upload-cap")?,
            application_id: request.application_id,
            session_id: request.session_id,
            upload_id: request.upload_id.clone(),
            store_id,
            object_key: request.object_key.clone(),
            expected_size_bytes: request.expected_size_bytes,
            expected_checksum: request.expected_checksum.clone(),
            audience: request.audience,
            issued_at_unix_seconds: now,
            expires_at_unix_seconds: expires,
            nonce: random_capability_slug("nonce")?,
        };
        capability
            .validate()
            .map_err(|error| upload_error(error.to_string()))?;
        let reservation_id = format!("application-upload-{}", capability.capability_id);
        let capacity_provider = self
            .service_orchestrator
            .capacity_provider()
            .ok_or_else(|| {
                upload_error("capacity admission is unavailable for application upload completion")
            })?;
        let admission = capacity_provider.admit_remote_upload(
            capability.store_id.as_str(),
            capability.expected_size_bytes,
            &reservation_id,
        )?;
        if admission.decision != CapacityAdmissionDecision::Admitted {
            return Err(upload_error(admission.message.unwrap_or_else(|| {
                "application upload exceeds available capacity".to_string()
            })));
        }
        let issue_result = issue_application_upload_capability(
            &self.application_upload_capability_path,
            PendingApplicationUploadCapability {
                capability: capability.clone(),
                completion: RemoteUploadProviderCompletion {
                    upload_id: request.upload_id,
                    provider: request.provider,
                    bucket: request.bucket,
                    object_id: request.object_id,
                    object_version: request.object_version,
                    object_key: request.object_key,
                    expected_checksum: request.expected_checksum,
                    endpoint_url: request.endpoint_url,
                },
                capacity_reservation_id: Some(reservation_id.clone()),
                capacity_settlement: crate::runtime::ApplicationUploadCapacitySettlement::Reserved,
            },
            now,
        );
        if let Err(error) = issue_result {
            capacity_provider.release(&capability.store_id, &reservation_id)?;
            return Err(error);
        }
        Ok(ApplicationUploadCapabilityIssueResponse { capability })
    }

    fn complete_application_upload(
        &self,
        request: ApplicationUploadCompletionRequest,
        now_utc: &str,
    ) -> Result<ApplicationUploadCompletionResponse, DaemonServiceRuntimeError> {
        request.validate().map_err(upload_error)?;
        let now = parse_utc_timestamp_seconds(now_utc)
            .ok_or_else(|| upload_error("daemon clock is not a supported UTC timestamp"))?
            as u64;
        let pending = read_application_upload_capability(
            &self.application_upload_capability_path,
            &request.capability,
            now,
        )?;
        let reservation_id = pending.capacity_reservation_id.clone().ok_or_else(|| {
            upload_error("upload capability has no daemon-owned capacity reservation")
        })?;
        let capacity_provider = self
            .service_orchestrator
            .capacity_provider()
            .ok_or_else(|| {
                upload_error("capacity settlement is unavailable for application upload completion")
            })?;
        let mutation_lock = lock_application_store_mutation(&pending.capability.store_id)?;
        let _mutation_guard = mutation_lock
            .lock()
            .map_err(|_| upload_error("application ObjectStore mutation guard is unavailable"))?;
        let credentials =
            read_managed_credential_registry(&self.credential_registry_path, now_utc)?
                .credentials
                .into_iter()
                .find(|credential| {
                    credential.store_id.as_str() == pending.capability.store_id.as_str()
                        && credential.bucket_name == pending.completion.bucket
                })
                .ok_or_else(|| {
                    upload_error("no daemon-managed credential exists for upload completion")
                })?;
        let environment = vec![
            ("AWS_ACCESS_KEY_ID".to_string(), credentials.access_key_id),
            (
                "AWS_SECRET_ACCESS_KEY".to_string(),
                credentials.secret_access_key,
            ),
        ];
        let record = RemoteUploadCompletionRecord {
            job_id: pending.capability.upload_id.clone(),
            object_store: pending.capability.store_id.to_string(),
            source_bytes: pending.capability.expected_size_bytes,
            metadata: Some(RemoteUploadCompletionMetadata {
                upload_id: pending.capability.upload_id.clone(),
                object_key: pending.capability.object_key.clone(),
                expected_size_bytes: pending.capability.expected_size_bytes,
                expected_checksum: pending.capability.expected_checksum.clone(),
            }),
        };
        let completion = pending.completion.clone();
        let verification_completion = completion.clone();
        let verification_environment = environment.clone();
        let outcome = complete_upload_with_capability(
            &self.application_capability_replay_path,
            &pending.capability,
            now,
            |_| {
                self.service_orchestrator
                    .verify_application_upload_completion(
                        &record,
                        verification_completion,
                        verification_environment,
                        self.live_sqlite_path.clone(),
                        now_utc,
                    )
                    .map_err(|error| error.to_string())
            },
            |_| {
                self.service_orchestrator
                    .commit_application_upload_catalogue(
                        &record,
                        completion,
                        environment,
                        self.live_sqlite_path.clone(),
                        now_utc,
                    )
                    .map_err(|error| error.to_string())?;
                let settlement = crate::runtime::prepare_application_upload_capacity_settlement(
                    &self.application_upload_capability_path,
                    &pending.capability.capability_id,
                )
                .map_err(|error| error.to_string())?;
                if settlement != crate::runtime::ApplicationUploadCapacitySettlement::Committed {
                    match capacity_provider
                        .reservation_bytes(&pending.capability.store_id, &reservation_id)
                        .map_err(|error| error.to_string())?
                    {
                        Some(bytes) if bytes == pending.capability.expected_size_bytes => {
                            capacity_provider
                                .commit(&pending.capability.store_id, &reservation_id)
                                .map_err(|error| error.to_string())?;
                        }
                        Some(bytes) => {
                            return Err(format!(
                                "capacity reservation size drift: expected {}, got {bytes}",
                                pending.capability.expected_size_bytes
                            ));
                        }
                        None => {
                            // A durable Prepared marker is written before settlement.
                            // Capabilities expire before reservation leases, so a missing
                            // reservation here represents the prior commit crash window.
                        }
                    }
                    crate::runtime::commit_application_upload_capacity_settlement(
                        &self.application_upload_capability_path,
                        &pending.capability.capability_id,
                    )
                    .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
        )?;
        Ok(ApplicationUploadCompletionResponse {
            capability_id: pending.capability.capability_id,
            outcome: match outcome {
                crate::runtime::UploadCompletionCapabilityOutcome::Committed => {
                    ApplicationUploadCompletionOutcome::Committed
                }
                crate::runtime::UploadCompletionCapabilityOutcome::AlreadyConsumed => {
                    ApplicationUploadCompletionOutcome::AlreadyCommitted
                }
            },
        })
    }

    fn delete_application_object(
        &self,
        request: ApplicationObjectDeleteRequest,
        now_utc: &str,
    ) -> Result<ApplicationObjectDeleteResponse, DaemonServiceRuntimeError> {
        request.validate().map_err(upload_error)?;
        let now = parse_utc_timestamp_seconds(now_utc)
            .ok_or_else(|| upload_error("daemon clock is not a supported UTC timestamp"))?
            as u64;
        if request.provider != "garage"
            || self
                .service_orchestrator
                .application_upload_endpoint()
                .as_deref()
                != Some(request.endpoint_url.as_str())
        {
            return Err(upload_error(
                "only the configured Garage provider endpoint is supported",
            ));
        }
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
        let grant = session_store
            .authorize_completion(
                &request.session_id,
                &request.renewal_token,
                &request.object_store,
                now_utc,
            )
            .map_err(|error| upload_error(error.to_string()))?;
        if grant.bucket != request.bucket {
            return Err(upload_error(
                "requested bucket is outside the paired session grant",
            ));
        }
        let store_id = StoreId::new(request.object_store.clone())
            .map_err(|error| upload_error(error.to_string()))?;
        let capacity_provider = self
            .service_orchestrator
            .capacity_provider()
            .ok_or_else(|| {
                upload_error("capacity reconciliation is unavailable for application deletion")
            })?;
        let identity = read_application_identity(
            &self.application_identity_registry_path,
            &request.application_id,
        )?
        .ok_or_else(|| upload_error("application identity is not registered"))?;
        identity
            .authorize_object_delete(
                &store_id,
                &request.object_key,
                request.expected_size_bytes,
                now,
            )
            .map_err(|error| upload_error(error.to_string()))?;
        let credentials =
            read_managed_credential_registry(&self.credential_registry_path, now_utc)?
                .credentials
                .into_iter()
                .find(|credential| {
                    credential.store_id.as_str() == store_id.as_str()
                        && credential.bucket_name == request.bucket
                })
                .ok_or_else(|| {
                    upload_error("no daemon-managed credential exists for object deletion")
                })?;
        let environment = vec![
            ("AWS_ACCESS_KEY_ID".to_string(), credentials.access_key_id),
            (
                "AWS_SECRET_ACCESS_KEY".to_string(),
                credentials.secret_access_key,
            ),
        ];
        let deletion = crate::runtime::ApplicationObjectDeletion {
            store_id,
            object_id: request.object_id,
            object_version: request.object_version,
            object_key: request.object_key,
            expected_size_bytes: request.expected_size_bytes,
            expected_checksum: request.expected_checksum,
            provider: request.provider,
            bucket: request.bucket,
            endpoint_url: request.endpoint_url,
        };
        let mutation_lock = lock_application_store_mutation(&deletion.store_id)?;
        let _mutation_guard = mutation_lock
            .lock()
            .map_err(|_| upload_error("application ObjectStore mutation guard is unavailable"))?;
        let outcome = self.service_orchestrator.delete_application_object(
            &deletion,
            environment,
            self.live_sqlite_path.clone(),
        )?;
        reconcile_application_deletion_capacity(
            capacity_provider.as_ref(),
            &deletion.store_id,
            outcome,
        )?;
        let reason = match request.reason {
            ApplicationObjectDeleteReason::UserRequested => "user_requested",
            ApplicationObjectDeleteReason::SourceRemoved => "source_removed",
            ApplicationObjectDeleteReason::PolicyRequired => "policy_required",
        };
        let audit = record_application_audit_event(
            &self.application_audit_log_path,
            now_utc,
            "delete_object",
            &request.application_id,
            None,
            None,
            reason,
            false,
        )?;
        Ok(ApplicationObjectDeleteResponse {
            schema_version: APPLICATION_OBJECT_DELETE_SCHEMA_VERSION.to_string(),
            request_id: request.request_id,
            outcome: match outcome {
                crate::runtime::ApplicationObjectDeletionOutcome::Deleted { .. } => {
                    ApplicationObjectDeleteOutcome::Deleted
                }
                crate::runtime::ApplicationObjectDeletionOutcome::AlreadyAbsent { .. } => {
                    ApplicationObjectDeleteOutcome::AlreadyAbsent
                }
            },
            audit_event_id: audit.event_id,
        })
    }
    fn create_remote_easyconnect_pairing(
        &self,
        request: RemoteEasyconnectCreatePairingRequest,
        created_at_utc: &str,
    ) -> Result<RemoteEasyconnectCreatePairingResponse, RemoteEasyconnectPairingStoreError> {
        let lifetime_seconds = resolve_remote_easyconnect_session_lifetime_seconds(
            request.requested_session_lifetime_seconds,
        )
        .map_err(|error| RemoteEasyconnectPairingStoreError::Json {
            path: self.remote_easyconnect_pairing_store_path.clone(),
            message: error.to_string(),
        })?;
        let expires_at_utc = add_seconds_to_utc_timestamp(created_at_utc, lifetime_seconds)
            .ok_or_else(|| RemoteEasyconnectPairingStoreError::Json {
                path: self.remote_easyconnect_pairing_store_path.clone(),
                message: format!(
                    "daemon clock value {created_at_utc} is not a supported UTC timestamp"
                ),
            })?;
        let pairing_id = stable_easyconnect_id("pairing", &request.client_name, created_at_utc);
        let store = FileBackedRemoteEasyconnectPairingStore::new(
            &self.remote_easyconnect_pairing_store_path,
        );
        store.upsert(RemoteEasyconnectPairingRecord {
            pairing_id: pairing_id.clone(),
            client_name: request.client_name,
            callback_url: request.callback_url.clone(),
            requested_object_store: request.requested_object_store,
            requested_session_lifetime_seconds: request.requested_session_lifetime_seconds,
            client_request_id: request.client_request_id,
            created_at_utc: created_at_utc.to_string(),
            expires_at_utc: expires_at_utc.clone(),
            approval: None,
            exchanged_at_utc: None,
        })?;

        Ok(RemoteEasyconnectCreatePairingResponse {
            pairing_id: pairing_id.clone(),
            browser_login_url: format!(
                "/products/dasobjectstore/remote/easyconnect/login?pairing_id={pairing_id}"
            ),
            callback_url: request.callback_url,
            expires_at_utc,
            polling_url: format!("/api/v1/remote/easyconnect/pairings/{pairing_id}"),
        })
    }

    fn approve_remote_easyconnect_pairing(
        &self,
        request: RemoteEasyconnectApprovePairingRequest,
    ) -> Result<RemoteEasyconnectApprovePairingResponse, RemoteEasyconnectPairingStoreError> {
        let exchange_code = random_easyconnect_value("exchange-", 24).map_err(|message| {
            RemoteEasyconnectPairingStoreError::Json {
                path: self.remote_easyconnect_pairing_store_path.clone(),
                message,
            }
        })?;
        let store = FileBackedRemoteEasyconnectPairingStore::new(
            &self.remote_easyconnect_pairing_store_path,
        );
        let pairing = store.approve(RemoteEasyconnectPairingApproval {
            pairing_id: request.pairing_id.clone(),
            approved_actor: request.approved_actor,
            auth_provider: request.auth_provider,
            allowed_object_stores: request.allowed_object_stores,
            approval_expires_at_utc: request.approval_expires_at_utc.clone(),
            exchange_code: exchange_code.clone(),
        })?;

        Ok(RemoteEasyconnectApprovePairingResponse {
            pairing_id: request.pairing_id,
            exchange_code,
            callback_url: pairing.callback_url,
            expires_at_utc: request.approval_expires_at_utc,
        })
    }

    fn exchange_remote_easyconnect_pairing(
        &self,
        request: RemoteEasyconnectExchangePairingRequest,
        exchanged_at_utc: &str,
    ) -> Result<RemoteEasyconnectExchangePairingResponse, RemoteEasyconnectExchangeDispatchError>
    {
        let pairing_store = FileBackedRemoteEasyconnectPairingStore::new(
            &self.remote_easyconnect_pairing_store_path,
        );
        let pairing = pairing_store
            .exchange(RemoteEasyconnectPairingExchange {
                pairing_id: request.pairing_id,
                exchange_code: request.exchange_code,
                exchanged_at_utc: exchanged_at_utc.to_string(),
            })
            .map_err(RemoteEasyconnectExchangeDispatchError::PairingStore)?;
        let approval = pairing.approval.ok_or_else(|| {
            RemoteEasyconnectExchangeDispatchError::PairingStore(
                RemoteEasyconnectPairingStoreError::PairingNotApproved {
                    pairing_id: pairing.pairing_id.clone(),
                },
            )
        })?;
        let lifetime_seconds = resolve_remote_easyconnect_session_lifetime_seconds(
            pairing.requested_session_lifetime_seconds,
        )
        .map_err(
            |error| RemoteEasyconnectExchangeDispatchError::InvalidRequest {
                message: error.to_string(),
            },
        )?;
        let renew_after_offset = remote_easyconnect_renew_after_offset_seconds(lifetime_seconds)
            .map_err(
                |error| RemoteEasyconnectExchangeDispatchError::InvalidRequest {
                    message: error.to_string(),
                },
            )?;
        let expires_at_utc = add_seconds_to_utc_timestamp(exchanged_at_utc, lifetime_seconds)
            .ok_or_else(|| RemoteEasyconnectExchangeDispatchError::InvalidClock {
                value: exchanged_at_utc.to_string(),
            })?;
        let renew_after_utc = add_seconds_to_utc_timestamp(exchanged_at_utc, renew_after_offset)
            .ok_or_else(|| RemoteEasyconnectExchangeDispatchError::InvalidClock {
                value: exchanged_at_utc.to_string(),
            })?;
        let first_grant = approval.allowed_object_stores.first().ok_or_else(|| {
            RemoteEasyconnectExchangeDispatchError::InvalidRequest {
                message: "approved pairing did not contain object store grants".to_string(),
            }
        })?;
        if approval.allowed_object_stores.len() != 1 {
            return Err(RemoteEasyconnectExchangeDispatchError::InvalidRequest {
                message: "remote S3 sessions must contain exactly one ObjectStore grant"
                    .to_string(),
            });
        }
        if !first_grant.can_write {
            return Err(RemoteEasyconnectExchangeDispatchError::InvalidRequest {
                message: "remote S3 sessions require a writable ObjectStore grant until Garage read-only credential provisioning is available".to_string(),
            });
        }
        let credentials = temporary_s3_session_credentials().map_err(|message| {
            RemoteEasyconnectExchangeDispatchError::InvalidRequest { message }
        })?;
        let session_id = random_easyconnect_value("session-", 16).map_err(|message| {
            RemoteEasyconnectExchangeDispatchError::InvalidRequest { message }
        })?;
        let renewal_token = random_easyconnect_value("renewal-", 32).map_err(|message| {
            RemoteEasyconnectExchangeDispatchError::InvalidRequest { message }
        })?;
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
        let approved_actor = approval.approved_actor.clone();
        let auth_provider = approval.auth_provider;
        session_store
            .upsert(RemoteEasyconnectPairedSessionRecord {
                session_id: session_id.clone(),
                approved_actor: approval.approved_actor,
                auth_provider: approval.auth_provider,
                issued_at_utc: exchanged_at_utc.to_string(),
                expires_at_utc: expires_at_utc.clone(),
                renew_after_utc: renew_after_utc.clone(),
                renewal_token: renewal_token.clone(),
                credentials: credentials.clone(),
                object_stores: approval.allowed_object_stores.clone(),
                revoked_at_utc: None,
            })
            .map_err(RemoteEasyconnectExchangeDispatchError::SessionStore)?;

        Ok(RemoteEasyconnectExchangePairingResponse {
            appliance_id: "standalone-dasobjectstore".to_string(),
            appliance_base_url: "/products/dasobjectstore/api".to_string(),
            approved_actor,
            auth_provider,
            session: RemoteEasyconnectSession {
                session_id: session_id.clone(),
                issued_at_utc: exchanged_at_utc.to_string(),
                expires_at_utc,
                credentials,
                renewal: RemoteEasyconnectSessionRenewal {
                    renew_url: format!("/api/v1/remote/easyconnect/sessions/{session_id}/renew"),
                    renew_after_utc,
                    renewal_token,
                },
            },
            object_stores: approval.allowed_object_stores,
        })
    }

    fn revoke_remote_easyconnect_session(
        &self,
        session_id: &str,
        revoked_at_utc: &str,
    ) -> Result<bool, RemoteEasyconnectPairedSessionStoreError> {
        FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        )
        .revoke(session_id, revoked_at_utc)
    }

    fn renew_remote_easyconnect_session(
        &self,
        request: RemoteEasyconnectRenewSessionRequest,
        renewed_at_utc: &str,
    ) -> Result<RemoteEasyconnectRenewSessionResponse, RemoteEasyconnectRenewalDispatchError> {
        let lifetime_seconds =
            resolve_remote_easyconnect_session_lifetime_seconds(request.requested_lifetime_seconds)
                .map_err(
                    |error| RemoteEasyconnectRenewalDispatchError::InvalidRequest {
                        message: error.to_string(),
                    },
                )?;
        let renew_after_offset = remote_easyconnect_renew_after_offset_seconds(lifetime_seconds)
            .map_err(
                |error| RemoteEasyconnectRenewalDispatchError::InvalidRequest {
                    message: error.to_string(),
                },
            )?;
        let expires_at_utc = add_seconds_to_utc_timestamp(renewed_at_utc, lifetime_seconds)
            .ok_or_else(|| RemoteEasyconnectRenewalDispatchError::InvalidClock {
                value: renewed_at_utc.to_string(),
            })?;
        let renew_after_utc = add_seconds_to_utc_timestamp(renewed_at_utc, renew_after_offset)
            .ok_or_else(|| RemoteEasyconnectRenewalDispatchError::InvalidClock {
                value: renewed_at_utc.to_string(),
            })?;
        let rotated_renewal_token = random_easyconnect_value("renewal-", 32)
            .map_err(|message| RemoteEasyconnectRenewalDispatchError::InvalidRequest { message })?;
        let rotated_credentials = temporary_s3_session_credentials()
            .map_err(|message| RemoteEasyconnectRenewalDispatchError::InvalidRequest { message })?;
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
        let renewed = store
            .renew(RemoteEasyconnectPairedSessionRenewalRequest {
                session_id: request.session_id,
                renewal_token: request.renewal_token,
                renewed_at_utc: renewed_at_utc.to_string(),
                expires_at_utc,
                renew_after_utc,
                rotated_renewal_token,
                rotated_credentials,
            })
            .map_err(RemoteEasyconnectRenewalDispatchError::SessionStore)?;

        Ok(RemoteEasyconnectRenewSessionResponse {
            session: RemoteEasyconnectSession {
                session_id: renewed.session_id.clone(),
                issued_at_utc: renewed.issued_at_utc,
                expires_at_utc: renewed.expires_at_utc,
                credentials: renewed.credentials,
                renewal: RemoteEasyconnectSessionRenewal {
                    renew_url: format!(
                        "/api/v1/remote/easyconnect/sessions/{}/renew",
                        renewed.session_id
                    ),
                    renew_after_utc: renewed.renew_after_utc,
                    renewal_token: renewed.renewal_token,
                },
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{CapacityAdmissionRequest, CapacityAdmissionResponse};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    struct ReconciliationFixture {
        failures_remaining: AtomicUsize,
        reconciled_bytes: Mutex<Vec<u64>>,
    }

    impl crate::runtime::CapacityAdmissionProvider for ReconciliationFixture {
        fn admit(
            &self,
            _request: CapacityAdmissionRequest,
        ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
            Err(upload_error("admission is outside this fixture"))
        }

        fn reconcile_used_bytes(
            &self,
            _store_id: &StoreId,
            used_bytes: u64,
        ) -> Result<(), DaemonServiceRuntimeError> {
            self.reconciled_bytes
                .lock()
                .expect("reconciliations")
                .push(used_bytes);
            if self
                .failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(upload_error("synthetic reconciliation failure"));
            }
            Ok(())
        }
    }

    #[test]
    fn reconciliation_failure_is_repaired_by_idempotent_absent_retry() {
        let provider = ReconciliationFixture {
            failures_remaining: AtomicUsize::new(1),
            reconciled_bytes: Mutex::new(Vec::new()),
        };
        let store_id = StoreId::new("store-retry").expect("store");
        reconcile_application_deletion_capacity(
            &provider,
            &store_id,
            crate::runtime::ApplicationObjectDeletionOutcome::Deleted {
                remaining_logical_bytes: 17,
            },
        )
        .expect_err("first reconciliation fails");
        reconcile_application_deletion_capacity(
            &provider,
            &store_id,
            crate::runtime::ApplicationObjectDeletionOutcome::AlreadyAbsent {
                remaining_logical_bytes: 17,
            },
        )
        .expect("retry repairs ledger");
        assert_eq!(
            provider
                .reconciled_bytes
                .lock()
                .expect("reconciliations")
                .as_slice(),
            &[17, 17]
        );
    }

    #[test]
    fn upload_settlement_and_deletion_are_serialized_per_store_only() {
        let store_id =
            StoreId::new(format!("store-shared-{}", std::process::id())).expect("shared store");
        let other_store_id =
            StoreId::new(format!("store-other-{}", std::process::id())).expect("other store");
        let logical_usage = Arc::new(AtomicU64::new(40));
        let (upload_started_tx, upload_started_rx) = mpsc::channel();
        let (allow_upload_tx, allow_upload_rx) = mpsc::channel();
        let upload_store = store_id.clone();
        let upload_usage = Arc::clone(&logical_usage);
        let upload = thread::spawn(move || {
            let lock = lock_application_store_mutation(&upload_store).expect("upload lock");
            let _guard = lock.lock().expect("upload guard");
            upload_started_tx.send(()).expect("upload started");
            allow_upload_rx.recv().expect("allow upload");
            upload_usage.fetch_add(10, Ordering::SeqCst);
        });
        upload_started_rx.recv().expect("upload started");

        let other_lock =
            lock_application_store_mutation(&other_store_id).expect("other store lock");
        let _other_guard = other_lock
            .try_lock()
            .expect("other store remains independent");

        let (delete_done_tx, delete_done_rx) = mpsc::channel();
        let delete_store = store_id.clone();
        let delete_usage = Arc::clone(&logical_usage);
        let deletion = thread::spawn(move || {
            let lock = lock_application_store_mutation(&delete_store).expect("delete lock");
            let _guard = lock.lock().expect("delete guard");
            delete_usage.store(10, Ordering::SeqCst);
            delete_done_tx.send(()).expect("delete done");
        });
        assert!(
            delete_done_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "same-store deletion must wait for upload settlement"
        );
        allow_upload_tx.send(()).expect("allow upload");
        upload.join().expect("upload");
        delete_done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("deletion completes");
        deletion.join().expect("deletion");
        assert_eq!(logical_usage.load(Ordering::SeqCst), 10);
    }
}
