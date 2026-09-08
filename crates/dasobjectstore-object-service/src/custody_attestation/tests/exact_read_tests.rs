//! Real journal transactions, synthetic existing Ed25519 authority only.
use super::*;
use std::cell::Cell;
const NOW: &str = "2026-09-05T10:01:00Z";
fn deadline() -> CustodyReadDeadline {
    crate::custody::CustodyReadLimits {
        maximum_bytes: 1024,
        timeout: std::time::Duration::from_secs(5),
    }
    .start()
    .unwrap()
}
struct Fixture {
    directory: PathBuf,
    journal: CustodyOffNucJournal,
    raw: Vec<u8>,
    body: CustodyOffNucPreReadRequestV1,
    key: SigningKey,
    authority: CustodyEd25519AuthorityV1,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}
fn fixture() -> Fixture {
    let directory = std::env::temp_dir().join(format!("das-exact-journal-{}", Uuid::new_v4()));
    let journal = CustodyOffNucJournal::create(directory.join("journal.sqlite")).unwrap();
    let (key, authority) = authority();
    let body = request(1, None);
    let raw = sign(body.clone(), &key, &authority);
    journal
        .issue_pre_read_request(&raw, &authority, NOW)
        .unwrap();
    Fixture {
        directory,
        journal,
        raw,
        body,
        key,
        authority,
    }
}
#[test]
fn exact_issued_bytes_reach_callback_only_after_commit_and_never_replay() {
    let f = fixture();
    let calls = Cell::new(0);
    f.journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |attempt, raw| {
            assert_eq!(raw, f.raw);
            assert_eq!(attempt.request_id, f.body.request_id);
            let state: String = Connection::open(&f.journal.path)
                .unwrap()
                .query_row("SELECT status FROM issued_pre_read_requests", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(state, "started");
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
    let reopened = CustodyOffNucJournal::open_existing(&f.journal.path).unwrap();
    assert!(reopened
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 1);
}
#[test]
fn valid_resigned_substitutions_same_id_do_not_start_or_call_transport() {
    let f = fixture();
    let original = serde_json::to_value(&f.body).unwrap();
    for (field, replacement) in [
        ("target_id", "other".into()),
        ("nonce", Uuid::new_v4().to_string()),
        ("routing_sha256", "f".repeat(64)),
        ("expires_at_utc", "2026-09-05T13:00:00Z".into()),
    ] {
        let mut value = original.clone();
        value[field] = serde_json::Value::String(replacement);
        let changed: CustodyOffNucPreReadRequestV1 = serde_json::from_value(value).unwrap();
        let raw = sign(changed, &f.key, &f.authority);
        let calls = Cell::new(0);
        assert!(f
            .journal
            .perform_pre_read_exact(&raw, &f.authority, NOW, deadline(), |_, _| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .is_err());
        assert_eq!(calls.get(), 0);
        assert_eq!(
            Connection::open(&f.journal.path)
                .unwrap()
                .query_row::<u64, _, _>("SELECT COUNT(*) FROM first_attempts", [], |r| r.get(0))
                .unwrap(),
            0
        );
    }
    f.journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| Ok(()))
        .unwrap();
}
#[test]
fn corrupt_stored_digest_and_expired_row_deny_before_callback() {
    for statement in [
        "UPDATE issued_pre_read_requests SET raw_sha256=replace(raw_sha256,substr(raw_sha256,1,1),'z')",
        "UPDATE issued_pre_read_requests SET expires_at_utc='2026-09-05T10:00:01Z'",
        "UPDATE issued_pre_read_requests SET target_id='different'",
        "UPDATE issued_pre_read_requests SET nonce='different'",
        "UPDATE issued_pre_read_requests SET sequence=2",
        "UPDATE issued_pre_read_requests SET previous_request_sha256='different'",
        "UPDATE issued_pre_read_requests SET issued_at_utc='2026-09-05T10:00:59Z'",
    ] {
        let f = fixture();
        Connection::open(&f.journal.path).unwrap().execute(statement, []).unwrap();
        let calls = Cell::new(0);
        assert!(f.journal.perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| { calls.set(calls.get()+1); Ok(()) }).is_err());
        assert_eq!(calls.get(), 0);
    }
}
#[test]
fn callback_failure_is_terminal_and_legacy_start_cannot_be_laundered() {
    let f = fixture();
    assert!(f
        .journal
        .perform_pre_read_exact::<()>(&f.raw, &f.authority, NOW, deadline(), |_, _| Err(invalid(
            "synthetic disconnect"
        )))
        .is_err());
    let calls = Cell::new(0);
    assert!(f
        .journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 0);
    let f = fixture();
    f.journal
        .begin_pre_read_attempt(&f.body.request_id, NOW)
        .unwrap();
    assert!(f
        .journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 0);
}

#[test]
fn new_path_denies_busy_or_missing_journal_without_wait_or_creation() {
    let f = fixture();
    let lock = Connection::open(&f.journal.path).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let calls = Cell::new(0);
    let began = std::time::Instant::now();
    assert!(f
        .journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert!(began.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(calls.get(), 0);
    drop(lock);
    fs::remove_file(&f.journal.path).unwrap();
    assert!(f
        .journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 0);
    assert!(!f.journal.path.exists());
}

#[test]
fn deadline_expiry_preserves_started_without_renewed_settlement_or_retry() {
    let f = fixture();
    let short = crate::custody::CustodyReadLimits {
        maximum_bytes: 1024,
        timeout: std::time::Duration::from_secs(1),
    }
    .start()
    .unwrap();
    let calls = Cell::new(0);
    assert!(f
        .journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, short, |_, _| {
            calls.set(calls.get() + 1);
            std::thread::sleep(std::time::Duration::from_millis(1100));
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 1);
    assert_eq!(
        Connection::open(&f.journal.path)
            .unwrap()
            .query_row::<String, _, _>("SELECT status FROM issued_pre_read_requests", [], |r| r
                .get(0))
            .unwrap(),
        "started"
    );
    assert!(f
        .journal
        .perform_pre_read_exact(&f.raw, &f.authority, NOW, deadline(), |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.get(), 1);
}
