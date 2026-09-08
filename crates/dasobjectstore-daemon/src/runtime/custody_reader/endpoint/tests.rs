//! Pure server selection/replay tests. No fabricated ReaderContinuation or platform admission.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::signature::KeyPair;

fn fixture() -> (ReaderBindingV1, ReaderSealV1, Vec<u8>, SelectedRead) {
    let mut binding = ReaderBindingV1::decode(include_bytes!(
        "../../../../../../docs/adr/fixtures/0011-reader-wire/binding.jcs.json"
    ))
    .unwrap();
    let mut seal = ReaderSealV1::decode(include_bytes!(
        "../../../../../../docs/adr/fixtures/0011-reader-wire/seal.jcs.json"
    ))
    .unwrap();
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
    let authority = CustodyEd25519AuthorityV1 {
        authority_id: "synthetic-verifier".into(),
        algorithm: "ed25519".into(),
        public_key_base64: STANDARD.encode(key.public_key().as_ref()),
        public_key_sha256: raw_sha256(key.public_key().as_ref()),
    };
    let authority = serde_jcs::to_vec(&authority).unwrap();
    binding.verifier_authority_sha256 = raw_sha256(&authority);
    let receipt = CustodyIntegrityReceiptV1 {
        schema: dasobjectstore_object_service::custody::CUSTODY_RECEIPT_SCHEMA_V1.into(),
        assurance_class: "local_trusted_administrator_overlay".into(),
        store_id: dasobjectstore_core::ids::StoreId::new(binding.store_id.clone()).unwrap(),
        bucket_name: binding.bucket_name.clone(),
        target_id: "target".into(),
        object_id: "object".into(),
        object_key: format!("custody/sha256/{}", raw_sha256(b"abc")),
        content_sha256: raw_sha256(b"abc"),
        content_length: 3,
        object_type: "evidence".into(),
        version: 1,
        retention_until_utc: "2027-09-08T12:00:00Z".into(),
        legal_hold: true,
        object_lock_policy_sha256: binding.object_lock_policy_sha256.clone(),
        hold_authority: "authority".into(),
        reader_identity: binding.reader_identity.clone(),
        observed_at_utc: "2026-09-08T12:00:00Z".into(),
        configuration_sha256: binding.configuration_sha256.clone(),
        ledger_event_sha256: seal.ledger_head_sha256.clone(),
    };
    let receipt_jcs = serde_jcs::to_vec(&receipt).unwrap();
    seal.receipt_jcs_sha256 = vec![raw_sha256(&receipt_jcs)];
    let measurements = CustodyOffNucPreReadRequestV1 {
        schema: "dasobjectstore.local_trusted_administrator_custody_pre_read_request.v1".into(),
        assurance_class: "local_trusted_administrator_overlay".into(),
        request_id: uuid::Uuid::new_v4().to_string(),
        release_train: "r237".into(),
        release_stage: "s4".into(),
        purpose: "custody".into(),
        verifier_id: "verifier".into(),
        target_id: receipt.target_id.clone(),
        machine_identity_sha256: binding.host_identity_sha256.clone(),
        s3_endpoint_authority: "https://backend.test".into(),
        endpoint_authority_sha256: binding.endpoint_authority_sha256.clone(),
        tls_peer_sha256: binding.tls_peer_sha256.clone(),
        routing_sha256: "a".repeat(64),
        reader_identity: binding.reader_identity.clone(),
        store_id: binding.store_id.clone(),
        bucket_name: binding.bucket_name.clone(),
        stores_namespace_sha256: binding.stores_namespace_sha256.clone(),
        object_lock_policy_sha256: binding.object_lock_policy_sha256.clone(),
        lock_ledger_sha256: "b".repeat(64),
        ledger_head_sha256: seal.ledger_head_sha256.clone(),
        inventory_sha256: binding.inventory_sha256.clone(),
        lockset_sha256: "c".repeat(64),
        verifier_executable_sha256: "d".repeat(64),
        verifier_provenance_sha256: "e".repeat(64),
        receipt_jcs_sha256: raw_sha256(&receipt_jcs),
        nonce: uuid::Uuid::new_v4().to_string(),
        sequence: 1,
        previous_request_sha256: None,
        issued_at_utc: "2026-09-08T12:00:00Z".into(),
        expires_at_utc: "2026-09-08T13:00:00Z".into(),
    };
    (
        binding,
        seal,
        authority,
        SelectedRead {
            receipt_jcs,
            measurements,
        },
    )
}
fn policy() -> (RequestPolicy, CustodyOffNucPreReadRequestV1) {
    let (binding, seal, authority, selected) = fixture();
    let request = selected.measurements.clone();
    (
        RequestPolicy::new(
            &binding,
            &seal,
            "reader.test".into(),
            &authority,
            vec![selected],
            "2026-09-08T12:00:01Z",
        )
        .unwrap(),
        request,
    )
}

#[test]
fn every_non_attempt_measurement_substitution_denies_before_claim() {
    let (mut policy, request) = policy();
    let fields = serde_json::to_value(&request).unwrap();
    for field in fields.as_object().unwrap().keys() {
        if [
            "request_id",
            "nonce",
            "sequence",
            "previous_request_sha256",
            "issued_at_utc",
            "expires_at_utc",
        ]
        .contains(&field.as_str())
        {
            continue;
        }
        let mut changed = fields.clone();
        changed[field] = serde_json::json!(if field.ends_with("sha256") {
            "f".repeat(64)
        } else {
            "substituted".into()
        });
        let changed: CustodyOffNucPreReadRequestV1 = serde_json::from_value(changed).unwrap();
        assert!(
            policy.claim(&changed, "2026-09-08T12:00:02Z").is_err(),
            "{field}"
        );
        assert!(policy.attempted.is_empty());
    }
    policy.claim(&request, "2026-09-08T12:00:02Z").unwrap();
    assert!(policy.claim(&request, "2026-09-08T12:00:02Z").is_err());
    let mut same_nonce = request.clone();
    same_nonce.request_id = uuid::Uuid::new_v4().to_string();
    assert!(policy.claim(&same_nonce, "2026-09-08T12:00:02Z").is_err());
    let mut same_id = request.clone();
    same_id.nonce = uuid::Uuid::new_v4().to_string();
    assert!(policy.claim(&same_id, "2026-09-08T12:00:02Z").is_err());
}

#[test]
fn replay_capacity_never_evicts_live_entries_and_clock_regression_denies() {
    let (mut policy, mut request) = policy();
    for _ in 0..4096 {
        request.request_id = uuid::Uuid::new_v4().to_string();
        request.nonce = uuid::Uuid::new_v4().to_string();
        policy.claim(&request, "2026-09-08T12:00:02Z").unwrap();
    }
    request.request_id = uuid::Uuid::new_v4().to_string();
    request.nonce = uuid::Uuid::new_v4().to_string();
    assert!(policy.claim(&request, "2026-09-08T12:00:03Z").is_err());
    assert_eq!(policy.attempted.len(), 4096);
    request.expires_at_utc = "2026-09-08T14:00:00Z".into();
    assert!(policy.claim(&request, "2026-09-08T13:00:00Z").is_err());
    assert_eq!(policy.attempted.len(), 4096);
    request.request_id = uuid::Uuid::new_v4().to_string();
    request.nonce = uuid::Uuid::new_v4().to_string();
    assert!(policy.claim(&request, "2026-09-08T12:30:00Z").is_err());
}

#[test]
fn expired_id_or_nonce_cannot_reenter_with_a_new_valid_envelope() {
    let (mut policy, mut request) = policy();
    policy.claim(&request, "2026-09-08T12:00:02Z").unwrap();
    request.issued_at_utc = "2026-09-08T13:00:00Z".into();
    request.expires_at_utc = "2026-09-08T14:00:00Z".into();
    assert!(policy.claim(&request, "2026-09-08T13:00:01Z").is_err());
    let nonce = request.nonce.clone();
    request.nonce = uuid::Uuid::new_v4().to_string();
    assert!(policy.claim(&request, "2026-09-08T13:00:01Z").is_err());
    request.nonce = nonce;
    request.request_id = uuid::Uuid::new_v4().to_string();
    assert!(policy.claim(&request, "2026-09-08T13:00:01Z").is_err());
    assert_eq!(policy.attempted.len(), 1);
}

#[test]
fn authority_and_complete_receipt_selection_cannot_be_substituted() {
    let (mut binding, seal, authority, selected) = fixture();
    binding.verifier_authority_sha256 = "f".repeat(64);
    assert!(RequestPolicy::new(
        &binding,
        &seal,
        "reader.test".into(),
        &authority,
        vec![selected],
        "2026-09-08T12:00:01Z"
    )
    .is_err());
    let (binding, seal, authority, mut selected) = fixture();
    selected.measurements.routing_sha256 = "f".repeat(64); // Legitimate selected different measurement, never a request override.
    let old = policy().1;
    let mut configured = RequestPolicy::new(
        &binding,
        &seal,
        "reader.test".into(),
        &authority,
        vec![selected],
        "2026-09-08T12:00:01Z",
    )
    .unwrap();
    assert!(configured.claim(&old, "2026-09-08T12:00:02Z").is_err());
    let (binding, mut seal, authority, selected) = fixture();
    seal.receipt_jcs_sha256.push("f".repeat(64));
    assert!(RequestPolicy::new(
        &binding,
        &seal,
        "reader.test".into(),
        &authority,
        vec![selected],
        "2026-09-08T12:00:01Z"
    )
    .is_err());
}
