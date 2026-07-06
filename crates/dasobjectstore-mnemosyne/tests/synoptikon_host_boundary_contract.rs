use dasobjectstore_mnemosyne::{
    validate_synoptikon_integrated_host_boundary, ProductHostMode, StorageAuthority,
    SynoptikonIntegratedHostBoundaryContext, SynoptikonIntegratedHostBoundaryError,
    DASOBJECTSTORE_PRODUCT_ID, REQUEST_CONTEXT_SCHEMA_VERSION,
};

#[test]
fn accepts_complete_synoptikon_integrated_context() {
    let context = valid_context();

    let boundary =
        validate_synoptikon_integrated_host_boundary(&context).expect("context validates");

    assert_eq!(boundary.profile.mode, ProductHostMode::SynoptikonIntegrated);
    assert!(boundary.profile.requires_entitlement);
    assert_eq!(boundary.context.account_id, "account-1");
    assert_eq!(boundary.context.project_id, "project-1");
    assert_eq!(boundary.context.entitlement_id, "entitlement-1");
    assert_eq!(boundary.context.correlation_id, "corr-1");
    assert_eq!(
        boundary.context.storage_authority,
        StorageAuthority::SynoptikonStorageBinding
    );
    assert_eq!(boundary.context.storage_binding_id, "binding-1");
}

#[test]
fn rejects_context_for_another_product() {
    let mut context = valid_context();
    context.product_id = "mnematikon".to_string();

    let err = validate_synoptikon_integrated_host_boundary(&context).expect_err("product rejected");

    assert_eq!(
        err,
        SynoptikonIntegratedHostBoundaryError::InvalidProductId {
            value: "mnematikon".to_string()
        }
    );
}

#[test]
fn rejects_missing_central_audit() {
    let mut context = valid_context();
    context.central_audit_enabled = false;

    let err = validate_synoptikon_integrated_host_boundary(&context).expect_err("audit rejected");

    assert_eq!(
        err,
        SynoptikonIntegratedHostBoundaryError::MissingCentralAudit
    );
}

#[test]
fn rejects_local_storage_authority() {
    let mut context = valid_context();
    context.storage_authority = StorageAuthority::LocalProductState;

    let err = validate_synoptikon_integrated_host_boundary(&context).expect_err("storage rejected");

    assert_eq!(
        err,
        SynoptikonIntegratedHostBoundaryError::InvalidStorageAuthority {
            value: StorageAuthority::LocalProductState
        }
    );
}

#[test]
fn rejects_duplicate_roles() {
    let mut context = valid_context();
    context.roles = vec![
        "storage_operator".to_string(),
        "storage_operator".to_string(),
    ];

    let err = validate_synoptikon_integrated_host_boundary(&context).expect_err("roles rejected");

    assert_eq!(
        err,
        SynoptikonIntegratedHostBoundaryError::DuplicateRole {
            value: "storage_operator".to_string()
        }
    );
}

#[test]
fn serializes_with_request_context_field_names() {
    let serialized = serde_json::to_value(valid_context()).expect("context serializes");

    assert_eq!(
        serialized["request_context_schema_version"],
        REQUEST_CONTEXT_SCHEMA_VERSION
    );
    assert_eq!(serialized["product_id"], DASOBJECTSTORE_PRODUCT_ID);
    assert_eq!(serialized["account_id"], "account-1");
    assert_eq!(serialized["project_id"], "project-1");
    assert_eq!(serialized["entitlement_id"], "entitlement-1");
    assert_eq!(serialized["correlation_id"], "corr-1");
    assert_eq!(
        serialized["storage_authority"],
        "synoptikon_storage_binding"
    );
}

fn valid_context() -> SynoptikonIntegratedHostBoundaryContext {
    SynoptikonIntegratedHostBoundaryContext {
        request_context_schema_version: REQUEST_CONTEXT_SCHEMA_VERSION.to_string(),
        product_id: DASOBJECTSTORE_PRODUCT_ID.to_string(),
        tenant_id: "tenant-1".to_string(),
        account_id: "account-1".to_string(),
        user_id: "user-1".to_string(),
        project_id: "project-1".to_string(),
        entitlement_id: "entitlement-1".to_string(),
        roles: vec!["storage_operator".to_string()],
        correlation_id: "corr-1".to_string(),
        central_audit_enabled: true,
        storage_authority: StorageAuthority::SynoptikonStorageBinding,
        storage_binding_id: "binding-1".to_string(),
    }
}
