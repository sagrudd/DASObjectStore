use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use super::super::super::{
    compact_hash, compact_path, compact_run_id, format_bytes, format_bytes_compact,
    friendly_file_order, humanize_report_token, performance_artifact_signature,
    report_renderer_command, CliError,
};
use super::*;
use super::{yes_no, PerformanceChartArtifact};

pub(crate) fn render_performance_report_from_json_artifact(
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

pub(crate) fn write_performance_chart_svgs_from_json(
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

pub(crate) fn json_plot_label(row: &Value) -> String {
    format!(
        "{} {} c{} r{}",
        json_string(row, &["scenario"]).unwrap_or_else(|| "unknown".to_string()),
        json_string(row, &["file_order"]).unwrap_or_else(|| "order".to_string()),
        json_u64(row, &["hdd_concurrency"]).unwrap_or_default(),
        json_u64(row, &["redundancy"]).unwrap_or_default()
    )
}

pub(crate) fn performance_chart_artifacts_from_pdf_path(
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

pub(crate) fn performance_io_chart_artifacts_from_json(
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
