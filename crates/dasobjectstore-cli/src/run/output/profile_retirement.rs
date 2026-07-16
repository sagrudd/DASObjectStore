use dasobjectstore_daemon::ProfileRetirementReport;
use std::io::{self, Write};

pub(super) fn write_profile_retirement_report(
    report: &ProfileRetirementReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    let heading = if report.dry_run {
        "Profile retirement dry run"
    } else if report.already_retired {
        "Profile already retired"
    } else {
        "Profile retired"
    };
    writeln!(writer, "{heading}: {}", report.store_id)?;
    writeln!(
        writer,
        "Shared objects withdrawn: {}",
        report.shared_objects_removed
    )?;
    writeln!(
        writer,
        "Shared transactions withdrawn: {}",
        report.shared_transactions_removed
    )?;
    writeln!(
        writer,
        "Private catalogue retained: {}",
        report.private_catalogue_retained
    )?;
    writeln!(writer, "Payloads retained: {}", report.payloads_retained)?;
    writeln!(
        writer,
        "Quota ledger retained: {}",
        report.quota_ledger_retained
    )?;
    writeln!(
        writer,
        "Registry definition retained: {}",
        report.registry_definition_retained
    )
}
