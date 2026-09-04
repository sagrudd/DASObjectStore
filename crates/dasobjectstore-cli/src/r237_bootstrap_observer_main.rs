//! Fixed, local-only r237 bootstrap observer entry point.
//!
//! This binary accepts no arguments, configuration, target, or input file.
//! It only emits a redacted denied report; it has no provision or service path.

use dasobjectstore_core::{
    assess_r237_bootstrap_local_observation, canonical_r237_bootstrap_observer_report,
};
use dasobjectstore_platform::{LinuxR237BootstrapObserver, R237BootstrapReadOnlyObserver};
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        let _ = writeln!(
            io::stderr(),
            "this fixed r237 observer accepts no arguments"
        );
        return ExitCode::FAILURE;
    }

    let observation = LinuxR237BootstrapObserver::system().observe();
    let report = assess_r237_bootstrap_local_observation(&observation);
    let Some(encoded) = canonical_r237_bootstrap_observer_report(&report) else {
        let _ = writeln!(io::stderr(), "could not encode r237 observer report");
        return ExitCode::FAILURE;
    };
    if io::stdout().write_all(&encoded).is_err() || io::stdout().write_all(b"\n").is_err() {
        return ExitCode::FAILURE;
    }

    // A zero exit status could be used as a shell-level provision signal. The
    // observer release deliberately cannot authorise any subsequent action.
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    #[test]
    fn standalone_binary_has_no_argument_or_apply_contract() {
        let source = include_str!("r237_bootstrap_observer_main.rs");
        assert!(source.contains("args_os().len() != 1"));
        assert!(!source.contains("--target"));
        assert!(!source.contains("--apply"));
        assert!(!source.contains("systemctl"));
    }
}
