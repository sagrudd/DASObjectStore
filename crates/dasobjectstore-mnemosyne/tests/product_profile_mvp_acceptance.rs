use dasobjectstore_core::{
    backend::{BackendError, BackendObjectKey},
    deployment::{DeploymentProfile, HostMode},
    ids::StoreId,
    ingress::IngressOrigin,
    manifest::{BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION},
    protection::ProtectionPolicy,
    store::{CapacityPolicy, StoreClass, StorePolicy},
    StoragePolicyTemplate,
};
use dasobjectstore_daemon::{
    runtime::{
        delete_profile_object, get_profile_object, get_profile_object_range, list_profile_objects,
        put_profile_object, verify_profile_object, DaemonServiceRuntimeError, FolderBackend,
    },
    DaemonClient, DaemonClientError, DaemonLocalActor, DaemonRequestHandler,
    DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOrchestrator,
    DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusRequest,
    DaemonServiceStatusResponse, FixedDaemonClock, InProcessDaemonTransport,
};
use dasobjectstore_mnemosyne::{
    provision_product_profile, ProductPolicyAdapterKind, ProductPolicyTemplateAdapter,
    ProductProfileProvisioningPlan,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const OBJECT_COUNT: usize = 64;
const OBJECT_BYTES: usize = 4096;

#[test]
fn product_profile_provisions_and_survives_generated_data_s3_stress() {
    let root = acceptance_root();
    let backend_root = root.join("backend");
    let state_root = root.join("state");
    fs::create_dir_all(&state_root).expect("state root");

    let plan = provisioning_plan(backend_root.clone());
    let actor = DaemonLocalActor::new(0).with_username("release-acceptance");
    let handler = DaemonRequestHandler::new(
        AcceptanceOrchestrator,
        FixedDaemonClock::new("2026-07-16T12:00:00Z"),
    )
    .with_registry_paths(
        state_root.join("stores.json"),
        state_root.join("subobjects.json"),
    )
    .with_profile_binding_registry_path(state_root.join("profile-bindings.json"));
    let client = DaemonClient::new(InProcessDaemonTransport::new(move |request| {
        handler
            .handle_with_progress_for_actor(request, Some(&actor), |_| Ok(()))
            .map_err(|error| DaemonClientError::Transport(error.to_string()))
    }));

    let first = provision_product_profile(&client, &plan).expect("first provision succeeds");
    assert!(!first.reused);
    assert!(first.store_definition_published);
    let second = provision_product_profile(&client, &plan).expect("provision retry succeeds");
    assert!(
        second.reused,
        "identical product provisioning must be idempotent"
    );

    let capacity = plan.policy_template.template.capacity.clone();
    let manifest = plan.manifest.clone();
    let mut backend = FolderBackend::open(&backend_root, manifest.clone(), capacity.clone(), 0)
        .expect("provisioned backend opens");
    let payloads = (0..OBJECT_COUNT)
        .map(|index| generated_payload(index))
        .collect::<Vec<_>>();
    for (index, payload) in payloads.iter().enumerate() {
        put_profile_object(
            &mut backend,
            &format!("acceptance-{index}"),
            &object_key(index),
            &mut payload.as_slice(),
            payload.len() as u64,
        )
        .expect("generated object PUT succeeds");
    }

    let listed = list_profile_objects(&backend, Some("generated/")).expect("LIST succeeds");
    assert_eq!(listed.len(), OBJECT_COUNT);
    for index in [0, OBJECT_COUNT / 2, OBJECT_COUNT - 1] {
        let key = object_key(index);
        verify_profile_object(&backend, &key).expect("VERIFY succeeds");
        let mut body = Vec::new();
        get_profile_object(&backend, &key)
            .expect("GET opens")
            .read_to_end(&mut body)
            .expect("GET verifies while streaming");
        assert_eq!(body, payloads[index]);
        let mut range = Vec::new();
        get_profile_object_range(&backend, &key, 101, 257)
            .expect("range GET opens")
            .read_to_end(&mut range)
            .expect("range GET streams");
        assert_eq!(range, payloads[index][101..358]);
    }

    assert!(matches!(
        put_profile_object(
            &mut backend,
            "over-quota",
            &BackendObjectKey {
                object_id: "generated/over-quota.bin".to_string(),
                version: 1,
            },
            &mut vec![0_u8; OBJECT_BYTES * 2].as_slice(),
            (OBJECT_BYTES * 2) as u64,
        ),
        Err(BackendError::InvalidRequest(message)) if message.contains("capacity")
    ));

    let removed = delete_profile_object(&mut backend, &object_key(0)).expect("DELETE succeeds");
    assert!(removed);
    drop(backend);

    let reopened = FolderBackend::open(&backend_root, manifest, capacity, 0)
        .expect("restart derives durable catalogue and usage");
    assert_eq!(
        list_profile_objects(&reopened, Some("generated/"))
            .expect("LIST after restart")
            .len(),
        OBJECT_COUNT - 1
    );
    assert_eq!(
        reopened.capacity().used_bytes,
        ((OBJECT_COUNT - 1) * OBJECT_BYTES) as u64
    );
    assert!(get_profile_object(&reopened, &object_key(0)).is_err());

    cleanup(&root);
}

fn provisioning_plan(backend_root: PathBuf) -> ProductProfileProvisioningPlan {
    let capacity = CapacityPolicy::bounded((OBJECT_COUNT * OBJECT_BYTES + OBJECT_BYTES) as u64, 0);
    let envelope = ProductPolicyTemplateAdapter::for_product(ProductPolicyAdapterKind::Synoptikon)
        .adapt(StoragePolicyTemplate {
            template_id: "mvp-generated-data".to_string(),
            owner_product: "synoptikon".to_string(),
            profile: DeploymentProfile::Folder,
            host_mode: HostMode::Integrated,
            protection: ProtectionPolicy::Reproducible,
            capacity: capacity.clone(),
            copies: 1,
            ingress_origin: IngressOrigin::Synoptikon,
        })
        .expect("product template");
    let mut store_policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
    store_policy.capacity = capacity;
    ProductProfileProvisioningPlan {
        policy_template: envelope,
        manifest: ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("synoptikon-mvp").expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::Integrated,
            protection: ProtectionPolicy::Reproducible,
            backend: BackendReference::Folder {
                root_identity: "acceptance:synoptikon-mvp".to_string(),
            },
        },
        store_policy,
        bucket_name: Some("synoptikon-mvp".to_string()),
        reader_group: Some("synoptikon-readers".to_string()),
        writer_group: Some("synoptikon-writers".to_string()),
        public: false,
        backend_root,
        ssd_staging_root: None,
        client_request_id: "synoptikon-mvp-install".to_string(),
        administrator_actor: "ignored-until-peer-authenticated".to_string(),
        dry_run: false,
    }
}

fn object_key(index: usize) -> BackendObjectKey {
    BackendObjectKey {
        object_id: format!("generated/object-{index:04}.bin"),
        version: 1,
    }
}

fn generated_payload(index: usize) -> Vec<u8> {
    (0..OBJECT_BYTES)
        .map(|offset| ((index * 31 + offset * 17) % 251) as u8)
        .collect()
}

fn acceptance_root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME is required"));
    let approved = home.join(".dasobjectstore-codex-validation");
    let configured = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| approved.clone());
    assert!(
        configured.starts_with(&approved),
        "acceptance data must remain below {}",
        approved.display()
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    configured.join(format!(
        "product-profile-mvp-{}-{now}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

struct AcceptanceOrchestrator;

impl DaemonServiceOrchestrator for AcceptanceOrchestrator {
    fn initialize_profile_capacity(
        &self,
        _store_id: &StoreId,
        _policy: CapacityPolicy,
    ) -> Result<bool, DaemonServiceRuntimeError> {
        Ok(true)
    }

    fn status(
        &self,
        _request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        Err(unsupported("status"))
    }

    fn lifecycle(
        &self,
        _request: DaemonServiceLifecycleRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        Err(unsupported("lifecycle"))
    }

    fn provision(
        &self,
        _request: DaemonServiceProvisionRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
        Err(unsupported("service provision"))
    }
}

fn unsupported(operation: &str) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("{operation} is outside product-profile acceptance"),
    }
}
