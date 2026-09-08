use super::*;
const B: &[u8] = include_bytes!("../../../../docs/adr/fixtures/0011-reader-wire/binding.jcs.json");
const C: &[u8] = include_bytes!("../../../../docs/adr/fixtures/0011-reader-wire/current.jcs.json");
const S: &[u8] = include_bytes!("../../../../docs/adr/fixtures/0011-reader-wire/seal.jcs.json");
#[test]
fn accepted_public_goldens_and_generation_identity() {
    let binding = ReaderBindingV1::decode(B).unwrap();
    let current = ReaderCurrentV1::decode(C).unwrap();
    let seal = ReaderSealV1::decode(S).unwrap();
    binding
        .verify(&current, &seal, "2026-09-08T12:00:02Z")
        .unwrap();
    assert_eq!(binding.encode().unwrap(), B);
    let revoked = ReaderCurrentV1::decode(include_bytes!(
        "../../../../docs/adr/fixtures/0011-reader-wire/revoked.jcs.json"
    ))
    .unwrap();
    revoked.follows(&current).unwrap();
    assert!(binding
        .verify(&revoked, &seal, "2026-09-08T13:00:02Z")
        .is_err());
    let next = ReaderBindingV1::decode(include_bytes!(
        "../../../../docs/adr/fixtures/0011-reader-wire/binding-generation-2.jcs.json"
    ))
    .unwrap();
    let next_current = ReaderCurrentV1::decode(include_bytes!(
        "../../../../docs/adr/fixtures/0011-reader-wire/current-generation-2.jcs.json"
    ))
    .unwrap();
    next_current.follows(&revoked).unwrap();
    next.verify(&next_current, &seal, "2026-09-08T13:00:02Z")
        .unwrap();
    assert_eq!(binding.reader_identity, next.reader_identity);
    assert_eq!(binding.configuration_sha256, next.configuration_sha256);
    assert!(next_current.follows(&current).is_err());
}
#[test]
fn strict_closed_records_and_bounds() {
    for raw in [B, C, S] {
        let mut value: Value = serde_json::from_slice(raw).unwrap();
        value["extra"] = Value::Bool(true);
        let added = serde_jcs::to_vec(&value).unwrap();
        let mut newline = raw.to_vec();
        newline.push(b'\n');
        let dup = format!(
            "{{\"schema\":\"wrong\",{}",
            std::str::from_utf8(raw).unwrap().trim_start_matches('{')
        );
        for bad in [added, newline, dup.into_bytes()] {
            assert!(ReaderBindingV1::decode(&bad).is_err());
            assert!(ReaderCurrentV1::decode(&bad).is_err());
            assert!(ReaderSealV1::decode(&bad).is_err());
        }
    }
    let mut b = ReaderBindingV1::decode(B).unwrap();
    b.credential_name = "../alias".into();
    assert!(b.encode().is_err());
    b = ReaderBindingV1::decode(B).unwrap();
    b.credential_generation = 9_007_199_254_740_992;
    assert!(b.encode().is_err());
    b = ReaderBindingV1::decode(B).unwrap();
    b.uid = 0;
    assert!(b.encode().is_err());
    b = ReaderBindingV1::decode(B).unwrap();
    b.uid = u32::MAX;
    assert!(b.encode().is_err());
    let mut s = ReaderSealV1::decode(S).unwrap();
    s.receipt_jcs_sha256.push(s.receipt_jcs_sha256[0].clone());
    assert!(s.encode().is_err());
}
#[test]
fn bindings_clock_and_replay_fail_closed() {
    let b = ReaderBindingV1::decode(B).unwrap();
    let c = ReaderCurrentV1::decode(C).unwrap();
    let s = ReaderSealV1::decode(S).unwrap();
    for time in [
        "2026-09-08T11:59:59Z",
        "2027-09-08T12:00:00Z",
        "2026-09-08T12:00:02+00:00",
    ] {
        assert!(b.verify(&c, &s, time).is_err());
    }
    let mut other = s.clone();
    other.inventory_sha256 = "f".repeat(64);
    assert!(b.verify(&c, &other, "2026-09-08T12:00:02Z").is_err());
    assert!(c.follows(&c).is_err());
}
