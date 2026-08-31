use super::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dasobjectstore_core::application_auth::{
    ApplicationCredentialKind, ApplicationIdentity, ApplicationKeyAlgorithm,
    ApplicationKeyDescriptor, ApplicationOperation,
};
use dasobjectstore_core::application_auth_v2::{
    ErgasterionCapabilityDiscoveryStateV1, ErgasterionCapabilityDiscoveryV1,
    ErgasterionCapabilityExchangeRequestV1, ErgasterionCapabilityRenewalRequestV1,
    ErgasterionCapabilityResponseV1, ErgasterionRequestedScopeV1, GeneratedOutputBindingV1,
    ERGASTERION_APPLICATION_ID, ERGASTERION_CAPABILITY_AUDIENCE,
    ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS, ERGASTERION_CAPABILITY_DISCOVERY_SCHEMA_VERSION,
    ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION, ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS,
    ERGASTERION_GENERATED_OUTPUT_AUDIENCE, ERGASTERION_GENERATED_OUTPUT_AUDIT_PURPOSE,
    ERGASTERION_GENERATED_OUTPUT_BINDING_SCHEMA_VERSION_V1, GOVERNED_BINDING_SCHEMA_VERSION_V2,
};

pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request_inner(handler, request, actor) {
        Err(DaemonRequestHandlerError::ServiceRuntime(
            DaemonServiceRuntimeError::UnsupportedOperation { operation },
        )) if safe_error_parts(&operation).is_some() => {
            let (code, message) = safe_error_parts(&operation).expect("safe error was matched");
            Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                code, message,
            )))
        }
        result => result,
    }
}

fn request_inner<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::AdmitGovernedBindingAuthority(request) => {
            admit_binding_authority(handler, request, actor)
        }
        DaemonApiRequest::AdmitGeneratedOutputBindingAuthority(request) => {
            admit_generated_output_binding_authority(handler, request, actor)
        }
        DaemonApiRequest::DiscoverErgasterionCapability => discover(handler),
        DaemonApiRequest::ExchangeErgasterionCapability(request) => {
            exchange(handler, request.exchange)
        }
        DaemonApiRequest::RenewErgasterionCapability(request) => renew(handler, request.renewal),
        DaemonApiRequest::ErgasterionObjectSnapshot(request) => {
            let now = now_unix_seconds(handler)?;
            let validated = authorize_use(
                handler,
                request.capability.expose_to_daemon(),
                request.snapshot.store_id.as_str(),
                &request.snapshot.prefix,
                "list",
                0,
                now,
            )?;
            let response = remote_object_snapshot(&handler.live_sqlite_path, &request.snapshot)
                .map_err(|error| safe_error("provider_unavailable", error.to_string()))?;
            record_application_audit_event(
                &handler.application_audit_log_path,
                &handler.clock.now_utc(),
                "ergasterion_object_snapshot",
                &validated.claims.application_id,
                Some(&validated.claims.key_id),
                None,
                "governed application catalogue access",
                false,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            Ok(DaemonApiResponse::ErgasterionObjectSnapshot(
                crate::api::ErgasterionObjectSnapshotResponse { snapshot: response },
            ))
        }
        DaemonApiRequest::ErgasterionObjectGroupStatus(request) => {
            let now = now_unix_seconds(handler)?;
            let validated = authorize_use(
                handler,
                request.capability.expose_to_daemon(),
                request.status.store_id.as_str(),
                &request.status.key,
                "verify",
                0,
                now,
            )?;
            let response =
                remote_object_group_status(&handler.live_sqlite_path, &request.status)
                    .map_err(|error| safe_error("provider_unavailable", error.to_string()))?;
            record_application_audit_event(
                &handler.application_audit_log_path,
                &handler.clock.now_utc(),
                "ergasterion_object_group_status",
                &validated.claims.application_id,
                Some(&validated.claims.key_id),
                None,
                "governed application object verification status",
                false,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            Ok(DaemonApiResponse::ErgasterionObjectGroupStatus(
                crate::api::ErgasterionObjectGroupStatusResponse { status: response },
            ))
        }
        request => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "unsupported_contract",
            format!("{} is not an Ergasterion operation", request.command_name()),
        ))),
    }
}

fn admit_binding_authority<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: crate::api::GovernedBindingAuthorityAdmissionRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now = now_unix_seconds(handler)?;
    request
        .binding
        .validate_at(now)
        .map_err(|error| safe_error("invalid_request", error.to_string()))?;
    if !request.dry_run && !actor.is_some_and(|actor| actor.is_administrator()) {
        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "administrator_authorization_required",
            "trusted binding admission requires a local DASObjectStore administrator",
        )));
    }
    let digest = binding_digest(&request.binding)?;
    if !request.dry_run {
        crate::runtime::upsert_trusted_governed_binding_authority(
            &handler.governed_binding_authority_path,
            crate::runtime::TrustedGovernedBindingAuthority {
                binding_id: request.binding.binding_id.clone(),
                object_store_id: request.binding.object_store_id.clone(),
                binding_digest_sha256: digest.clone(),
                tenant_id: request.binding.tenant_id.clone(),
                host_authority: request.binding.host_authority.clone(),
                prosopikon_authority: request.binding.prosopikon_authority.clone(),
                admitted_at_unix_seconds: now,
                expires_at_unix_seconds: parse_timestamp(&request.binding.expires_at)?,
                active: true,
                revoked_at_unix_seconds: None,
            },
        )
        .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
    }
    record_application_audit_event(
        &handler.application_audit_log_path,
        &handler.clock.now_utc(),
        "admit_governed_binding_authority",
        ERGASTERION_APPLICATION_ID,
        None,
        actor.map(DaemonLocalActor::display_name).as_deref(),
        "trusted governed binding authority admission",
        request.dry_run,
    )
    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
    Ok(DaemonApiResponse::AdmitGovernedBindingAuthority(
        crate::api::GovernedBindingAuthorityAdmissionResponse {
            binding_id: request.binding.binding_id,
            object_store_id: request.binding.object_store_id,
            binding_digest_sha256: digest,
            dry_run: request.dry_run,
            active: !request.dry_run,
        },
    ))
}

fn admit_generated_output_binding_authority<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: crate::api::GeneratedOutputBindingAuthorityAdmissionRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now = now_unix_seconds(handler)?;
    request
        .binding
        .validate_at(now)
        .map_err(|error| safe_error("invalid_request", error.to_string()))?;
    validate_generated_output_application_authority(handler, &request.binding, now)?;
    if !request.dry_run && !actor.is_some_and(|actor| actor.is_administrator()) {
        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "administrator_authorization_required",
            "generated-output binding admission requires a local DASObjectStore administrator",
        )));
    }
    let digest = generated_output_binding_digest(&request.binding)?;
    if !request.dry_run {
        crate::runtime::upsert_trusted_generated_output_binding_authority(
            &handler.generated_output_binding_authority_path,
            crate::runtime::TrustedGeneratedOutputBindingAuthority {
                binding: request.binding.clone(),
                binding_digest_sha256: digest.clone(),
                admitted_at_unix_seconds: now,
                active: true,
            },
        )
        .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
    }
    record_application_audit_event(
        &handler.application_audit_log_path,
        &handler.clock.now_utc(),
        "admit_generated_output_binding_authority",
        &request.binding.application_id,
        None,
        actor.map(DaemonLocalActor::display_name).as_deref(),
        "trusted generated-output binding authority admission",
        request.dry_run,
    )
    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
    Ok(DaemonApiResponse::AdmitGeneratedOutputBindingAuthority(
        crate::api::GeneratedOutputBindingAuthorityAdmissionResponse {
            receipt_schema_version:
                crate::api::GENERATED_OUTPUT_BINDING_ADMISSION_RECEIPT_SCHEMA_VERSION.to_string(),
            receipt_kind: crate::api::GENERATED_OUTPUT_BINDING_ADMISSION_RECEIPT_KIND.to_string(),
            binding_id: request.binding.binding_id,
            application_id: request.binding.application_id,
            object_store_id: request.binding.object_store_id,
            binding_digest_sha256: digest,
            admitted_at_unix_seconds: now,
            dry_run: request.dry_run,
            active: !request.dry_run,
        },
    ))
}

/// The binding is only trusted if the separate output-completion application
/// remains current and has an enrolled public credential compatible with its
/// declared authentication method. No key identifier is selected here, so
/// overlapping rotation descriptors remain possible without binding a policy
/// to a transient credential.
fn validate_generated_output_application_authority<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    binding: &GeneratedOutputBindingV1,
    now: u64,
) -> Result<(), DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let identity = read_application_identity(
        &handler.application_identity_registry_path,
        &binding.application_id,
    )
    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
    let Some(identity) = identity else {
        return Err(safe_error(
            "authority_unavailable",
            "generated-output application identity is unavailable",
        ));
    };
    let policy = identity.dynamic_binding.as_ref();
    let policy_matches = policy.is_some_and(|policy| {
        policy.schema_version == ERGASTERION_GENERATED_OUTPUT_BINDING_SCHEMA_VERSION_V1
            && policy.audience == ERGASTERION_GENERATED_OUTPUT_AUDIENCE
            && policy.audit_purpose == ERGASTERION_GENERATED_OUTPUT_AUDIT_PURPOSE
            && policy.max_object_bytes >= binding.max_object_bytes
            && policy.max_total_bytes >= binding.max_total_bytes
    });
    if !identity.active
        || now < identity.issued_at_unix_seconds
        || now >= identity.expires_at_unix_seconds
        || identity.validate().is_err()
        || !policy_matches
    {
        return Err(safe_error(
            "authority_unavailable",
            "generated-output application identity is inactive or does not match the binding policy",
        ));
    }
    let enrolled_key_exists = list_application_keys(&handler.application_key_registry_path)
        .map_err(DaemonRequestHandlerError::ServiceRuntime)?
        .iter()
        .any(|key| generated_output_key_is_current_for_identity(key, &identity, now));
    if enrolled_key_exists {
        Ok(())
    } else {
        Err(safe_error(
            "authority_unavailable",
            "generated-output application has no current enrolled public credential",
        ))
    }
}

fn generated_output_key_is_current_for_identity(
    key: &ApplicationKeyDescriptor,
    identity: &ApplicationIdentity,
    now: u64,
) -> bool {
    if key.application_id != identity.application_id
        || !key.active
        || now < key.issued_at_unix_seconds
        || now >= key.expires_at_unix_seconds
        || key.validate().is_err()
    {
        return false;
    }
    match (identity.credential_kind, key.algorithm) {
        (ApplicationCredentialKind::AsymmetricKey, ApplicationKeyAlgorithm::Ed25519)
        | (ApplicationCredentialKind::AsymmetricKey, ApplicationKeyAlgorithm::EcdsaP256Sha256) => {
            key.public_key_material.is_some()
        }
        (ApplicationCredentialKind::MtlsCertificate, ApplicationKeyAlgorithm::MtlsCertificate) => {
            true
        }
        _ => false,
    }
}

fn discover<S, C>(
    handler: &DaemonRequestHandler<S, C>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now = now_unix_seconds(handler)?;
    let authority_ready = crate::runtime::governed_binding_authority_ready(
        &handler.governed_binding_authority_path,
        now,
    )
    .unwrap_or(false);
    let application_ready = current_application_authority(handler, now).is_ok();
    let discovery = ErgasterionCapabilityDiscoveryV1 {
        schema_version: ERGASTERION_CAPABILITY_DISCOVERY_SCHEMA_VERSION.to_string(),
        exchange_contract: ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION.to_string(),
        binding_schema: GOVERNED_BINDING_SCHEMA_VERSION_V2.to_string(),
        state: if authority_ready && application_ready {
            ErgasterionCapabilityDiscoveryStateV1::Ready
        } else {
            ErgasterionCapabilityDiscoveryStateV1::Unavailable
        },
        max_capability_lifetime_seconds:
            dasobjectstore_core::application_auth::MAX_ACCESS_TOKEN_TTL_SECONDS,
        renewal_window_seconds: ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS,
        clock_skew_seconds: ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS,
        operations: vec![
            ApplicationOperation::List,
            ApplicationOperation::Read,
            ApplicationOperation::Verify,
        ],
    };
    discovery
        .validate()
        .map_err(|error| safe_error("provider_unavailable", error.to_string()))?;
    Ok(DaemonApiResponse::DiscoverErgasterionCapability(
        crate::api::ErgasterionCapabilityDiscoveryResponse { discovery },
    ))
}

fn exchange<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    exchange: ErgasterionCapabilityExchangeRequestV1,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now = now_unix_seconds(handler)?;
    exchange
        .validate_at(now)
        .map_err(|error| safe_error("invalid_request", error.to_string()))?;
    let (identity, key) = current_application_authority(handler, now)?;
    validate_identity_scope(&identity, &exchange.requested_scope)?;
    let request_digest = crate::ergasterion_proof_verifier::verify_ergasterion_ed25519_proof(
        ErgasterionCapabilityExchangeRequestV1::SIGNING_DOMAIN,
        &exchange.proof_free_value(),
        &exchange.proof,
        &key,
    )
    .map_err(|error| safe_error("proof_invalid", error))?;
    let binding_digest = binding_digest(&exchange.binding)?;
    crate::runtime::verify_current_governed_binding_authority(
        &handler.governed_binding_authority_path,
        &exchange.binding,
        &binding_digest,
        now,
    )
    .map_err(|_| {
        safe_error(
            "authority_unavailable",
            "trusted authority rejected the binding",
        )
    })?;
    let issue = capability_issue(
        &exchange.request_id,
        request_digest,
        &exchange.nonce,
        &exchange.issued_at,
        &exchange.expires_at,
        &exchange.binding,
        binding_digest,
        &exchange.requested_scope,
    )?;
    let issued = crate::runtime::issue_opaque_application_capability(
        &handler.application_capability_ledger_path,
        &handler.application_capability_master_key_path,
        issue,
        now,
    )
    .map_err(|error| {
        let code = replay_or_provider_code(&error);
        let message = if code == "replay_detected" {
            "exchange request or nonce was reused inconsistently"
        } else {
            "capability authority is unavailable"
        };
        safe_error(code, message)
    })?;
    capability_response(
        handler,
        exchange.request_id,
        exchange.correlation_id,
        exchange.requested_scope,
        issued,
        "issue_ergasterion_capability",
    )
}

fn renew<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    renewal: ErgasterionCapabilityRenewalRequestV1,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now = now_unix_seconds(handler)?;
    renewal
        .validate_at(now)
        .map_err(|error| safe_error("invalid_request", error.to_string()))?;
    let (identity, key) = current_application_authority(handler, now)?;
    validate_identity_scope(&identity, &renewal.requested_scope)?;
    let request_digest = crate::ergasterion_proof_verifier::verify_ergasterion_ed25519_proof(
        ErgasterionCapabilityRenewalRequestV1::SIGNING_DOMAIN,
        &renewal.proof_free_value(),
        &renewal.proof,
        &key,
    )
    .map_err(|error| safe_error("proof_invalid", error))?;
    let binding_digest = binding_digest(&renewal.binding)?;
    crate::runtime::verify_current_governed_binding_authority(
        &handler.governed_binding_authority_path,
        &renewal.binding,
        &binding_digest,
        now,
    )
    .map_err(|_| {
        safe_error(
            "authority_unavailable",
            "trusted authority rejected the binding",
        )
    })?;
    let issue = capability_issue(
        &renewal.request_id,
        request_digest,
        &renewal.nonce,
        &renewal.issued_at,
        &renewal.expires_at,
        &renewal.binding,
        binding_digest,
        &renewal.requested_scope,
    )?;
    let issued = crate::runtime::renew_opaque_application_capability(
        &handler.application_capability_ledger_path,
        &handler.application_capability_master_key_path,
        &renewal.capability_id,
        issue,
        now,
    )
    .map_err(|error| {
        let code = replay_or_provider_code(&error);
        let message = if code == "replay_detected" {
            "renewal request or nonce was reused inconsistently"
        } else {
            "capability authority is unavailable"
        };
        safe_error(code, message)
    })?;
    capability_response(
        handler,
        renewal.request_id,
        renewal.correlation_id,
        renewal.requested_scope,
        issued,
        "renew_ergasterion_capability",
    )
}

fn capability_response<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request_id: String,
    correlation_id: String,
    resolved_scope: ErgasterionRequestedScopeV1,
    issued: crate::runtime::IssuedApplicationCapability,
    operation: &str,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now_utc = handler.clock.now_utc();
    let response = ErgasterionCapabilityResponseV1 {
        schema_version:
            dasobjectstore_core::application_auth_v2::ERGASTERION_CAPABILITY_RESPONSE_SCHEMA_VERSION
                .to_string(),
        request_id,
        capability: issued.opaque_capability,
        capability_id: issued.capability_id,
        issued_at: dasobjectstore_core::utc::format_utc_timestamp_seconds(
            i64::try_from(issued.issued_at_unix_seconds).unwrap_or(i64::MAX),
        ),
        expires_at: dasobjectstore_core::utc::format_utc_timestamp_seconds(
            i64::try_from(issued.expires_at_unix_seconds).unwrap_or(i64::MAX),
        ),
        resolved_scope,
        renewal_window_seconds: ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS,
        revocation_checked_at: now_utc.clone(),
        correlation_id,
    };
    response
        .validate()
        .map_err(|error| safe_error("provider_unavailable", error.to_string()))?;
    record_application_audit_event(
        &handler.application_audit_log_path,
        &now_utc,
        operation,
        &issued.claims.application_id,
        Some(&issued.claims.key_id),
        None,
        "governed application capability exchange",
        false,
    )
    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
    let envelope = crate::api::ErgasterionCapabilityExchangeResponse {
        capability: response,
    };
    Ok(if operation.starts_with("renew") {
        DaemonApiResponse::RenewErgasterionCapability(envelope)
    } else {
        DaemonApiResponse::ExchangeErgasterionCapability(envelope)
    })
}

fn capability_issue(
    request_id: &str,
    request_digest_sha256: String,
    nonce: &str,
    issued_at: &str,
    expires_at: &str,
    binding: &dasobjectstore_core::application_auth_v2::GovernedObjectStoreBindingV2,
    binding_digest_sha256: String,
    scope: &ErgasterionRequestedScopeV1,
) -> Result<crate::runtime::ApplicationCapabilityIssue, DaemonRequestHandlerError> {
    let nonce = URL_SAFE_NO_PAD
        .decode(nonce)
        .map_err(|_| safe_error("invalid_request", "nonce is malformed"))?;
    Ok(crate::runtime::ApplicationCapabilityIssue {
        request_id: request_id.to_string(),
        request_digest_sha256,
        nonce,
        issued_at_unix_seconds: parse_timestamp(issued_at)?,
        expires_at_unix_seconds: parse_timestamp(expires_at)?,
        claims: crate::runtime::ApplicationCapabilityClaims {
            application_id: ERGASTERION_APPLICATION_ID.to_string(),
            key_id: dasobjectstore_core::application_auth_v2::ERGASTERION_APPLICATION_KEY_ID
                .to_string(),
            binding_id: binding.binding_id.clone(),
            binding_digest_sha256,
            tenant_id: binding.tenant_id.clone(),
            host_authority: binding.host_authority.clone(),
            prosopikon_authority: binding.prosopikon_authority.clone(),
            audience: ERGASTERION_CAPABILITY_AUDIENCE.to_string(),
            store_id: scope.object_store_id.to_string(),
            prefixes: scope.prefixes.clone(),
            operations: scope
                .operations
                .iter()
                .map(operation_name)
                .map(str::to_string)
                .collect(),
            max_object_bytes: scope.max_object_bytes,
            max_total_bytes: scope.max_total_bytes,
        },
    })
}

pub(super) fn authorize_use<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    capability: &str,
    store_id: &str,
    object_key: &str,
    operation: &str,
    bytes: u64,
    now: u64,
) -> Result<crate::runtime::ValidatedApplicationCapability, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let validated = crate::runtime::validate_and_account_application_capability(
        &handler.application_capability_ledger_path,
        capability,
        &crate::runtime::ApplicationCapabilityUse {
            audience: ERGASTERION_CAPABILITY_AUDIENCE.to_string(),
            store_id: store_id.to_string(),
            object_key: object_key.to_string(),
            operation: operation.to_string(),
            bytes,
            now_unix_seconds: now,
        },
    )
    .map_err(|_| {
        safe_error(
            "capability_revoked",
            "capability is invalid or outside scope",
        )
    })?;
    current_application_authority(handler, now)?;
    crate::runtime::verify_current_governed_authority_claims(
        &handler.governed_binding_authority_path,
        &validated.claims,
        now,
    )
    .map_err(|_| {
        safe_error(
            "capability_revoked",
            "governed authority is no longer current",
        )
    })?;
    Ok(validated)
}

pub(super) fn authorize_provider_read<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    capability: &crate::api::OpaqueApplicationCapability,
    store_id: &str,
    object_key: &str,
    bytes: u64,
) -> Result<(), DaemonApiResponse>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let now = dasobjectstore_core::utc::parse_utc_timestamp_seconds(&handler.clock.now_utc())
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_unavailable",
                "daemon clock cannot validate the application capability",
            ))
        })?;
    authorize_use(
        handler,
        capability.expose_to_daemon(),
        store_id,
        object_key,
        "read",
        bytes,
        now,
    )
    .map(|_| ())
    .map_err(|_| {
        DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "capability_revoked",
            "application capability is invalid, revoked, or outside scope",
        ))
    })
}

pub(super) fn authorize_provider_store<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: &crate::api::ProviderStreamOpenRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<StoreId, DaemonApiResponse>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    if request.synoptikon_projection.is_some() {
        return handler
            .authorize_synoptikon_projection_read(actor, request)
            .map_err(|error| {
                DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))
            });
    }
    if let Some(capability) = request.application_capability.as_ref() {
        authorize_provider_read(
            handler,
            capability,
            request.store_id.as_str(),
            &request.object.object_id,
            0,
        )?;
        return Ok(request.store_id.clone());
    }
    if let Some(store_id) = handler
        .authorize_verified_object_browser_subject(
            actor,
            request.verified_subject.as_ref(),
            &request.store_id,
            Some(&request.object.object_id),
        )
        .map_err(|error| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(error.code(), error.to_string()))
        })?
    {
        return Ok(store_id);
    }
    let delegated = handler
        .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
        .map_err(|error| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(error.code(), error.to_string()))
        })?;
    handler
        .authorize_endpoint_read(delegated.as_ref().or(actor), &request.store_id)
        .map_err(|error| {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(error.code(), error.to_string()))
        })
}

fn current_application_authority<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    now: u64,
) -> Result<(ApplicationIdentity, ApplicationKeyDescriptor), DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let identity = read_application_identity(
        &handler.application_identity_registry_path,
        ERGASTERION_APPLICATION_ID,
    )
    .map_err(DaemonRequestHandlerError::ServiceRuntime)?
    .ok_or_else(|| {
        safe_error(
            "authority_unavailable",
            "application authority is unavailable",
        )
    })?;
    let key = read_application_key(
        &handler.application_key_registry_path,
        ERGASTERION_APPLICATION_ID,
        dasobjectstore_core::application_auth_v2::ERGASTERION_APPLICATION_KEY_ID,
    )
    .map_err(DaemonRequestHandlerError::ServiceRuntime)?
    .ok_or_else(|| safe_error("authority_unavailable", "application key is unavailable"))?;
    if !identity.active
        || !key.active
        || now < identity.issued_at_unix_seconds
        || now >= identity.expires_at_unix_seconds
        || now < key.issued_at_unix_seconds
        || now >= key.expires_at_unix_seconds
    {
        return Err(safe_error(
            "capability_revoked",
            "application authority is inactive",
        ));
    }
    identity
        .validate()
        .map_err(|_| safe_error("authority_unavailable", "application authority is invalid"))?;
    key.validate()
        .map_err(|_| safe_error("authority_unavailable", "application key is invalid"))?;
    Ok((identity, key))
}

fn validate_identity_scope(
    identity: &ApplicationIdentity,
    scope: &ErgasterionRequestedScopeV1,
) -> Result<(), DaemonRequestHandlerError> {
    let policy = identity.dynamic_binding.as_ref().ok_or_else(|| {
        safe_error(
            "governed_scope_denied",
            "application has no governed binding policy",
        )
    })?;
    let allowed = policy.schema_version == GOVERNED_BINDING_SCHEMA_VERSION_V2
        && policy.audience == ERGASTERION_CAPABILITY_AUDIENCE
        && policy.max_object_bytes >= scope.max_object_bytes
        && policy.max_total_bytes >= scope.max_total_bytes
        && (identity.scope.store_ids.is_empty()
            || identity.scope.store_ids.contains(&scope.object_store_id))
        && scope
            .operations
            .iter()
            .all(|operation| identity.scope.operations.contains(operation));
    if allowed {
        Ok(())
    } else {
        Err(safe_error(
            "governed_scope_denied",
            "requested scope exceeds registered application authority",
        ))
    }
}

fn binding_digest(
    binding: &dasobjectstore_core::application_auth_v2::GovernedObjectStoreBindingV2,
) -> Result<String, DaemonRequestHandlerError> {
    crate::ergasterion_proof_verifier::canonical_value_sha256(
        &serde_json::to_value(binding)
            .map_err(|_| safe_error("invalid_request", "binding cannot be canonicalized"))?,
    )
    .map_err(|error| safe_error("invalid_request", error))
}

fn generated_output_binding_digest(
    binding: &GeneratedOutputBindingV1,
) -> Result<String, DaemonRequestHandlerError> {
    crate::ergasterion_proof_verifier::canonical_value_sha256(
        &serde_json::to_value(binding)
            .map_err(|_| safe_error("invalid_request", "binding cannot be canonicalized"))?,
    )
    .map_err(|error| safe_error("invalid_request", error))
}

fn now_unix_seconds<S, C>(
    handler: &DaemonRequestHandler<S, C>,
) -> Result<u64, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    parse_timestamp(&handler.clock.now_utc())
}

fn parse_timestamp(value: &str) -> Result<u64, DaemonRequestHandlerError> {
    dasobjectstore_core::utc::parse_utc_timestamp_seconds(value)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| safe_error("provider_unavailable", "daemon clock is invalid"))
}

fn operation_name(operation: &ApplicationOperation) -> &'static str {
    match operation {
        ApplicationOperation::List => "list",
        ApplicationOperation::Read => "read",
        ApplicationOperation::Verify => "verify",
        ApplicationOperation::Write => "write",
        ApplicationOperation::CompleteUpload => "complete_upload",
        ApplicationOperation::Delete => "delete",
    }
}

fn replay_or_provider_code(error: &DaemonServiceRuntimeError) -> &'static str {
    if error.to_string().contains("replay") {
        "replay_detected"
    } else {
        "provider_unavailable"
    }
}

fn safe_error(code: &str, message: impl Into<String>) -> DaemonRequestHandlerError {
    DaemonRequestHandlerError::ServiceRuntime(DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("{code}: {}", message.into()),
    })
}

fn safe_error_parts(operation: &str) -> Option<(&str, &str)> {
    let (code, message) = operation.split_once(": ")?;
    const SAFE_CODES: &[&str] = &[
        "authority_unavailable",
        "capability_revoked",
        "governed_scope_denied",
        "invalid_request",
        "proof_invalid",
        "provider_unavailable",
        "replay_detected",
    ];
    SAFE_CODES.contains(&code).then_some((code, message))
}
