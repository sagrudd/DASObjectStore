//! Independent pure-validator/planner conformance, never live custody evidence.
use super::*;
use crate::validate_custody_inventory;

#[test]
fn inventory_review_library_requires_exact_bare_lowercase_digest() {
    let plain = "a".repeat(64);
    assert!(validate_custody_inventory([(plain.as_str(), 1)]).is_ok());
    for invalid in [
        format!("sha256:{plain}"),
        "A".repeat(64),
        "g".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        String::new(),
    ] {
        assert!(validate_custody_inventory([(invalid.as_str(), 1)]).is_err());
    }
}

#[test]
fn inventory_review_cardinality_and_arithmetic_boundaries() {
    let hashes: Vec<_> = (0..4097).map(|n| format!("{n:064x}")).collect();
    assert!(validate_custody_inventory(hashes[..4096].iter().map(|h| (h.as_str(), 1))).is_ok());
    assert!(validate_custody_inventory(hashes.iter().map(|h| (h.as_str(), 1))).is_err());
    assert!(validate_custody_inventory(std::iter::empty::<(&str, u64)>()).is_err());
    assert!(validate_custody_inventory([(hashes[0].as_str(), 0)]).is_err());
    assert!(
        validate_custody_inventory([(hashes[0].as_str(), 1), (hashes[0].as_str(), 2)]).is_err()
    );
    assert!(validate_custody_inventory([(hashes[0].as_str(), u64::MAX)]).is_ok());
    assert!(
        validate_custody_inventory([(hashes[0].as_str(), u64::MAX), (hashes[1].as_str(), 1)])
            .is_err()
    );
}

#[test]
fn inventory_review_planner_keeps_its_prefixed_wire_format() {
    let (manifest, observation) = fixture();
    assert!(check(&manifest, &observation).is_ok());
    let plain = "b".repeat(64);
    for invalid in [
        plain.clone(),
        format!("sha256:sha256:{plain}"),
        format!("SHA256:{plain}"),
        format!("sha256:{}", "B".repeat(64)),
        format!("sha256:{plain}\n"),
    ] {
        let mut changed = manifest.clone();
        changed["stores"][0]["content_policy"]["objects"][0]["content_sha256"] = invalid.into();
        assert_eq!(check(&changed, &observation), Err(PlanDenial::Custody));
    }
}

#[test]
fn inventory_review_planner_accepts_limit_and_denies_next_entry() {
    let (mut manifest, observation) = fixture();
    let objects: Vec<_> = (0..4096)
        .map(|n| {
            json!({
                "content_sha256":format!("sha256:{n:064x}"), "size_bytes":1
            })
        })
        .collect();
    manifest["stores"][0]["content_policy"]["objects"] = objects.into();
    let result = check(&manifest, &observation).unwrap();
    assert_eq!(result.object_count, 4097); // 4096 corpus + one separate delivery.
    assert!(!result.execution_authorized);
    assert!(!result.live_target_verified);
    manifest["stores"][0]["content_policy"]["objects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"content_sha256":format!("sha256:{:064x}",4096),"size_bytes":1}));
    assert_eq!(check(&manifest, &observation), Err(PlanDenial::Custody));
}

#[test]
fn inventory_review_planner_rejects_checked_total_overflow() {
    let (mut manifest, observation) = fixture();
    manifest["stores"][0]["content_policy"]["objects"] = json!([
        {"content_sha256":hash('a'),"size_bytes":u64::MAX},
        {"content_sha256":hash('b'),"size_bytes":1}
    ]);
    assert_eq!(check(&manifest, &observation), Err(PlanDenial::Custody));
}
