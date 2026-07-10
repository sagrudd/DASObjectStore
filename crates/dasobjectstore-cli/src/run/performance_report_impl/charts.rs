use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::super::super::performance_plan::{
    PerformanceIoSample, PerformanceReport, PerformanceScenarioResult,
};
use super::super::super::{CliError, DiskId};
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct PerformanceChartArtifact {
    pub(crate) title: String,
    pub(crate) path: PathBuf,
}

pub(crate) fn performance_chart_artifacts(
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

pub(crate) fn performance_io_chart_artifacts(
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
pub(crate) fn write_performance_chart_svgs(report: &PerformanceReport) -> Result<(), CliError> {
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

pub(crate) fn performance_chart_scenario_label(scenario: &PerformanceScenarioResult) -> String {
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

pub(crate) fn performance_hdd_disk_rate_chart_rows(
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

pub(crate) fn render_svg_bar_chart(
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

pub(crate) fn render_svg_io_line_chart(title: &str, samples: &[PerformanceIoSample]) -> String {
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

pub(crate) fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn median_rate(values: impl Iterator<Item = f64>) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    values[values.len() / 2]
}

pub(crate) fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
