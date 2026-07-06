use dasobjectstore_mnemosyne::{
    validate_monas_standalone_host_boundary, MonasStandaloneHostBoundaryContext,
    MonasStandaloneHostBoundaryError, ProductHostMode, StorageAuthority,
    DASOBJECTSTORE_PRODUCT_ROOT,
};

#[test]
fn accepts_complete_monas_standalone_context() {
    let context = valid_context();

    let boundary = validate_monas_standalone_host_boundary(&context).expect("context validates");

    assert_eq!(boundary.profile.mode, ProductHostMode::Standalone);
    assert!(!boundary.profile.requires_entitlement);
    assert_eq!(
        boundary.context.product_root,
        DASOBJECTSTORE_PRODUCT_ROOT.to_string()
    );
    assert!(boundary.context.local_audit_export_enabled);
    assert!(boundary.context.local_hardware_workflows_enabled);
    assert!(boundary.context.local_state_store_enabled);
    assert_eq!(
        boundary.context.state_store_authority,
        StorageAuthority::LocalProductState
    );
}

#[test]
fn rejects_wrong_product_root() {
    let mut context = valid_context();
    context.product_root = "/opt/mnematikon".to_string();

    let err = validate_monas_standalone_host_boundary(&context).expect_err("root rejected");

    assert_eq!(
        err,
        MonasStandaloneHostBoundaryError::InvalidProductRoot {
            value: "/opt/mnematikon".to_string()
        }
    );
}

#[test]
fn rejects_missing_local_audit() {
    let mut context = valid_context();
    context.local_audit_export_enabled = false;

    let err = validate_monas_standalone_host_boundary(&context).expect_err("audit rejected");

    assert_eq!(err, MonasStandaloneHostBoundaryError::MissingLocalAudit);
}

#[test]
fn rejects_missing_local_hardware_workflows() {
    let mut context = valid_context();
    context.local_hardware_workflows_enabled = false;

    let err = validate_monas_standalone_host_boundary(&context).expect_err("hardware rejected");

    assert_eq!(
        err,
        MonasStandaloneHostBoundaryError::MissingLocalHardwareWorkflows
    );
}

#[test]
fn rejects_missing_local_state_store() {
    let mut context = valid_context();
    context.local_state_store_enabled = false;

    let err = validate_monas_standalone_host_boundary(&context).expect_err("state rejected");

    assert_eq!(
        err,
        MonasStandaloneHostBoundaryError::MissingLocalStateStore
    );
}

#[test]
fn rejects_synoptikon_storage_authority() {
    let mut context = valid_context();
    context.state_store_authority = StorageAuthority::SynoptikonStorageBinding;

    let err = validate_monas_standalone_host_boundary(&context).expect_err("authority rejected");

    assert_eq!(
        err,
        MonasStandaloneHostBoundaryError::InvalidStateStoreAuthority {
            value: StorageAuthority::SynoptikonStorageBinding
        }
    );
}

#[test]
fn serializes_with_standalone_field_names() {
    let serialized = serde_json::to_value(valid_context()).expect("context serializes");

    assert_eq!(serialized["installation_id"], "install-1");
    assert_eq!(serialized["profile_id"], "profile-1");
    assert_eq!(serialized["local_user_id"], "local-user-1");
    assert_eq!(serialized["product_root"], DASOBJECTSTORE_PRODUCT_ROOT);
    assert_eq!(serialized["local_audit_export_enabled"], true);
    assert_eq!(serialized["local_hardware_workflows_enabled"], true);
    assert_eq!(serialized["local_state_store_enabled"], true);
    assert_eq!(serialized["state_store_authority"], "local_product_state");
}

fn valid_context() -> MonasStandaloneHostBoundaryContext {
    MonasStandaloneHostBoundaryContext {
        installation_id: "install-1".to_string(),
        profile_id: "profile-1".to_string(),
        local_user_id: "local-user-1".to_string(),
        product_root: DASOBJECTSTORE_PRODUCT_ROOT.to_string(),
        local_audit_export_enabled: true,
        local_hardware_workflows_enabled: true,
        local_state_store_enabled: true,
        state_store_authority: StorageAuthority::LocalProductState,
    }
}
