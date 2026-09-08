//! Test-only bridge from actual Garage retention to the existing TLS fixture.
//! Neither guest UID separation nor these fixture selections grant S4 authority.
use super::*;
use std::os::unix::fs::MetadataExt;

const GARAGE: &str = "/var/lib/das-garage-fixture";

pub(super) fn enabled() -> bool {
    let path = Path::new("/run/das-systemd-vm-fixture/garage-joined");
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Ok(meta) => {
            assert!(meta.is_file() && !meta.file_type().is_symlink());
            assert_eq!(meta.uid(), 0);
            assert_eq!(meta.mode() & 0o022, 0);
            true
        }
        Err(_) => panic!("fixture selection boundary"),
    }
}

pub(super) fn retained_fixture() -> (Fixture, zeroize::Zeroizing<String>) {
    loader_vm_tests::guest_guard(0);
    assert!(enabled());
    let source = Path::new(GARAGE);
    assert_eq!(source.canonicalize().unwrap(), source);
    let retained: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("retained.json")).unwrap()).unwrap();
    let continuation: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("continuation.json")).unwrap()).unwrap();
    let definition: dasobjectstore_object_service::CustodyStoreDefinitionV1 =
        serde_json::from_value(retained["definition"].clone()).unwrap();
    definition.validate().unwrap();
    assert_eq!(
        continuation["reader_identity"],
        definition.profile.reader_identity
    );
    assert_eq!(continuation["ledger_sha256"], retained["ledger_sha256"]);
    let ledger = PathBuf::from(retained["ledger"].as_str().unwrap());
    assert!(ledger.starts_with(source.join("sealed")));
    assert_eq!(ledger.canonicalize().unwrap(), ledger);
    let original = fs::symlink_metadata(&ledger).unwrap();
    assert!(original.is_file() && original.nlink() == 1 && original.len() <= 16 * 1024 * 1024);
    assert_eq!(original.uid(), 2002);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert_eq!(
            fs::symlink_metadata(format!("{}{suffix}", ledger.display()))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }
    let raw = fs::read(&ledger).unwrap();
    assert_eq!(raw_sha256(&raw), retained["ledger_sha256"]);
    let receipts: Vec<CustodyIntegrityReceiptV1> =
        serde_json::from_value(retained["receipts"].clone()).unwrap();
    assert_eq!(receipts.len(), 2);
    let inventory: Vec<(String, u64)> =
        serde_json::from_value(retained["inventory"].clone()).unwrap();
    assert_eq!(inventory.len(), 2);
    let root = Path::new("/var/lib").join(format!(".das-manager-review-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("records"))
        .unwrap();
    let destination = root.join("ledger.sqlite3");
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&destination)
        .unwrap();
    output.write_all(&raw).unwrap();
    output.sync_all().unwrap();
    drop(output);
    assert_eq!(
        raw_sha256(&fs::read(&destination).unwrap()),
        retained["ledger_sha256"]
    );
    assert_eq!(fs::read(&ledger).unwrap(), raw);
    let after = fs::symlink_metadata(&ledger).unwrap();
    assert_eq!(
        (
            original.dev(),
            original.ino(),
            original.uid(),
            original.mode(),
            original.ctime(),
            original.ctime_nsec()
        ),
        (
            after.dev(),
            after.ino(),
            after.uid(),
            after.mode(),
            after.ctime(),
            after.ctime_nsec()
        )
    );
    let mut fixture = fixture_from_retained(
        root,
        destination,
        receipts,
        &definition.profile.reader_identity,
    );
    assert_eq!(fixture.selection.inventory, inventory);
    // This exact small fixture inventory is server-owned test selection, not a
    // new deployed companion inventory encoding or formal17-object mapping.
    let digest = raw_sha256(&serde_jcs::to_vec(&inventory).unwrap());
    let now = clock_now();
    fixture.seal.completed_at_utc = now.clone();
    fixture.selection.binding.not_before_utc = now.clone();
    fixture.current.updated_at_utc = now;
    fixture.seal.inventory_sha256 = digest.clone();
    fixture.selection.selected_inventory_sha256 = digest.clone();
    fixture.selection.binding.inventory_sha256 = digest;
    fixture.selection.binding.seal_sha256 = raw_sha256(&fixture.seal.encode().unwrap());
    fixture.selection.binding.bucket_name = definition.bucket_name;
    fixture.selection.binding.backend_key_id = continuation["key"].as_str().unwrap().into();
    fixture.selection.backend_endpoint = "http://127.0.0.1:3901".into();
    let private = source.join("continuation.private");
    let metadata = fs::symlink_metadata(&private).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1);
    assert_eq!(metadata.uid(), 2002);
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert!(metadata.len() <= 65536);
    let raw = zeroize::Zeroizing::new(fs::read(&private).unwrap());
    let credential = SystemdServiceCredentialHandoffResolver::decode_continuation(
        &fixture.selection.binding,
        &raw,
    )
    .unwrap();
    let (_, mut environment) = credential.into_parts();
    let index = environment
        .iter()
        .position(|(key, _)| key == "AWS_SECRET_ACCESS_KEY")
        .unwrap();
    let secret = zeroize::Zeroizing::new(environment.remove(index).1);
    fs::remove_file(private).unwrap();
    (fixture, secret)
}
