mod cli;

use clap::Parser;
use cli::TuiCli;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = TuiCli::parse();
    let mut stdout = io::stdout();

    match write_launch_preview(&cli, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn write_launch_preview(cli: &TuiCli, output: &mut impl Write) -> io::Result<()> {
    let plan = cli.import_plan();
    let launch = cli
        .launch_confirmation()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    let review = launch.review(&plan);
    let planning = &review.planning;
    let resources = cli.resource_policy().display_data();

    writeln!(output, "DASObjectStore TUI planning scaffold")?;
    writeln!(output, "Target: {}", planning.target_label)?;
    writeln!(output, "Sources: {}", planning.source_count)?;
    writeln!(output, "Files: {}", planning.file_count)?;
    writeln!(output, "Data volume: {}", planning.total_size_label)?;
    writeln!(output, "Description: {}", review.metadata.description_label)?;
    writeln!(
        output,
        "Metadata: {}",
        if review.metadata.field_labels.is_empty() {
            "none".to_string()
        } else {
            review.metadata.field_labels.join(", ")
        }
    )?;
    writeln!(output, "Workers: {}", resources.worker_counts_label)?;
    writeln!(output, "Memory budget: {}", resources.memory_budget_label)?;
    writeln!(output, "SSD reserve: {}", resources.ssd_reserve_label)?;
    writeln!(
        output,
        "HDD queue depth: {}",
        resources.hdd_queue_depth_label
    )?;
    writeln!(
        output,
        "Verification parallelism: {}",
        resources.verification_parallelism_label
    )?;
    writeln!(output, "Launch status: {}", review.status_label())?;
    writeln!(
        output,
        "Required confirmation: {}",
        review.required_confirmation_phrase
    )?;
    for blocker in review.blocker_labels() {
        writeln!(output, "Launch blocker: {blocker}")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{write_launch_preview, TuiCli};
    use clap::Parser;

    #[test]
    fn launch_preview_shows_metadata_and_ready_status() {
        let cli = TuiCli::parse_from([
            "dasobjectstore-tui",
            "--object-store",
            "research",
            "--source",
            "/data/a",
            "--file-count",
            "1",
            "--total-bytes",
            "1048576",
            "--description",
            "Zymo fecal dataset",
            "--metadata",
            "ticket=LAB-42",
            "--confirm-launch",
            "confirm import launch",
        ]);
        let mut output = Vec::new();

        write_launch_preview(&cli, &mut output).expect("preview writes");
        let output = String::from_utf8(output).expect("output is utf8");

        assert!(output.contains("Description: Zymo fecal dataset"));
        assert!(output.contains("Metadata: ticket=LAB-42"));
        assert!(output.contains("Launch status: ready"));
        assert!(!output.contains("Launch blocker:"));
    }

    #[test]
    fn launch_preview_shows_blockers_without_description_or_confirmation() {
        let cli = TuiCli::parse_from([
            "dasobjectstore-tui",
            "--object-store",
            "research",
            "--source",
            "/data/a",
        ]);
        let mut output = Vec::new();

        write_launch_preview(&cli, &mut output).expect("preview writes");
        let output = String::from_utf8(output).expect("output is utf8");

        assert!(output.contains("Description: not provided"));
        assert!(output.contains("Metadata: none"));
        assert!(output.contains("Launch status: blocked"));
        assert!(output.contains("Launch blocker: import description is required"));
        assert!(output
            .contains("Launch blocker: launch confirmation is required: `confirm import launch`"));
    }
}
