use super::*;

#[derive(Debug, Deserialize)]
struct ThroughputJson {
    #[serde(default = "default_window_days")]
    window_days: u8,
    #[serde(default)]
    read_bytes: u64,
    #[serde(default)]
    written_bytes: u64,
    #[serde(default)]
    ingest_bytes: u64,
    #[serde(default)]
    avg_read_bytes_per_second: u64,
    #[serde(default)]
    avg_write_bytes_per_second: u64,
    #[serde(default)]
    daily: Vec<ThroughputDayJson>,
}

#[derive(Debug, Deserialize)]
struct ThroughputDayJson {
    date: String,
    #[serde(default)]
    read_bytes: u64,
    #[serde(default)]
    written_bytes: u64,
    #[serde(default)]
    ingest_bytes: u64,
}

pub(super) fn read_throughput_7d(path: &Path) -> Option<ThroughputSummaryView> {
    let contents = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<ThroughputJson>(&contents).ok()?;
    Some(ThroughputSummaryView {
        window_days: parsed.window_days,
        read_tib: format_tib(parsed.read_bytes),
        written_tib: format_tib(parsed.written_bytes),
        ingest_tib: format_tib(parsed.ingest_bytes),
        avg_read_mib_s: mib_per_second(parsed.avg_read_bytes_per_second),
        avg_write_mib_s: mib_per_second(parsed.avg_write_bytes_per_second),
        source: "legacy_file".to_string(),
        message: None,
        daily: parsed
            .daily
            .into_iter()
            .map(|day| ThroughputDayView {
                date: day.date,
                read_tib: format_tib(day.read_bytes),
                written_tib: format_tib(day.written_bytes),
                ingest_tib: format_tib(day.ingest_bytes),
            })
            .collect(),
    })
}

fn default_window_days() -> u8 {
    7
}

pub(super) fn read_smart_warnings(path: &Path) -> Result<Vec<SmartWarningView>, DashboardWarning> {
    let contents = fs::read_to_string(path).map_err(|error| {
        DashboardWarning::new(
            "smart_warning_telemetry_unreadable",
            format!(
                "SMART warning telemetry could not be read from {}: {error}.",
                path.display()
            ),
        )
    })?;
    serde_json::from_str::<Vec<SmartWarningView>>(&contents).map_err(|error| {
        DashboardWarning::new(
            "smart_warning_telemetry_invalid",
            format!(
                "SMART warning telemetry {} is invalid JSON: {error}.",
                path.display()
            ),
        )
    })
}

pub(super) fn health_label(
    state: DashboardHealthStateView,
    hdd_count: usize,
    store_count: usize,
) -> &'static str {
    match (state, hdd_count, store_count) {
        (DashboardHealthStateView::Healthy, _, _) => "Live inventory healthy",
        (_, 0, _) => "Managed storage unavailable",
        (_, _, 0) => "ObjectStore registry empty",
        _ => "Live inventory watch",
    }
}
