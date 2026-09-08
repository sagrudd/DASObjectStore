//! Independent existing-ledger seal regressions; synthetic custody only.
use super::*;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::time::Duration;

struct Fixture {
    directory: PathBuf,
    path: PathBuf,
    seal: ReaderSealV1,
    inventory: Vec<(String, u64)>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).expect("owned synthetic fixture cleanup");
    }
}
fn fixture() -> Fixture {
    let parent = PathBuf::from(std::env::var_os("HOME").unwrap())
        .canonicalize()
        .unwrap();
    let directory = parent.join(format!(".das-seal-review-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let path = directory.join("ledger.sqlite3");
    let receipt = super::super::tests::retained_fixture(&path, b"independent sealed evidence");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let inspected = inspect_custody_ledger(&path).unwrap();
    let seal = ReaderSealV1 {
        schema: "das.custody.reader_seal.v1".into(),
        companion_sha256: "a".repeat(64),
        bootstrap_transaction_id: uuid::Uuid::new_v4().to_string(),
        store_id: inspected.store_id.to_string(),
        configuration_sha256: inspected.configuration_sha256,
        inventory_sha256: "b".repeat(64),
        ledger_head_sha256: inspected.ledger_head_sha256,
        receipt_jcs_sha256: vec![raw_sha256(canonical_json(&receipt).unwrap().as_bytes())],
        completed_at_utc: "2026-09-05T12:00:00Z".into(),
    };
    Fixture {
        directory,
        path,
        seal,
        inventory: vec![(receipt.content_sha256, receipt.content_length)],
    }
}
fn verify(
    f: &Fixture,
    seal: &ReaderSealV1,
    inventory: &[(String, u64)],
) -> Result<(), CustodyReadError> {
    verify_reader_seal_existing(
        &f.path,
        seal,
        "custody-reader-v1",
        inventory,
        CustodyReadLimits {
            maximum_bytes: 1024,
            timeout: Duration::from_secs(5),
        }
        .start()
        .unwrap(),
    )
}
#[test]
fn review_complete_seal_preserves_exact_ledger() {
    let f = fixture();
    let before = fs::read(&f.path).unwrap();
    verify(&f, &f.seal, &f.inventory).unwrap();
    assert_eq!(fs::read(&f.path).unwrap(), before);
    assert_eq!(fs::read_dir(&f.directory).unwrap().count(), 1);
}
#[test]
fn review_seal_rejects_wrong_head_receipt_and_object_bijection() {
    let f = fixture();
    let before = fs::read(&f.path).unwrap();
    let mut seal = f.seal.clone();
    seal.ledger_head_sha256 = "c".repeat(64);
    assert!(verify(&f, &seal, &f.inventory).is_err());
    seal = f.seal.clone();
    seal.receipt_jcs_sha256 = vec!["d".repeat(64)];
    assert!(verify(&f, &seal, &f.inventory).is_err());
    let mut inventory = f.inventory.clone();
    inventory[0].1 += 1;
    assert!(verify(&f, &f.seal, &inventory).is_err());
    inventory[0].0 = "e".repeat(64);
    assert!(verify(&f, &f.seal, &inventory).is_err());
    assert_eq!(fs::read(&f.path).unwrap(), before);
}
#[test]
fn review_seal_denies_sidecar_and_missing_without_creation() {
    let f = fixture();
    let before = fs::read(&f.path).unwrap();
    let sidecar = f.directory.join("ledger.sqlite3-wal");
    fs::write(&sidecar, b"synthetic").unwrap();
    assert!(verify(&f, &f.seal, &f.inventory).is_err());
    assert_eq!(fs::read(&f.path).unwrap(), before);
    fs::remove_file(&sidecar).unwrap();
    fs::remove_file(&f.path).unwrap();
    assert!(verify(&f, &f.seal, &f.inventory).is_err());
    assert!(!f.path.exists());
}
#[test]
fn review_seal_denies_tampered_raw_receipt_without_repair() {
    let f = fixture();
    let connection = Connection::open(&f.path).unwrap();
    // Corruption injection only in this owned fixture. Restore the exact trigger
    // so schema drift alone cannot explain the subsequent denial.
    let trigger: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'custody_receipts_no_update'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    connection
        .execute_batch("DROP TRIGGER custody_receipts_no_update")
        .unwrap();
    connection
        .execute(
            "UPDATE custody_readback_receipts SET receipt_jcs = ?1",
            ["x".repeat(1048577)],
        )
        .unwrap();
    connection.execute_batch(&trigger).unwrap();
    drop(connection);
    let before = fs::read(&f.path).unwrap();
    assert!(verify(&f, &f.seal, &f.inventory).is_err());
    assert_eq!(fs::read(&f.path).unwrap(), before);
}
