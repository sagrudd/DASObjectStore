//! Health command dispatch and output selection.

use super::*;

pub(super) fn run_health(args: &HealthArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let selected_modes = [
        args.summary(),
        args.verbose(),
        args.connections(),
        args.json(),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected_modes > 1 {
        return Err(CliError::UnsupportedHealthFormat);
    }

    if args.connections() {
        let report = super::read_current_platform_connection_status()?;
        write_host_connection_status(&report, writer)?;
    } else if args.json() {
        let report = super::read_current_platform_health()?;
        write_health_json(&report, writer)?;
    } else if args.verbose() {
        let report = super::read_current_platform_health()?;
        write_health_verbose(&report, writer)?;
    } else {
        let report = super::read_current_platform_health()?;
        write_health_summary(&report, writer)?;
    }

    Ok(())
}
