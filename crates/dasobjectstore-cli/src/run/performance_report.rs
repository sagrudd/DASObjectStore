#[path = "performance_report_impl/mod.rs"]
mod implementation;
pub(super) use implementation::*;

use super::*;

pub(super) fn run_performance_report(
    args: &PerformanceReportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let artifact = read_performance_json_artifact(args.json_artifact())?;
    let report_path = args
        .report()
        .map(Path::to_path_buf)
        .or_else(|| artifact_pdf_path(&artifact))
        .ok_or_else(|| {
            CliError::CommandFailed(
                "performance-report requires --report when the JSON artifact does not record a PDF path"
                    .to_string(),
            )
        })?;
    validate_pdf_report_path(&report_path)?;
    let markdown_path = args.tmp_dir().join(format!(
        "{}-rebuilt.md",
        report_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("dasobjectstore-performance-report")
    ));
    if let Some(parent) = markdown_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_performance_chart_svgs_from_json(&artifact, &report_path)?;
    let markdown = render_performance_report_from_json_artifact(&artifact, &report_path);
    fs::write(&markdown_path, markdown)?;
    write_formal_performance_pdf_report_from_artifact(&markdown_path, &report_path, &artifact)?;
    if !args.keep_markdown() {
        let _ = fs::remove_file(&markdown_path);
    }
    writeln!(writer, "Report: {}", report_path.display())?;
    writeln!(writer, "JSON: {}", args.json_artifact().display())?;
    Ok(())
}
