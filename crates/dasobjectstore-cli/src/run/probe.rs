//! Read-only platform probe command handler.

use super::*;

pub(super) fn run_probe(args: &ProbeArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if args.json() && args.pretty() {
        return Err(CliError::UnsupportedProbeFormat);
    }

    let mut report = probe_current_platform()?;
    report.enclosures = group_enclosures(&report.disks);

    if args.json() {
        serde_json::to_writer(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_pretty_report(&report, writer)?;
    }

    Ok(())
}
