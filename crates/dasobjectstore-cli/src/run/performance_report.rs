use super::{
    compact_hash, compact_identifier, compact_path, compact_run_id, format_bytes,
    format_bytes_compact, format_concurrency_list, friendly_file_order, humanize_report_token,
    performance_artifact_signature, report_renderer_command, ActiveHddWrite, ActiveHddWriteMap,
    CliError, DiskId, PerformanceBenchmarkResults, PerformanceCopyProgressPhase,
    PerformanceDiskResult, PerformanceFileOrder, PerformanceFileResult, PerformanceIoSample,
    PerformanceMeasurement, PerformanceRecommendation, PerformanceReport, PerformanceScenarioKind,
    PerformanceScenarioResult,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

pub(super) fn render_performance_report_from_json_artifact(
    artifact: &Value,
    report_path: &Path,
) -> String {
    let run_id =
        json_string(artifact, &["run", "run_id"]).unwrap_or_else(|| "not recorded".to_string());
    let generated_at = json_string(artifact, &["run", "generated_at_utc"])
        .unwrap_or_else(|| "not recorded".to_string());
    let command = json_array_strings(artifact, &["run", "command"]).join(" ");
    let mut output = String::new();
    output.push_str("# DASObjectStore Performance Test Report\n\n");
    output.push_str("## Executive Summary\n\n");
    output.push_str(&format!("- Run ID: `{}`\n", compact_run_id(&run_id)));
    output.push_str(&format!("- Generated at: `{generated_at}`\n"));
    output.push_str(&format!(
        "- Report artifact: `{}`\n",
        compact_path(&report_path.display().to_string())
    ));
    output.push_str(&format!(
        "- JSON artifact: `{}`\n",
        json_string(artifact, &["run", "artifacts", "json_path"])
            .map(|path| compact_path(&path))
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    if !command.is_empty() {
        output.push_str("- Reproduction command: recorded in the JSON artifact.\n");
    }
    output.push('\n');

    output.push_str("## Recommendation\n\n");
    output.push_str("| Field | Value |\n| --- | --- |\n");
    output.push_str(&format!(
        "| Strategy | `{}` |\n",
        json_string(artifact, &["recommendation", "strategy"])
            .map(|value| humanize_report_token(&value))
            .unwrap_or_else(|| "Not recorded".to_string())
    ));
    output.push_str(&format!(
        "| File order | `{}` |\n",
        json_string(artifact, &["recommendation", "file_order"])
            .map(|value| friendly_file_order(&value))
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| HDD concurrency | {} |\n",
        json_u64(artifact, &["recommendation", "hdd_concurrency"])
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Redundancy | {} |\n",
        json_u64(artifact, &["recommendation", "redundancy"])
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Estimated aggregate | {}/s |\n\n",
        json_u64(
            artifact,
            &["recommendation", "estimated_aggregate_bytes_per_second"]
        )
        .map(|value| format_bytes_compact(value as f64))
        .unwrap_or_else(|| "not recorded".to_string())
    ));
    if let Some(rows) = json_array(artifact, &["recommendation", "rationale"]) {
        output.push_str("## Recommendation Rationale\n\n");
        for row in rows.iter().filter_map(Value::as_str) {
            output.push_str(&format!("- {row}\n"));
        }
        output.push('\n');
    }

    output.push_str("## Workload and Hardware\n\n");
    output.push_str("| Field | Value |\n| --- | --- |\n");
    for (label, path) in [
        ("Workload kind", &["run", "parameters", "workload_kind"][..]),
        ("Source path", &["run", "parameters", "source_path"][..]),
        (
            "File selection",
            &["run", "parameters", "file_selection"][..],
        ),
        ("SSD root", &["hardware", "roots", "ssd_root"][..]),
        ("HDD root", &["hardware", "roots", "hdd_root"][..]),
    ] {
        output.push_str(&format!(
            "| {label} | `{}` |\n",
            json_string(artifact, path)
                .map(|value| {
                    if label.ends_with("root") || label == "Source path" {
                        compact_path(&value)
                    } else {
                        humanize_report_token(&value)
                    }
                })
                .unwrap_or_else(|| "not recorded".to_string())
        ));
    }
    output.push_str(&format!(
        "| File orders | `{}` |\n",
        json_array_strings(artifact, &["run", "parameters", "file_orders"]).join("`, `")
    ));
    output.push_str(&format!(
        "| Planned files | {} |\n",
        json_u64(artifact, &["run", "parameters", "file_count"])
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Logical source bytes | {} |\n\n",
        json_u64(artifact, &["run", "parameters", "total_source_bytes"])
            .map(|value| format_bytes(value as f64))
            .unwrap_or_else(|| "not recorded".to_string())
    ));

    output.push_str("## Scenario Summary\n\n");
    output.push_str("| Scenario | File order | HDD concurrency | Redundancy | Logical source | Physical HDD writes | Operations | Aggregate landing | Overlapped SSD staging |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
    for row in performance_scenario_rows_from_json(artifact) {
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} | {}/s | {} |\n",
            row.scenario,
            row.file_order,
            row.hdd_concurrency,
            row.redundancy,
            format_bytes_compact(row.logical_source_bytes as f64),
            format_bytes_compact(row.physical_hdd_write_bytes as f64),
            row.operations,
            format_bytes_compact(row.aggregate_bytes_per_second as f64),
            yes_no(row.overlapped)
        ));
    }

    output.push_str("\n## Per-Disk HDD Write Rates\n\n");
    output.push_str(
        "| Scenario | File order | HDD concurrency | Disk | Assigned | Write rate | Operations |\n",
    );
    output.push_str("| --- | --- | ---: | --- | ---: | ---: | ---: |\n");
    for row in json_array(artifact, &["plot_data", "per_disk_hdd_write_rate"])
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {:.1} GiB | {:.1} MiB/s | {} |\n",
            json_string(row, &["scenario_label"]).unwrap_or_else(|| json_string(
                row,
                &["scenario"]
            )
            .unwrap_or_else(|| "unknown".to_string())),
            json_string(row, &["file_order"]).unwrap_or_else(|| "not recorded".to_string()),
            json_u64(row, &["hdd_concurrency"]).unwrap_or_default(),
            json_string(row, &["disk_id"]).unwrap_or_else(|| "unknown".to_string()),
            json_f64(row, &["assigned_gib"]).unwrap_or_default(),
            json_f64(row, &["write_mib_per_second"]).unwrap_or_default(),
            json_u64(row, &["write_operations"]).unwrap_or_default(),
        ));
    }

    output.push_str("\n## Figures\n\n");
    for artifact in performance_chart_artifacts_from_pdf_path(report_path) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }
    for artifact in performance_io_chart_artifacts_from_json(artifact, report_path) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }

    output.push_str("## Reproducibility\n\n");
    output.push_str("| Field | Value |\n| --- | --- |\n");
    output.push_str(&format!(
        "| JSON artifact | `{}` |\n",
        json_string(artifact, &["run", "artifacts", "json_path"])
            .map(|path| compact_path(&path))
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Artifact SHA-256 | `{}` |\n",
        compact_hash(&performance_artifact_signature(artifact))
    ));
    output.push_str("| Reproduction command | Recorded in the JSON artifact |\n");
    output.push_str("| QR payload | Encoded in the report QR code |\n\n");
    output.push_str(
        "The complete machine-readable benchmark artifact is retained as JSON and is deliberately not embedded in the formal PDF body. Use the JSON artifact for exact reproduction, audit, and daemon ingress-policy import.\n",
    );
    output
}

#[derive(Debug)]
struct JsonScenarioRow {
    scenario: String,
    file_order: String,
    hdd_concurrency: u64,
    redundancy: u64,
    logical_source_bytes: u64,
    physical_hdd_write_bytes: u64,
    operations: u64,
    aggregate_bytes_per_second: u64,
    overlapped: bool,
}

fn performance_scenario_rows_from_json(artifact: &Value) -> Vec<JsonScenarioRow> {
    let mut rows = Vec::new();
    for order in json_array(artifact, &["scenarios", "ssd_only", "orders"])
        .into_iter()
        .flatten()
    {
        rows.push(JsonScenarioRow {
            scenario: "SSD only".to_string(),
            file_order: json_string(order, &["file_order"])
                .unwrap_or_else(|| "not recorded".to_string()),
            hdd_concurrency: 0,
            redundancy: 0,
            logical_source_bytes: json_u64(artifact, &["scenarios", "ssd_only", "total_bytes"])
                .unwrap_or_default(),
            physical_hdd_write_bytes: 0,
            operations: 0,
            aggregate_bytes_per_second: json_u64(order, &["median_ssd_write_bytes_per_second"])
                .unwrap_or_default(),
            overlapped: false,
        });
    }
    for path in [
        &["scenarios", "ssd_stage_then_drain_pipeline", "concurrency"][..],
        &["scenarios", "ssd_hdd_pipeline", "concurrency"][..],
        &["scenarios", "direct_hdd_pipeline", "concurrency"][..],
    ] {
        for row in json_array(artifact, path).into_iter().flatten() {
            rows.push(JsonScenarioRow {
                scenario: json_string(row, &["scenario"]).unwrap_or_else(|| "unknown".to_string()),
                file_order: json_string(row, &["file_order"])
                    .unwrap_or_else(|| "not recorded".to_string()),
                hdd_concurrency: json_u64(row, &["concurrency"]).unwrap_or_default(),
                redundancy: json_u64(row, &["redundancy"]).unwrap_or_default(),
                logical_source_bytes: json_u64(row, &["logical_source_bytes"]).unwrap_or_default(),
                physical_hdd_write_bytes: json_u64(row, &["physical_hdd_write_bytes"])
                    .unwrap_or_default(),
                operations: json_u64(row, &["hdd_write_operations"]).unwrap_or_default(),
                aggregate_bytes_per_second: json_u64(row, &["aggregate_write_bytes_per_second"])
                    .unwrap_or_default(),
                overlapped: json_bool(row, &["hdd_drain_started_before_all_ssd_staged"])
                    .unwrap_or(false),
            });
        }
    }
    rows
}

pub(super) fn write_performance_chart_svgs_from_json(
    artifact: &Value,
    pdf_path: &Path,
) -> Result<(), CliError> {
    let json_path = json_string(artifact, &["run", "artifacts", "json_path"])
        .map(PathBuf::from)
        .ok_or_else(|| {
            CliError::CommandFailed(
                "performance chart rendering requires run.artifacts.json_path".to_string(),
            )
        })?;
    let output_dir = pdf_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_dir)?;
    let output_stem = pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let status = ProcessCommand::new(report_renderer_command())
        .arg("render-performance-plots")
        .arg("--provider")
        .arg("container")
        .arg("--input-json")
        .arg(&json_path)
        .arg("--output-dir")
        .arg(output_dir)
        .arg("--output-stem")
        .arg(output_stem)
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(CliError::CommandFailed(format!(
            "formal performance plot rendering failed with status {status}; install/repair the DASObjectStore packaged report renderer, Docker/container runtime, and the Grammateus report provider"
        ))),
        Err(error) => Err(CliError::CommandFailed(format!(
            "formal performance plot rendering requires the DASObjectStore packaged report renderer or an external gnostikon-workflow-control command with Grammateus support plus a Docker/container runtime: {error}"
        ))),
    }
}

pub(super) fn json_plot_label(row: &Value) -> String {
    format!(
        "{} {} c{} r{}",
        json_string(row, &["scenario"]).unwrap_or_else(|| "unknown".to_string()),
        json_string(row, &["file_order"]).unwrap_or_else(|| "order".to_string()),
        json_u64(row, &["hdd_concurrency"]).unwrap_or_default(),
        json_u64(row, &["redundancy"]).unwrap_or_default()
    )
}

pub(super) fn performance_chart_artifacts_from_pdf_path(
    pdf_path: &Path,
) -> Vec<PerformanceChartArtifact> {
    let base = pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = pdf_path.parent().unwrap_or_else(|| Path::new("."));
    [
        ("Landing rate by strategy", "landing-rate"),
        ("Elapsed time by strategy", "elapsed-time"),
        ("Physical HDD write volume by strategy", "hdd-write-volume"),
        ("HDD write operations by strategy", "hdd-write-operations"),
        ("Per-disk HDD write rate", "per-disk-write-rate"),
    ]
    .into_iter()
    .map(|(title, suffix)| PerformanceChartArtifact {
        title: title.to_string(),
        path: parent.join(format!("{base}-{suffix}.png")),
    })
    .collect()
}

pub(super) fn performance_io_chart_artifacts_from_json(
    artifact: &Value,
    pdf_path: &Path,
) -> Vec<PerformanceChartArtifact> {
    let base = pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = pdf_path.parent().unwrap_or_else(|| Path::new("."));
    let mut labels = BTreeSet::new();
    for row in json_array(artifact, &["plot_data", "io_time_series"])
        .into_iter()
        .flatten()
    {
        labels.insert(json_plot_label(row));
    }
    labels
        .into_iter()
        .map(|label| PerformanceChartArtifact {
            title: format!("Per-second IO rates: {label}"),
            path: parent.join(format!(
                "{base}-io-{}.png",
                label.replace(' ', "-").replace('/', "-")
            )),
        })
        .collect()
}

pub(super) fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let value = json_path(value, path)?;
    if value.is_null() {
        None
    } else if let Some(text) = value.as_str() {
        Some(text.to_string())
    } else {
        Some(value.to_string())
    }
}

pub(super) fn json_array_strings(value: &Value, path: &[&str]) -> Vec<String> {
    json_array(value, path)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

pub(super) fn json_u64(value: &Value, path: &[&str]) -> Option<u64> {
    json_path(value, path)?.as_u64()
}

pub(super) fn json_f64(value: &Value, path: &[&str]) -> Option<f64> {
    json_path(value, path)?.as_f64()
}

pub(super) fn json_bool(value: &Value, path: &[&str]) -> Option<bool> {
    json_path(value, path)?.as_bool()
}

pub(super) fn json_array<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    json_path(value, path)?.as_array()
}

pub(super) fn json_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

pub(super) fn hostname_for_report() -> String {
    ProcessCommand::new("hostname")
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "not recorded".to_string())
}

pub(super) fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
pub(super) fn render_simple_pdf(markdown: &str) -> Vec<u8> {
    let lines = markdown
        .lines()
        .map(strip_markdown_for_pdf)
        .collect::<Vec<_>>();
    let lines_per_page = 48_usize;
    let page_count = lines.len().div_ceil(lines_per_page).max(1);
    let font_id = 3 + page_count * 2;
    let mut objects = Vec::<String>::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
    ));
    for page_index in 0..page_count {
        let page_id = 3 + page_index * 2;
        let content_id = page_id + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"
        ));
        let page_lines = lines
            .iter()
            .skip(page_index * lines_per_page)
            .take(lines_per_page)
            .collect::<Vec<_>>();
        let mut stream = String::from("BT /F1 9 Tf 36 756 Td 0 -14 Td\n");
        for line in page_lines {
            stream.push_str(&format!("({}) Tj 0 -14 Td\n", escape_pdf_text(line)));
        }
        stream.push_str("ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref_start = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[cfg(test)]
pub(super) fn strip_markdown_for_pdf(line: &str) -> String {
    line.replace("**", "")
        .replace('`', "")
        .replace("<br>", " | ")
        .chars()
        .take(110)
        .collect()
}

#[cfg(test)]
pub(super) fn escape_pdf_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

pub(super) fn throughput(measurement: PerformanceMeasurement) -> f64 {
    measurement.bytes as f64 / measurement.seconds.max(0.001)
}

pub(super) fn zero_measurement() -> PerformanceMeasurement {
    PerformanceMeasurement {
        bytes: 0,
        seconds: 0.0,
    }
}

pub(super) fn update_file_read_measurements_from_disk_results(
    file_results: &mut [PerformanceFileResult],
    disk_results: &[PerformanceDiskResult],
) {
    let mut by_file = BTreeMap::<u32, PerformanceMeasurement>::new();
    for row in disk_results {
        let entry = by_file
            .entry(row.file_index)
            .or_insert_with(zero_measurement);
        entry.bytes = entry.bytes.saturating_add(row.ssd_read.bytes);
        entry.seconds += row.ssd_read.seconds.max(0.001);
    }
    for row in file_results {
        if let Some(measurement) = by_file.get(&row.file_index) {
            row.ssd_read = *measurement;
        }
    }
}

pub(super) fn measurement_rate(
    measurements: impl Iterator<Item = PerformanceMeasurement>,
) -> Option<f64> {
    measurement_rate_with_current(measurements, 0, 0.0)
}

pub(super) fn measurement_rate_with_current(
    measurements: impl Iterator<Item = PerformanceMeasurement>,
    current_bytes: u64,
    current_seconds: f64,
) -> Option<f64> {
    let (mut bytes, mut seconds) = measurements.fold((0_u64, 0.0_f64), |acc, measurement| {
        (
            acc.0.saturating_add(measurement.bytes),
            acc.1 + measurement.seconds.max(0.001),
        )
    });
    bytes = bytes.saturating_add(current_bytes);
    seconds += current_seconds.max(0.0);
    if bytes == 0 || seconds <= 0.0 {
        None
    } else {
        Some(bytes as f64 / seconds.max(0.001))
    }
}

pub(super) fn active_hdd_disk_rates(
    active_writes: &ActiveHddWriteMap,
) -> Result<Vec<String>, CliError> {
    let now = Instant::now();
    let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
    for active in active_writes
        .lock()
        .map_err(|_| {
            CliError::CommandFailed("performance-test active HDD write lock poisoned".to_string())
        })?
        .values()
    {
        if active.bytes_written == 0 {
            continue;
        }
        let elapsed = now.duration_since(active.started).as_secs_f64().max(0.001);
        let entry = by_disk.entry(active.disk_id.clone()).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(active.bytes_written);
        entry.1 = entry.1.max(elapsed);
    }
    Ok(by_disk
        .into_iter()
        .filter_map(|(disk_id, (bytes, seconds))| {
            if bytes == 0 {
                None
            } else {
                Some(format!(
                    "{} {}/s",
                    disk_id.as_str(),
                    format_bytes(bytes as f64 / seconds.max(0.001))
                ))
            }
        })
        .collect())
}

pub(super) fn active_hdd_landing_lines(
    active_writes: &ActiveHddWriteMap,
    file_count: u32,
) -> Result<Vec<String>, CliError> {
    let now = Instant::now();
    let active = active_writes.lock().map_err(|_| {
        CliError::CommandFailed("performance-test active HDD write lock poisoned".to_string())
    })?;
    Ok(active
        .values()
        .map(|write| {
            format!(
                "file {}/{} copy {} -> {}: {}/{} @ {} {}",
                write.file_index + 1,
                file_count,
                write.copy_index + 1,
                write.disk_id.as_str(),
                format_bytes(write.bytes_written as f64),
                format_bytes(write.size_bytes as f64),
                active_hdd_write_rate(write, now),
                write.relative_path.display()
            )
        })
        .collect())
}

pub(super) fn active_hdd_write_rate(write: &ActiveHddWrite, now: Instant) -> String {
    match (write.phase, write.bytes_written) {
        (PerformanceCopyProgressPhase::Copying, 0) => "copying".to_string(),
        (PerformanceCopyProgressPhase::Syncing, 0) => "settling".to_string(),
        (PerformanceCopyProgressPhase::Syncing, bytes) => {
            let elapsed = now.duration_since(write.started).as_secs_f64().max(0.001);
            format!("settling; avg {}/s", format_bytes(bytes as f64 / elapsed))
        }
        (PerformanceCopyProgressPhase::Copying, bytes) => {
            let elapsed = now.duration_since(write.started).as_secs_f64().max(0.001);
            format!("{}/s", format_bytes(bytes as f64 / elapsed))
        }
    }
}

pub(super) fn render_performance_json(report: &PerformanceReport) -> String {
    let artifact = serde_json::json!({
        "schema": "dasobjectstore.performance_test.recommendation.v1",
        "artifact_kind": "ingress_recommendation",
        "run": {
            "run_id": report.run_id,
            "generated_at_utc": report.generated_at_utc,
            "repository_revision": report.repository_revision,
            "cli_version": dasobjectstore_core::VERSION,
            "command": report.reproduction_args,
            "parameters": {
                "workload_kind": report.workload_kind.as_str(),
                "source_path": report
                    .source_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                "file_size_bytes": report.file_size,
                "file_count": report.file_count,
                "total_source_bytes": report.total_source_bytes,
                "source_cap_bytes": report.source_cap_bytes,
                "file_selection": report.file_selection.as_str(),
                "file_orders": report.file_orders.iter().map(|order| order.as_str()).collect::<Vec<_>>(),
                "discovered_file_count": report.discovered_file_count,
                "discovered_total_bytes": report.discovered_total_bytes,
                "max_hdd_concurrency": report.max_concurrency,
                "selected_scenarios": report
                    .selected_scenario_names()
                    .into_iter()
                    .collect::<Vec<_>>(),
                "selected_hdd_concurrency": report.selected_hdd_concurrency(),
                "redundancy": report.redundancy,
                "keep_temp": report.keep_temp,
            },
            "artifacts": {
                "pdf_path": report.pdf_path.to_string_lossy(),
                "qr_path": report.qr_path.to_string_lossy(),
                "json_path": report.json_path.to_string_lossy(),
                "recommendation_json_path": report.json_path.to_string_lossy(),
            },
        },
        "hardware": {
            "roots": {
                "ssd_root": report.ssd_root.to_string_lossy(),
                "hdd_root": report.hdd_root.to_string_lossy(),
                "tmp_dir": report.tmp_dir.to_string_lossy(),
            },
            "disks": report.disks.iter().map(|(disk_id, root_path)| {
                serde_json::json!({
                    "disk_id": disk_id.as_str(),
                    "role": "hdd_capacity",
                    "root_path": root_path.to_string_lossy(),
                })
            }).collect::<Vec<_>>(),
        },
        "scenarios": {
            "ssd_only": ssd_only_json(&report.results.ssd_only, report.file_size, report.total_source_bytes),
            "ssd_stage_then_drain_pipeline": {
                "selected": !report.results.ssd_stage_then_drain.is_empty(),
                "description": "Source payloads are fully staged to SSD first; HDD drain begins only after all selected files have landed.",
                "concurrency": report.results.ssd_stage_then_drain.iter().map(scenario_concurrency_json).collect::<Vec<_>>(),
            },
            "ssd_hdd_pipeline": {
                "selected": !report.results.ssd_pipeline.is_empty(),
                "description": "Source payloads are staged to SSD and FIFO HDD drain begins as soon as staged files are available, overlapping SSD reads and writes.",
                "concurrency": report.results.ssd_pipeline.iter().map(scenario_concurrency_json).collect::<Vec<_>>(),
            },
            "direct_hdd_pipeline": {
                "selected": !report.results.direct_hdd.is_empty(),
                "description": "Source payloads are written directly to selected HDD members without SSD staging.",
                "concurrency": report.results.direct_hdd.iter().map(scenario_concurrency_json).collect::<Vec<_>>(),
            },
        },
        "recommendation": {
            "strategy": recommendation_strategy_name(report.recommendation.strategy),
            "file_order": report.recommendation.file_order.as_str(),
            "hdd_concurrency": report.recommendation.hdd_concurrency,
            "redundancy": report.redundancy,
            "estimated_aggregate_bytes_per_second": rate_u64(report.recommendation.aggregate_bytes_per_second),
            "ssd_read_limited": recommendation_is_ssd_read_limited(report),
            "rationale": recommendation_rationale(report),
        },
        "plot_data": {
            "schema": "dasobjectstore.performance_test.plot_data.v1",
            "landing_rate_by_strategy": landing_rate_plot_rows(report),
            "elapsed_seconds_by_strategy": elapsed_plot_rows(report),
            "hdd_write_volume_by_strategy": hdd_volume_plot_rows(report),
            "hdd_write_operations_by_strategy": hdd_operations_plot_rows(report),
            "per_disk_hdd_write_rate": per_disk_rate_plot_rows(report),
            "io_time_series": io_time_series_plot_rows(report),
        },
        "daemon_policy": {
            "authoritative": report.authoritative,
            "effective_after": "daemon_restart",
            "authoritative_path": report
                .authoritative_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            "source_routes": {
                "remote_upload": "ssd_first",
                "external_disk_ingress": "ssd_first",
                "nvme_source_ingress": recommendation_strategy_name(report.recommendation.strategy),
            },
            "ssd_hdd_settlement": {
                "strategy": "ssd_hdd_pipeline",
                "file_order": report.recommendation.file_order.as_str(),
                "hdd_concurrency": report.recommendation.hdd_concurrency,
                "redundancy": report.redundancy,
                "estimated_aggregate_bytes_per_second": rate_u64(report.recommendation.aggregate_bytes_per_second),
            },
        },
    });
    serde_json::to_string_pretty(&artifact).expect("serialize performance recommendation JSON")
}

pub(super) fn ssd_only_json(
    scenarios: &[PerformanceScenarioResult],
    file_size: u64,
    total_source_bytes: u64,
) -> serde_json::Value {
    if scenarios.is_empty() {
        return serde_json::json!({
            "selected": false,
            "file_count": 0,
            "file_size_bytes": file_size,
            "total_bytes": total_source_bytes,
            "orders": [],
            "files": [],
            "io_samples": [],
        });
    }
    let first = &scenarios[0];
    serde_json::json!({
        "selected": true,
        "file_count": first.file_results.len() as u64,
        "file_size_bytes": file_size,
        "total_bytes": total_source_bytes,
        "file_order": first.file_order.as_str(),
        "median_generate_bytes_per_second": rate_u64(median_rate(
            first.file_results.iter().map(|row| throughput(row.ssd_write))
        )),
        "median_ssd_write_bytes_per_second": rate_u64(median_rate(
            first.file_results.iter().map(|row| throughput(row.ssd_write))
        )),
        "median_ssd_read_bytes_per_second": rate_u64(median_rate(
            first.file_results.iter().map(|row| throughput(row.ssd_read))
        )),
        "io_samples": scenario_io_samples_json(first),
        "files": first.file_results.iter().map(|row| {
            serde_json::json!({
                "file_index": row.file_index,
                "generated_bytes": row.ssd_write.bytes,
                "generate_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                "ssd_write_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                "ssd_read_bytes_per_second": rate_u64(throughput(row.ssd_read)),
            })
        }).collect::<Vec<_>>(),
        "orders": scenarios.iter().map(|scenario| {
            serde_json::json!({
                "file_order": scenario.file_order.as_str(),
                "file_count": scenario.file_results.len() as u64,
                "median_generate_bytes_per_second": rate_u64(median_rate(
                    scenario.file_results.iter().map(|row| throughput(row.ssd_write))
                )),
                "median_ssd_write_bytes_per_second": rate_u64(median_rate(
                    scenario.file_results.iter().map(|row| throughput(row.ssd_write))
                )),
                "median_ssd_read_bytes_per_second": rate_u64(median_rate(
                    scenario.file_results.iter().map(|row| throughput(row.ssd_read))
                )),
                "io_samples": scenario_io_samples_json(scenario),
                "files": scenario.file_results.iter().map(|row| {
                    serde_json::json!({
                        "file_index": row.file_index,
                        "generated_bytes": row.ssd_write.bytes,
                        "generate_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                        "ssd_write_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                        "ssd_read_bytes_per_second": rate_u64(throughput(row.ssd_read)),
                    })
                }).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

pub(super) fn scenario_concurrency_json(scenario: &PerformanceScenarioResult) -> serde_json::Value {
    serde_json::json!({
        "scenario": scenario.kind.as_str(),
        "file_order": scenario.file_order.as_str(),
        "concurrency": scenario.concurrency,
        "redundancy": scenario.redundancy,
        "queue_capacity": scenario.queue_capacity,
        "logical_source_bytes": scenario.logical_source_bytes,
        "physical_hdd_write_bytes": scenario.physical_hdd_write_bytes,
        "hdd_write_operations": scenario.hdd_write_operations,
        "aggregate_assigned_bytes": scenario.total_bytes,
        "aggregate_write_bytes_per_second": rate_u64(
            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001)
        ),
        "hdd_drain_started_before_all_ssd_staged": scenario.hdd_drain_started_before_all_ssd_staged,
        "slowest_member_seconds": scenario.concurrency_result.slowest_seconds,
        "members": scenario.concurrency_result.members.iter().map(DiskId::as_str).collect::<Vec<_>>(),
        "per_disk": per_disk_json(&scenario.disk_results),
        "io_samples": scenario_io_samples_json(scenario),
    })
}

pub(super) fn scenario_io_samples_json(
    scenario: &PerformanceScenarioResult,
) -> Vec<serde_json::Value> {
    scenario
        .io_samples
        .iter()
        .map(|sample| {
            serde_json::json!({
                "elapsed_second": sample.elapsed_second,
                "device_label": sample.device_label,
                "device_name": sample.device_name,
                "read_bytes_per_second": sample.read_bytes_per_second,
                "write_bytes_per_second": sample.write_bytes_per_second,
                "read_mib_per_second": sample.read_bytes_per_second as f64 / 1024.0 / 1024.0,
                "write_mib_per_second": sample.write_bytes_per_second as f64 / 1024.0 / 1024.0,
            })
        })
        .collect()
}

pub(super) fn per_disk_json(rows: &[PerformanceDiskResult]) -> Vec<serde_json::Value> {
    let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
    for row in rows {
        let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(row.write.bytes);
        entry.1 += row.write.seconds.max(0.001);
    }
    by_disk
        .into_iter()
        .map(|(disk_id, (assigned_bytes, seconds))| {
            serde_json::json!({
                "disk_id": disk_id.as_str(),
                "assigned_bytes": assigned_bytes,
                "write_bytes_per_second": rate_u64(assigned_bytes as f64 / seconds.max(0.001)),
                "write_operations": rows.iter().filter(|row| row.disk_id == disk_id).count(),
            })
        })
        .collect()
}

pub(super) fn landing_rate_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "aggregate_mib_per_second"))
        .collect()
}

pub(super) fn elapsed_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "elapsed_seconds"))
        .collect()
}

pub(super) fn hdd_volume_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "physical_hdd_write_gib"))
        .collect()
}

pub(super) fn hdd_operations_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "hdd_write_operations"))
        .collect()
}

pub(super) fn scenario_plot_row(
    report: &PerformanceReport,
    scenario: &PerformanceScenarioResult,
    metric: &str,
) -> serde_json::Value {
    let aggregate_mib_per_second =
        scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001) / 1024.0 / 1024.0;
    let physical_hdd_write_gib =
        scenario.physical_hdd_write_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    let value = match metric {
        "aggregate_mib_per_second" => aggregate_mib_per_second,
        "elapsed_seconds" => scenario.elapsed_seconds,
        "physical_hdd_write_gib" => physical_hdd_write_gib,
        "hdd_write_operations" => scenario.hdd_write_operations as f64,
        _ => 0.0,
    };
    serde_json::json!({
        "run_id": report.run_id,
        "scenario": scenario.kind.as_str(),
        "scenario_label": scenario.kind.label(),
        "file_order": scenario.file_order.as_str(),
        "hdd_concurrency": scenario.concurrency,
        "redundancy": scenario.redundancy,
        "metric": metric,
        "value": value,
        "aggregate_mib_per_second": aggregate_mib_per_second,
        "elapsed_seconds": scenario.elapsed_seconds,
        "logical_source_gib": scenario.logical_source_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
        "physical_hdd_write_gib": physical_hdd_write_gib,
        "hdd_write_operations": scenario.hdd_write_operations,
        "hdd_drain_overlapped_ssd_staging": scenario.hdd_drain_started_before_all_ssd_staged,
    })
}

pub(super) fn per_disk_rate_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for scenario in report.all_scenarios() {
        let mut by_disk = BTreeMap::<DiskId, (u64, f64, usize)>::new();
        for row in &scenario.disk_results {
            let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0, 0));
            entry.0 = entry.0.saturating_add(row.write.bytes);
            entry.1 += row.write.seconds.max(0.001);
            entry.2 += 1;
        }
        for (disk_id, (bytes, seconds, operations)) in by_disk {
            rows.push(serde_json::json!({
                "run_id": report.run_id,
                "scenario": scenario.kind.as_str(),
                "scenario_label": scenario.kind.label(),
                "file_order": scenario.file_order.as_str(),
                "hdd_concurrency": scenario.concurrency,
                "redundancy": scenario.redundancy,
                "disk_id": disk_id.as_str(),
                "write_mib_per_second": bytes as f64 / seconds.max(0.001) / 1024.0 / 1024.0,
                "assigned_gib": bytes as f64 / 1024.0 / 1024.0 / 1024.0,
                "write_operations": operations,
            }));
        }
    }
    rows
}

pub(super) fn io_time_series_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for scenario in report.all_scenarios() {
        for sample in &scenario.io_samples {
            rows.push(serde_json::json!({
                "run_id": report.run_id,
                "scenario": scenario.kind.as_str(),
                "scenario_label": scenario.kind.label(),
                "file_order": scenario.file_order.as_str(),
                "hdd_concurrency": scenario.concurrency,
                "redundancy": scenario.redundancy,
                "elapsed_second": sample.elapsed_second,
                "device_label": sample.device_label,
                "device_name": sample.device_name,
                "read_mib_per_second": sample.read_bytes_per_second as f64 / 1024.0 / 1024.0,
                "write_mib_per_second": sample.write_bytes_per_second as f64 / 1024.0 / 1024.0,
            }));
        }
    }
    rows
}

pub(super) fn recommendation_strategy_name(strategy: PerformanceScenarioKind) -> &'static str {
    match strategy {
        PerformanceScenarioKind::SsdOnly => "ssd_only",
        PerformanceScenarioKind::SsdStageThenDrain => "ssd_stage_then_drain_pipeline",
        PerformanceScenarioKind::SsdPipeline => "ssd_hdd_pipeline",
        PerformanceScenarioKind::DirectHdd => "direct_hdd_pipeline",
    }
}

pub(super) fn recommendation_is_ssd_read_limited(report: &PerformanceReport) -> bool {
    let Some(ssd_only) = report.results.ssd_only.first() else {
        return false;
    };
    let ssd_read = median_rate(
        ssd_only
            .file_results
            .iter()
            .map(|row| throughput(row.ssd_read)),
    );
    report.recommendation.aggregate_bytes_per_second > ssd_read * 0.9
}

pub(super) fn recommendation_rationale(report: &PerformanceReport) -> Vec<String> {
    let mut rationale = vec![report.recommendation.reason.clone()];
    if report.results.ssd_only.is_empty() {
        rationale.push(
            "SSD-only read/write baselines were not included in this selected benchmark matrix."
                .to_string(),
        );
    } else if recommendation_is_ssd_read_limited(report) {
        rationale.push(
            "Selected aggregate throughput is close to measured SSD read throughput; avoid raising concurrency without retesting."
                .to_string(),
        );
    } else {
        rationale.push(
            "Measured SSD read throughput leaves headroom for the selected landing strategy."
                .to_string(),
        );
    }
    rationale.push(
        "Use per-disk assigned byte and throughput rows to identify weak disks before scheduling latency-sensitive ingest."
            .to_string(),
    );
    rationale
}

pub(super) fn rate_u64(rate: f64) -> u64 {
    if rate.is_finite() && rate > 0.0 {
        rate.round() as u64
    } else {
        0
    }
}

pub(super) fn render_performance_report(report: PerformanceReport) -> String {
    let mut output = String::new();
    output.push_str("# DASObjectStore Performance Test Report\n\n");
    output.push_str("| Field | Value |\n");
    output.push_str("| --- | --- |\n");
    output.push_str(&format!(
        "| Brand | Mnemosyne Biosciences |\n\
| Product | DASObjectStore |\n\
| Report type | Performance test |\n\
| Report status | final |\n\
| Run ID | `{}` |\n\
| Generated at (UTC) | `{}` |\n\
| Repository revision | `{}` |\n\
| CLI version | `{}` |\n\
| Command | Recorded in reproduction payload |\n\
| JSON artifact | `{}` |\n\
| PDF artifact | `{}` |\n\
| QR artifact | `{}` |\n\
| QR status | `{}` |\n\
| Redundancy | `{}` HDD copy/copies per logical file |\n\
| Command digest | `{}` |\n\
| Reproduction payload SHA-256 | `{}` |\n\
| Reproduction QR payload | Encoded in report QR code |\n\n",
        compact_run_id(&report.run_id),
        report.generated_at_utc,
        compact_identifier(&report.repository_revision, 18),
        dasobjectstore_core::VERSION,
        compact_path(&report.json_path.display().to_string()),
        compact_path(&report.pdf_path.display().to_string()),
        compact_path(&report.qr_path.display().to_string()),
        report.qr_status,
        report.redundancy,
        compact_hash(&sha256_hex_bytes(report.reproduce_command.as_bytes())),
        compact_hash(&report.reproduction_payload_sha256)
    ));
    output.push_str(&format!(
        "QR code image: ![Reproduce]({})\n\n",
        report.qr_path.display()
    ));
    let source = report
        .source_path
        .as_ref()
        .map(|path| format!("; source `{}`", path.display()))
        .unwrap_or_default();
    let cap = report
        .source_cap_bytes
        .map(|bytes| format!("; cap {}", format_bytes(bytes as f64)))
        .unwrap_or_default();
    let discovered = if report.source_cap_bytes.is_some() {
        format!(
            "; discovered {} files, {} total",
            report.discovered_file_count,
            format_bytes(report.discovered_total_bytes as f64)
        )
    } else {
        String::new()
    };
    let file_orders = report
        .file_orders
        .iter()
        .map(|order| format!("`{}`", order.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!(
        "Scenario: {} workload, {} files, {} logical source total{}{}{}; file selection `{}`; file order(s) {}. Redundancy {}; SSD root `{}`; HDD root `{}`; disks {}; selected scenarios {}; selected HDD concurrency {}.\n\n",
        report.workload_kind.as_str(),
        report.file_count,
        format_bytes(report.total_source_bytes as f64),
        source,
        cap,
        discovered,
        report.file_selection.as_str(),
        file_orders,
        report.redundancy,
        report.ssd_root.display(),
        report.hdd_root.display(),
        report.disk_count,
        report.selected_scenario_names().join(", "),
        format_concurrency_list(&report.selected_hdd_concurrency())
    ));
    output.push_str("## Summary\n\n");
    output.push_str(&format!("- Run id: `{}`\n", compact_run_id(&report.run_id)));
    output.push_str("- Reproduce with: command recorded in the JSON artifact.\n");
    output.push_str("\n## Reproducibility\n\n");
    output.push_str("| Field | Value |\n| --- | --- |\n");
    output.push_str(&format!(
        "| JSON artifact | `{}` |\n",
        compact_path(&report.json_path.display().to_string())
    ));
    output.push_str(&format!(
        "| Payload SHA-256 | `{}` |\n",
        compact_hash(&report.reproduction_payload_sha256)
    ));
    output.push_str(&format!(
        "| Command digest | `{}` |\n",
        compact_hash(&sha256_hex_bytes(report.reproduce_command.as_bytes()))
    ));
    output.push_str("| Reproduction command | Recorded in the JSON artifact |\n");
    output.push_str("| QR payload | Encoded in the report QR code |\n\n");
    output.push_str(
        "The complete machine-readable benchmark artifact is retained as JSON and is deliberately not embedded in the formal PDF body. Use the JSON artifact for exact reproduction, audit, and daemon ingress-policy import.\n\n",
    );
    output.push_str(&format!(
        "- Total elapsed: {:.1} s\n",
        report.elapsed_seconds
    ));
    if let Some(ssd_only) = report.results.ssd_only.first() {
        output.push_str(&format!(
            "- Median SSD write: {}/s\n",
            format_bytes(median_rate(
                ssd_only
                    .file_results
                    .iter()
                    .map(|row| throughput(row.ssd_write))
            ))
        ));
        output.push_str(&format!(
            "- Median SSD read: {}/s\n",
            format_bytes(median_rate(
                ssd_only
                    .file_results
                    .iter()
                    .map(|row| throughput(row.ssd_read))
            ))
        ));
    } else {
        output.push_str("- Median SSD write: not measured in selected scenario matrix\n");
        output.push_str("- Median SSD read: not measured in selected scenario matrix\n");
    }
    output.push_str(&format!(
        "- Recommended strategy: {} with `{}` order at {} HDD worker(s), observed aggregate {}/s\n",
        humanize_report_token(report.recommendation.strategy.as_str()),
        friendly_file_order(report.recommendation.file_order.as_str()),
        report.recommendation.hdd_concurrency,
        format_bytes_compact(report.recommendation.aggregate_bytes_per_second)
    ));

    output.push_str("\n## Scenario Summary\n\n");
    output.push_str("| Scenario | File order | HDD concurrency | Redundancy | Logical source | Physical HDD writes | Operations | Aggregate landing | Elapsed | HDD drain overlapped SSD staging |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
    for scenario in report.all_scenarios() {
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} | {}/s | {:.0} s | {} |\n",
            scenario.kind.label(),
            scenario.file_order.as_str(),
            scenario.concurrency,
            scenario.redundancy,
            format_bytes_compact(scenario.logical_source_bytes as f64),
            format_bytes_compact(scenario.physical_hdd_write_bytes as f64),
            scenario.hdd_write_operations,
            format_bytes_compact(scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001)),
            scenario.elapsed_seconds,
            yes_no(scenario.hdd_drain_started_before_all_ssd_staged)
        ));
    }

    output.push_str("\n## SSD Timings\n\n");
    output.push_str("| Scenario | File order | HDD concurrency | File | SSD write | SSD read |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: |\n");
    for scenario in report.all_scenarios() {
        for row in &scenario.file_results {
            output.push_str(&format!(
                "| {} | `{}` | {} | {} | {}/s | {}/s |\n",
                scenario.kind.as_str(),
                scenario.file_order.as_str(),
                scenario.concurrency,
                row.file_index + 1,
                format_bytes(throughput(row.ssd_write)),
                format_bytes(throughput(row.ssd_read))
            ));
        }
    }

    output.push_str("\n## Per-disk Landed Files\n\n");
    output.push_str(
        "| Scenario | File order | HDD concurrency | File | Copy | Disk | Write rate |\n",
    );
    output.push_str("| --- | --- | ---: | ---: | ---: | --- | ---: |\n");
    for scenario in report.all_scenarios() {
        for row in &scenario.disk_results {
            output.push_str(&format!(
                "| {} | `{}` | {} | {} | {} | {} | {}/s |\n",
                row.scenario.as_str(),
                scenario.file_order.as_str(),
                row.concurrency,
                row.file_index + 1,
                row.copy_index + 1,
                row.disk_id,
                format_bytes(throughput(row.write))
            ));
        }
    }

    output.push_str("\n## Concurrency Results\n\n");
    output.push_str(
        "| Scenario | File order | HDD concurrency | Members | Aggregate landing | Slowest file write | HDD drain overlapped SSD staging |\n",
    );
    output.push_str("| --- | --- | ---: | --- | ---: | ---: | --- |\n");
    for scenario in report.all_scenarios() {
        let row = &scenario.concurrency_result;
        let members = row
            .members
            .iter()
            .map(DiskId::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {}/s | {:.0} s | {} |\n",
            row.scenario.as_str(),
            scenario.file_order.as_str(),
            row.concurrency,
            members,
            format_bytes_compact(row.aggregate_bytes as f64 / row.seconds.max(0.001)),
            row.slowest_seconds,
            yes_no(scenario.hdd_drain_started_before_all_ssd_staged)
        ));
    }

    output.push_str("\n## Quantitative Plot Data\n\n");
    output.push_str("The JSON artifact includes tidy bar-chart and IO time-series rows under `plot_data` for scientifically labelled Grammateus/floundeR plots. The report bundle includes ggplot2 PNG bar charts and per-run IO line charts rendered from the same benchmark rows.\n\n");
    for artifact in performance_chart_artifacts(&report) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }
    for artifact in performance_io_chart_artifacts(&report) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }
    output.push_str("| Plot dataset | Intended quantitative question |\n");
    output.push_str("| --- | --- |\n");
    output.push_str(
        "| `landing_rate_by_strategy` | Which strategy landed the complete dataset fastest? |\n",
    );
    output.push_str("| `elapsed_seconds_by_strategy` | Which strategy completed the workload in the least wall time? |\n");
    output.push_str("| `hdd_write_volume_by_strategy` | How much physical HDD data did each strategy write after redundancy? |\n");
    output.push_str("| `hdd_write_operations_by_strategy` | How many write operations did each strategy perform? |\n");
    output.push_str("| `per_disk_hdd_write_rate` | Which disks were faster or slower under the tested configuration? |\n");
    output.push_str("| `io_time_series` | How did SSD and HDD read/write IO rates change each second during each run? |\n");

    output.push_str("\n## Recommendation\n\n");
    output.push_str(&format!(
        "- Use `{}` with `{}` file order and {} HDD worker(s) for this hardware constellation.\n",
        report.recommendation.strategy.as_str(),
        report.recommendation.file_order.as_str(),
        report.recommendation.hdd_concurrency
    ));
    output.push_str(&format!("- {}.\n", report.recommendation.reason));
    output.push_str("- Use the JSON artifact as the machine-readable placement and concurrency guidance for future ingest policy.\n");
    output
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceChartArtifact {
    title: String,
    path: PathBuf,
}

pub(super) fn performance_chart_artifacts(
    report: &PerformanceReport,
) -> Vec<PerformanceChartArtifact> {
    let base = report
        .pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = report.pdf_path.parent().unwrap_or_else(|| Path::new("."));
    [
        ("Landing rate by strategy", "landing-rate"),
        ("Elapsed time by strategy", "elapsed-time"),
        ("Physical HDD write volume by strategy", "hdd-write-volume"),
        ("HDD write operations by strategy", "hdd-write-operations"),
        ("Per-disk HDD write rate", "per-disk-write-rate"),
    ]
    .into_iter()
    .map(|(title, suffix)| PerformanceChartArtifact {
        title: title.to_string(),
        path: parent.join(format!("{base}-{suffix}.png")),
    })
    .collect()
}

pub(super) fn performance_io_chart_artifacts(
    report: &PerformanceReport,
) -> Vec<PerformanceChartArtifact> {
    let base = report
        .pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = report.pdf_path.parent().unwrap_or_else(|| Path::new("."));
    report
        .all_scenarios()
        .into_iter()
        .filter(|scenario| !scenario.io_samples.is_empty())
        .map(|scenario| {
            let scenario_label = performance_chart_scenario_label(scenario);
            let suffix = scenario_label.replace(' ', "-");
            PerformanceChartArtifact {
                title: format!("Per-second IO rates: {scenario_label}"),
                path: parent.join(format!("{base}-io-{suffix}.png")),
            }
        })
        .collect()
}

#[allow(dead_code)]
pub(super) fn write_performance_chart_svgs(report: &PerformanceReport) -> Result<(), CliError> {
    let artifacts = performance_chart_artifacts(report);
    for artifact in &artifacts {
        if let Some(parent) = artifact.path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let scenario_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001) / 1024.0 / 1024.0,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[0].path,
        render_svg_bar_chart(
            &artifacts[0].title,
            "Tested strategy",
            "Aggregate landing rate (MiB/s)",
            &scenario_rows,
        ),
    )?;
    let elapsed_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.elapsed_seconds,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[1].path,
        render_svg_bar_chart(
            &artifacts[1].title,
            "Tested strategy",
            "Elapsed time (s)",
            &elapsed_rows,
        ),
    )?;
    let volume_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.physical_hdd_write_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[2].path,
        render_svg_bar_chart(
            &artifacts[2].title,
            "Tested strategy",
            "Physical HDD write volume (GiB)",
            &volume_rows,
        ),
    )?;
    let operation_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.hdd_write_operations as f64,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[3].path,
        render_svg_bar_chart(
            &artifacts[3].title,
            "Tested strategy",
            "HDD write operations",
            &operation_rows,
        ),
    )?;
    let disk_rows = performance_hdd_disk_rate_chart_rows(report);
    fs::write(
        &artifacts[4].path,
        render_svg_bar_chart(
            &artifacts[4].title,
            "Scenario and disk",
            "HDD write rate (MiB/s)",
            &disk_rows,
        ),
    )?;
    for (scenario, artifact) in report
        .all_scenarios()
        .into_iter()
        .filter(|scenario| !scenario.io_samples.is_empty())
        .zip(performance_io_chart_artifacts(report).into_iter())
    {
        if let Some(parent) = artifact.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &artifact.path,
            render_svg_io_line_chart(&artifact.title, &scenario.io_samples),
        )?;
    }
    Ok(())
}

pub(super) fn performance_chart_scenario_label(scenario: &PerformanceScenarioResult) -> String {
    if scenario.concurrency == 0 {
        format!(
            "{} {} r{}",
            scenario.kind.as_str(),
            scenario.file_order.as_str(),
            scenario.redundancy
        )
    } else {
        format!(
            "{} {} c{} r{}",
            scenario.kind.as_str(),
            scenario.file_order.as_str(),
            scenario.concurrency,
            scenario.redundancy
        )
    }
}

pub(super) fn performance_hdd_disk_rate_chart_rows(
    report: &PerformanceReport,
) -> Vec<(String, f64)> {
    let mut rows = Vec::new();
    for scenario in report.all_scenarios() {
        let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
        for row in &scenario.disk_results {
            let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0));
            entry.0 = entry.0.saturating_add(row.write.bytes);
            entry.1 += row.write.seconds.max(0.001);
        }
        for (disk_id, (bytes, seconds)) in by_disk {
            rows.push((
                format!(
                    "{} {} c{} {}",
                    scenario.kind.as_str(),
                    scenario.file_order.as_str(),
                    scenario.concurrency,
                    disk_id
                ),
                bytes as f64 / seconds.max(0.001) / 1024.0 / 1024.0,
            ));
        }
    }
    rows
}

pub(super) fn render_svg_bar_chart(
    title: &str,
    x_label: &str,
    y_label: &str,
    rows: &[(String, f64)],
) -> String {
    let width = 960.0_f64;
    let height = 460.0_f64;
    let left = 86.0_f64;
    let right = 28.0_f64;
    let top = 58.0_f64;
    let bottom = 132.0_f64;
    let plot_width = width - left - right;
    let plot_height = height - top - bottom;
    let max_value = rows
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let slot_width = plot_width / rows.len().max(1) as f64;
    let bar_width = (slot_width * 0.68).max(6.0);
    let mut marks = String::new();
    let palette = ["#2f5d50", "#6f7f35", "#2f6f8f", "#8f5d2f", "#5f548a"];
    for (idx, (label, value)) in rows.iter().enumerate() {
        let bar_height = (value / max_value) * plot_height;
        let x = left + idx as f64 * slot_width + (slot_width - bar_width) / 2.0;
        let y = top + plot_height - bar_height;
        let color = palette[idx % palette.len()];
        marks.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{bar_width:.1}\" height=\"{bar_height:.1}\" fill=\"{color}\"/>\n\
             <text x=\"{label_x:.1}\" y=\"{label_y:.1}\" transform=\"rotate(45 {label_x:.1} {label_y:.1})\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#222\">{label}</text>\n\
             <text x=\"{value_x:.1}\" y=\"{value_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#222\">{value:.1}</text>\n",
            label_x = x + bar_width / 2.0 - 4.0,
            label_y = top + plot_height + 18.0,
            value_x = x + bar_width / 2.0,
            value_y = (y - 6.0).max(20.0),
            label = escape_xml_text(label),
        ));
    }
    let tick_mid = max_value / 2.0;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
         <rect width=\"{width:.0}\" height=\"{height:.0}\" fill=\"#ffffff\"/>\n\
         <text x=\"24\" y=\"32\" font-family=\"Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"#111\">{}</text>\n\
         <line x1=\"{left:.1}\" y1=\"{axis_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{left:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{mid_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{mid_y:.1}\" stroke=\"#d9ddd2\" stroke-width=\"1\"/>\n\
         <text x=\"{tick_x:.1}\" y=\"{axis_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">0</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{mid_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{tick_mid:.1}</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{top_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{max_value:.1}</text>\n\
         {marks}\
         <text x=\"{x_label_x:.1}\" y=\"{x_label_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">{x_axis_label}</text>\n\
         <text transform=\"translate(22 {y_label_y:.1}) rotate(-90)\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">{y_axis_label}</text>\n\
         </svg>\n",
        escape_xml_text(title),
        escape_xml_text(title),
        axis_y = top + plot_height,
        axis_right = width - right,
        mid_y = top + plot_height / 2.0,
        tick_x = left - 8.0,
        mid_text_y = top + plot_height / 2.0 + 4.0,
        top_text_y = top + 4.0,
        marks = marks,
        x_label_x = left + plot_width / 2.0,
        x_label_y = height - 18.0,
        x_axis_label = escape_xml_text(x_label),
        y_label_y = top + plot_height / 2.0,
        y_axis_label = escape_xml_text(y_label),
    )
}

pub(super) fn render_svg_io_line_chart(title: &str, samples: &[PerformanceIoSample]) -> String {
    let width = 1120.0_f64;
    let height = 520.0_f64;
    let left = 86.0_f64;
    let right = 250.0_f64;
    let top = 62.0_f64;
    let bottom = 72.0_f64;
    let plot_width = width - left - right;
    let plot_height = height - top - bottom;
    let axis_y = top + plot_height;
    let axis_right = left + plot_width;
    if samples.is_empty() {
        return format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
             <rect width=\"{width:.0}\" height=\"{height:.0}\" fill=\"#ffffff\"/>\n\
             <text x=\"24\" y=\"36\" font-family=\"Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"#111\">{}</text>\n\
             <text x=\"24\" y=\"74\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#555\">No per-second IO samples were captured for this run.</text>\n\
             </svg>\n",
            escape_xml_text(title),
            escape_xml_text(title),
        );
    }
    let max_second = samples
        .iter()
        .map(|sample| sample.elapsed_second)
        .max()
        .unwrap_or(1)
        .max(1);
    let max_mib = samples
        .iter()
        .flat_map(|sample| {
            [
                sample.read_bytes_per_second as f64 / 1024.0 / 1024.0,
                sample.write_bytes_per_second as f64 / 1024.0 / 1024.0,
            ]
        })
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let palette = [
        "#2f5d50", "#2f6f8f", "#8f5d2f", "#5f548a", "#6f7f35", "#9a4f68", "#49728f", "#7b6230",
    ];
    let by_device = samples.iter().fold(
        BTreeMap::<String, Vec<&PerformanceIoSample>>::new(),
        |mut by_device, sample| {
            by_device
                .entry(sample.device_label.clone())
                .or_default()
                .push(sample);
            by_device
        },
    );
    let mut marks = String::new();
    let mut legend = String::new();
    for (idx, (label, device_samples)) in by_device.iter().enumerate() {
        let color = palette[idx % palette.len()];
        let mut read_points = String::new();
        let mut write_points = String::new();
        let mut point_marks = String::new();
        for sample in device_samples {
            let x = left + (sample.elapsed_second as f64 / max_second as f64) * plot_width;
            let read_mib = sample.read_bytes_per_second as f64 / 1024.0 / 1024.0;
            let write_mib = sample.write_bytes_per_second as f64 / 1024.0 / 1024.0;
            let read_y = axis_y - (read_mib / max_mib) * plot_height;
            let write_y = axis_y - (write_mib / max_mib) * plot_height;
            read_points.push_str(&format!("{x:.1},{read_y:.1} "));
            write_points.push_str(&format!("{x:.1},{write_y:.1} "));
            point_marks.push_str(&format!(
                "<circle cx=\"{x:.1}\" cy=\"{write_y:.1}\" r=\"2.7\" fill=\"{color}\"/>\n\
                 <circle cx=\"{x:.1}\" cy=\"{read_y:.1}\" r=\"2.7\" fill=\"#ffffff\" stroke=\"{color}\" stroke-width=\"1.4\"/>\n"
            ));
        }
        marks.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.0\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n\
             <polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.0\" stroke-dasharray=\"6 5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n",
            escape_xml_text(write_points.trim()),
            escape_xml_text(read_points.trim()),
        ));
        marks.push_str(&point_marks);
        let legend_y = top + 30.0 + idx as f64 * 42.0;
        legend.push_str(&format!(
            "<line x1=\"{legend_x:.1}\" y1=\"{write_y:.1}\" x2=\"{legend_line_x:.1}\" y2=\"{write_y:.1}\" stroke=\"{color}\" stroke-width=\"2\"/>\n\
             <line x1=\"{legend_x:.1}\" y1=\"{read_y:.1}\" x2=\"{legend_line_x:.1}\" y2=\"{read_y:.1}\" stroke=\"{color}\" stroke-width=\"2\" stroke-dasharray=\"6 5\"/>\n\
             <text x=\"{legend_text_x:.1}\" y=\"{label_y:.1}\" font-family=\"Arial, sans-serif\" font-size=\"11\" fill=\"#222\">{label}</text>\n\
             <text x=\"{legend_text_x:.1}\" y=\"{read_label_y:.1}\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#666\">solid write, dashed read</text>\n",
            legend_x = axis_right + 26.0,
            legend_line_x = axis_right + 56.0,
            legend_text_x = axis_right + 66.0,
            write_y = legend_y,
            read_y = legend_y + 12.0,
            label_y = legend_y + 4.0,
            read_label_y = legend_y + 18.0,
            label = escape_xml_text(label),
        ));
    }
    let tick_mid = max_mib / 2.0;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
         <rect width=\"{width:.0}\" height=\"{height:.0}\" fill=\"#ffffff\"/>\n\
         <text x=\"24\" y=\"34\" font-family=\"Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"#111\">{}</text>\n\
         <line x1=\"{left:.1}\" y1=\"{axis_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{left:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{mid_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{mid_y:.1}\" stroke=\"#d9ddd2\" stroke-width=\"1\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{axis_right:.1}\" y2=\"{top:.1}\" stroke=\"#edf0ea\" stroke-width=\"1\"/>\n\
         <text x=\"{tick_x:.1}\" y=\"{axis_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">0</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{mid_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{tick_mid:.1}</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{top_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{max_mib:.1}</text>\n\
         <text x=\"{left:.1}\" y=\"{x_tick_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">0s</text>\n\
         <text x=\"{axis_right:.1}\" y=\"{x_tick_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{max_second}s</text>\n\
         {marks}\
         {legend}\
         <text x=\"{x_label_x:.1}\" y=\"{x_label_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">Elapsed time (s)</text>\n\
         <text transform=\"translate(22 {y_label_y:.1}) rotate(-90)\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">IO rate (MiB/s)</text>\n\
         </svg>\n",
        escape_xml_text(title),
        escape_xml_text(title),
        mid_y = top + plot_height / 2.0,
        tick_x = left - 8.0,
        mid_text_y = top + plot_height / 2.0 + 4.0,
        top_text_y = top + 4.0,
        x_tick_y = axis_y + 18.0,
        marks = marks,
        legend = legend,
        x_label_x = left + plot_width / 2.0,
        x_label_y = height - 20.0,
        y_label_y = top + plot_height / 2.0,
    )
}

pub(super) fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) fn median_rate(values: impl Iterator<Item = f64>) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    values[values.len() / 2]
}

pub(super) fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

pub(super) fn performance_hdd_tui_rates(
    scenario: &PerformanceScenarioResult,
) -> (Option<f64>, Vec<String>) {
    let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
    for row in &scenario.disk_results {
        let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(row.write.bytes);
        entry.1 += row.write.seconds.max(0.001);
    }
    let mut aggregate = 0.0_f64;
    let mut labels = Vec::new();
    for (disk_id, (bytes, seconds)) in by_disk {
        let rate = bytes as f64 / seconds.max(0.001);
        aggregate += rate;
        labels.push(format!("{disk_id} {}/s", format_bytes(rate)));
    }
    let aggregate = if aggregate > 0.0 {
        Some(aggregate)
    } else {
        None
    };
    (aggregate, labels)
}

pub(super) fn recommend_performance_strategy(
    results: &PerformanceBenchmarkResults,
) -> PerformanceRecommendation {
    let mut candidates = Vec::new();
    for scenario in results
        .ssd_pipeline
        .iter()
        .chain(results.direct_hdd.iter())
        .chain(results.ssd_stage_then_drain.iter())
    {
        candidates.push((
            scenario.kind,
            scenario.file_order,
            scenario.concurrency,
            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
        ));
    }
    if candidates.is_empty() {
        if let Some(ssd_only) = results.ssd_only.first() {
            candidates.push((
                PerformanceScenarioKind::SsdOnly,
                ssd_only.file_order,
                0,
                ssd_only.total_bytes as f64 / ssd_only.elapsed_seconds.max(0.001),
            ));
        }
    }
    let (strategy, file_order, hdd_concurrency, aggregate_bytes_per_second) = candidates
        .into_iter()
        .max_by(|left, right| left.3.total_cmp(&right.3))
        .unwrap_or((
            PerformanceScenarioKind::SsdPipeline,
            PerformanceFileOrder::SizeDesc,
            0,
            0.0,
        ));
    let ssd_only_write = results.ssd_only.first().map(|scenario| {
        median_rate(
            scenario
                .file_results
                .iter()
                .map(|row| throughput(row.ssd_write)),
        )
    });
    let ssd_only_context = ssd_only_write
        .map(|rate| format!("; SSD-only median write was {}/s", format_bytes(rate)))
        .unwrap_or_else(|| "; SSD-only was not included in this selected matrix".to_string());
    let reason = match strategy {
        PerformanceScenarioKind::SsdPipeline => format!(
            "overlapping SSD-first ingest was the highest observed landing strategy{ssd_only_context}"
        ),
        PerformanceScenarioKind::DirectHdd => format!(
            "direct-to-HDD bypass produced the highest observed aggregate rate while avoiding SSD backlog{ssd_only_context}"
        ),
        PerformanceScenarioKind::SsdStageThenDrain => format!(
            "separated SSD stage then HDD drain produced the highest observed aggregate rate among selected scenarios{ssd_only_context}"
        ),
        PerformanceScenarioKind::SsdOnly => {
            "only SSD-only ingest was measured; no HDD landing strategy was selected".to_string()
        }
    };
    PerformanceRecommendation {
        strategy,
        file_order,
        hdd_concurrency,
        aggregate_bytes_per_second,
        reason,
    }
}

impl PerformanceReport {
    fn selected_scenario_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if !self.results.ssd_only.is_empty() {
            names.push(PerformanceScenarioKind::SsdOnly.as_str());
        }
        if !self.results.ssd_stage_then_drain.is_empty() {
            names.push(PerformanceScenarioKind::SsdStageThenDrain.as_str());
        }
        if !self.results.ssd_pipeline.is_empty() {
            names.push(PerformanceScenarioKind::SsdPipeline.as_str());
        }
        if !self.results.direct_hdd.is_empty() {
            names.push(PerformanceScenarioKind::DirectHdd.as_str());
        }
        names
    }

    fn selected_hdd_concurrency(&self) -> Vec<usize> {
        self.all_scenarios()
            .into_iter()
            .filter_map(|scenario| (scenario.concurrency > 0).then_some(scenario.concurrency))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn all_scenarios(&self) -> Vec<&PerformanceScenarioResult> {
        let mut scenarios = Vec::new();
        scenarios.extend(self.results.ssd_only.iter());
        scenarios.extend(self.results.ssd_stage_then_drain.iter());
        scenarios.extend(self.results.ssd_pipeline.iter());
        scenarios.extend(self.results.direct_hdd.iter());
        scenarios
    }
}
