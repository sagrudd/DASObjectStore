use serde::{Deserialize, Serialize};

pub const FLOUNDER_APPLIANCE_TELEMETRY_SCHEMA_VERSION: &str =
    "mnemosyne.flounder.appliance_telemetry.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderApplianceTelemetryContract {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub producer_product: String,
    pub window: FlounderTelemetryWindow,
    pub charts: Vec<FlounderTelemetryChart>,
}

impl FlounderApplianceTelemetryContract {
    pub fn new(
        generated_at_utc: impl Into<String>,
        producer_product: impl Into<String>,
        window: FlounderTelemetryWindow,
        charts: Vec<FlounderTelemetryChart>,
    ) -> Self {
        Self {
            schema_version: FLOUNDER_APPLIANCE_TELEMETRY_SCHEMA_VERSION.to_string(),
            generated_at_utc: generated_at_utc.into(),
            producer_product: producer_product.into(),
            window,
            charts,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryWindow {
    pub value: String,
    pub label: String,
    pub start_utc: Option<String>,
    pub end_utc: Option<String>,
    pub cadence_seconds: Option<u64>,
    pub downsample_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryChart {
    pub chart_id: String,
    pub title: String,
    pub layout: FlounderTelemetryChartLayout,
    pub x_axis: FlounderTelemetryAxis,
    pub y_axis: FlounderTelemetryAxis,
    pub series: Vec<FlounderTelemetrySeries>,
    #[serde(default)]
    pub bands: Vec<FlounderTelemetryBand>,
    #[serde(default)]
    pub missing_intervals: Vec<FlounderTelemetryMissingInterval>,
    #[serde(default)]
    pub small_multiples: Vec<FlounderTelemetrySmallMultiple>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlounderTelemetryChartLayout {
    LineWithGaps,
    PointSummary,
    StepSummary,
    CapacityBand,
    PerDiskIoTrace,
    SmallMultiple,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryAxis {
    pub label: String,
    pub unit: FlounderTelemetryUnit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlounderTelemetryUnit {
    TimeUtc,
    PercentBasisPoints,
    Bytes,
    BytesPerSecond,
    OperationsPerSecond,
    Count,
    Tib,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetrySeries {
    pub series_id: String,
    pub label: String,
    pub role: FlounderTelemetrySeriesRole,
    pub unit: FlounderTelemetryUnit,
    #[serde(default)]
    pub device: Option<FlounderTelemetryDevice>,
    pub points: Vec<FlounderTelemetryPoint>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlounderTelemetrySeriesRole {
    Line,
    Point,
    Step,
    Band,
    Trace,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryPoint {
    pub timestamp_utc: String,
    pub value: Option<i64>,
    pub quality: FlounderTelemetryPointQuality,
    #[serde(default)]
    pub label: Option<String>,
}

impl FlounderTelemetryPoint {
    pub fn observed(timestamp_utc: impl Into<String>, value: i64) -> Self {
        Self {
            timestamp_utc: timestamp_utc.into(),
            value: Some(value),
            quality: FlounderTelemetryPointQuality::Observed,
            label: None,
        }
    }

    pub fn missing(
        timestamp_utc: impl Into<String>,
        quality: FlounderTelemetryPointQuality,
        label: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_utc: timestamp_utc.into(),
            value: None,
            quality,
            label: Some(label.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlounderTelemetryPointQuality {
    Observed,
    MissingSample,
    UnavailableCounter,
    ServiceRestart,
    UnknownDevice,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryBand {
    pub band_id: String,
    pub label: String,
    pub lower_value: Option<i64>,
    pub upper_value: Option<i64>,
    pub unit: FlounderTelemetryUnit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryMissingInterval {
    pub start_utc: String,
    pub end_utc: String,
    pub reason: FlounderTelemetryMissingReason,
    pub label: String,
    #[serde(default)]
    pub affected_series_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlounderTelemetryMissingReason {
    NoSamples,
    ServiceStopped,
    CounterUnavailable,
    DeviceUnknown,
    CollectionError,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetrySmallMultiple {
    pub multiple_id: String,
    pub title: String,
    pub series_ids: Vec<String>,
    pub device: Option<FlounderTelemetryDevice>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryDevice {
    pub device_id: String,
    pub label: Option<String>,
    pub enclosure_id: Option<String>,
    pub bay_label: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_appliance_telemetry_contract_for_flounder() {
        let contract = FlounderApplianceTelemetryContract::new(
            "2026-07-09T19:50:00Z",
            "dasobjectstore",
            FlounderTelemetryWindow {
                value: "one_day".to_string(),
                label: "1 day".to_string(),
                start_utc: Some("2026-07-08T19:50:00Z".to_string()),
                end_utc: Some("2026-07-09T19:50:00Z".to_string()),
                cadence_seconds: Some(30),
                downsample_seconds: Some(60),
            },
            vec![
                FlounderTelemetryChart {
                    chart_id: "cpu_usage".to_string(),
                    title: "CPU usage".to_string(),
                    layout: FlounderTelemetryChartLayout::LineWithGaps,
                    x_axis: time_axis(),
                    y_axis: percent_axis("CPU"),
                    series: vec![FlounderTelemetrySeries {
                        series_id: "cpu".to_string(),
                        label: "CPU".to_string(),
                        role: FlounderTelemetrySeriesRole::Line,
                        unit: FlounderTelemetryUnit::PercentBasisPoints,
                        device: None,
                        points: vec![
                            FlounderTelemetryPoint::observed("2026-07-09T19:49:00Z", 4200),
                            FlounderTelemetryPoint::missing(
                                "2026-07-09T19:49:30Z",
                                FlounderTelemetryPointQuality::MissingSample,
                                "sample missing",
                            ),
                        ],
                    }],
                    bands: Vec::new(),
                    missing_intervals: vec![FlounderTelemetryMissingInterval {
                        start_utc: "2026-07-09T19:49:30Z".to_string(),
                        end_utc: "2026-07-09T19:50:00Z".to_string(),
                        reason: FlounderTelemetryMissingReason::NoSamples,
                        label: "collector produced no sample".to_string(),
                        affected_series_ids: vec!["cpu".to_string()],
                    }],
                    small_multiples: Vec::new(),
                },
                FlounderTelemetryChart {
                    chart_id: "capacity".to_string(),
                    title: "Capacity bands".to_string(),
                    layout: FlounderTelemetryChartLayout::CapacityBand,
                    x_axis: time_axis(),
                    y_axis: FlounderTelemetryAxis {
                        label: "Capacity".to_string(),
                        unit: FlounderTelemetryUnit::Bytes,
                    },
                    series: vec![FlounderTelemetrySeries {
                        series_id: "used_capacity".to_string(),
                        label: "Used capacity".to_string(),
                        role: FlounderTelemetrySeriesRole::Band,
                        unit: FlounderTelemetryUnit::Bytes,
                        device: None,
                        points: vec![FlounderTelemetryPoint::observed(
                            "2026-07-09T19:50:00Z",
                            2_199_023_255_552,
                        )],
                    }],
                    bands: vec![FlounderTelemetryBand {
                        band_id: "warning".to_string(),
                        label: "Warning threshold".to_string(),
                        lower_value: Some(8_000_000_000_000),
                        upper_value: None,
                        unit: FlounderTelemetryUnit::Bytes,
                    }],
                    missing_intervals: Vec::new(),
                    small_multiples: Vec::new(),
                },
                FlounderTelemetryChart {
                    chart_id: "disk_io".to_string(),
                    title: "Per-disk IO".to_string(),
                    layout: FlounderTelemetryChartLayout::SmallMultiple,
                    x_axis: time_axis(),
                    y_axis: FlounderTelemetryAxis {
                        label: "Write rate".to_string(),
                        unit: FlounderTelemetryUnit::BytesPerSecond,
                    },
                    series: vec![FlounderTelemetrySeries {
                        series_id: "qnap-1057-write".to_string(),
                        label: "QNAP bay 1 write".to_string(),
                        role: FlounderTelemetrySeriesRole::Trace,
                        unit: FlounderTelemetryUnit::BytesPerSecond,
                        device: Some(FlounderTelemetryDevice {
                            device_id: "qnap-1057".to_string(),
                            label: Some("QNAP bay 1".to_string()),
                            enclosure_id: Some("qnap".to_string()),
                            bay_label: Some("1".to_string()),
                        }),
                        points: vec![FlounderTelemetryPoint::observed(
                            "2026-07-09T19:50:00Z",
                            104_857_600,
                        )],
                    }],
                    bands: Vec::new(),
                    missing_intervals: Vec::new(),
                    small_multiples: vec![FlounderTelemetrySmallMultiple {
                        multiple_id: "qnap-1057".to_string(),
                        title: "QNAP bay 1".to_string(),
                        series_ids: vec!["qnap-1057-write".to_string()],
                        device: Some(FlounderTelemetryDevice {
                            device_id: "qnap-1057".to_string(),
                            label: Some("QNAP bay 1".to_string()),
                            enclosure_id: Some("qnap".to_string()),
                            bay_label: Some("1".to_string()),
                        }),
                    }],
                },
            ],
        );

        let encoded = serde_json::to_value(&contract).expect("contract serializes");

        assert_eq!(
            encoded["schema_version"],
            FLOUNDER_APPLIANCE_TELEMETRY_SCHEMA_VERSION
        );
        assert_eq!(encoded["window"]["value"], "one_day");
        assert_eq!(encoded["charts"][0]["layout"], "line_with_gaps");
        assert_eq!(
            encoded["charts"][0]["series"][0]["points"][1]["value"],
            json!(null)
        );
        assert_eq!(
            encoded["charts"][0]["missing_intervals"][0]["reason"],
            "no_samples"
        );
        assert_eq!(encoded["charts"][1]["layout"], "capacity_band");
        assert_eq!(encoded["charts"][1]["bands"][0]["band_id"], "warning");
        assert_eq!(encoded["charts"][2]["layout"], "small_multiple");
        assert_eq!(
            encoded["charts"][2]["series"][0]["device"]["device_id"],
            "qnap-1057"
        );
        assert_eq!(
            encoded["charts"][2]["small_multiples"][0]["series_ids"][0],
            "qnap-1057-write"
        );
    }

    #[test]
    fn serializes_required_chart_layout_names() {
        let layouts = vec![
            FlounderTelemetryChartLayout::LineWithGaps,
            FlounderTelemetryChartLayout::PointSummary,
            FlounderTelemetryChartLayout::StepSummary,
            FlounderTelemetryChartLayout::CapacityBand,
            FlounderTelemetryChartLayout::PerDiskIoTrace,
            FlounderTelemetryChartLayout::SmallMultiple,
        ];
        let encoded = serde_json::to_value(layouts).expect("layouts serialize");

        assert_eq!(
            encoded,
            json!([
                "line_with_gaps",
                "point_summary",
                "step_summary",
                "capacity_band",
                "per_disk_io_trace",
                "small_multiple"
            ])
        );
    }

    fn time_axis() -> FlounderTelemetryAxis {
        FlounderTelemetryAxis {
            label: "Time".to_string(),
            unit: FlounderTelemetryUnit::TimeUtc,
        }
    }

    fn percent_axis(label: &str) -> FlounderTelemetryAxis {
        FlounderTelemetryAxis {
            label: label.to_string(),
            unit: FlounderTelemetryUnit::PercentBasisPoints,
        }
    }
}
