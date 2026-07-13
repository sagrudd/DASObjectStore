use dasobjectstore_core::backend::ObjectStoreBackend;
use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::manifest::{
    BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
};
use dasobjectstore_core::protection::ProtectionPolicy;
use dasobjectstore_core::store::CapacityPolicy;
use dasobjectstore_daemon::runtime::{
    FolderBackend, ReconciliationAction, ReconciliationEntryState, ReconciliationManifest,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn bounded_folder_fixture_covers_create_adopt_restart_quota_and_drift() {
    let root = unique_root("folder-profile-fixture");
    let mut backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(32, 1), 0)
        .expect("programmatic bounded-folder creation succeeds");
    assert!(root.join(".dasobjectstore/catalogue.json").exists());

    let source = root.join("incoming/sample.txt");
    fs::create_dir_all(source.parent().expect("source parent")).expect("source parent creates");
    fs::write(&source, b"fixture-data").expect("source writes");
    let checkpoint_path = root.with_extension("checkpoint").join("manifest.json");
    let mut checkpoint = backend
        .plan_user_tree_reconciliation()
        .expect("adoption plan builds")
        .manifest;
    checkpoint
        .save_atomic(&checkpoint_path)
        .expect("checkpoint persists");
    let mut resumed = ReconciliationManifest::load(&checkpoint_path).expect("checkpoint reloads");
    let records = backend
        .adopt_user_tree_reconciliation(&checkpoint_path, &mut resumed, "fixture-adopt")
        .expect("explicit adoption succeeds");
    assert_eq!(records.len(), 1);
    assert!(source.exists(), "adoption preserves the user source");
    assert_eq!(backend.capacity().used_bytes, 12);
    assert_eq!(
        resumed.entries["incoming/sample.txt"].state,
        ReconciliationEntryState::Complete
    );

    drop(backend);
    let mut reopened = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(32, 1), 0)
        .expect("restart/reopen derives usage from the private catalogue");
    assert_eq!(reopened.capacity().used_bytes, 12);
    assert_eq!(reopened.catalogue_records(), records);
    assert!(reopened.reserve("over-quota", 21).is_err());
    assert!(matches!(
        reopened
            .replan_user_tree_reconciliation(&mut resumed)
            .expect("completed checkpoint replans"),
        plan if plan.actions == vec![ReconciliationAction::SkipComplete {
            key: "incoming/sample.txt".to_string(),
            relative_path: "incoming/sample.txt".to_string(),
        }]
    ));
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(checkpoint_path.parent().expect("checkpoint parent"));
}

#[cfg(unix)]
#[test]
fn bounded_folder_fixture_reports_symlink_drift_without_adopting_it() {
    use std::os::unix::fs::symlink;

    let root = unique_root("folder-profile-hostile");
    let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(32, 1), 0)
        .expect("bounded backend opens");
    let outside = root.with_extension("outside");
    fs::create_dir_all(&outside).expect("outside creates");
    fs::write(outside.join("secret.txt"), b"must-not-adopt").expect("outside file writes");
    let link = root.join("incoming");
    symlink(&outside, &link).expect("symlink creates");

    let report = backend.inspect_user_tree().expect("inspection succeeds");
    assert!(report.unsafe_paths.iter().any(|path| path == "incoming"));
    assert!(report.unmanaged_paths.is_empty());
    assert!(backend
        .plan_user_tree_reconciliation()
        .expect("hostile plan builds")
        .plan
        .actions
        .iter()
        .all(|action| !matches!(action, ReconciliationAction::Download { .. })));

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}

fn manifest() -> ObjectStoreManifest {
    ObjectStoreManifest {
        schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
        store_id: StoreId::new("codex-fixture").expect("store id"),
        deployment_profile: DeploymentProfile::Folder,
        host_mode: HostMode::PerUser,
        protection: ProtectionPolicy::LocalOnly,
        backend: BackendReference::Folder {
            root_identity: "fixture-root".to_string(),
        },
    }
}

fn unique_root(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    assert!(
        parent.is_absolute(),
        "fixture root parent must be absolute: {}",
        parent.display()
    );
    parent.join(format!(
        "dasobjectstore-{label}-{}-{now}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}
