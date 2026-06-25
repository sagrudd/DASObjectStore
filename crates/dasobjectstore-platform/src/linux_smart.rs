use crate::health::DiskHealthReport;
use crate::probe::{CommandRunner, ProbeError};
use dasobjectstore_core::health::HealthSignals;
use serde::Deserialize;

pub const SMARTCTL_COMMAND: &str = "smartctl";
pub const SMARTCTL_BASE_ARGS: [&str; 3] = ["--json", "--health", "--attributes"];

pub fn smartctl_health_args(device_path: &str) -> Vec<String> {
    SMARTCTL_BASE_ARGS
        .iter()
        .map(|arg| (*arg).to_string())
        .chain([device_path.to_string()])
        .collect()
}

pub fn read_smartctl_health<R>(
    runner: &R,
    device_path: &str,
) -> Result<DiskHealthReport, ProbeError>
where
    R: CommandRunner,
{
    let args = smartctl_health_args(device_path);
    let arg_refs: Vec<_> = args.iter().map(String::as_str).collect();
    let output = runner.run(SMARTCTL_COMMAND, &arg_refs)?;

    parse_smartctl_json(&output)
}

pub fn parse_smartctl_json(input: &str) -> Result<DiskHealthReport, ProbeError> {
    let output: SmartctlOutput =
        serde_json::from_str(input).map_err(|err| ProbeError::ParseFailed {
            source: SMARTCTL_COMMAND.to_string(),
            message: err.to_string(),
        })?;
    let smart_warnings = smart_warning_count(&output);
    let temperature_celsius = output
        .temperature
        .as_ref()
        .and_then(|temperature| temperature.current);

    Ok(DiskHealthReport {
        device_path: output.device.and_then(|device| device.name),
        model_hint: output.model_name,
        serial_hint: output.serial_number,
        smart_passed: output.smart_status.as_ref().map(|status| status.passed),
        signals: HealthSignals {
            smart_warnings,
            temperature_celsius,
            ..HealthSignals::default()
        },
        warnings: Vec::new(),
    })
}

fn smart_warning_count(output: &SmartctlOutput) -> u16 {
    let mut warnings = u16::from(
        output
            .smart_status
            .as_ref()
            .is_some_and(|status| !status.passed),
    );

    if let Some(attributes) = output.ata_smart_attributes.as_ref() {
        warnings += attributes
            .table
            .iter()
            .filter(|attribute| is_warning_attribute(attribute))
            .filter(|attribute| attribute.raw.value.unwrap_or(0) > 0)
            .count() as u16;
    }

    warnings
}

fn is_warning_attribute(attribute: &AtaSmartAttribute) -> bool {
    matches!(attribute.id, Some(5 | 187 | 188 | 197 | 198))
        || matches!(
            attribute.name.as_deref(),
            Some(
                "Reallocated_Sector_Ct"
                    | "Reported_Uncorrect"
                    | "Command_Timeout"
                    | "Current_Pending_Sector"
                    | "Offline_Uncorrectable"
            )
        )
}

#[derive(Debug, Deserialize)]
struct SmartctlOutput {
    device: Option<SmartctlDevice>,
    model_name: Option<String>,
    serial_number: Option<String>,
    smart_status: Option<SmartStatus>,
    temperature: Option<SmartTemperature>,
    ata_smart_attributes: Option<AtaSmartAttributes>,
}

#[derive(Debug, Deserialize)]
struct SmartctlDevice {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SmartStatus {
    passed: bool,
}

#[derive(Debug, Deserialize)]
struct SmartTemperature {
    current: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct AtaSmartAttributes {
    table: Vec<AtaSmartAttribute>,
}

#[derive(Debug, Deserialize)]
struct AtaSmartAttribute {
    id: Option<u16>,
    name: Option<String>,
    raw: AtaSmartRaw,
}

#[derive(Debug, Deserialize)]
struct AtaSmartRaw {
    value: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::{
        parse_smartctl_json, read_smartctl_health, smartctl_health_args, SMARTCTL_BASE_ARGS,
        SMARTCTL_COMMAND,
    };
    use crate::probe::{CommandRunner, ProbeError};

    const SMARTCTL_FIXTURE: &str = include_str!("../fixtures/linux/smartctl-sata-warning.json");

    #[test]
    fn defines_stable_smartctl_json_command() {
        assert_eq!(SMARTCTL_COMMAND, "smartctl");
        assert_eq!(SMARTCTL_BASE_ARGS, ["--json", "--health", "--attributes"]);
        assert_eq!(
            smartctl_health_args("/dev/sda"),
            ["--json", "--health", "--attributes", "/dev/sda"]
        );
    }

    #[test]
    fn parses_smartctl_json_into_health_signals() {
        let report = parse_smartctl_json(SMARTCTL_FIXTURE).expect("smartctl fixture parses");

        assert_eq!(report.device_path.as_deref(), Some("/dev/sda"));
        assert_eq!(report.serial_hint.as_deref(), Some("WD-OLD-001"));
        assert_eq!(report.smart_passed, Some(true));
        assert_eq!(report.signals.smart_warnings, 2);
        assert_eq!(report.signals.temperature_celsius, Some(57));
    }

    #[test]
    fn rejects_invalid_smartctl_json() {
        let err = parse_smartctl_json("not-json").expect_err("invalid json fails");

        assert!(err.to_string().contains("failed to parse smartctl"));
    }

    #[test]
    fn smartctl_reader_runs_command_and_parses_output() {
        let runner = FixtureRunner {
            output: Ok(SMARTCTL_FIXTURE.to_string()),
        };

        let report = read_smartctl_health(&runner, "/dev/sda").expect("smart health succeeds");

        assert_eq!(report.device_path.as_deref(), Some("/dev/sda"));
        assert_eq!(report.signals.smart_warnings, 2);
    }

    #[test]
    fn smartctl_reader_propagates_command_failure() {
        let runner = FixtureRunner {
            output: Err(ProbeError::CommandFailed {
                command: SMARTCTL_COMMAND.to_string(),
                message: "missing command".to_string(),
            }),
        };

        let err = read_smartctl_health(&runner, "/dev/sda").expect_err("smart health fails");

        assert_eq!(
            err,
            ProbeError::CommandFailed {
                command: SMARTCTL_COMMAND.to_string(),
                message: "missing command".to_string()
            }
        );
    }

    struct FixtureRunner {
        output: Result<String, ProbeError>,
    }

    impl CommandRunner for FixtureRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            assert_eq!(command, SMARTCTL_COMMAND);
            assert_eq!(args, ["--json", "--health", "--attributes", "/dev/sda"]);

            self.output.clone()
        }
    }
}
