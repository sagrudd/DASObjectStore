//! Disposable native-process fixture for the Synoptikon projection gateway.
//!
//! This binary is available only with `test-support`; release/package builds
//! cannot contain it. The process must run as the real `dasobjectstore` user so
//! the production Unix server derives the peer through unchanged SO_PEERCRED.

use dasobjectstore_daemon::{SynoptikonProjectionTestFixture, UnixSocketDaemonServer};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "fixture root argument is required".to_owned())?;
    let socket = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "fixture socket argument is required".to_owned())?;
    let now_utc = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| "fixture UTC timestamp argument is required".to_owned())?;
    if args.next().is_some() {
        return Err("unexpected fixture argument".to_owned());
    }
    let fixture = SynoptikonProjectionTestFixture::new(&root, now_utc)?;
    let worker = fixture.clone();
    thread::spawn(move || {
        // Let the real upload transaction publish its SSD receipt and queue
        // before the disposable worker begins contending for live metadata.
        thread::sleep(Duration::from_millis(500));
        loop {
            match worker.settle_one_hdd_placement() {
                Ok(_) => {}
                Err(error) => eprintln!("fixture destage deferred: {error}"),
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let server = UnixSocketDaemonServer::new(socket, fixture.handler());
    server.serve_forever().map_err(|error| error.to_string())
}
