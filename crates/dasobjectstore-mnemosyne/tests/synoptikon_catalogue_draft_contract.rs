use std::{fs, path::PathBuf};

use serde_json::Value;

#[test]
fn catalogue_draft_matches_product_manifest_identity_and_mounts() {
    let manifest = product_manifest();
    let draft = catalogue_draft();

    assert_contains(&draft, r#"id = "dasobjectstore""#);
    assert_contains(&draft, r#"display_name = "DASObjectStore""#);
    assert_contains(&draft, r#"mode = "dual_host""#);
    assert_contains(
        &draft,
        r#"manifest = "../DASObjectStore/product-manifest.json""#,
    );
    assert_contains(&draft, r#"api_path = "/products/dasobjectstore/api""#);
    assert_contains(&draft, r#"health_path = "/products/dasobjectstore/health""#);
    assert_contains(&draft, r#"port_policy = "catalogue_assigned""#);
    assert_contains(&draft, r#"entitlement_product_code = "dasobjectstore""#);

    assert_eq!(manifest["product"]["id"], "dasobjectstore");
    assert_eq!(manifest["product"]["name"], "DASObjectStore");
    assert_eq!(
        manifest["surfaces"]["api"]["base_path"],
        "/products/dasobjectstore/api"
    );
    assert_eq!(
        manifest["surfaces"]["api"]["health_path"],
        "/products/dasobjectstore/health"
    );
}

#[test]
fn catalogue_draft_keeps_standalone_port_out_of_synoptikon_entry() {
    let draft = catalogue_draft();

    assert_contains(
        &draft,
        "The Synoptikon catalogue entry must not set `fixed_port = 8448`.",
    );
    assert!(
        !draft.contains("fixed_port = 8448\n"),
        "integrated catalogue entry must not assign the standalone HTTPS port"
    );
}

#[test]
fn catalogue_draft_includes_monas_standalone_profile() {
    let draft = catalogue_draft();

    assert_contains(&draft, r#"id = "monas-dasobjectstore-standalone""#);
    assert_contains(&draft, r#"profile_kind = "monas_standalone""#);
    assert_contains(&draft, r#"products = ["dasobjectstore"]"#);
    assert_contains(&draft, r#"product_root_template = "/opt/<productName>""#);
    assert_contains(
        &draft,
        r#"local_persistence = "json_files_with_sqlite_index""#,
    );
    assert_contains(&draft, r#"workflow_provider = "local_hardware""#);
}

fn catalogue_draft() -> String {
    fs::read_to_string(repo_root().join("docs/synoptikon-catalogue-entry.md"))
        .expect("read catalogue draft")
}

fn product_manifest() -> Value {
    let raw = fs::read_to_string(repo_root().join("product-manifest.json")).expect("read manifest");
    serde_json::from_str(&raw).expect("manifest parses")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "catalogue draft should contain {needle}"
    );
}
