use dasobjectstore_mnemosyne::{
    host_mode_profile, standalone_host_mode_profile, synoptikon_integrated_host_mode_profile,
    AuditAuthority, AuthenticationAuthority, HostMode, ProductHostMode, StorageAuthority,
    DASOBJECTSTORE_PRODUCT_ROOT, DASOBJECTSTORE_STANDALONE_HTTPS_PORT,
};
use serde_json::json;

#[test]
fn product_host_modes_serialize_with_manifest_names() {
    assert_eq!(
        serde_json::to_value(ProductHostMode::Standalone).expect("mode serializes"),
        json!("standalone")
    );
    assert_eq!(
        serde_json::to_value(ProductHostMode::SynoptikonIntegrated).expect("mode serializes"),
        json!("synoptikon_integrated")
    );
}

#[test]
fn standalone_profile_owns_local_auth_and_public_https_port() {
    let profile = host_mode_profile(ProductHostMode::Standalone).expect("profile validates");

    assert_eq!(profile, standalone_host_mode_profile());
    assert_eq!(
        profile.product_root.as_deref(),
        Some(DASOBJECTSTORE_PRODUCT_ROOT)
    );
    assert_eq!(
        profile.public_https_port,
        Some(DASOBJECTSTORE_STANDALONE_HTTPS_PORT)
    );
    assert!(profile.local_authentication);
    assert!(profile.local_hardware);
    assert!(profile.product_owned_login_routes);
    assert!(!profile.requires_entitlement);
    assert_eq!(
        profile.authentication_authority,
        AuthenticationAuthority::LocalProduct
    );
    assert_eq!(profile.audit_authority, AuditAuthority::LocalProduct);
    assert_eq!(
        profile.storage_authority,
        StorageAuthority::LocalProductState
    );
    assert_eq!(
        profile.storage_boundary_host_mode,
        HostMode::MonasStandalone
    );
}

#[test]
fn synoptikon_integrated_profile_uses_host_authorities() {
    let profile =
        host_mode_profile(ProductHostMode::SynoptikonIntegrated).expect("profile validates");

    assert_eq!(profile, synoptikon_integrated_host_mode_profile());
    assert!(profile.product_root.is_none());
    assert!(profile.public_https_port.is_none());
    assert!(!profile.local_authentication);
    assert!(!profile.local_hardware);
    assert!(!profile.product_owned_login_routes);
    assert!(profile.requires_entitlement);
    assert_eq!(
        profile.authentication_authority,
        AuthenticationAuthority::Synoptikon
    );
    assert_eq!(
        profile.audit_authority,
        AuditAuthority::SynoptikonCentralAudit
    );
    assert_eq!(
        profile.storage_authority,
        StorageAuthority::SynoptikonStorageBinding
    );
    assert_eq!(
        profile.storage_boundary_host_mode,
        HostMode::SynoptikonIntegrated
    );
}

#[test]
fn host_mode_profile_serializes_as_contract_data() {
    let serialized = serde_json::to_value(synoptikon_integrated_host_mode_profile())
        .expect("profile serializes");

    assert_eq!(serialized["mode"], "synoptikon_integrated");
    assert_eq!(serialized["product_root"], serde_json::Value::Null);
    assert_eq!(serialized["public_https_port"], serde_json::Value::Null);
    assert_eq!(serialized["authentication_authority"], "synoptikon");
    assert_eq!(serialized["audit_authority"], "synoptikon_central_audit");
    assert_eq!(
        serialized["storage_authority"],
        "synoptikon_storage_binding"
    );
    assert_eq!(
        serialized["storage_boundary_host_mode"],
        "synoptikon_integrated"
    );
}
