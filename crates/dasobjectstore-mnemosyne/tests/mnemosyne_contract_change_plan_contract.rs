use std::{fs, path::PathBuf};

#[test]
fn contract_change_plan_names_native_endpoint_kinds() {
    let plan = contract_change_plan();

    for endpoint_kind in [
        "dasobjectstore_das",
        "dasobjectstore_nfs",
        "s3_compatible",
        "nfs",
        "posix",
    ] {
        assert_contains(&plan, endpoint_kind);
    }
}

#[test]
fn contract_change_plan_names_affected_repositories() {
    let plan = contract_change_plan();

    for repository in [
        "../DASObjectStore",
        "../mnemosyne/mneion-api-types",
        "../mnemosyne/mneion-server",
        "../mnemosyne/mneion-web",
        "../mnemosyne/mneion-admin",
        "../mnemosyne/mnemosyne-product-sdk",
        "../mnemosyne-docs",
        "../limen",
    ] {
        assert_contains(&plan, repository);
    }
}

#[test]
fn contract_change_plan_preserves_storage_boundaries() {
    let plan = contract_change_plan();

    assert_contains(
        &plan,
        "governance-domain storage binding is the storage authority",
    );
    assert_contains(&plan, "Do not change Synoptikon request-context ownership");
    assert_contains(
        &plan,
        "Do not expose DASObjectStore's standalone HTTPS port `8448`",
    );
    assert_contains(&plan, "Do not make Mneion a DAS disk manager");
    assert_contains(&plan, "Do not use raw filesystem paths");
}

#[test]
fn plugin_spec_links_to_contract_change_plan() {
    let spec = fs::read_to_string(repo_root().join("docs/web-gui-and-mnemosyne-plugin.md"))
        .expect("read plugin spec");

    assert_contains(&spec, "mnemosyne-contract-change-plan.md");
}

fn contract_change_plan() -> String {
    fs::read_to_string(repo_root().join("docs/mnemosyne-contract-change-plan.md"))
        .expect("read contract change plan")
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
        "document should contain {needle}"
    );
}
