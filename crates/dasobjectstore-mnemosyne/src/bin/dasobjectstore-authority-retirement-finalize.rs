use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use dasobjectstore_mnemosyne::authority_retirement_projection::{
    finalize_authority_retirement_projection_v1, LegacySurfaceProbeV1, ProjectionPathsV1,
};

struct ProductionProbeV1;

impl LegacySurfaceProbeV1 for ProductionProbeV1 {
    fn service_disabled_inactive(&self) -> bool {
        command_stdout(
            "systemctl",
            &["is-enabled", "dasobjectstore-server.service"],
        )
        .is_some_and(|value| service_activation_blocked(&value))
            && command_stdout("systemctl", &["is-active", "dasobjectstore-server.service"])
                == Some("inactive".into())
            && command_stdout(
                "systemctl",
                &[
                    "show",
                    "dasobjectstore-server.service",
                    "-p",
                    "MainPID",
                    "-p",
                    "Job",
                    "--value",
                ],
            )
            .is_some_and(|value| value.lines().all(|line| line.is_empty() || line == "0"))
    }

    fn legacy_listeners_absent(&self) -> bool {
        ["/proc/net/tcp", "/proc/net/tcp6"].iter().all(|path| {
            fs::read_to_string(path).is_ok_and(|contents| {
                !contents.lines().skip(1).any(|line| {
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|address| address.rsplit(':').next())
                        .is_some_and(|port| matches!(port, "2100" | "2101"))
                })
            })
        })
    }

    fn retired_process_absent(&self) -> bool {
        fs::read_dir("/proc").is_ok_and(|entries| {
            entries.filter_map(Result::ok).all(|entry| {
                let name = entry.file_name();
                if !name.as_encoded_bytes().iter().all(u8::is_ascii_digit) {
                    return true;
                }
                fs::read_link(entry.path().join("exe")).map_or(true, |path| {
                    path != Path::new("/usr/bin/dasobjectstore-server")
                })
            })
        })
    }

    fn helpers_and_pam_absent(&self) -> bool {
        [
            "/usr/bin/dasobjectstore-local-auth-helper",
            "/usr/bin/dasobjectstore-auth-migrate",
            "/etc/pam.d/dasobjectstore",
        ]
        .iter()
        .all(|path| fs::symlink_metadata(path).is_err())
            && command_stdout(
                "systemctl",
                &["list-unit-files", "--no-legend", "--no-pager"],
            )
            .is_some_and(|value| {
                !value.contains("dasobjectstore-local-auth-helper")
                    && !value.contains("dasobjectstore-auth-migrate")
            })
            && fixed_files_exclude_retired_helpers()
    }
}

fn service_activation_blocked(value: &str) -> bool {
    matches!(value, "disabled" | "masked" | "masked-runtime")
}

fn fixed_files_exclude_retired_helpers() -> bool {
    [
        "/usr/lib/systemd/system",
        "/etc/systemd/system",
        "/var/lib/dpkg/info",
    ]
    .iter()
    .all(|directory| {
        fs::read_dir(directory).is_ok_and(|entries| {
            entries.filter_map(Result::ok).all(|entry| {
                let name = entry.file_name();
                if directory.ends_with("/info")
                    && !name.to_string_lossy().starts_with("dasobjectstore.")
                {
                    return true;
                }
                fs::metadata(entry.path()).map_or(true, |metadata| {
                    !metadata.is_file()
                        || fs::read(entry.path()).is_ok_and(|bytes| {
                            !bytes
                                .windows(32)
                                .any(|value| value == b"dasobjectstore-local-auth-helper")
                                && !bytes
                                    .windows(28)
                                    .any(|value| value == b"dasobjectstore-auth-migrate")
                        })
                })
            })
        })
    })
}

fn command_stdout(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}

fn main() {
    let ok = std::env::args_os().len() == 1
        && SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|now| i64::try_from(now.as_secs()).ok())
            .and_then(|now| ProjectionPathsV1::production().map(|paths| (now, paths)))
            .is_some_and(|(now, paths)| {
                finalize_authority_retirement_projection_v1(&paths, &ProductionProbeV1, now).is_ok()
            });
    if !ok {
        eprintln!("DAS authority-retirement projection unavailable");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::service_activation_blocked;

    #[test]
    fn legacy_service_accepts_disabled_and_masked_states_only() {
        for state in ["disabled", "masked", "masked-runtime"] {
            assert!(service_activation_blocked(state), "rejected {state}");
        }
        for state in [
            "enabled",
            "enabled-runtime",
            "linked",
            "linked-runtime",
            "static",
            "indirect",
            "generated",
            "transient",
            "alias",
            "bad",
            "",
        ] {
            assert!(!service_activation_blocked(state), "accepted {state}");
        }
    }
}
