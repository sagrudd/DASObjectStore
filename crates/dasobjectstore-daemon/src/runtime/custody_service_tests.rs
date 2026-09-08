// Source fixtures only: use the real extracted orchestrator and durable catalogue,
// with synthetic command and attended-authority adapters. No service is launched.

fn direct_admission_request() -> CustodyAdmissionRequest {
    let definition = custody_definition();
    CustodyAdmissionRequest {
        provisioner_handoff_reference: definition.profile.provisioner_credential_reference.clone(),
        definition,
        dry_run: false,
        verified_subject: None,
        confirmation_marker: CUSTODY_ADMISSION_CONFIRMATION.to_string(),
    }
}

fn direct_provisioner() -> Arc<dyn CustodyAdmissionProvisioningAuthority> {
    Arc::new(
        TestOnlyCustodyAdmissionProvisioningAuthority::new([(
            "attended://provisioner".to_string(),
            custody_provisioning_request(&custody_definition()),
        )])
        .unwrap(),
    )
}

#[test]
fn custody_only_admission_without_normal_controller_is_connected_and_one_use() {
    use crate::runtime::{CustodyServiceBindings, CustodyServiceController, CustodyServiceState};
    let root = temp_root();
    let catalog = dasobjectstore_object_service::CustodyCatalogBinding::new(
        root.join("sealed/catalog.jsonl"),
    )
    .unwrap();
    let normal = config();
    let custody = custody_config();
    let runner = CustodyRetainRunner::default();
    let provisioner = direct_provisioner();
    let state = CustodyServiceState::default();
    let controller = CustodyServiceController::new(
        CustodyServiceBindings {
            custody_plane: &custody,
            excluded_ordinary_plane: &normal,
            catalog: &catalog,
            credentials: None,
            provisioner: Some(&provisioner),
        },
        &runner,
        &state,
    )
    .unwrap();
    assert!(!catalog.path().exists());
    controller
        .admit_custody_store(direct_admission_request(), "2026-09-05T12:00:00Z")
        .unwrap();
    assert_eq!(
        dasobjectstore_object_service::read_custody_catalog(catalog.path())
            .unwrap()
            .len(),
        1
    );
    assert!(controller
        .admit_custody_store(direct_admission_request(), "2026-09-05T12:00:00Z")
        .is_err());
    assert!(provisioner
        .consume_one_use_provisioning_request("attended://provisioner", &custody_definition())
        .is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn custody_only_failed_provision_keeps_claim_across_new_in_process_state() {
    use crate::runtime::{CustodyServiceBindings, CustodyServiceController, CustodyServiceState};
    let root = temp_root();
    let catalog = dasobjectstore_object_service::CustodyCatalogBinding::new(
        root.join("sealed/catalog.jsonl"),
    )
    .unwrap();
    let normal = config();
    let custody = custody_config();
    let runner = super::FakeRunner::failing();
    let provisioner = direct_provisioner();
    let state = CustodyServiceState::default();
    let controller = CustodyServiceController::new(
        CustodyServiceBindings {
            custody_plane: &custody,
            excluded_ordinary_plane: &normal,
            catalog: &catalog,
            credentials: None,
            provisioner: Some(&provisioner),
        },
        &runner,
        &state,
    )
    .unwrap();
    assert!(controller
        .admit_custody_store(direct_admission_request(), "2026-09-05T12:00:00Z")
        .is_err());
    assert!(provisioner
        .consume_one_use_provisioning_request("attended://provisioner", &custody_definition())
        .is_err());
    // A fresh synthetic handoff and process state still cannot reuse the durable claim.
    let retry_runner = CustodyRetainRunner::default();
    let retry_authority = direct_provisioner();
    let retry_state = CustodyServiceState::default();
    let retry = CustodyServiceController::new(
        CustodyServiceBindings {
            custody_plane: &custody,
            excluded_ordinary_plane: &normal,
            catalog: &catalog,
            credentials: None,
            provisioner: Some(&retry_authority),
        },
        &retry_runner,
        &retry_state,
    )
    .unwrap();
    assert!(retry
        .admit_custody_store(direct_admission_request(), "2026-09-05T12:00:00Z")
        .is_err());
    assert_eq!(retry_runner.bucket_info_calls.load(Ordering::SeqCst), 0);
    assert!(retry_runner.calls.lock().unwrap().is_empty());
    assert!(
        dasobjectstore_object_service::read_custody_catalog(catalog.path())
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn custody_only_constructor_denies_aliases_without_effects_or_fallback() {
    use crate::runtime::{CustodyServiceBindings, CustodyServiceController, CustodyServiceState};
    let root = temp_root();
    let catalog = dasobjectstore_object_service::CustodyCatalogBinding::new(
        root.join("sealed/catalog.jsonl"),
    )
    .unwrap();
    let normal = config();
    let runner = CustodyRetainRunner::default();
    let state = CustodyServiceState::default();
    for field in 0..8 {
        let mut custody = custody_config();
        match field {
            0 => custody.compose_file = normal.compose_file.clone(),
            1 => custody.project_directory = normal.project_directory.clone(),
            2 => custody.compose_project = normal.compose_project.clone(),
            3 => custody.service_name = normal.service_name.clone(),
            4 => custody.config_path = normal.config_path.clone(),
            5 => custody.metadata_path = normal.metadata_path.clone(),
            6 => custody.data_path = normal.data_path.clone(),
            _ => custody.endpoint = "http://localhost:3900".to_string(),
        }
        assert!(
            CustodyServiceController::new(
                CustodyServiceBindings {
                    custody_plane: &custody,
                    excluded_ordinary_plane: &normal,
                    catalog: &catalog,
                    credentials: None,
                    provisioner: None,
                },
                &runner,
                &state
            )
            .is_err(),
            "alias {field}"
        );
    }
    assert!(!catalog.path().exists());
    assert_eq!(runner.bucket_info_calls.load(Ordering::SeqCst), 0);
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(
        !root.exists(),
        "constructor denial must not create the fixture root"
    );
}

#[test]
fn custody_only_state_cannot_cross_catalogue_or_plane_compositions() {
    use crate::runtime::{CustodyServiceBindings, CustodyServiceController, CustodyServiceState};
    let root = temp_root();
    let catalog =
        dasobjectstore_object_service::CustodyCatalogBinding::new(root.join("first/catalog.jsonl"))
            .unwrap();
    let other_catalog = dasobjectstore_object_service::CustodyCatalogBinding::new(
        root.join("second/catalog.jsonl"),
    )
    .unwrap();
    let normal = config();
    let custody = custody_config();
    let runner = CustodyRetainRunner::default();
    let state = CustodyServiceState::default();
    let bindings = || CustodyServiceBindings {
        custody_plane: &custody,
        excluded_ordinary_plane: &normal,
        catalog: &catalog,
        credentials: None,
        provisioner: None,
    };
    let _first = CustodyServiceController::new(bindings(), &runner, &state).unwrap();
    let _same = CustodyServiceController::new(bindings(), &runner, &state).unwrap();
    let mut changed = bindings();
    changed.catalog = &other_catalog;
    assert!(CustodyServiceController::new(changed, &runner, &state).is_err());
    let mut other_plane = custody.clone();
    other_plane.compose_project.push_str("-other");
    let mut changed = bindings();
    changed.custody_plane = &other_plane;
    assert!(CustodyServiceController::new(changed, &runner, &state).is_err());
    let mut other_excluded = normal.clone();
    other_excluded.compose_project.push_str("-other");
    let mut changed = bindings();
    changed.excluded_ordinary_plane = &other_excluded;
    assert!(CustodyServiceController::new(changed, &runner, &state).is_err());
    assert_eq!(runner.bucket_info_calls.load(Ordering::SeqCst), 0);
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(!root.exists());
}
