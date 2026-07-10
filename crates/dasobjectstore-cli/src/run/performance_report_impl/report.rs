use super::super::super::performance_plan::{
    PerformanceDiskResult, PerformanceReport, PerformanceScenarioKind, PerformanceScenarioResult,
};
use std::collections::BTreeMap;

use super::super::super::{
    compact_hash, compact_identifier, compact_path, compact_run_id, format_bytes,
    format_bytes_compact, format_concurrency_list, friendly_file_order, humanize_report_token,
    DiskId,
};
use super::{
    median_rate, performance_chart_artifacts, performance_io_chart_artifacts, sha256_hex_bytes,
    throughput, yes_no,
};

pub(crate) fn render_performance_json(report: &PerformanceReport) -> String {
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

pub(crate) fn ssd_only_json(
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

pub(crate) fn scenario_concurrency_json(scenario: &PerformanceScenarioResult) -> serde_json::Value {
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

pub(crate) fn scenario_io_samples_json(
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

pub(crate) fn per_disk_json(rows: &[PerformanceDiskResult]) -> Vec<serde_json::Value> {
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

pub(crate) fn landing_rate_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "aggregate_mib_per_second"))
        .collect()
}

pub(crate) fn elapsed_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "elapsed_seconds"))
        .collect()
}

pub(crate) fn hdd_volume_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "physical_hdd_write_gib"))
        .collect()
}

pub(crate) fn hdd_operations_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "hdd_write_operations"))
        .collect()
}

pub(crate) fn scenario_plot_row(
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

pub(crate) fn per_disk_rate_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
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

pub(crate) fn io_time_series_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
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

pub(crate) fn recommendation_strategy_name(strategy: PerformanceScenarioKind) -> &'static str {
    match strategy {
        PerformanceScenarioKind::SsdOnly => "ssd_only",
        PerformanceScenarioKind::SsdStageThenDrain => "ssd_stage_then_drain_pipeline",
        PerformanceScenarioKind::SsdPipeline => "ssd_hdd_pipeline",
        PerformanceScenarioKind::DirectHdd => "direct_hdd_pipeline",
    }
}

pub(crate) fn recommendation_is_ssd_read_limited(report: &PerformanceReport) -> bool {
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

pub(crate) fn recommendation_rationale(report: &PerformanceReport) -> Vec<String> {
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

pub(crate) fn rate_u64(rate: f64) -> u64 {
    if rate.is_finite() && rate > 0.0 {
        rate.round() as u64
    } else {
        0
    }
}

pub(crate) fn render_performance_report(report: PerformanceReport) -> String {
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
