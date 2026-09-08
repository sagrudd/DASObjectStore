//! Independent snapshot and no-side-effect regressions; synthetic data only.
// The author_* cases were added by the implementation author;
// the original seven review_* cases are the independent lead-agent tests.
use super::*;
use std::os::unix::fs::{symlink, DirBuilderExt, PermissionsExt};

struct OwnedDirectory(PathBuf);
impl OwnedDirectory {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove only owned synthetic fixture");
    }
}

struct Reader {
    bytes: Vec<u8>,
    identity: &'static str,
    calls: usize,
    effect: Option<Box<dyn FnOnce()>>,
}
impl BoundedCustodyObjectReader for Reader {
    fn identity(&self) -> &str {
        self.identity
    }
    fn read_bounded(
        &mut self,
        _key: &str,
        _length: u64,
        deadline: CustodyReadDeadline,
    ) -> Result<Vec<u8>, CustodyReadError> {
        deadline.remaining()?;
        self.calls += 1;
        if let Some(effect) = self.effect.take() {
            effect();
        }
        Ok(self.bytes.clone())
    }
}
fn fixture() -> (OwnedDirectory, PathBuf, CustodyIntegrityReceiptV1, Reader) {
    let parent = PathBuf::from(std::env::var_os("HOME").expect("owned test parent"))
        .canonicalize()
        .unwrap();
    let directory =
        OwnedDirectory(parent.join(format!(".das-read-review-{}", uuid::Uuid::new_v4())));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(directory.path())
        .unwrap();
    let path = directory.path().join("ledger.sqlite3");
    let bytes = b"independent bounded custody evidence".to_vec();
    let receipt = super::super::tests::retained_fixture(&path, &bytes);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    (
        directory,
        path,
        receipt,
        Reader {
            bytes,
            identity: "custody-reader-v1",
            calls: 0,
            effect: None,
        },
    )
}
fn limits() -> CustodyReadLimits {
    CustodyReadLimits {
        maximum_bytes: 1024,
        timeout: Duration::from_secs(5),
    }
}

#[test]
fn author_full_raw_ledger_binding_rejects_wrong_digest_and_foreign_descriptor_before_get() {
    let (_owned, path, receipt, mut reader) = fixture();
    let original = fs::read(&path).unwrap();
    let digest = sha256_hex(&original);
    let mut file = fs::File::open(&path).unwrap();
    assert!(verify_custody_readback_existing_bound(
        &path,
        &receipt,
        &mut reader,
        limits(),
        &mut file,
        &"f".repeat(64)
    )
    .is_err());
    assert_eq!(reader.calls, 0);
    let (_other, foreign, _, _) = fixture();
    let mut file = fs::File::open(foreign).unwrap();
    assert!(verify_custody_readback_existing_bound(
        &path,
        &receipt,
        &mut reader,
        limits(),
        &mut file,
        &digest
    )
    .is_err());
    assert_eq!(reader.calls, 0);
    let mut file = fs::File::open(&path).unwrap();
    verify_custody_readback_existing_bound(
        &path,
        &receipt,
        &mut reader,
        limits(),
        &mut file,
        &digest,
    )
    .unwrap();
    assert_eq!(reader.calls, 1);
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn author_same_length_raw_ledger_drift_after_get_cannot_return_success() {
    let (_owned, path, receipt, mut reader) = fixture();
    let original = fs::read(&path).unwrap();
    let digest = sha256_hex(&original);
    let mut changed = original.clone();
    *changed.last_mut().unwrap() ^= 1;
    let effect_path = path.clone();
    reader.effect = Some(Box::new(move || fs::write(effect_path, changed).unwrap()));
    let mut file = fs::File::open(&path).unwrap();
    assert!(verify_custody_readback_existing_bound(
        &path,
        &receipt,
        &mut reader,
        limits(),
        &mut file,
        &digest
    )
    .is_err());
    assert_eq!(reader.calls, 1);
    assert_eq!(fs::read(path).unwrap().len(), original.len());
}
fn names(path: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}
#[test]
fn review_snapshot_success_preserves_exact_database_and_directory() {
    let (directory, path, receipt, mut reader) = fixture();
    let before = fs::read(&path).unwrap();
    let entries = names(directory.path());
    let verified =
        verify_custody_readback_existing(&path, &receipt, &mut reader, limits()).unwrap();
    assert_eq!(verified.receipt, receipt);
    assert_eq!(verified.bytes, reader.bytes);
    assert_eq!(verified.configuration_sha256, receipt.configuration_sha256);
    assert_eq!(verified.ledger_head_sha256.len(), 64);
    assert_eq!(reader.calls, 1);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(names(directory.path()), entries);
}
#[test]
fn review_invalid_budgets_and_foreign_reader_never_acquire() {
    let (_directory, path, receipt, mut reader) = fixture();
    for budget in [
        CustodyReadLimits {
            maximum_bytes: 0,
            ..limits()
        },
        CustodyReadLimits {
            maximum_bytes: receipt.content_length - 1,
            ..limits()
        },
        CustodyReadLimits {
            maximum_bytes: u64::MAX,
            ..limits()
        },
        CustodyReadLimits {
            timeout: Duration::ZERO,
            ..limits()
        },
        CustodyReadLimits {
            timeout: Duration::from_secs(301),
            ..limits()
        },
    ] {
        assert!(matches!(
            verify_custody_readback_existing(&path, &receipt, &mut reader, budget),
            Err(CustodyReadError::Input)
        ));
    }
    reader.identity = "foreign-reader";
    assert!(verify_custody_readback_existing(&path, &receipt, &mut reader, limits()).is_err());
    assert_eq!(reader.calls, 0);
}
#[test]
fn review_missing_alias_and_each_sidecar_deny_without_acquisition() {
    let (directory, path, receipt, mut reader) = fixture();
    let missing = directory.path().join("absent.sqlite3");
    assert!(verify_custody_readback_existing(&missing, &receipt, &mut reader, limits()).is_err());
    assert!(!missing.exists());
    let alias = directory.path().join("alias.sqlite3");
    symlink(&path, &alias).unwrap();
    assert!(verify_custody_readback_existing(&alias, &receipt, &mut reader, limits()).is_err());
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        fs::write(&sidecar, b"synthetic sidecar").unwrap();
        assert!(verify_custody_readback_existing(&path, &receipt, &mut reader, limits()).is_err());
        fs::remove_file(sidecar).unwrap();
    }
    assert_eq!(reader.calls, 0);
}
#[test]
fn review_unknown_schema_denies_before_acquisition() {
    let (_directory, path, receipt, mut reader) = fixture();
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE VIEW injected_view AS SELECT 1")
        .unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        verify_custody_readback_existing(&path, &receipt, &mut reader, limits()),
        Err(CustodyReadError::Ledger)
    ));
    assert_eq!(reader.calls, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
}
#[test]
fn review_wrong_exact_length_bytes_return_no_verified_value() {
    let (_directory, path, receipt, mut reader) = fixture();
    reader.bytes[0] ^= 1;
    assert!(matches!(
        verify_custody_readback_existing(&path, &receipt, &mut reader, limits()),
        Err(CustodyReadError::Acquisition)
    ));
    assert_eq!(reader.calls, 1);
}
#[test]
fn review_persistent_replacement_during_read_denies() {
    let (directory, path, receipt, mut reader) = fixture();
    let replacement = directory.path().join("replacement.sqlite3");
    fs::copy(&path, &replacement).unwrap();
    let destination = path.clone();
    reader.effect = Some(Box::new(move || {
        fs::rename(replacement, destination).unwrap()
    }));
    assert!(matches!(
        verify_custody_readback_existing(&path, &receipt, &mut reader, limits()),
        Err(CustodyReadError::Boundary)
    ));
    assert_eq!(reader.calls, 1);
}
#[test]
fn review_sidecar_appearing_during_read_denies_without_cleanup() {
    let (_directory, path, receipt, mut reader) = fixture();
    let sidecar = PathBuf::from(format!("{}-journal", path.display()));
    let create = sidecar.clone();
    reader.effect = Some(Box::new(move || {
        fs::write(create, b"independent conflict").unwrap()
    }));
    assert!(matches!(
        verify_custody_readback_existing(&path, &receipt, &mut reader, limits()),
        Err(CustodyReadError::Boundary)
    ));
    assert_eq!(fs::read(sidecar).unwrap(), b"independent conflict");
}
#[test]
fn author_sql_progress_hook_interrupts_real_recursive_work() {
    let connection = Connection::open_in_memory().unwrap();
    let deadline = CustodyReadLimits {
        maximum_bytes: 1,
        timeout: Duration::from_millis(20),
    }
    .start()
    .unwrap();
    configure_read_connection(&connection, deadline).unwrap();
    let started = Instant::now();
    let result = connection.query_row("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n<1000000000) SELECT sum(n) FROM numbers", [], |r| r.get::<_,i64>(0));
    assert!(result.is_err());
    assert!(deadline.remaining().is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn author_exclusive_sql_lock_denies_before_get_without_busy_wait() {
    let (_directory, path, receipt, mut reader) = fixture();
    let before = fs::read(&path).unwrap();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let started = Instant::now();
    assert!(verify_custody_readback_existing(&path, &receipt, &mut reader, limits()).is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(reader.calls, 0);
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn author_inherited_budget_is_never_renewed_and_expiry_denies_before_get() {
    let original = limits().start().unwrap();
    assert_eq!(
        original.capped(Duration::from_secs(300)).unwrap().0,
        original.0
    );
    assert!(original.capped(Duration::from_millis(1)).unwrap().0 < original.0);
    let (_directory, path, receipt, mut reader) = fixture();
    let mut file = fs::File::open(&path).unwrap();
    let digest = crate::custody_reader::raw_sha256(&fs::read(&path).unwrap());
    let budget = CustodyReadLimits {
        maximum_bytes: 1024,
        timeout: Duration::from_millis(1),
    }
    .start()
    .unwrap();
    std::thread::sleep(Duration::from_millis(3)); // ingress consumed the inherited budget
    assert!(verify_custody_readback_existing_bound_at(
        &path,
        &receipt,
        &mut reader,
        1024,
        (&mut file, &digest),
        budget
    )
    .is_err());
    assert_eq!(reader.calls, 0);
    assert!(budget.capped(Duration::from_secs(300)).is_err());
}
