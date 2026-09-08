//! Real local process/FIFO fixtures; synthetic bytes, no network or credentials.
use super::*;
use dasobjectstore_object_service::custody::CustodyReadLimits;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let parent = PathBuf::from(std::env::var_os("HOME").expect("test home"))
            .canonicalize()
            .unwrap();
        let root = parent.join(format!(
            ".custody-bounded-fixture-{}-{}",
            std::process::id(),
            CUSTODY_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let executable = root.join("fixture-aws");
        let body = format!(
            r#"#!/usr/bin/python3
import os, sys, time, subprocess
mode = {mode:?}
fifo = sys.argv[-1]
assert os.environ.get('AWS_CONFIG_FILE') == '/dev/null'
assert os.environ.get('AWS_SHARED_CREDENTIALS_FILE') == '/dev/null'
assert os.environ.get('AWS_MAX_ATTEMPTS') == '1'
assert os.environ.get('AWS_EC2_METADATA_DISABLED') == 'true'
assert 'SSH_AUTH_SOCK' not in os.environ
assert 'AWS_PROFILE' not in os.environ
assert '--endpoint-url' in sys.argv
if mode == 'no-open':
    time.sleep(30)
elif mode == 'stdout':
    sys.stdout.write('x' * 100000); sys.stdout.flush(); time.sleep(30)
elif mode == 'stderr':
    sys.stderr.write('x' * 100000); sys.stderr.flush(); time.sleep(30)
elif mode == 'replace':
    os.unlink(fifo)
    open(fifo, 'wb').write(b'exact')
else:
    with open(fifo, 'wb', buffering=0) as out:
        if mode == 'stall':
            out.write(b'e'); time.sleep(30)
        elif mode == 'short': out.write(b'ex')
        elif mode == 'over': out.write(b'exact!')
        elif mode == 'huge': out.write(b'x' * 1000000)
        else: out.write(b'exact')
    if mode == 'nonzero': sys.exit(1)
    if mode == 'cleanup': open(os.path.join(os.path.dirname(fifo), 'unrelated'), 'w').write('retain')
    if mode == 'descendant':
        child = subprocess.Popen(['/usr/bin/python3', '-c', 'import time;time.sleep(30)'])
        open(os.path.join(os.path.dirname(os.path.dirname(fifo)), 'descendant.pid'), 'w').write(str(child.pid))
"#
        );
        fs::write(&executable, body).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root, executable }
    }
    fn read(&self, timeout: Duration) -> Result<Vec<u8>, CustodyReadError> {
        let runner = super::super::super::SystemServiceCommandRunner;
        let reader = GarageCustodyS3Reader::new(
            &runner,
            "http://127.0.0.1:1",
            "synthetic",
            "synthetic-reader",
            vec![
                ("AWS_ACCESS_KEY_ID".into(), "fixture-only".into()),
                ("AWS_SECRET_ACCESS_KEY".into(), "not-a-secret".into()),
            ],
            &self.root,
        );
        let mut bounded = reader.into_bounded(self.executable.clone())?;
        bounded.read_bounded(
            "sha256/synthetic",
            5,
            CustodyReadLimits {
                maximum_bytes: 5,
                timeout,
            }
            .start()?,
        )
    }
    fn scratch(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("bounded-get-")
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn bounded_process_exact_fifo_and_cleanup() {
    let f = Fixture::new("exact");
    assert_eq!(f.read(Duration::from_secs(3)).unwrap(), b"exact");
    assert!(f.scratch().is_empty());
}

#[test]
fn bounded_process_observation_keeps_leader_waitable_until_cleanup() {
    use std::os::unix::process::CommandExt;
    let owned = OwnedProcess(
        Command::new("/usr/bin/true")
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let pid = owned.0.id() as i32;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while owned.observed_success().unwrap().is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(owned.observed_success().unwrap(), Some(true));
    // SAFETY: signal zero only checks the unreaped directly owned child exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    drop(owned);
    // SAFETY: waitpid probes this exact already-reaped child without blocking.
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn bounded_process_cleanup_handles_fifo_never_created() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new("exact");
    let directory = f.root.join("only-created-directory");
    fs::create_dir(&directory).unwrap();
    let m = fs::metadata(&directory).unwrap();
    let scratch = Scratch {
        fifo: directory.join("body"),
        directory: directory.clone(),
        directory_identity: (m.dev(), m.ino()),
        fifo_identity: None,
    };
    scratch.cleanup().unwrap();
    assert!(!directory.exists());
}

#[test]
fn bounded_process_short_overrun_and_nonzero_never_return_partial() {
    for mode in ["short", "over", "huge", "nonzero", "stdout", "stderr"] {
        let f = Fixture::new(mode);
        assert_eq!(
            f.read(Duration::from_secs(3)),
            Err(CustodyReadError::Acquisition),
            "{mode}"
        );
        assert!(f.scratch().is_empty(), "{mode}");
    }
}

#[test]
fn bounded_process_stalls_and_descendants_hit_deadline() {
    for mode in ["no-open", "stall", "descendant"] {
        let f = Fixture::new(mode);
        let before = std::time::Instant::now();
        assert_eq!(
            f.read(Duration::from_millis(400)),
            Err(CustodyReadError::Deadline),
            "{mode}"
        );
        assert!(before.elapsed() < Duration::from_secs(3));
        assert!(f.scratch().is_empty());
        let pidfile = f.root.join("descendant.pid");
        if pidfile.exists() {
            let pid: i32 = fs::read_to_string(pidfile).unwrap().parse().unwrap();
            let mut dead = false;
            for _ in 0..50 {
                // SAFETY: signal zero only probes the fixture-reported child PID.
                if unsafe { libc::kill(pid, 0) } == -1 {
                    dead = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(dead, "descendant still alive after owned-group termination");
        }
    }
}

#[test]
fn bounded_process_replacement_and_cleanup_failure_preserve_foreign_files() {
    for mode in ["replace", "cleanup"] {
        let f = Fixture::new(mode);
        assert!(f.read(Duration::from_secs(3)).is_err());
        let scratch = f.scratch();
        assert_eq!(scratch.len(), 1);
        let preserved = scratch[0].join(if mode == "replace" {
            "body"
        } else {
            "unrelated"
        });
        assert!(preserved.is_file());
    }
}
