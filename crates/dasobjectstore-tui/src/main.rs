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
    let planning = plan.summary();
    let resources = cli.resource_policy().display_data();

    writeln!(output, "DASObjectStore TUI planning scaffold")?;
    writeln!(output, "Target: {}", planning.target_label)?;
    writeln!(output, "Sources: {}", planning.source_count)?;
    writeln!(output, "Files: {}", planning.file_count)?;
    writeln!(output, "Data volume: {}", planning.total_size_label)?;
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

    Ok(())
}
