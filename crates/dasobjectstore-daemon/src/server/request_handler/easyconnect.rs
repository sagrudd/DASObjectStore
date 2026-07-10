use super::*;
use std::sync::Arc;

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
            match handler
                .service_orchestrator
                .remote_easyconnect_aws_cli_upload_job(
                    registry.as_ref(),
                    Arc::clone(&handler.remote_upload_admission_gate),
                    remote_easyconnect_aws_cli_upload_job_request(
                        request,
                        &accepted_at_utc,
                        actor.and_then(|actor| actor.username.clone()),
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
        _ => unreachable!("easyconnect dispatcher received an unrelated request"),
    }
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
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
        let exchange_code = stable_easyconnect_id(
            "exchange",
            &request.approved_actor,
            &request.approval_expires_at_utc,
        );
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
        let credential = generate_per_store_credentials(
            &[StoreCredentialRequest {
                store_id: StoreId::new(&first_grant.object_store).map_err(|error| {
                    RemoteEasyconnectExchangeDispatchError::InvalidRequest {
                        message: error.to_string(),
                    }
                })?,
                bucket_name: first_grant.bucket.clone(),
            }],
            &mut SystemCredentialEntropy,
        )
        .map_err(RemoteEasyconnectExchangeDispatchError::ObjectService)?
        .into_iter()
        .next()
        .expect("one credential request yields one credential");
        let credentials = session_credentials_from_store_credentials(credential);
        let session_id = stable_easyconnect_id("session", &pairing.pairing_id, exchanged_at_utc);
        let renewal_token = rotated_easyconnect_renewal_token(&session_id, exchanged_at_utc);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
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
        let rotated_renewal_token =
            rotated_easyconnect_renewal_token(&request.session_id, renewed_at_utc);
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
