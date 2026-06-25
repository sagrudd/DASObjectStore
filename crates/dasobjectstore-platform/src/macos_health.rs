use crate::health::DiskHealthReport;
use crate::probe::{CommandRunner, ProbeError};
use dasobjectstore_core::health::HealthSignals;
use serde::Deserialize;

pub const DISKUTIL_HEALTH_COMMAND: &str = "diskutil";
pub const DISKUTIL_INFO_BASE_ARGS: [&str; 2] = ["info", "-plist"];

pub fn diskutil_info_args(device_path: &str) -> Vec<String> {
    DISKUTIL_INFO_BASE_ARGS
        .iter()
        .map(|arg| (*arg).to_string())
        .chain([device_path.to_string()])
        .collect()
}

pub fn read_diskutil_health<R>(
    runner: &R,
    device_path: &str,
) -> Result<DiskHealthReport, ProbeError>
where
    R: CommandRunner,
{
    let args = diskutil_info_args(device_path);
    let arg_refs: Vec<_> = args.iter().map(String::as_str).collect();
    let output = runner.run(DISKUTIL_HEALTH_COMMAND, &arg_refs)?;

    parse_diskutil_info_plist(output.as_bytes())
}

pub fn parse_diskutil_info_plist(input: &[u8]) -> Result<DiskHealthReport, ProbeError> {
    let output: DiskutilInfo = plist::from_bytes(input).map_err(|err| ProbeError::ParseFailed {
        source: DISKUTIL_HEALTH_COMMAND.to_string(),
        message: err.to_string(),
    })?;
    let smart_passed = smart_passed(output.smart_status.as_deref());
    let smart_warnings = u16::from(smart_passed == Some(false));
    let warnings = smart_warning_messages(output.smart_status.as_deref());

    Ok(DiskHealthReport {
        device_path: output.device_node.or_else(|| {
            output
                .device_identifier
                .map(|identifier| format!("/dev/{identifier}"))
        }),
        model_hint: output.media_name,
        serial_hint: output.serial_number,
        smart_passed,
        signals: HealthSignals {
            smart_warnings,
            ..HealthSignals::default()
        },
        warnings,
    })
}

fn smart_passed(smart_status: Option<&str>) -> Option<bool> {
    match smart_status {
        Some("Verified") => Some(true),
        Some("Failing") => Some(false),
        Some("Not Supported") | None => None,
        Some(_) => None,
    }
}

fn smart_warning_messages(smart_status: Option<&str>) -> Vec<String> {
    match smart_status {
        Some("Not Supported") => vec!["macOS reports SMART as not supported".to_string()],
        Some(status) if !matches!(status, "Verified" | "Failing") => {
            vec![format!("macOS returned unknown SMART status `{status}`")]
        }
        _ => Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
struct DiskutilInfo {
    #[serde(rename = "DeviceIdentifier")]
    device_identifier: Option<String>,
    #[serde(rename = "DeviceNode")]
    device_node: Option<String>,
    #[serde(rename = "MediaName")]
    media_name: Option<String>,
    #[serde(rename = "SMARTStatus")]
    smart_status: Option<String>,
    #[serde(rename = "SerialNumber")]
    serial_number: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        diskutil_info_args, parse_diskutil_info_plist, read_diskutil_health,
        DISKUTIL_HEALTH_COMMAND, DISKUTIL_INFO_BASE_ARGS,
    };
    use crate::probe::{CommandRunner, ProbeError};

    const FAILING_FIXTURE: &[u8] =
        include_bytes!("../fixtures/macos/diskutil-info-smart-failing.plist");
    const UNSUPPORTED_FIXTURE: &[u8] =
        include_bytes!("../fixtures/macos/diskutil-info-smart-unsupported.plist");

    #[test]
    fn defines_stable_diskutil_info_command() {
        assert_eq!(DISKUTIL_HEALTH_COMMAND, "diskutil");
        assert_eq!(DISKUTIL_INFO_BASE_ARGS, ["info", "-plist"]);
        assert_eq!(
            diskutil_info_args("/dev/disk4"),
            ["info", "-plist", "/dev/disk4"]
        );
    }

    #[test]
    fn parses_failing_macos_smart_status_into_health_signals() {
        let report = parse_diskutil_info_plist(FAILING_FIXTURE).expect("fixture parses");

        assert_eq!(report.device_path.as_deref(), Some("/dev/disk4"));
        assert_eq!(report.model_hint.as_deref(), Some("Old SATA HDD"));
        assert_eq!(report.serial_hint.as_deref(), Some("WD-OLD-001"));
        assert_eq!(report.smart_passed, Some(false));
        assert_eq!(report.signals.smart_warnings, 1);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn treats_unsupported_macos_smart_as_unknown_not_unhealthy() {
        let report = parse_diskutil_info_plist(UNSUPPORTED_FIXTURE).expect("fixture parses");

        assert_eq!(report.device_path.as_deref(), Some("/dev/disk5"));
        assert_eq!(report.smart_passed, None);
        assert_eq!(report.signals.smart_warnings, 0);
        assert_eq!(
            report.warnings,
            ["macOS reports SMART as not supported".to_string()]
        );
    }

    #[test]
    fn rejects_invalid_diskutil_info_plist() {
        let err = parse_diskutil_info_plist(b"not-plist").expect_err("invalid plist fails");

        assert!(err.to_string().contains("failed to parse diskutil"));
    }

    #[test]
    fn diskutil_health_reader_runs_command_and_parses_output() {
        let runner = FixtureRunner {
            output: Ok(String::from_utf8(FAILING_FIXTURE.to_vec()).expect("utf8 fixture")),
        };

        let report = read_diskutil_health(&runner, "/dev/disk4").expect("health succeeds");

        assert_eq!(report.device_path.as_deref(), Some("/dev/disk4"));
        assert_eq!(report.signals.smart_warnings, 1);
    }

    #[test]
    fn diskutil_health_reader_propagates_command_failure() {
        let runner = FixtureRunner {
            output: Err(ProbeError::CommandFailed {
                command: DISKUTIL_HEALTH_COMMAND.to_string(),
                message: "missing command".to_string(),
            }),
        };

        let err = read_diskutil_health(&runner, "/dev/disk4").expect_err("health fails");

        assert_eq!(
            err,
            ProbeError::CommandFailed {
                command: DISKUTIL_HEALTH_COMMAND.to_string(),
                message: "missing command".to_string()
            }
        );
    }

    struct FixtureRunner {
        output: Result<String, ProbeError>,
    }

    impl CommandRunner for FixtureRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            assert_eq!(command, DISKUTIL_HEALTH_COMMAND);
            assert_eq!(args, ["info", "-plist", "/dev/disk4"]);

            self.output.clone()
        }
    }
}
