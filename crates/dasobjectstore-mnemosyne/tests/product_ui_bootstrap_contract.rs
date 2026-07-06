use dasobjectstore_mnemosyne::{
    bootstrap_path_for_web_mount, export_product_ui_bootstrap,
    export_synoptikon_product_ui_bootstrap, HostMode, ProductUiHostCapability,
    ProductUiVisibilityState, CORRELATION_ID_HEADER, DASOBJECTSTORE_API_MOUNT,
    DASOBJECTSTORE_WEB_MOUNT, PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION,
};
use serde_json::json;

#[test]
fn exports_synoptikon_bootstrap_for_dasobjectstore_mounts() {
    let metadata = export_synoptikon_product_ui_bootstrap().expect("Synoptikon bootstrap exports");

    assert_eq!(metadata.schema_version, PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION);
    assert_eq!(metadata.product_id, "dasobjectstore");
    assert_eq!(metadata.product_name, "DASObjectStore");
    assert_eq!(metadata.host_mode, HostMode::SynoptikonIntegrated);
    assert_eq!(metadata.web_mount, DASOBJECTSTORE_WEB_MOUNT);
    assert_eq!(metadata.api_mount, DASOBJECTSTORE_API_MOUNT);
    assert_eq!(
        metadata.bootstrap_path,
        "/products/dasobjectstore/.well-known/mnemosyne/product-bootstrap.json"
    );
    assert_eq!(metadata.correlation.header_name, CORRELATION_ID_HEADER);
    assert_eq!(metadata.visibility.state, ProductUiVisibilityState::Visible);
    assert!(metadata
        .host_capabilities
        .contains(&ProductUiHostCapability::SynoptikonAccounts));
    assert!(metadata
        .host_capabilities
        .contains(&ProductUiHostCapability::SynoptikonObjectStoreArtifacts));
    assert_eq!(metadata.navigation.len(), 6);
    assert_eq!(metadata.navigation[0].route_id, "overview");
    assert_eq!(
        metadata.navigation[0].path,
        "/products/dasobjectstore/overview"
    );
    assert_eq!(metadata.navigation[5].route_id, "activity");
}

#[test]
fn serializes_with_mnemosyne_bootstrap_field_names() {
    let metadata = export_synoptikon_product_ui_bootstrap().expect("Synoptikon bootstrap exports");
    let serialized = serde_json::to_value(metadata).expect("bootstrap serializes");

    assert_eq!(
        serialized["host_capabilities"],
        json!([
            "synoptikon_accounts",
            "synoptikon_entitlements",
            "synoptikon_central_audit",
            "synoptikon_object_store_artifacts",
            "synoptikon_project_rdbms"
        ])
    );
    assert_eq!(serialized["correlation"]["mode"], "host_generated");
    assert_eq!(serialized["visibility"]["state"], "visible");
    assert_eq!(
        serialized["navigation"][0]["host_modes"][0],
        "synoptikon_integrated"
    );
}

#[test]
fn monas_bootstrap_exposes_local_capabilities_for_standalone_host() {
    let metadata =
        export_product_ui_bootstrap(HostMode::MonasStandalone).expect("Monas bootstrap exports");

    assert_eq!(metadata.host_mode, HostMode::MonasStandalone);
    assert!(metadata
        .host_capabilities
        .contains(&ProductUiHostCapability::MonasLocalJsonStorage));
    assert!(metadata
        .host_capabilities
        .contains(&ProductUiHostCapability::MonasLocalHardwareWorkflows));
    assert!(metadata
        .navigation
        .iter()
        .all(|item| item.host_modes == [HostMode::MonasStandalone]));
}

#[test]
fn post_login_navigation_starts_at_operations_overview_not_landing() {
    let metadata = export_synoptikon_product_ui_bootstrap().expect("Synoptikon bootstrap exports");
    let first_route = metadata.navigation.first().expect("navigation exists");

    assert_eq!(first_route.route_id, "overview");
    assert_eq!(first_route.label, "Overview");
    assert_eq!(first_route.path, "/products/dasobjectstore/overview");

    for forbidden in ["landing", "home", "welcome", "marketing"] {
        assert!(
            !first_route.route_id.contains(forbidden),
            "post-login route must not be a {forbidden} surface"
        );
        assert!(
            !first_route.path.contains(forbidden),
            "post-login path must not be a {forbidden} surface"
        );
    }
}

#[test]
fn derives_bootstrap_path_from_web_mount() {
    let path = bootstrap_path_for_web_mount("/products/dasobjectstore/").expect("path derives");

    assert_eq!(
        path,
        "/products/dasobjectstore/.well-known/mnemosyne/product-bootstrap.json"
    );
}
