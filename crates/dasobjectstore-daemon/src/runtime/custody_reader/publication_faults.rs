//! Test-binary-only interruption seam; absent from production builds.
use super::{Path, ReaderError};

pub(super) fn point(root: &Path, name: &str, phase: &str) -> Result<(), ReaderError> {
    if std::env::var_os("DAS_TEST_PUBLICATION_ROOT").as_deref() != Some(root.as_os_str()) {
        return Ok(());
    }
    let role = if name.starts_with("seal-") {
        "seal"
    } else if name.starts_with("binding-") {
        "binding"
    } else if name.starts_with("state-") {
        "state"
    } else {
        name
    };
    if std::env::var("DAS_TEST_PUBLICATION_POINT").ok().as_deref()
        != Some(format!("{role}:{phase}").as_str())
    {
        return Ok(());
    }
    match std::env::var("DAS_TEST_PUBLICATION_ACTION").as_deref() {
        Ok("crash") => std::process::exit(86), // no Rust destructors/recovery
        Ok("error") => Err(ReaderError::Boundary),
        Ok("pause") => {
            let parent = root.parent().expect("owned fixture root");
            std::fs::write(parent.join("paused"), b"").expect("fixture rendezvous");
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !parent.join("resume").exists() {
                assert!(
                    std::time::Instant::now() < until,
                    "fixture rendezvous timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(())
        }
        _ => panic!("invalid isolated publication test action"),
    }
}
