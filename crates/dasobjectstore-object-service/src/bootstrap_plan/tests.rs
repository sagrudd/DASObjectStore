use super::*;
use crate::{
    CustodyAssuranceClass, CustodyRetentionPolicyV1, CustodyStoreProfileV1,
    CUSTODY_OVERLAY_SCHEMA_V1, CUSTODY_PROFILE_V1,
};
use dasobjectstore_core::ids::StoreId;
use serde_json::{json, Value};

#[path = "fixture.rs"]
mod fixture;
use fixture::{bytes, fixture, hash};
fn check(m: &Value, o: &Value) -> Result<BootstrapPlan, PlanDenial> {
    let (m, o) = bytes(m, o);
    plan_bootstrap(&m, &o)
}

#[test]
fn result_is_deterministic_redacted_and_never_authority() {
    let (m, o) = fixture();
    let (raw, observed) = bytes(&m, &o);
    let result = plan_bootstrap(&raw, &observed).unwrap();
    assert_eq!(result, plan_bootstrap(&raw, &observed).unwrap());
    assert!(!result.execution_authorized);
    assert!(!result.live_target_verified);
    assert_eq!((result.store_count, result.object_count), (3, 2));
    assert_eq!(result.generated_receipt_limit, 1);
    let output = serde_json::to_string(&result).unwrap();
    for hidden in [
        "192.168",
        "/var/",
        "provisioner-ref",
        "custody-runtime",
        "r239-synthetic",
    ] {
        assert!(!output.contains(hidden));
    }
    let mut changed_raw = raw.clone();
    changed_raw.push(b' ');
    assert_eq!(
        plan_bootstrap(&changed_raw, &observed),
        Err(PlanDenial::Binding)
    );
}

#[test]
fn malformed_closed_nested_duplicate_and_oversized_inputs_deny() {
    let (m, o) = fixture();
    let (raw, observed) = bytes(&m, &o);
    for raw_bad in [
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{".to_vec(),
        vec![b' '; MAX_INPUT_BYTES + 1],
        format!(
            "{{\"schema\":\"a\",{}",
            std::str::from_utf8(&raw).unwrap().trim_start_matches('{')
        )
        .into_bytes(),
    ] {
        assert_eq!(
            plan_bootstrap(&raw_bad, &observed),
            Err(PlanDenial::Encoding)
        );
    }
    for pointer in [
        "",
        "/target",
        "/source",
        "/isolation",
        "/verifier",
        "/reader_continuation",
        "/stores/0/definition/profile",
        "/stores/0/definition/profile/retention",
        "/stores/0/content_policy/objects/0",
    ] {
        let mut bad = m.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_secret".into(), json!("never-echo-this"));
        assert_eq!(check(&bad, &o), Err(PlanDenial::Encoding), "{pointer}");
    }
    let duplicate_nested = String::from_utf8(raw).unwrap().replacen(
        "\"architecture\":\"amd64\"",
        "\"architecture\":\"amd64\",\"architecture\":\"amd64\"",
        1,
    );
    assert_eq!(
        plan_bootstrap(duplicate_nested.as_bytes(), &observed),
        Err(PlanDenial::Encoding)
    );
}

#[test]
fn all_manifest_fields_are_required() {
    let (m, o) = fixture();
    for key in m.as_object().unwrap().keys() {
        let mut bad = m.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert_eq!(check(&bad, &o), Err(PlanDenial::Encoding), "{key}");
    }
    for key in o
        .as_object()
        .unwrap()
        .keys()
        .filter(|v| *v != "manifest_sha256")
    {
        let mut bad = o.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert_eq!(check(&m, &bad), Err(PlanDenial::Encoding), "{key}");
    }
}

#[test]
fn binding_custody_time_and_evidence_mutation_matrix_denies() {
    let (m, o) = fixture();
    let mutations = [
        ("/schema", json!("v2")),
        ("/transaction_id", json!("r237-reuse")),
        ("/purpose", json!("apply")),
        ("/target/endpoint", json!("192.168.0.48")),
        ("/target/architecture", json!("arm64")),
        ("/source/revision", json!("main")),
        ("/source/image_digest", json!("latest")),
        ("/maximum_runtime_seconds", json!(0)),
        ("/maximum_runtime_seconds", json!(3600)),
        ("/expires_at_utc", json!("2026-09-07T12:00:00Z")),
        ("/issued_at_utc", json!("2026-09-07T12:00:00+00:00")),
        ("/stores/0/purpose", json!("nuc-delivery")),
        ("/stores/0/definition/profile/legal_hold", json!(false)),
        ("/stores/0/definition/profile/target_id", json!("unbound")),
        (
            "/stores/0/definition/profile/writer_identity",
            json!("reader-0"),
        ),
        (
            "/stores/0/definition/profile/writer_credential_reference",
            json!("reader-ref-0"),
        ),
        (
            "/stores/0/definition/profile/reader_identity",
            json!("reader-1"),
        ),
        (
            "/stores/0/definition/store_id",
            json!("r237_s4_bootstrap_custody"),
        ),
        (
            "/stores/0/definition/bucket_name",
            json!("dos-r237-s4-bootstrap-custody"),
        ),
        ("/stores/0/content_policy/objects", json!([])),
        ("/stores/0/content_policy/objects/0/size_bytes", json!(0)),
        (
            "/stores/0/content_policy/objects/0/content_sha256",
            json!("bad"),
        ),
        ("/verifier/machine_identity_sha256", json!(hash('a'))),
        ("/reader_continuation/read_only", json!(false)),
        (
            "/reader_continuation/available_until_utc",
            json!("2026-09-07T14:00:00Z"),
        ),
        ("/marker_exclusion_evidence_sha256", json!("asserted")),
    ];
    for (pointer, value) in mutations {
        let mut bad = m.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(check(&bad, &o).is_err(), "{pointer}");
    }
    for key in [
        "ordinary_plane_excluded",
        "marker_excluded_from_backup",
        "verifier_independent",
        "reader_continuation_available",
    ] {
        let mut bad = o.clone();
        bad[key] = json!(false);
        assert!(check(&m, &bad).is_err(), "{key}");
    }
}

#[test]
fn isolation_alias_and_preexisting_state_matrix_denies() {
    let (m, o) = fixture();
    for (pointer, value) in [
        (
            "/isolation/marker_path",
            json!("/var/lib/dasobjectstore/custody-activation.json"),
        ),
        (
            "/isolation/paths/data",
            json!("/var/lib/dasobjectstore/data"),
        ),
        ("/isolation/paths/data", json!("/var/lib/custody")),
        ("/isolation/paths/data", json!("/var/lib/../data")),
        ("/isolation/paths/data", json!("/var/lib/custody/ledger")),
        (
            "/isolation/private_endpoint",
            json!("http://localhost:3900"),
        ),
        ("/isolation/private_endpoint", json!("http://[::1]:3900")),
        (
            "/isolation/custody_endpoint",
            json!("http://192.168.0.193:4901"),
        ),
        ("/isolation/garage_project", json!("custody-service")),
    ] {
        let mut bad = m.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(check(&bad, &o).is_err(), "{pointer}");
    }
    for (key, value) in [
        ("existing_paths", "/var/lib/custody/data"),
        ("symlink_paths", "/var/lib"),
        ("existing_buckets", "custody-bucket-0"),
        ("existing_store_ids", "custody-0"),
        ("existing_namespaces", "namespace-0"),
    ] {
        let mut bad = o.clone();
        bad[key] = json!([value]);
        assert!(check(&m, &bad).is_err(), "{key}");
    }
}

#[test]
fn static_denials_never_echo_input() {
    for error in [
        PlanDenial::Encoding,
        PlanDenial::Binding,
        PlanDenial::Time,
        PlanDenial::Custody,
        PlanDenial::Isolation,
        PlanDenial::Evidence,
    ] {
        assert!(error.to_string().starts_with("bootstrap_plan_"));
        assert!(!error.to_string().contains('/'));
    }
}

#[test]
fn every_nested_field_is_required_and_each_json_prefix_denied() {
    fn fields(value: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    out.push((prefix.into(), key.clone()));
                    fields(child, &format!("{prefix}/{key}"), out);
                }
            }
            Value::Array(items) => {
                for (n, child) in items.iter().enumerate() {
                    fields(child, &format!("{prefix}/{n}"), out);
                }
            }
            _ => (),
        }
    }
    let (m, o) = fixture();
    let mut all = Vec::new();
    fields(&m, "", &mut all);
    for (pointer, key) in &all {
        let mut bad = m.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(check(&bad, &o).is_err(), "{pointer}/{key}");
    }
    let (raw, observed) = bytes(&m, &o);
    for end in 0..raw.len() {
        assert!(plan_bootstrap(&raw[..end], &observed).is_err());
    }
    assert!(
        all.len() > 100,
        "recursive coverage must include every store profile"
    );
}

#[test]
fn locked_command_verifier_and_role_path_binding_denials() {
    let (m, o) = fixture();
    for (pointer, value) in [
        ("/source/compiler_identity", json!("")),
        ("/source/locked_commands/0/arguments", json!(["build"])),
        ("/source/locked_commands/0/exit_status", json!(1)),
        ("/source/locked_commands/1/operation", json!("build")),
        ("/verifier/endpoint", json!("https://192.168.0.193:4902")),
        ("/verifier/tls_identity_sha256", json!("asserted")),
        (
            "/verifier/journal_backup_exclusion_evidence_sha256",
            json!("asserted"),
        ),
        (
            "/isolation/paths/data",
            json!("/var/lib/custody/metadata/subdir"),
        ),
        (
            "/isolation/paths/catalog",
            json!("/var/lib/external-marker/claim"),
        ),
    ] {
        let mut bad = m.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(check(&bad, &o).is_err(), "{pointer}");
    }
}

#[test]
fn generated_receipt_is_bounded_executor_policy_never_prehash_or_payload() {
    let (m, o) = fixture();
    for (field, value) in [
        ("kind", json!("other")),
        ("schema", json!("other")),
        ("maximum_count", json!(2)),
        ("maximum_size_bytes", json!(65537)),
        ("payload_source", json!("caller")),
        ("required_bindings", json!([])),
    ] {
        let mut bad = m.clone();
        bad["stores"][2]["content_policy"][field] = value;
        assert!(check(&bad, &o).is_err(), "{field}");
    }
    for field in [
        "content_sha256",
        "payload",
        "signature",
        "objects",
        "size_bytes",
    ] {
        let mut bad = m.clone();
        bad["stores"][2]["content_policy"][field] = json!("guessed");
        assert_eq!(check(&bad, &o), Err(PlanDenial::Encoding), "{field}");
    }
    let mut bad = m.clone();
    bad["stores"][2]["content_policy"] = m["stores"][0]["content_policy"].clone();
    assert_eq!(check(&bad, &o), Err(PlanDenial::Custody));
    let mut bad = m.clone();
    bad["stores"][0]["content_policy"] = m["stores"][2]["content_policy"].clone();
    assert_eq!(check(&bad, &o), Err(PlanDenial::Custody));
    let mut bad = m.clone();
    bad["stores"][2]["content_policy"]["required_bindings"][1] = json!("target_identity");
    assert_eq!(check(&bad, &o), Err(PlanDenial::Custody));
    let good = check(&m, &o).unwrap();
    assert_eq!(good.object_count, 2);
    assert!(!good.execution_authorized);
}
#[path = "inventory_review.rs"]
mod inventory_review;
