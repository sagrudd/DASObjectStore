//! Test-owned synthetic fixtures cannot supply an installed/backend authority.

use super::{codec, exact_source_continuity_source_response, owner};
use serde_json::Value;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("synthetic-conformance.json")).expect("retained fixture JSON")
}

fn bytes(fixture: &Value, name: &str) -> Vec<u8> {
    hex::decode(
        fixture["records"][name]["canonical_hex"]
            .as_str()
            .expect("hex"),
    )
    .expect("bytes")
}

fn request(value: &Value) -> codec::Request {
    codec::decode(&codec::canonical(value).expect("canonical"), 100).expect("synthetic request")
}

fn decoder(record: &str, input: &[u8]) -> Result<(), codec::Error> {
    match record {
        "binding" => codec::binding(input).map(|_| ()),
        "checkpoint" => {
            let checkpoint: codec::Checkpoint =
                serde_json::from_slice(input).map_err(|_| codec::Error::Denied)?;
            if codec::canonical(&checkpoint)? != input {
                return Err(codec::Error::Denied);
            }
            checkpoint.validate()
        }
        "external-object" => {
            let object: codec::Object =
                serde_json::from_slice(input).map_err(|_| codec::Error::Denied)?;
            if codec::canonical(&object)? != input {
                return Err(codec::Error::Denied);
            }
            object.validate()
        }
        _ => codec::decode(input, 100).map(|_| ()),
    }
}

#[test]
fn accepted_golden_bytes_hashes_and_domains_are_exact() {
    let fixture = fixtures();
    assert_eq!(
        fixture["contract_sha256"],
        "79e60cf49b93e9f947c284e35cffd316f7994a75827de495f626bb7beff76a9c"
    );
    for (name, record) in fixture["records"].as_object().expect("records") {
        let raw = bytes(&fixture, name);
        assert_eq!(codec::digest(&raw), record["canonical_sha256"]);
        assert_eq!(codec::canonical(&record["object"]).expect("JCS"), raw);
    }
    for preimage in fixture["preimages"]
        .as_object()
        .expect("preimages")
        .values()
    {
        let raw = hex::decode(preimage["hex"].as_str().expect("hex")).expect("bytes");
        assert_eq!(codec::digest(&raw), preimage["sha256"]);
    }
    for name in [
        "binding",
        "checkpoint",
        "external-object",
        "cas-request",
        "latest-request",
        "history-request",
    ] {
        decoder(name, &bytes(&fixture, name)).expect("inbound golden");
    }
    codec::binding(&bytes(&fixture, "binding-boundary-max")).expect("positive u64/L boundary");
    let checkpoint: codec::Checkpoint =
        serde_json::from_slice(&bytes(&fixture, "checkpoint")).expect("checkpoint");
    assert_eq!(
        checkpoint.root().expect("root"),
        fixture["preimages"]["checkpoint-root"]["sha256"]
    );
}

#[test]
fn every_retained_inbound_decoder_and_http_hostile_case_is_denied() {
    let fixture = fixtures();
    let mut checked = 0;
    for hostile in fixture["hostile"].as_array().expect("hostiles") {
        let record = hostile["record"].as_str().expect("record");
        let stage = hostile["expected_stage"].as_str().expect("stage");
        let input = hex::decode(hostile["input_hex"].as_str().expect("hex")).expect("bytes");
        assert_eq!(codec::digest(&input), hostile["input_sha256"]);
        if stage == "decoder"
            && matches!(
                record,
                "binding"
                    | "checkpoint"
                    | "external-object"
                    | "cas-request"
                    | "latest-request"
                    | "history-request"
            )
        {
            assert!(decoder(record, &input).is_err(), "{}", hostile["id"]);
            checked += 1;
        } else if stage == "http"
            && matches!(record, "cas-request" | "latest-request" | "history-request")
        {
            assert!(
                codec::http_body(&input, "fixture.das.invalid").is_err(),
                "{}",
                hostile["id"]
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 350);
}

#[test]
fn golden_http_uses_content_length_without_client_eof_and_no_second_request() {
    let fixture = fixtures();
    for name in ["cas-request", "latest-request", "history-request"] {
        let raw = hex::decode(fixture["records"][name]["http_hex"].as_str().expect("hex"))
            .expect("bytes");
        assert_eq!(
            codec::http_body(&raw, "fixture.das.invalid").expect("frame"),
            bytes(&fixture, name)
        );
        assert!(codec::http_body(&raw, "alias.invalid").is_err());
        let mut extra = raw;
        extra.push(b'x');
        assert!(codec::http_body(&extra, "fixture.das.invalid").is_err());
    }
}

#[test]
fn parsed_golden_cannot_admit_production_or_access_a_backend() {
    let fixture = fixtures();
    for name in ["cas-request", "latest-request", "history-request"] {
        let response = exact_source_continuity_source_response(&bytes(&fixture, name), 100);
        assert_eq!(response.0, 503);
        assert!(response.1.contains("unavailable"));
    }
}

fn exercise(
    fixture: &Value,
    value: &Value,
    objects: Vec<codec::Object>,
    fail_readback: bool,
) -> (Result<codec::Object, codec::Error>, Vec<codec::Object>) {
    owner::test_support::exercise(
        request(value),
        bytes(fixture, "binding"),
        bytes(fixture, "binding"),
        fixture["records"]["cas-request"]["object"]["binding_sha256"]
            .as_str()
            .expect("digest")
            .to_owned(),
        objects,
        100,
        fail_readback,
    )
}

#[test]
fn lost_ack_reserves_original_and_exact_replay_does_not_append() {
    let fixture = fixtures();
    let cas = &fixture["records"]["cas-request"]["object"];
    let (first, retained) = exercise(&fixture, cas, vec![], true);
    assert!(matches!(first, Err(codec::Error::Unavailable)));
    assert_eq!(retained.len(), 1);
    let (retry, retained) = exercise(&fixture, cas, retained, false);
    assert!(retry.is_ok());
    assert_eq!(retained.len(), 1);
    let mut different = cas.clone();
    different["checkpoint"]["history_root_sha256"] = Value::String("ff".repeat(32));
    let (conflict, retained) = exercise(&fixture, &different, retained, false);
    assert!(matches!(conflict, Err(codec::Error::Conflict)));
    assert_eq!(retained.len(), 1);
}

#[test]
fn advanced_remote_history_not_local_projection_controls_replay_and_restore() {
    let fixture = fixtures();
    let cas = &fixture["records"]["cas-request"]["object"];
    let (_, retained) = exercise(&fixture, cas, vec![], false);
    let mut successor = cas.clone();
    successor["expected_sequence"] = "1".into();
    successor["expected_floor"] = "100".into();
    let root = retained[0].checkpoint.root().expect("root");
    successor["expected_root_sha256"] = root.clone().into();
    successor["checkpoint"]["previous_root_sha256"] = root.into();
    successor["checkpoint"]["sequence"] = "2".into();
    successor["checkpoint"]["operation_id"] = "00000000-0000-4000-8000-000000000004".into();
    let (second, retained) = exercise(&fixture, &successor, retained, false);
    assert!(second.is_ok());
    let (retry, retained) = exercise(&fixture, cas, retained, false);
    assert!(retry.is_ok());
    assert_eq!(retained.len(), 2);
    let latest = &fixture["records"]["latest-request"]["object"];
    let (restored, _) = exercise(&fixture, latest, retained.clone(), false);
    assert_eq!(
        restored.expect("fresh remote head").checkpoint.sequence,
        "2"
    );
    let (gap, _) = exercise(&fixture, latest, vec![retained[1].clone()], false);
    assert!(matches!(gap, Err(codec::Error::Unavailable)));
    let mut future_floor = retained[0].clone();
    future_floor.checkpoint.floor = "101".into();
    let (rollback, _) = exercise(&fixture, latest, vec![future_floor], false);
    assert!(matches!(rollback, Err(codec::Error::Unavailable)));
}

fn scenario_result(
    fixture: &Value,
    value: &Value,
    scenario: owner::test_support::Scenario,
) -> (Result<codec::Object, codec::Error>, Vec<codec::Object>) {
    let binding = codec::digest(&scenario.current_binding);
    let mut value = value.clone();
    value["binding_sha256"] = binding.clone().into();
    owner::test_support::exercise_scenario(
        request(&value),
        bytes(fixture, "binding"),
        binding,
        vec![],
        100,
        scenario,
    )
}

#[test]
fn conditional_conflict_reconciles_only_identical_authenticated_winner() {
    let fixture = fixtures();
    let cas = &fixture["records"]["cas-request"]["object"];
    let winner: codec::Object =
        serde_json::from_value(fixture["records"]["external-object"]["object"].clone())
            .expect("winner");
    let mut scenario =
        owner::test_support::Scenario::ordinary(bytes(&fixture, "binding"), 100, false);
    scenario.competing_winner = Some(winner.clone());
    let (result, retained) = scenario_result(&fixture, cas, scenario);
    assert_eq!(result.expect("identical competing winner"), winner);
    assert_eq!(retained.len(), 1);

    let mut different = winner.clone();
    different.checkpoint.history_root_sha256 = "ee".repeat(32);
    let mut scenario =
        owner::test_support::Scenario::ordinary(bytes(&fixture, "binding"), 100, false);
    scenario.competing_winner = Some(different.clone());
    let (result, retained) = scenario_result(&fixture, cas, scenario);
    assert!(matches!(result, Err(codec::Error::Conflict)));
    assert_eq!(retained, vec![different]);

    let mut broken_prefix = winner.clone();
    broken_prefix.checkpoint.previous_root_sha256 = "ab".repeat(32);
    let mut scenario =
        owner::test_support::Scenario::ordinary(bytes(&fixture, "binding"), 100, false);
    scenario.competing_winner = Some(broken_prefix.clone());
    let (result, retained) = scenario_result(&fixture, cas, scenario);
    assert!(matches!(result, Err(codec::Error::Unavailable)));
    assert_eq!(retained, vec![broken_prefix]);

    let mut scenario =
        owner::test_support::Scenario::ordinary(bytes(&fixture, "binding"), 100, true);
    scenario.competing_winner = Some(winner.clone());
    let (result, retained) = scenario_result(&fixture, cas, scenario);
    assert!(matches!(result, Err(codec::Error::Unavailable)));
    assert_eq!(retained, vec![winner]);
}

#[test]
fn fresh_clock_denies_expiry_and_regression_before_create_and_after_remote_append() {
    let fixture = fixtures();
    let cas = &fixture["records"]["cas-request"]["object"];
    for clocks in [
        vec![100, 100, 100, 100, 100, 102],
        vec![100, 100, 100, 100, 100, 99],
    ] {
        let mut scenario =
            owner::test_support::Scenario::ordinary(bytes(&fixture, "binding"), 100, false);
        scenario.clocks = clocks;
        let (result, retained) = scenario_result(&fixture, cas, scenario);
        assert!(result.is_err());
        assert!(retained.is_empty());
    }
    let mut scenario =
        owner::test_support::Scenario::ordinary(bytes(&fixture, "binding"), 100, false);
    scenario.clocks = vec![100, 100, 100, 100, 100, 100, 102];
    let (result, retained) = scenario_result(&fixture, cas, scenario);
    assert!(matches!(result, Err(codec::Error::Denied)));
    assert_eq!(retained.len(), 1);

    let mut binding = fixture["records"]["binding"]["object"].clone();
    binding["expires_at"] = "101".into();
    let mut scenario = owner::test_support::Scenario::ordinary(
        codec::canonical(&binding).expect("binding"),
        100,
        false,
    );
    scenario.clocks = vec![100, 100, 100, 100, 100, 101];
    let (result, retained) = scenario_result(&fixture, cas, scenario);
    assert!(matches!(result, Err(codec::Error::Unavailable)));
    assert!(retained.is_empty());
}

#[test]
fn rotated_current_binding_preserves_original_genesis_and_old_expiry_is_not_authority() {
    let fixture = fixtures();
    let current = bytes(&fixture, "current-rotated-binding");
    let binding = codec::digest(&current);
    let mut cas = fixture["records"]["cas-request"]["object"].clone();
    cas["binding_sha256"] = binding.clone().into();
    cas["issued_at"] = "501".into();
    cas["expires_at"] = "503".into();
    cas["checkpoint"]["created_at"] = "501".into();
    cas["checkpoint"]["floor"] = "501".into();
    let request = codec::decode(&codec::canonical(&cas).expect("request"), 501).expect("request");
    let (result, retained) = owner::test_support::exercise_scenario(
        request,
        bytes(&fixture, "binding"),
        binding,
        vec![],
        501,
        owner::test_support::Scenario::ordinary(current, 501, false),
    );
    assert!(result.is_ok());
    assert_eq!(
        retained[0].checkpoint.previous_root_sha256,
        fixture["preimages"]["genesis"]["sha256"]
    );

    let mut unavailable = fixture["records"]["current-rotated-binding"]["object"].clone();
    unavailable["state"] = "revoked".into();
    let current = codec::canonical(&unavailable).expect("revoked");
    let binding = codec::digest(&current);
    cas["binding_sha256"] = binding.clone().into();
    let request = codec::decode(&codec::canonical(&cas).expect("request"), 501).expect("request");
    let (result, _) = owner::test_support::exercise_scenario(
        request,
        bytes(&fixture, "binding"),
        binding,
        vec![],
        501,
        owner::test_support::Scenario::ordinary(current, 501, false),
    );
    assert!(matches!(result, Err(codec::Error::Unavailable)));
}
