//! Author-added process interruption matrix, not hardware power-loss proof.
use super::*;
use std::process::{Command, Stdio};

struct ChildGuard {
    child: std::process::Child,
    reaped: bool,
}
impl ChildGuard {
    fn spawn(command: &mut Command) -> Self {
        Self {
            child: command.spawn().unwrap(),
            reaped: false,
        }
    }
    fn wait(&mut self) -> std::process::ExitStatus {
        let status = self.child.wait().unwrap();
        self.reaped = true;
        status
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.reaped {
            // No try_wait/reaping occurred: cleanup only targets our owned child.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

const CHILD: &str = "runtime::custody_reader::manager_tests::interruption_tests::publication_child";

fn prepare(f: &Fixture) {
    // Only public, synthetic selection facts cross the test-process boundary.
    let raw = serde_jcs::to_vec(&(
        &f.selection.binding,
        &f.seal,
        &f.current,
        &f.selection.inventory,
    ))
    .unwrap();
    fs::write(f.root.join("interruption-fixture.jcs"), raw).unwrap();
}
fn command(f: &Fixture) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CHILD, "--nocapture"])
        .env("DAS_TEST_MANAGER_FIXTURE", &f.root)
        .env("DAS_TEST_PUBLICATION_ROOT", &f.selection.directory)
        .env_remove("DAS_TEST_PUBLICATION_POINT")
        .env_remove("DAS_TEST_PUBLICATION_ACTION")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}
fn snapshot(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(&f.selection.directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn publication_child() {
    let Some(root) = std::env::var_os("DAS_TEST_MANAGER_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root).canonicalize().unwrap();
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with(".das-manager-review-"));
    let (binding, seal, current, inventory): (
        ReaderBindingV1,
        ReaderSealV1,
        ReaderCurrentV1,
        Vec<(String, u64)>,
    ) = serde_json::from_slice(&fs::read(root.join("interruption-fixture.jcs")).unwrap()).unwrap();
    // No Fixture RAII owner is reconstructed: only the parent owns cleanup.
    let selection = ReaderSelection {
        selected_inventory_sha256: binding.inventory_sha256.clone(),
        binding,
        directory: root.join("records"),
        manager_uid: unsafe { libc::geteuid() },
        ledger: root.join("ledger.sqlite3"),
        inventory,
        encrypted_source: root.join("synthetic.enc"),
        protection: CredentialProtection::Host,
        backend_endpoint: "https://fixture.invalid".into(),
        aws_executable: root.join("not-executed"),
        aws_executable_sha256: "e".repeat(64),
    };
    let result = ReaderManager::open(selection.directory.clone())
        .unwrap()
        .publish_initial(&selection, &seal, &current, limits());
    if std::env::var("DAS_TEST_PUBLICATION_ACTION").ok().as_deref() == Some("error") {
        assert!(result.is_err(), "selected error boundary was not reached");
        std::process::exit(87);
    }
    // Used by fresh-process replay and concurrent publication checks. An injected
    // crash must exit earlier with 86; a missing hook therefore fails the parent.
    std::process::exit(if result.is_ok() { 90 } else { 91 });
}

#[test]
fn every_publication_boundary_crash_and_error_never_reissues() {
    let mut points = Vec::new();
    for name in ["manager.claim", "seal", "binding", "state", "current.jcs"] {
        for phase in [
            "after_open",
            "partial_write",
            "after_write",
            "after_file_sync",
            "after_dir_sync",
        ] {
            points.push(format!("{name}:{phase}"));
        }
    }
    points.extend([
        "manager.claim:after_unlink".into(),
        "manager.claim:after_unlink_sync".into(),
    ]);
    assert_eq!(points.len(), 27);
    for action in ["crash", "error"] {
        for point in &points {
            let f = fixture();
            prepare(&f);
            let ledger = fs::read(&f.selection.ledger).unwrap();
            let status = command(&f)
                .env("DAS_TEST_PUBLICATION_POINT", point)
                .env("DAS_TEST_PUBLICATION_ACTION", action)
                .status()
                .unwrap();
            assert_eq!(
                status.code(),
                Some(if action == "crash" { 86 } else { 87 }),
                "{action} {point}"
            );
            let retained = snapshot(&f);
            assert!(
                !retained.is_empty(),
                "exclusive claim must precede every injection"
            );
            let directory =
                files::Directory::open(f.selection.directory.clone(), f.selection.manager_uid)
                    .unwrap();
            let complete = point.starts_with("manager.claim:after_unlink");
            assert_eq!(
                load_records(&directory, &f.selection, &clock_now()).is_ok(),
                complete,
                "{action} {point}"
            );
            if complete {
                assert_eq!(retained.len(), 4);
                assert!(!retained.contains_key("manager.claim"));
                assert_eq!(retained["current.jcs"], f.current.encode().unwrap());
            } else {
                assert!(retained.contains_key("manager.claim"));
            }
            // Restart in another process, not merely reuse a cached manager handle.
            assert_eq!(
                command(&f).status().unwrap().code(),
                Some(91),
                "reentry {action} {point}"
            );
            assert_eq!(snapshot(&f), retained, "reentry must not repair or rewrite");
            assert_eq!(fs::read(&f.selection.ledger).unwrap(), ledger);
        }
    }
}

#[test]
fn concurrent_processes_publish_exactly_once() {
    let f = fixture();
    prepare(&f);
    let ledger = fs::read(&f.selection.ledger).unwrap();
    let mut children: Vec<_> = (0..4)
        .map(|_| ChildGuard::spawn(&mut command(&f)))
        .collect();
    let results: Vec<_> = children
        .iter_mut()
        .map(|child| child.wait().code().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|code| **code == 90).count(), 1);
    assert_eq!(results.iter().filter(|code| **code == 91).count(), 3);
    let directory =
        files::Directory::open(f.selection.directory.clone(), f.selection.manager_uid).unwrap();
    load_records(&directory, &f.selection, &clock_now()).unwrap();
    assert_eq!(snapshot(&f).len(), 4);
    assert_eq!(fs::read(&f.selection.ledger).unwrap(), ledger);
}

#[test]
fn stale_preflight_contender_cannot_reclaim_completed_publication() {
    let f = fixture();
    prepare(&f);
    let mut pause_command = command(&f);
    pause_command
        .env("DAS_TEST_PUBLICATION_POINT", "manager:after_empty")
        .env("DAS_TEST_PUBLICATION_ACTION", "pause");
    let mut paused = ChildGuard::spawn(&mut pause_command);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while !f.root.join("paused").exists() {
        assert!(
            std::time::Instant::now() < until,
            "child did not reach empty preflight"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let contender = command(&f).status().unwrap();
    // Resume before asserting so a regression does not leave the paused child.
    fs::write(f.root.join("resume"), b"").unwrap();
    let winner = paused.wait();
    assert_eq!(
        contender.code(),
        Some(91),
        "full-call lock must reject contender before effects"
    );
    assert_eq!(winner.code(), Some(90));
    assert!(!f.selection.directory.join("manager.claim").exists());
    let directory =
        files::Directory::open(f.selection.directory.clone(), f.selection.manager_uid).unwrap();
    load_records(&directory, &f.selection, &clock_now()).unwrap();
    assert_eq!(snapshot(&f).len(), 4);
}

#[test]
fn shared_manager_threads_have_independent_lock_descriptions() {
    let f = fixture();
    let manager = ReaderManager::open(f.selection.directory.clone()).unwrap();
    let held = manager.directory.lock_publication().unwrap();
    std::thread::scope(|scope| {
        assert!(scope
            .spawn(|| manager.publish_initial(&f.selection, &f.seal, &f.current, limits()))
            .join()
            .unwrap()
            .is_err());
    });
    assert!(
        snapshot(&f).is_empty(),
        "shared manager must not share the lock description"
    );
    drop(held);
    let barrier = std::sync::Barrier::new(4);
    let results = std::thread::scope(|scope| {
        let mut threads = Vec::new();
        for _ in 0..4 {
            let shared = &manager;
            let fixture = &f;
            let barrier = &barrier;
            threads.push(scope.spawn(move || {
                barrier.wait();
                shared.publish_initial(
                    &fixture.selection,
                    &fixture.seal,
                    &fixture.current,
                    limits(),
                )
            }));
        }
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        results.iter().filter(|value| value.is_ok()).count(),
        1,
        "publication outcomes (closed categories): {results:?}; fixture={:?}",
        f.root,
    );
    let directory =
        files::Directory::open(f.selection.directory.clone(), f.selection.manager_uid).unwrap();
    load_records(&directory, &f.selection, &clock_now()).unwrap();
    assert_eq!(snapshot(&f).len(), 4);
}
