//! Adversarial regression coverage for the governed application boundary.
//!
//! These tests deliberately exercise the public daemon contracts rather than
//! reaching into a provider. A rejected request must leave byte accounting
//! unchanged, which proves the authority gate completed before provider work
//! could be admitted.

use dasobjectstore_core::{
    application_auth_v2::{
        ErgasterionCapabilityDiscoveryStateV1, ErgasterionCapabilityDiscoveryV1,
        GovernedHostAuthorityV2, GovernedHostModeV2, GovernedProsopikonAuthorityV2,
        ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS, ERGASTERION_CAPABILITY_DISCOVERY_SCHEMA_VERSION,
        ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION,
        ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS, GOVERNED_BINDING_SCHEMA_VERSION_V2,
    },
    backend::BackendObjectKey,
    ids::StoreId,
};
use dasobjectstore_daemon::{
    api::{
        ObjectBrowserDelegatedActor, OpaqueApplicationCapability, ProviderStreamCondition,
        ProviderStreamOpenRequest, ProviderStreamValidationError, PROVIDER_STREAM_SCHEMA_VERSION,
    },
    runtime::{
        issue_opaque_application_capability, revoke_application_capabilities,
        upsert_trusted_governed_binding_authority, validate_and_account_application_capability,
        verify_current_governed_authority_claims, ApplicationCapabilityClaims,
        ApplicationCapabilityIssue, ApplicationCapabilityUse, TrustedGovernedBindingAuthority,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: u64 = 1_800_000_000;
const TOKEN_PROBE: &str = "dosc_v2_abcdefghijklmnopqrstuvwxyz0123456789";

fn root(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
        })
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "ergasterion-adversarial-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&root).expect("test root");
    root
}

fn claims() -> ApplicationCapabilityClaims {
    ApplicationCapabilityClaims {
        application_id: "app-7e4a31c9b260".to_string(),
        key_id: "ergasterion-ed25519-2026-07-19".to_string(),
        binding_id: "binding-epic-001".to_string(),
        binding_digest_sha256: "a".repeat(64),
        tenant_id: "00000000-0000-0000-0000-000000000003".to_string(),
        host_authority: GovernedHostAuthorityV2 {
            mode: GovernedHostModeV2::Monas,
            authority_id: "00000000-0000-0000-0000-000000000001".to_string(),
            project_id: "epic-project".to_string(),
            project_revision: 17,
        },
        prosopikon_authority: GovernedProsopikonAuthorityV2 {
            authority_id: "00000000-0000-0000-0000-000000000002".to_string(),
            authority_revision: 41,
        },
        audience: "ergasterion-governed-data-service".to_string(),
        store_id: "epic-collection".to_string(),
        prefixes: vec!["EPICv1".to_string()],
        operations: vec!["list".to_string(), "read".to_string(), "verify".to_string()],
        max_object_bytes: 1_024,
        max_total_bytes: 4_096,
    }
}

fn issue(request_id: &str, nonce_byte: u8) -> ApplicationCapabilityIssue {
    ApplicationCapabilityIssue {
        request_id: request_id.to_string(),
        request_digest_sha256: "b".repeat(64),
        nonce: vec![nonce_byte; 32],
        issued_at_unix_seconds: NOW,
        expires_at_unix_seconds: NOW + 900,
        claims: claims(),
    }
}

fn issue_capability(
    root: &Path,
    request_id: &str,
    nonce_byte: u8,
) -> dasobjectstore_daemon::runtime::IssuedApplicationCapability {
    issue_opaque_application_capability(
        root.join("ledger.json"),
        root.join("master.key"),
        issue(request_id, nonce_byte),
        NOW,
    )
    .expect("issue capability")
}

#[test]
fn opaque_material_is_redacted_and_never_persisted_in_plaintext() {
    let root = root("redaction");
    let issued = issue_capability(&root, "request-redaction", 1);
    let opaque =
        OpaqueApplicationCapability::new(issued.opaque_capability.clone()).expect("opaque token");

    assert_eq!(
        format!("{opaque:?}"),
        "OpaqueApplicationCapability([REDACTED])"
    );
    let ledger = fs::read_to_string(root.join("ledger.json")).expect("ledger");
    let master = fs::read(root.join("master.key")).expect("master key");
    assert!(!ledger.contains(&issued.opaque_capability));
    assert!(!ledger.contains(TOKEN_PROBE));
    assert!(!master
        .windows(issued.opaque_capability.len())
        .any(|window| window == issued.opaque_capability.as_bytes()));
}

#[test]
fn malformed_bearers_fail_without_mutating_the_ledger() {
    let root = root("malformed");
    let issued = issue_capability(&root, "request-malformed", 2);
    let before = fs::read(root.join("ledger.json")).expect("ledger before");

    assert!(OpaqueApplicationCapability::new("short").is_err());
    assert!(validate_and_account_application_capability(
        root.join("ledger.json"),
        "not_a_das_capability_but_long_enough_to_parse",
        &use_request("EPICv1/object.cram", "read", 10),
    )
    .is_err());
    assert_eq!(
        fs::read(root.join("ledger.json")).expect("ledger after"),
        before,
        "a malformed bearer must not consume authority or byte budget"
    );
    assert!(issued.opaque_capability.starts_with("dosc_v2_"));
}

#[test]
fn exact_retry_is_stable_while_changed_or_nonce_replay_fails() {
    let root = root("replay");
    let first = issue_capability(&root, "request-replay", 3);
    let exact = issue_capability(&root, "request-replay", 3);
    assert!(exact.exact_replay);
    assert_eq!(exact.capability_id, first.capability_id);
    assert_eq!(exact.opaque_capability, first.opaque_capability);

    let mut changed = issue("request-replay", 3);
    changed.request_digest_sha256 = "c".repeat(64);
    assert!(issue_opaque_application_capability(
        root.join("ledger.json"),
        root.join("master.key"),
        changed,
        NOW,
    )
    .is_err());
    assert!(issue_opaque_application_capability(
        root.join("ledger.json"),
        root.join("master.key"),
        issue("different-request", 3),
        NOW,
    )
    .is_err());
}

#[test]
fn stale_scope_and_revocation_are_denied_before_byte_accounting() {
    let root = root("authorization");
    let issued = issue_capability(&root, "request-authorization", 4);

    assert!(validate_and_account_application_capability(
        root.join("ledger.json"),
        &issued.opaque_capability,
        &use_request("OTHER/object.cram", "read", 100),
    )
    .is_err());
    let first_authorized = validate_and_account_application_capability(
        root.join("ledger.json"),
        &issued.opaque_capability,
        &use_request("EPICv1/object.cram", "read", 100),
    )
    .expect("scope remains unused after denial");
    assert_eq!(first_authorized.accounted_bytes, 100);

    assert_eq!(
        revoke_application_capabilities(
            root.join("ledger.json"),
            "app-7e4a31c9b260",
            Some("ergasterion-ed25519-2026-07-19"),
            Some("binding-epic-001"),
            NOW + 1,
        )
        .expect("revoke"),
        1
    );
    assert!(validate_and_account_application_capability(
        root.join("ledger.json"),
        &issued.opaque_capability,
        &use_request("EPICv1/object.cram", "read", 1),
    )
    .is_err());
}

#[test]
fn changed_trusted_authority_invalidates_previously_issued_claims() {
    let root = root("stale-authority");
    let authority_path = root.join("authority.json");
    let issued = issue_capability(&root, "request-stale-authority", 5);
    let authority = TrustedGovernedBindingAuthority {
        binding_id: issued.claims.binding_id.clone(),
        object_store_id: StoreId::new(&issued.claims.store_id).expect("store"),
        binding_digest_sha256: issued.claims.binding_digest_sha256.clone(),
        tenant_id: issued.claims.tenant_id.clone(),
        host_authority: issued.claims.host_authority.clone(),
        prosopikon_authority: issued.claims.prosopikon_authority.clone(),
        admitted_at_unix_seconds: NOW - 1,
        expires_at_unix_seconds: NOW + 900,
        active: true,
        revoked_at_unix_seconds: None,
    };
    upsert_trusted_governed_binding_authority(&authority_path, authority.clone())
        .expect("admit exact authority");
    verify_current_governed_authority_claims(&authority_path, &issued.claims, NOW)
        .expect("exact authority");

    let mut advanced = authority;
    advanced.host_authority.project_revision += 1;
    upsert_trusted_governed_binding_authority(&authority_path, advanced)
        .expect("advance trusted authority");
    assert!(
        verify_current_governed_authority_claims(&authority_path, &issued.claims, NOW).is_err(),
        "a capability must not survive a host-project authority revision"
    );
}

#[test]
fn discovery_is_non_secret_and_exact_about_readiness_contract() {
    let discovery = ErgasterionCapabilityDiscoveryV1 {
        schema_version: ERGASTERION_CAPABILITY_DISCOVERY_SCHEMA_VERSION.to_string(),
        exchange_contract: ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION.to_string(),
        binding_schema: GOVERNED_BINDING_SCHEMA_VERSION_V2.to_string(),
        state: ErgasterionCapabilityDiscoveryStateV1::Ready,
        max_capability_lifetime_seconds: 900,
        renewal_window_seconds: ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS,
        clock_skew_seconds: ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS,
        operations: vec![
            dasobjectstore_core::application_auth::ApplicationOperation::List,
            dasobjectstore_core::application_auth::ApplicationOperation::Read,
            dasobjectstore_core::application_auth::ApplicationOperation::Verify,
        ],
    };
    discovery.validate().expect("discovery contract");
    let encoded = serde_json::to_string(&discovery).expect("serialize discovery");
    for forbidden in [
        "dosc_v2_",
        "private_key",
        "secret",
        "bucket",
        "endpoint",
        "/srv/",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "discovery exposed forbidden material: {forbidden}"
        );
    }
}

#[test]
fn provider_stream_rejects_mixed_application_and_delegated_authority() {
    let request = ProviderStreamOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id: "mixed-authority".to_string(),
        store_id: StoreId::new("epic-collection").expect("store"),
        object: BackendObjectKey {
            object_id: "EPICv1/object.cram".to_string(),
            version: 1,
        },
        delegated_actor: Some(ObjectBrowserDelegatedActor {
            username: "stephen".to_string(),
            uid: Some(1_000),
            primary_gid: Some(1_000),
            groups: vec!["mnemosyne".to_string()],
        }),
        application_capability: Some(
            OpaqueApplicationCapability::new(TOKEN_PROBE).expect("capability"),
        ),
        range: None,
        condition: ProviderStreamCondition::default(),
        chunk_size_bytes: 64 * 1024,
    };
    assert!(matches!(
        request.validate(),
        Err(ProviderStreamValidationError::InvalidDelegatedActor(message))
            if message.contains("mutually exclusive")
    ));
}

fn use_request(object_key: &str, operation: &str, bytes: u64) -> ApplicationCapabilityUse {
    ApplicationCapabilityUse {
        audience: "ergasterion-governed-data-service".to_string(),
        store_id: "epic-collection".to_string(),
        object_key: object_key.to_string(),
        operation: operation.to_string(),
        bytes,
        now_unix_seconds: NOW,
    }
}
