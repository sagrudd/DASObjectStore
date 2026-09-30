use jsonschema::{Draft, JSONSchema};
use serde_json::Value;

const SCHEMA: &str =
    include_str!("../../../docs/schemas/dasobjectstore.service-provision-plan.v1.schema.json");
const FIXTURE: &str =
    include_str!("../../../docs/contracts/service-provision-plan-v1.fixture.json");

#[test]
fn proposal_schema_and_fixture_fail_closed_on_provider_visibility() {
    let schema: Value = serde_json::from_str(SCHEMA).expect("plan schema is JSON");
    let fixture: Value = serde_json::from_str(FIXTURE).expect("plan fixture is JSON");
    let validator = JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .expect("plan schema compiles as Draft 2020-12");

    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        "dasobjectstore.service_provision_plan.v1"
    );
    assert_eq!(
        schema["properties"]["provider_visibility"]["const"],
        "unknown"
    );
    assert_eq!(schema["properties"]["provider_observation"]["type"], "null");
    assert_eq!(schema["properties"]["execution_authorized"]["const"], false);

    assert_eq!(
        fixture["schema_version"],
        "dasobjectstore.service_provision_plan.v1"
    );
    assert_eq!(fixture["provider_visibility"], "unknown");
    assert!(fixture["provider_observation"].is_null());
    assert_eq!(fixture["execution_authorized"], false);
    assert_eq!(fixture["provenance"]["source_revision"], "0".repeat(40));
    assert!(
        validator.is_valid(&fixture),
        "synthetic fixture validates against schema"
    );

    let mut observed = fixture.clone();
    observed["provider_visibility"] = serde_json::json!("observed");
    assert!(!validator.is_valid(&observed));

    let mut wrong_grant_operand = fixture;
    wrong_grant_operand["stores"][0]["actions"][2]["key_name"] =
        serde_json::json!("dasobjectstore:synthetic-store-a");
    assert!(!validator.is_valid(&wrong_grant_operand));
}

#[test]
fn fixture_rows_bind_a_complete_catalogue_snapshot_without_credentials() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("plan fixture is JSON");
    let snapshot = &fixture["source_snapshot"];
    let stores = fixture["stores"].as_array().expect("stores is an array");

    assert_eq!(snapshot["complete"], true);
    assert_eq!(snapshot["scope"], "normal_s3_exported_store_registry");
    assert_eq!(
        snapshot["record_count"].as_u64().unwrap(),
        snapshot["eligible_store_count"].as_u64().unwrap()
            + snapshot["excluded_store_count"].as_u64().unwrap()
    );
    assert_eq!(
        stores.len() as u64,
        snapshot["eligible_store_count"].as_u64().unwrap()
    );

    let mut store_ids = std::collections::BTreeSet::new();
    let mut bucket_names = std::collections::BTreeSet::new();
    let mut action_count = 0;
    for store in stores {
        let store_id = store["store_id"].as_str().unwrap();
        let bucket_name = store["bucket_name"].as_str().unwrap();
        assert!(store_ids.insert(store_id));
        assert!(bucket_names.insert(bucket_name));

        let actions = store["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0]["kind"], "import_key");
        assert_eq!(actions[0]["key_name"], store["key_name"]);
        assert_eq!(actions[0]["credential_binding"]["status"], "unresolved");
        let expected_reference = format!("secret://dasobjectstore/stores/{store_id}/s3");
        assert_eq!(
            actions[0]["credential_binding"]["credential_reference"],
            expected_reference
        );
        assert_eq!(actions[1]["kind"], "create_bucket");
        assert_eq!(actions[2]["kind"], "allow_bucket");
        assert!(actions[2].get("key_name").is_none());
        assert_eq!(actions[2]["credential_binding"]["status"], "unresolved");
        assert_eq!(
            actions[2]["credential_binding"]["credential_reference"],
            expected_reference
        );
        assert_eq!(
            actions[2]["grants"],
            serde_json::json!(["read", "write", "owner"])
        );
        action_count += actions.len();
    }
    assert_eq!(fixture["resource_action_count"], action_count);

    assert!(!has_credential_payload(&fixture));
}

#[test]
fn reference_consumer_rejects_a_credential_binding_for_another_store() {
    let schema: Value = serde_json::from_str(SCHEMA).expect("plan schema is JSON");
    let fixture: Value = serde_json::from_str(FIXTURE).expect("plan fixture is JSON");
    let validator = JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .expect("plan schema compiles as Draft 2020-12");

    let mut mismatched = fixture;
    mismatched["stores"][0]["actions"][2]["credential_binding"]["credential_reference"] =
        serde_json::json!("secret://dasobjectstore/stores/synthetic-store-b/s3");

    // JSON Schema checks the reference syntax; the consumer must also bind it
    // to the enclosing catalogue row's store identity.
    assert!(validator.is_valid(&mismatched));
    assert!(!credential_bindings_match_stores(&mismatched));
}

fn credential_bindings_match_stores(plan: &Value) -> bool {
    plan["stores"]
        .as_array()
        .into_iter()
        .flatten()
        .all(|store| {
            let Some(store_id) = store["store_id"].as_str() else {
                return false;
            };
            let expected = format!("secret://dasobjectstore/stores/{store_id}/s3");
            [0, 2].iter().all(|action| {
                store["actions"][*action]["credential_binding"]["credential_reference"] == expected
            })
        })
}

fn has_credential_payload(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields
                .keys()
                .any(|name| matches!(name.as_str(), "access_key_id" | "secret_access_key"))
                || fields.values().any(has_credential_payload)
        }
        Value::Array(items) => items.iter().any(has_credential_payload),
        _ => false,
    }
}
