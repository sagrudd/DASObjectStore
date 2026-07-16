use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use dasobjectstore_core::{
    application_auth::{
        AccessTokenExchangeRequest, ApplicationCredentialKind, ApplicationEnvironment,
        ApplicationIdentity, ApplicationKeyAlgorithm, ApplicationKeyDescriptor,
        ApplicationOperation, ApplicationScope, APPLICATION_AUTH_SCHEMA_VERSION,
    },
    ids::StoreId,
    ingress::IngressOrigin,
    object_type::ObjectType,
};
use dasobjectstore_daemon::{
    api::{
        ApplicationAccessTokenExchangeRequest, ApplicationCredentialRevocationRequest,
        ApplicationIdentityRegistrationRequest, ApplicationKeyRegistrationRequest,
        ApplicationMtlsAuthorizationContext, ApplicationMtlsAuthorizationRequest,
        APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION,
        APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION,
    },
    runtime::{read_application_audit_events, DaemonServiceRuntimeError},
    DaemonClient, DaemonClientError, DaemonLocalActor, DaemonRequestHandler,
    DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOrchestrator,
    DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusRequest,
    DaemonServiceStatusResponse, FixedDaemonClock, InProcessDaemonTransport,
};
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn application_credentials_rotate_exchange_revoke_and_audit_end_to_end() {
    let root = acceptance_root();
    fs::create_dir_all(&root).expect("acceptance root");
    let identity_registry = root.join("application-identities.json");
    let key_registry = root.join("application-keys.json");
    let audit_path = root.join("application-audit.json");
    let handler = DaemonRequestHandler::new(
        AcceptanceOrchestrator,
        FixedDaemonClock::new("2026-07-16T15:00:00Z"),
    )
    .with_application_identity_registry_path(&identity_registry)
    .with_application_key_registry_path(&key_registry)
    .with_application_audit_log_path(&audit_path);
    let actor = DaemonLocalActor::new(0).with_username("release-acceptance");
    let client = DaemonClient::new(InProcessDaemonTransport::new(move |request| {
        handler
            .handle_with_progress_for_actor(request, Some(&actor), |_| Ok(()))
            .map_err(|error| DaemonClientError::Transport(error.to_string()))
    }));

    let application_id = "synoptikon-release";
    let scope = application_scope();
    let identity = application_identity(application_id, scope.clone());
    let registered = client
        .register_application_identity(identity_registration(identity))
        .expect("identity registration succeeds");
    assert_eq!(registered.identity.application_id, application_id);
    assert_eq!(
        registered.administrator_actor.as_deref(),
        Some("release-acceptance")
    );

    let first_signing_key = signing_key(11);
    client
        .register_application_key(key_registration(
            application_id,
            "key-1",
            &first_signing_key,
        ))
        .expect("first key registration succeeds");
    let first_exchange =
        signed_exchange(application_id, "key-1", &first_signing_key, scope.clone());
    let first_claims = client
        .exchange_application_access_token(ApplicationAccessTokenExchangeRequest {
            exchange: first_exchange.clone(),
        })
        .expect("first proof exchange succeeds")
        .claims;
    assert_eq!(first_claims.application_id, application_id);
    assert_eq!(first_claims.scope, scope);
    assert_eq!(first_claims.expires_at_unix_seconds, 2_600);

    let rotated_signing_key = signing_key(29);
    client
        .register_application_key(key_registration(
            application_id,
            "key-2",
            &rotated_signing_key,
        ))
        .expect("overlapping rotation key registers");
    client
        .exchange_application_access_token(ApplicationAccessTokenExchangeRequest {
            exchange: signed_exchange(
                application_id,
                "key-2",
                &rotated_signing_key,
                application_scope(),
            ),
        })
        .expect("rotated key exchanges before old-key revocation");

    client
        .revoke_application_credential(revocation(application_id, Some("key-1"), "rotate key"))
        .expect("old key revokes");
    assert!(client
        .exchange_application_access_token(ApplicationAccessTokenExchangeRequest {
            exchange: first_exchange,
        })
        .is_err());
    client
        .exchange_application_access_token(ApplicationAccessTokenExchangeRequest {
            exchange: signed_exchange(
                application_id,
                "key-2",
                &rotated_signing_key,
                application_scope(),
            ),
        })
        .expect("rotated key remains active");

    client
        .revoke_application_credential(revocation(application_id, None, "retire principal"))
        .expect("identity revokes");
    assert!(client
        .exchange_application_access_token(ApplicationAccessTokenExchangeRequest {
            exchange: signed_exchange(
                application_id,
                "key-2",
                &rotated_signing_key,
                application_scope(),
            ),
        })
        .is_err());

    let audit = read_application_audit_events(&audit_path).expect("audit reads");
    for operation in ["register_identity", "register_key", "issue_access_token"] {
        assert!(
            audit.iter().any(|event| event.operation == operation),
            "missing audit operation {operation}"
        );
    }
    assert!(audit.iter().any(|event| {
        event.operation == "revoke_credential" && event.key_id.as_deref() == Some("key-1")
    }));
    assert!(audit
        .iter()
        .any(|event| { event.operation == "revoke_credential" && event.key_id.is_none() }));
    let encoded = fs::read_to_string(&audit_path).expect("audit file");
    assert!(!encoded.contains("rotate key"));
    assert!(!encoded.contains("retire principal"));
    assert!(!encoded.contains(&BASE64.encode(first_signing_key.public_key().as_ref())));
    let persisted_authority = format!(
        "{}\n{}\n{}",
        fs::read_to_string(&identity_registry).expect("identity registry"),
        fs::read_to_string(&key_registry).expect("key registry"),
        encoded
    );
    assert!(!persisted_authority.contains("PRIVATE KEY"));
    assert!(!persisted_authority.contains(&BASE64.encode([11_u8; 32])));
    assert!(!persisted_authority.contains(&BASE64.encode([29_u8; 32])));

    cleanup(&root);
}

#[test]
fn mtls_mapping_is_rechecked_after_certificate_revocation() {
    let root = acceptance_root();
    fs::create_dir_all(&root).expect("acceptance root");
    let audit_path = root.join("application-audit.json");
    let handler = DaemonRequestHandler::new(
        AcceptanceOrchestrator,
        FixedDaemonClock::new("2026-07-16T15:05:00Z"),
    )
    .with_application_identity_registry_path(root.join("application-identities.json"))
    .with_application_key_registry_path(root.join("application-keys.json"))
    .with_application_audit_log_path(&audit_path);
    let actor = DaemonLocalActor::new(0).with_username("release-acceptance");
    let client = DaemonClient::new(InProcessDaemonTransport::new(move |request| {
        handler
            .handle_with_progress_for_actor(request, Some(&actor), |_| Ok(()))
            .map_err(|error| DaemonClientError::Transport(error.to_string()))
    }));
    let application_id = "mneion-mtls";
    let mut identity = application_identity(application_id, application_scope());
    identity.credential_kind = ApplicationCredentialKind::MtlsCertificate;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    identity.issued_at_unix_seconds = now.saturating_sub(60);
    identity.expires_at_unix_seconds = now + 3_600;
    client
        .register_application_identity(identity_registration(identity))
        .expect("mTLS identity registration");
    let fingerprint = format!("sha256:{}", "b".repeat(64));
    client
        .register_application_key(ApplicationKeyRegistrationRequest {
            key: ApplicationKeyDescriptor {
                schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
                application_id: application_id.to_string(),
                key_id: "certificate-1".to_string(),
                algorithm: ApplicationKeyAlgorithm::MtlsCertificate,
                public_key_fingerprint: fingerprint.clone(),
                public_key_material: None,
                issued_at_unix_seconds: now.saturating_sub(60),
                expires_at_unix_seconds: now + 3_600,
                active: true,
            },
            dry_run: false,
            client_request_id: Some("mneion-mtls-certificate".to_string()),
            administrator_actor: None,
            confirmation_marker: APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION.to_string(),
        })
        .expect("certificate mapping registers");

    let authorized = client
        .authorize_application_mtls(ApplicationMtlsAuthorizationRequest {
            certificate_fingerprint_sha256: fingerprint.clone(),
            requested_application_id: Some(application_id.to_string()),
            context: ApplicationMtlsAuthorizationContext::Request,
        })
        .expect("mTLS request authorization");
    assert!(authorized.authorized);

    client
        .revoke_application_credential(revocation(
            application_id,
            Some("certificate-1"),
            "replace certificate",
        ))
        .expect("certificate revokes");
    let rejected = client
        .authorize_application_mtls(ApplicationMtlsAuthorizationRequest {
            certificate_fingerprint_sha256: fingerprint,
            requested_application_id: Some(application_id.to_string()),
            context: ApplicationMtlsAuthorizationContext::Request,
        })
        .expect("revoked mTLS mapping returns typed rejection");
    assert!(!rejected.authorized);
    assert!(rejected.application_id.is_none());
    let audit = read_application_audit_events(&audit_path).expect("audit reads");
    assert!(audit
        .iter()
        .any(|event| event.operation == "reject_mtls_request"));

    cleanup(&root);
}

fn application_scope() -> ApplicationScope {
    ApplicationScope {
        store_ids: vec![StoreId::new("codex").expect("store id")],
        prefixes: vec!["analysis".to_string()],
        object_types: vec![ObjectType::Fastq],
        operations: vec![ApplicationOperation::Read, ApplicationOperation::Write],
        ingress_origin: IngressOrigin::Synoptikon,
        max_object_bytes: Some(10_000),
        max_total_bytes: Some(100_000),
    }
}

fn application_identity(application_id: &str, scope: ApplicationScope) -> ApplicationIdentity {
    ApplicationIdentity {
        schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
        application_id: application_id.to_string(),
        owner: "mnemosyne".to_string(),
        purpose: "release acceptance".to_string(),
        environment: ApplicationEnvironment::Production,
        credential_kind: ApplicationCredentialKind::AsymmetricKey,
        scope,
        issued_at_unix_seconds: 1_000,
        expires_at_unix_seconds: 100_000,
        active: true,
    }
}

fn identity_registration(identity: ApplicationIdentity) -> ApplicationIdentityRegistrationRequest {
    ApplicationIdentityRegistrationRequest {
        client_request_id: Some(format!("{}-identity", identity.application_id)),
        identity,
        dry_run: false,
        administrator_actor: None,
        confirmation_marker: APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION.to_string(),
    }
}

fn signing_key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).expect("Ed25519 key")
}

fn key_registration(
    application_id: &str,
    key_id: &str,
    signing_key: &Ed25519KeyPair,
) -> ApplicationKeyRegistrationRequest {
    let public_key = signing_key.public_key().as_ref();
    ApplicationKeyRegistrationRequest {
        key: ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: application_id.to_string(),
            key_id: key_id.to_string(),
            algorithm: ApplicationKeyAlgorithm::Ed25519,
            public_key_fingerprint: format!("sha256:{:x}", Sha256::digest(public_key)),
            public_key_material: Some(BASE64.encode(public_key)),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        },
        dry_run: false,
        client_request_id: Some(format!("{application_id}-{key_id}")),
        administrator_actor: None,
        confirmation_marker: APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION.to_string(),
    }
}

fn signed_exchange(
    application_id: &str,
    key_id: &str,
    signing_key: &Ed25519KeyPair,
    scope: ApplicationScope,
) -> AccessTokenExchangeRequest {
    let mut exchange = AccessTokenExchangeRequest {
        schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
        application_id: application_id.to_string(),
        key_id: key_id.to_string(),
        audience: "dasobjectstore".to_string(),
        requested_issued_at_unix_seconds: 2_000,
        requested_expires_at_unix_seconds: 2_600,
        scope,
        proof: String::new(),
    };
    exchange.proof = BASE64.encode(signing_key.sign(&exchange.signing_payload()).as_ref());
    exchange
}

fn revocation(
    application_id: &str,
    key_id: Option<&str>,
    reason: &str,
) -> ApplicationCredentialRevocationRequest {
    ApplicationCredentialRevocationRequest {
        application_id: application_id.to_string(),
        key_id: key_id.map(str::to_string),
        reason: reason.to_string(),
        dry_run: false,
        client_request_id: Some(format!("{application_id}-revoke")),
        administrator_actor: None,
        confirmation_marker: APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION.to_string(),
    }
}

fn acceptance_root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME is required"));
    let approved = home.join(".dasobjectstore-codex-validation");
    let configured = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| approved.clone());
    assert!(configured.starts_with(&approved));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    configured.join(format!(
        "application-auth-mvp-{}-{now}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

struct AcceptanceOrchestrator;

impl DaemonServiceOrchestrator for AcceptanceOrchestrator {
    fn status(
        &self,
        _request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        Err(unsupported("status"))
    }

    fn lifecycle(
        &self,
        _request: DaemonServiceLifecycleRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        Err(unsupported("lifecycle"))
    }

    fn provision(
        &self,
        _request: DaemonServiceProvisionRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
        Err(unsupported("provision"))
    }
}

fn unsupported(operation: &str) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("{operation} is outside application-auth acceptance"),
    }
}
