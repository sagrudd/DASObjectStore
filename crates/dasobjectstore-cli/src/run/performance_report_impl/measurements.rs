use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::super::super::performance_execution::{ActiveHddWrite, ActiveHddWriteMap};
use super::super::super::performance_plan::{
    PerformanceBenchmarkResults, PerformanceDiskResult, PerformanceFileResult,
    PerformanceMeasurement, PerformanceRecommendation, PerformanceReport, PerformanceScenarioKind,
    PerformanceScenarioResult,
};
use super::super::super::{format_bytes, CliError, DiskId, PerformanceCopyProgressPhase};
use super::*;
use crate::cli::PerformanceFileOrder;

pub(crate) fn throughput(measurement: PerformanceMeasurement) -> f64 {
    measurement.bytes as f64 / measurement.seconds.max(0.001)
}

pub(crate) fn zero_measurement() -> PerformanceMeasurement {
    PerformanceMeasurement {
        bytes: 0,
        seconds: 0.0,
    }
}

pub(crate) fn update_file_read_measurements_from_disk_results(
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

pub(crate) fn measurement_rate(
    measurements: impl Iterator<Item = PerformanceMeasurement>,
) -> Option<f64> {
    measurement_rate_with_current(measurements, 0, 0.0)
}

pub(crate) fn measurement_rate_with_current(
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

pub(crate) fn active_hdd_disk_rates(
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

pub(crate) fn active_hdd_landing_lines(
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

pub(crate) fn active_hdd_write_rate(write: &ActiveHddWrite, now: Instant) -> String {
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

pub(crate) fn performance_hdd_tui_rates(
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

pub(crate) fn recommend_performance_strategy(
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
    pub(crate) fn selected_scenario_names(&self) -> Vec<&'static str> {
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

    pub(crate) fn selected_hdd_concurrency(&self) -> Vec<usize> {
        self.all_scenarios()
            .into_iter()
            .filter_map(|scenario| (scenario.concurrency > 0).then_some(scenario.concurrency))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn all_scenarios(&self) -> Vec<&PerformanceScenarioResult> {
        let mut scenarios = Vec::new();
        scenarios.extend(self.results.ssd_only.iter());
        scenarios.extend(self.results.ssd_stage_then_drain.iter());
        scenarios.extend(self.results.ssd_pipeline.iter());
        scenarios.extend(self.results.direct_hdd.iter());
        scenarios
    }
}
