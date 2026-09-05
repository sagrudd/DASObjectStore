use super::*;
use crate::api::{CapacityAdmissionRequest, CapacityAdmissionResponse};
use crate::runtime::{CapacityAdmissionProvider, ServiceCommandOutput, ServiceCommandRunner};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_object_service::{
    create_custody_catalog_entry, CustodyAssuranceClass, CustodyRetentionPolicyV1,
    CustodyStoreDefinitionV1, CustodyStoreProfileV1, CUSTODY_OVERLAY_SCHEMA_V1, CUSTODY_PROFILE_V1,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct NoopRunner;

impl ServiceCommandRunner for NoopRunner {
    fn run(
        &self,
        _program: &str,
        _args: &[String],
    ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
        Ok(ServiceCommandOutput {
            stdout: String::new(),
        })
    }
}

#[derive(Default)]
struct RecordingCreationCapacity {
    exists: Mutex<bool>,
    initialize_calls: Mutex<u32>,
    rollback_calls: Mutex<u32>,
    sabotage_registry_once: Mutex<Option<PathBuf>>,
}

impl RecordingCreationCapacity {
    fn existing() -> Self {
        Self {
            exists: Mutex::new(true),
            ..Self::default()
        }
    }

    fn sabotage_once(path: PathBuf) -> Self {
        Self {
            sabotage_registry_once: Mutex::new(Some(path)),
            ..Self::default()
        }
    }
}

impl CapacityAdmissionProvider for RecordingCreationCapacity {
    fn initialize_store(
        &self,
        _store_id: &StoreId,
        _policy: dasobjectstore_core::store::CapacityPolicy,
    ) -> Result<bool, DaemonServiceRuntimeError> {
        *self.initialize_calls.lock().expect("initialize calls") += 1;
        if let Some(path) = self.sabotage_registry_once.lock().expect("sabotage").take() {
            fs::remove_file(&path).expect("remove registry before injected failure");
            fs::create_dir(&path).expect("replace registry with directory");
        }
        let mut exists = self.exists.lock().expect("exists");
        let created = !*exists;
        *exists = true;
        Ok(created)
    }

    fn rollback_initialized_store(
        &self,
        _store_id: &StoreId,
    ) -> Result<(), DaemonServiceRuntimeError> {
        *self.rollback_calls.lock().expect("rollback calls") += 1;
        *self.exists.lock().expect("exists") = false;
        Ok(())
    }

    fn admit(
        &self,
        _request: CapacityAdmissionRequest,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "not used by creation saga test".to_string(),
        })
    }
}

fn controller(provider: Arc<dyn CapacityAdmissionProvider>) -> GarageServiceController<NoopRunner> {
    GarageServiceController::new(
        crate::runtime::GarageServiceRuntimeConfig {
            compose_file: PathBuf::from("/tmp/compose.yml"),
            project_directory: Some(PathBuf::from("/tmp")),
            compose_project: "test".to_string(),
            service_name: "garage".to_string(),
            config_path: PathBuf::from("/tmp/garage.toml"),
            metadata_path: PathBuf::from("/tmp/garage-meta"),
            data_path: PathBuf::from("/tmp/garage-data"),
            endpoint: "http://127.0.0.1:3900".to_string(),
        },
        NoopRunner,
    )
    .with_capacity_admission_provider(provider)
}

fn request(client_request_id: &str) -> CreateObjectStoreRequest {
    CreateObjectStoreRequest {
        store_id: "generated-data".to_string(),
        store_class: "generated_data".to_string(),
        required_copies: 2,
        bucket: Some("generated-data".to_string()),
        reader_group: None,
        writer_group: "mnemosyne".to_string(),
        ssd_root: PathBuf::from("/srv/dasobjectstore/ssd"),
        object_type: "naive".to_string(),
        enclosure_id: None,
        public: false,
        writeable: true,
        capacity_behavior: "backpressure_by_priority".to_string(),
        retention: "tombstone_then_gc".to_string(),
        endpoint_export_mode: "s3_bucket".to_string(),
        dry_run: false,
        client_request_id: Some(client_request_id.to_string()),
        administrator_actor: Some("root".to_string()),
        confirmation_marker: crate::api::OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
    }
}

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-creation-saga-{name}-{}",
        std::process::id()
    ))
}

fn sealed_definition(store_id: &str, bucket_name: &str) -> CustodyStoreDefinitionV1 {
    CustodyStoreDefinitionV1 {
        store_id: StoreId::new(store_id).expect("sealed store id"),
        bucket_name: bucket_name.to_string(),
        profile: CustodyStoreProfileV1 {
            schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
            profile: CUSTODY_PROFILE_V1.to_string(),
            assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
            retention: CustodyRetentionPolicyV1::required(),
            target_id: "nuc-192-168-0-193".to_string(),
            retention_until_utc: "2036-09-05T12:00:00Z".to_string(),
            legal_hold: true,
            provisioner_credential_reference: "systemd-credential://provisioner".to_string(),
            provisioner_identity: "provisioner".to_string(),
            writer_credential_reference: "systemd-credential://writer".to_string(),
            writer_identity: "writer".to_string(),
            reader_credential_reference: "systemd-credential://reader".to_string(),
            reader_identity: "reader".to_string(),
        },
    }
}

#[test]
fn sealed_store_or_bucket_is_denied_before_creation_intent_or_capacity_effect() {
    let root = root("custody-pre-intent-denial");
    fs::create_dir_all(&root).expect("root");
    let catalog = root.join("sealed/catalog.jsonl");
    let sealed = sealed_definition("sealed-store", "dos-sealed-store");
    create_custody_catalog_entry(
        &catalog,
        &sealed,
        root.join("sealed/ledger.sqlite"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "2026-09-05T12:00:00Z",
    )
    .expect("sealed catalog");
    for (label, request) in [
        ("store", {
            let mut request = request("sealed-store");
            request.store_id = "sealed-store".to_string();
            request.bucket = Some("different-normal-bucket".to_string());
            request
        }),
        ("bucket", {
            let mut request = request("sealed-bucket-alias");
            request.store_id = "normal-alias".to_string();
            request.bucket = Some(sealed.bucket_name.clone());
            request
        }),
    ] {
        let provider = Arc::new(RecordingCreationCapacity::default());
        let guarded = controller(provider.clone())
            .try_with_custody_catalog_path(&catalog)
            .expect("catalog binding");
        let intent = root.join(format!("{label}-intents.json"));
        let registry = root.join(format!("{label}-stores.json"));
        fs::write(&registry, "[]").expect("registry");
        assert!(create_object_store_with_capacity_and_intent_path(
            &guarded,
            request,
            "2026-09-05T12:01:00Z",
            &intent,
            &registry,
        )
        .is_err());
        assert!(
            !intent.exists(),
            "{label} denial must precede durable creation intent"
        );
        assert_eq!(*provider.initialize_calls.lock().expect("calls"), 0);
        assert_eq!(*provider.rollback_calls.lock().expect("calls"), 0);
        assert_eq!(fs::read_to_string(&registry).expect("registry"), "[]");
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn replay_adopts_exact_orphan_ledger_after_capacity_side_effect_crash() {
    let root = root("capacity-crash");
    fs::create_dir_all(&root).expect("root");
    let intent_path = root.join("intents.json");
    let registry_path = root.join("stores.json");
    fs::write(&registry_path, "[]").expect("empty registry");
    let request = request("capacity-crash");
    let intent = crate::runtime::begin_object_store_creation_intent(
        &intent_path,
        &request,
        "root",
        "2026-07-27T10:00:00Z",
    )
    .expect("intent");
    crate::runtime::advance_object_store_creation_intent(
        &intent_path,
        &intent,
        crate::runtime::ObjectStoreCreationPhase::CapacityInitializing,
        false,
    )
    .expect("pre-side-effect checkpoint");
    let provider = Arc::new(RecordingCreationCapacity::existing());
    let response = create_object_store_with_capacity_and_intent_path(
        &controller(provider.clone()),
        request.clone(),
        "later",
        &intent_path,
        &registry_path,
    )
    .expect("replay completes");

    assert_eq!(response.store_id, "generated-data");
    assert_eq!(*provider.initialize_calls.lock().unwrap(), 1);
    assert_eq!(*provider.rollback_calls.lock().unwrap(), 0);
    let completed =
        crate::runtime::begin_object_store_creation_intent(&intent_path, &request, "root", "later")
            .expect("completed intent");
    assert_eq!(
        completed.phase,
        crate::runtime::ObjectStoreCreationPhase::Complete
    );
    assert!(completed.capacity_created);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn publication_failure_rolls_back_owned_capacity_and_retry_recovers() {
    let root = root("rollback");
    fs::create_dir_all(&root).expect("root");
    let intent_path = root.join("intents.json");
    let registry_path = root.join("stores.json");
    fs::write(&registry_path, "[]").expect("empty registry");
    let request = request("rollback");
    let provider = Arc::new(RecordingCreationCapacity::sabotage_once(
        registry_path.clone(),
    ));
    let error = create_object_store_with_capacity_and_intent_path(
        &controller(provider.clone()),
        request.clone(),
        "2026-07-27T10:00:00Z",
        &intent_path,
        &registry_path,
    )
    .expect_err("injected publication failure");
    assert!(error.to_string().contains("registry"));
    assert_eq!(*provider.rollback_calls.lock().unwrap(), 1);
    assert!(!*provider.exists.lock().unwrap());
    let rolled_back =
        crate::runtime::begin_object_store_creation_intent(&intent_path, &request, "root", "later")
            .expect("rolled-back intent");
    assert_eq!(
        rolled_back.phase,
        crate::runtime::ObjectStoreCreationPhase::Validated
    );
    assert!(!rolled_back.capacity_created);

    fs::remove_dir(&registry_path).expect("remove injected directory");
    fs::write(&registry_path, "[]").expect("restore registry");
    create_object_store_with_capacity_and_intent_path(
        &controller(provider.clone()),
        request,
        "later",
        &intent_path,
        &registry_path,
    )
    .expect("retry completes");
    assert_eq!(*provider.initialize_calls.lock().unwrap(), 2);
    let _ = fs::remove_dir_all(root);
}
