use std::{collections::BTreeSet, env, fs, path::PathBuf};

use serde_json::Value;

const PRODUCT_MANIFEST_SCHEMA_VERSION: &str = "mnemosyne.product.manifest.v1";

#[test]
fn product_manifest_declares_dasobjectstore_plugin_contract() {
    let manifest = product_manifest();

    assert_eq!(manifest["schema_version"], PRODUCT_MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest["product"]["id"], "dasobjectstore");
    assert_eq!(manifest["product"]["name"], "DASObjectStore");
    assert_eq!(manifest["product"]["version"], "0.0.0");
    assert!(manifest["modes"]["standalone"]
        .as_bool()
        .expect("standalone is bool"));
    assert!(manifest["modes"]["synoptikon_integrated"]
        .as_bool()
        .expect("synoptikon_integrated is bool"));
    assert_eq!(
        manifest["surfaces"]["api"]["base_path"],
        "/products/dasobjectstore/api"
    );
    assert_eq!(
        manifest["surfaces"]["api"]["health_path"],
        "/products/dasobjectstore/health"
    );
    assert_eq!(
        manifest["surfaces"]["web"]["base_path"],
        "/products/dasobjectstore"
    );
    assert_eq!(manifest["data_model"]["persistence"], "hybrid");
    assert!(manifest["support"]["standalone"]["supported"]
        .as_bool()
        .expect("standalone support is bool"));
    assert!(manifest["support"]["standalone"]["local_authentication"]
        .as_bool()
        .expect("local_authentication is bool"));
    assert!(manifest["support"]["standalone"]["local_hardware"]
        .as_bool()
        .expect("local_hardware is bool"));
    assert!(manifest["support"]["synoptikon_integrated"]["supported"]
        .as_bool()
        .expect("Synoptikon support is bool"));
    assert!(
        manifest["support"]["synoptikon_integrated"]["uses_synoptikon_accounts"]
            .as_bool()
            .expect("uses_synoptikon_accounts is bool")
    );
    assert!(
        manifest["support"]["synoptikon_integrated"]["uses_synoptikon_audit"]
            .as_bool()
            .expect("uses_synoptikon_audit is bool")
    );

    let package_behaviors = manifest["support"]["standalone"]["package_behaviors"]
        .as_array()
        .expect("package_behaviors is an array");
    assert_eq!(package_behaviors.len(), 2);
    for behavior in package_behaviors {
        assert_eq!(behavior["product_root"], "/opt/dasobjectstore");
        assert!(!behavior["requires_synoptikon"]
            .as_bool()
            .expect("requires_synoptikon is bool"));
        assert!(!behavior["external_sql_allowed"]
            .as_bool()
            .expect("external_sql_allowed is bool"));
        assert!(!behavior["object_store_required"]
            .as_bool()
            .expect("object_store_required is bool"));
        assert_eq!(
            behavior["migration_export_contract"],
            "mnemosyne.dasobjectstore.storage_endpoint_export_bundle.v1"
        );
    }
}

#[test]
fn product_manifest_matches_mnemosyne_schema_expectations_when_available() {
    let Some(schema_path) = mnemosyne_product_schema_path() else {
        eprintln!(
            "skipping sibling Mnemosyne schema validation: set \
             DASOBJECTSTORE_MNEMOSYNE_PRODUCT_SCHEMA or check out ../Mnemosyne"
        );
        return;
    };
    let schema = read_json(schema_path);
    let manifest = product_manifest();

    assert_eq!(
        manifest["schema_version"],
        schema["properties"]["schema_version"]["const"]
    );
    assert_schema_object_shape(&manifest, &schema, "");

    assert_schema_object_shape(
        &manifest["product"],
        &schema["properties"]["product"],
        "product",
    );
    assert_identifier(
        manifest["product"]["id"]
            .as_str()
            .expect("product.id string"),
    );
    assert_schema_object_shape(
        &manifest["compatibility"],
        &schema["properties"]["compatibility"],
        "compatibility",
    );
    assert_schema_object_shape(&manifest["modes"], &schema["properties"]["modes"], "modes");
    assert_schema_object_shape(
        &manifest["support"],
        &schema["properties"]["support"],
        "support",
    );
    assert_schema_object_shape(
        &manifest["required_synoptikon_api"],
        &schema["properties"]["required_synoptikon_api"],
        "required_synoptikon_api",
    );
    assert_schema_object_shape(
        &manifest["surfaces"],
        &schema["properties"]["surfaces"],
        "surfaces",
    );
    assert_schema_object_shape(
        &manifest["data_model"],
        &schema["properties"]["data_model"],
        "data_model",
    );
    assert_schema_object_shape(
        &manifest["workflow_model"],
        &schema["properties"]["workflow_model"],
        "workflow_model",
    );

    assert_values_are_in_schema_enum(
        manifest["support"]["standalone"]["package_formats"]
            .as_array()
            .expect("package_formats array"),
        &schema["$defs"]["standalone_support"]["properties"]["package_formats"]["items"]["enum"],
        "support.standalone.package_formats",
    );
    for behavior in manifest["support"]["standalone"]["package_behaviors"]
        .as_array()
        .expect("package_behaviors array")
    {
        assert_value_is_in_schema_enum(
            &behavior["host"],
            &schema["$defs"]["standalone_package_behavior"]["properties"]["host"]["enum"],
            "support.standalone.package_behaviors.host",
        );
        assert_values_are_in_schema_enum(
            behavior["layouts"].as_array().expect("layouts array"),
            &schema["$defs"]["standalone_package_behavior"]["properties"]["layouts"]["items"]
                ["enum"],
            "support.standalone.package_behaviors.layouts",
        );
        assert_value_is_in_schema_enum(
            &behavior["state_store"],
            &schema["$defs"]["standalone_package_behavior"]["properties"]["state_store"]["enum"],
            "support.standalone.package_behaviors.state_store",
        );
    }
    assert_values_are_in_schema_enum(
        manifest["support"]["migration"]["directions"]
            .as_array()
            .expect("migration directions array"),
        &schema["$defs"]["migration_support"]["properties"]["directions"]["items"]["enum"],
        "support.migration.directions",
    );
    assert_value_is_in_schema_enum(
        &manifest["data_model"]["persistence"],
        &schema["properties"]["data_model"]["properties"]["persistence"]["enum"],
        "data_model.persistence",
    );
    assert_values_are_in_schema_enum(
        manifest["workflow_model"]["execution_providers"]
            .as_array()
            .expect("execution_providers array"),
        &schema["properties"]["workflow_model"]["properties"]["execution_providers"]["items"]
            ["enum"],
        "workflow_model.execution_providers",
    );
}

fn product_manifest() -> Value {
    read_json(repo_root().join("product-manifest.json"))
}

fn mnemosyne_product_schema_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("DASOBJECTSTORE_MNEMOSYNE_PRODUCT_SCHEMA") {
        return Some(PathBuf::from(path));
    }
    let sibling_root = repo_root()
        .parent()
        .expect("repo root has a parent")
        .to_path_buf();
    ["Mnemosyne", "mnemosyne"]
        .into_iter()
        .map(|directory| {
            sibling_root
                .join(directory)
                .join("mneion-api-types/schemas/mnemosyne.product.manifest.v1.schema.json")
        })
        .find(|path| path.exists())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn read_json(path: PathBuf) -> Value {
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("parse {}: {error}", path.display());
    })
}

fn assert_schema_object_shape(instance: &Value, schema: &Value, label: &str) {
    let object = instance.as_object().unwrap_or_else(|| {
        panic!("{label} must be an object");
    });
    let properties = schema["properties"].as_object().unwrap_or_else(|| {
        panic!("{label} schema must declare properties");
    });
    let required = schema["required"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().expect("required item string"))
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    for key in &required {
        assert!(
            object.contains_key(*key),
            "{label} is missing required schema field `{key}`"
        );
    }
    if schema["additionalProperties"] == false {
        for key in object.keys() {
            assert!(
                properties.contains_key(key),
                "{label} contains non-schema field `{key}`"
            );
        }
    }
}

fn assert_value_is_in_schema_enum(value: &Value, schema_enum: &Value, label: &str) {
    let allowed = schema_enum.as_array().unwrap_or_else(|| {
        panic!("{label} schema enum is an array");
    });
    assert!(
        allowed.contains(value),
        "{label} value `{value}` is not allowed by the Mnemosyne schema"
    );
}

fn assert_values_are_in_schema_enum(values: &[Value], schema_enum: &Value, label: &str) {
    let mut seen = BTreeSet::new();
    for value in values {
        assert!(
            seen.insert(value.to_string()),
            "{label} contains duplicate value `{value}`"
        );
        assert_value_is_in_schema_enum(value, schema_enum, label);
    }
}

fn assert_identifier(value: &str) {
    let mut chars = value.chars();
    let first = chars.next().expect("identifier is non-empty");
    assert!(first.is_ascii_lowercase());
    assert!(
        chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    );
}
