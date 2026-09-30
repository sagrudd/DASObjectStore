use serde_json::Value;

const SCHEMA: &str =
    include_str!("../../../docs/schemas/dasobjectstore.service-provision-plan.v1.schema.json");
const FIXTURE: &str =
    include_str!("../../../docs/contracts/service-provision-plan-v1.fixture.json");

#[test]
fn proposal_schema_and_fixture_fail_closed_on_provider_visibility() {
    let schema: Value = serde_json::from_str(SCHEMA).expect("plan schema is JSON");
    let fixture: Value = serde_json::from_str(FIXTURE).expect("plan fixture is JSON");

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
        assert_eq!(actions[0]["secret_material_included"], false);
        assert_eq!(actions[1]["kind"], "create_bucket");
        assert_eq!(actions[2]["kind"], "allow_bucket");
        assert_eq!(
            actions[2]["grants"],
            serde_json::json!(["read", "write", "owner"])
        );
        action_count += actions.len();
    }
    assert_eq!(fixture["resource_action_count"], action_count);

    let serialized = fixture.to_string();
    assert!(!serialized.contains("access_key_id"));
    assert!(!serialized.contains("secret_access_key"));
}
