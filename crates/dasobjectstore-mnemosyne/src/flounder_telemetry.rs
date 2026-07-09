use serde::{Deserialize, Serialize};

pub const FLOUNDER_APPLIANCE_TELEMETRY_SCHEMA_VERSION: &str =
    "mnemosyne.flounder.appliance_telemetry.v1";
pub const FLOUNDER_TELEMETRY_CHART_CONTRACT_SCHEMA_VERSION: &str =
    "mnemosyne.flounder.telemetry_chart_contract.v1";

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

    pub fn into_chart_contract(
        self,
        audiences: Vec<FlounderTelemetryAudience>,
    ) -> FlounderTelemetryChartContract {
        FlounderTelemetryChartContract::new(
            self.generated_at_utc,
            FlounderTelemetryProducer::new(self.producer_product),
            audiences,
            self.window,
            self.charts,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryChartContract {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub producer: FlounderTelemetryProducer,
    #[serde(default)]
    pub audiences: Vec<FlounderTelemetryAudience>,
    pub window: FlounderTelemetryWindow,
    pub charts: Vec<FlounderTelemetryChart>,
}

impl FlounderTelemetryChartContract {
    pub fn new(
        generated_at_utc: impl Into<String>,
        producer: FlounderTelemetryProducer,
        audiences: Vec<FlounderTelemetryAudience>,
        window: FlounderTelemetryWindow,
        charts: Vec<FlounderTelemetryChart>,
    ) -> Self {
        Self {
            schema_version: FLOUNDER_TELEMETRY_CHART_CONTRACT_SCHEMA_VERSION.to_string(),
            generated_at_utc: generated_at_utc.into(),
            producer,
            audiences,
            window,
            charts,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryProducer {
    pub product_id: String,
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub component_id: Option<String>,
}

impl FlounderTelemetryProducer {
    pub fn new(product_id: impl Into<String>) -> Self {
        Self {
            product_id: product_id.into(),
            product_name: None,
            component_id: None,
        }
    }

    pub fn named(mut self, product_name: impl Into<String>) -> Self {
        self.product_name = Some(product_name.into());
        self
    }

    pub fn with_component(mut self, component_id: impl Into<String>) -> Self {
        self.component_id = Some(component_id.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlounderTelemetryAudience {
    WebDashboard,
    GrammateusReport,
    ApiExport,
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

impl FlounderTelemetryChart {
    pub fn render_plan(&self) -> FlounderTelemetryRenderPlan {
        let mut line_segments = Vec::new();
        let mut gap_labels = self
            .missing_intervals
            .iter()
            .map(FlounderTelemetryGapLabel::from)
            .collect::<Vec<_>>();

        for series in &self.series {
            let mut current_points = Vec::new();
            for point in &series.points {
                if point.is_observed() {
                    current_points.push(point.clone());
                    continue;
                }
                if !current_points.is_empty() {
                    line_segments.push(FlounderTelemetryRenderSegment {
                        series_id: series.series_id.clone(),
                        points: std::mem::take(&mut current_points),
                    });
                }
                gap_labels.push(FlounderTelemetryGapLabel::from_point(
                    &series.series_id,
                    point,
                ));
            }
            if !current_points.is_empty() {
                line_segments.push(FlounderTelemetryRenderSegment {
                    series_id: series.series_id.clone(),
                    points: current_points,
                });
            }
        }
        gap_labels.sort_by(|left, right| {
            left.start_utc
                .cmp(&right.start_utc)
                .then(left.end_utc.cmp(&right.end_utc))
                .then(left.label.cmp(&right.label))
        });

        FlounderTelemetryRenderPlan {
            chart_id: self.chart_id.clone(),
            line_segments,
            gap_labels,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryRenderPlan {
    pub chart_id: String,
    pub line_segments: Vec<FlounderTelemetryRenderSegment>,
    pub gap_labels: Vec<FlounderTelemetryGapLabel>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryRenderSegment {
    pub series_id: String,
    pub points: Vec<FlounderTelemetryPoint>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlounderTelemetryGapLabel {
    pub start_utc: String,
    pub end_utc: String,
    pub reason: FlounderTelemetryMissingReason,
    pub label: String,
    #[serde(default)]
    pub affected_series_ids: Vec<String>,
}

impl FlounderTelemetryGapLabel {
    fn from_point(series_id: &str, point: &FlounderTelemetryPoint) -> Self {
        Self {
            start_utc: point.timestamp_utc.clone(),
            end_utc: point.timestamp_utc.clone(),
            reason: point.quality.missing_reason(),
            label: point
                .label
                .clone()
                .unwrap_or_else(|| point.quality.default_gap_label().to_string()),
            affected_series_ids: vec![series_id.to_string()],
        }
    }
}

impl From<&FlounderTelemetryMissingInterval> for FlounderTelemetryGapLabel {
    fn from(interval: &FlounderTelemetryMissingInterval) -> Self {
        Self {
            start_utc: interval.start_utc.clone(),
            end_utc: interval.end_utc.clone(),
            reason: interval.reason,
            label: interval.label.clone(),
            affected_series_ids: interval.affected_series_ids.clone(),
        }
    }
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

    pub fn is_observed(&self) -> bool {
        self.value.is_some() && self.quality == FlounderTelemetryPointQuality::Observed
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

impl FlounderTelemetryPointQuality {
    fn missing_reason(self) -> FlounderTelemetryMissingReason {
        match self {
            Self::Observed | Self::MissingSample => FlounderTelemetryMissingReason::NoSamples,
            Self::UnavailableCounter => FlounderTelemetryMissingReason::CounterUnavailable,
            Self::ServiceRestart => FlounderTelemetryMissingReason::ServiceStopped,
            Self::UnknownDevice => FlounderTelemetryMissingReason::DeviceUnknown,
        }
    }

    fn default_gap_label(self) -> &'static str {
        match self {
            Self::Observed | Self::MissingSample => "sample missing",
            Self::UnavailableCounter => "counter unavailable",
            Self::ServiceRestart => "service restarted",
            Self::UnknownDevice => "device unknown",
        }
    }
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

    #[test]
    fn serializes_product_neutral_chart_contract_for_web_and_grammateus() {
        let contract = FlounderTelemetryChartContract::new(
            "2026-07-09T20:03:00Z",
            FlounderTelemetryProducer::new("lab-appliance")
                .named("Lab Appliance")
                .with_component("telemetry-service"),
            vec![
                FlounderTelemetryAudience::WebDashboard,
                FlounderTelemetryAudience::GrammateusReport,
            ],
            FlounderTelemetryWindow {
                value: "ten_days".to_string(),
                label: "10 days".to_string(),
                start_utc: Some("2026-06-29T20:03:00Z".to_string()),
                end_utc: Some("2026-07-09T20:03:00Z".to_string()),
                cadence_seconds: Some(30),
                downsample_seconds: Some(600),
            },
            vec![FlounderTelemetryChart {
                chart_id: "throughput".to_string(),
                title: "Throughput".to_string(),
                layout: FlounderTelemetryChartLayout::LineWithGaps,
                x_axis: time_axis(),
                y_axis: FlounderTelemetryAxis {
                    label: "Rate".to_string(),
                    unit: FlounderTelemetryUnit::BytesPerSecond,
                },
                series: vec![FlounderTelemetrySeries {
                    series_id: "write_rate".to_string(),
                    label: "Write rate".to_string(),
                    role: FlounderTelemetrySeriesRole::Line,
                    unit: FlounderTelemetryUnit::BytesPerSecond,
                    device: None,
                    points: vec![FlounderTelemetryPoint::observed(
                        "2026-07-09T20:03:00Z",
                        1_048_576,
                    )],
                }],
                bands: Vec::new(),
                missing_intervals: Vec::new(),
                small_multiples: Vec::new(),
            }],
        );

        let encoded = serde_json::to_value(&contract).expect("contract serializes");

        assert_eq!(
            encoded["schema_version"],
            FLOUNDER_TELEMETRY_CHART_CONTRACT_SCHEMA_VERSION
        );
        assert_eq!(encoded["producer"]["product_id"], "lab-appliance");
        assert_eq!(encoded["producer"]["component_id"], "telemetry-service");
        assert_eq!(encoded["audiences"][0], "web_dashboard");
        assert_eq!(encoded["audiences"][1], "grammateus_report");
        assert_eq!(encoded["charts"][0]["chart_id"], "throughput");
        assert_eq!(
            encoded["charts"][0]["series"][0]["points"][0]["value"],
            1_048_576
        );
    }

    #[test]
    fn converts_appliance_contract_to_product_neutral_chart_contract() {
        let appliance = FlounderApplianceTelemetryContract::new(
            "2026-07-09T20:04:00Z",
            "dasobjectstore",
            FlounderTelemetryWindow {
                value: "one_hour".to_string(),
                label: "1 hour".to_string(),
                start_utc: None,
                end_utc: Some("2026-07-09T20:04:00Z".to_string()),
                cadence_seconds: Some(30),
                downsample_seconds: None,
            },
            vec![FlounderTelemetryChart {
                chart_id: "users".to_string(),
                title: "Users".to_string(),
                layout: FlounderTelemetryChartLayout::StepSummary,
                x_axis: time_axis(),
                y_axis: FlounderTelemetryAxis {
                    label: "Users".to_string(),
                    unit: FlounderTelemetryUnit::Count,
                },
                series: vec![FlounderTelemetrySeries {
                    series_id: "active_users".to_string(),
                    label: "Active users".to_string(),
                    role: FlounderTelemetrySeriesRole::Step,
                    unit: FlounderTelemetryUnit::Count,
                    device: None,
                    points: vec![FlounderTelemetryPoint::observed("2026-07-09T20:04:00Z", 4)],
                }],
                bands: Vec::new(),
                missing_intervals: Vec::new(),
                small_multiples: Vec::new(),
            }],
        );

        let chart_contract =
            appliance.into_chart_contract(vec![FlounderTelemetryAudience::ApiExport]);

        assert_eq!(
            chart_contract.schema_version,
            FLOUNDER_TELEMETRY_CHART_CONTRACT_SCHEMA_VERSION
        );
        assert_eq!(chart_contract.producer.product_id, "dasobjectstore");
        assert_eq!(
            chart_contract.audiences,
            vec![FlounderTelemetryAudience::ApiExport]
        );
        assert_eq!(
            chart_contract.charts[0].layout,
            FlounderTelemetryChartLayout::StepSummary
        );
    }

    #[test]
    fn render_plan_breaks_lines_at_missing_points_and_labels_gaps() {
        let chart = FlounderTelemetryChart {
            chart_id: "memory".to_string(),
            title: "Memory".to_string(),
            layout: FlounderTelemetryChartLayout::LineWithGaps,
            x_axis: time_axis(),
            y_axis: percent_axis("Memory"),
            series: vec![FlounderTelemetrySeries {
                series_id: "memory_used".to_string(),
                label: "Memory used".to_string(),
                role: FlounderTelemetrySeriesRole::Line,
                unit: FlounderTelemetryUnit::PercentBasisPoints,
                device: None,
                points: vec![
                    FlounderTelemetryPoint::observed("2026-07-09T19:50:00Z", 6000),
                    FlounderTelemetryPoint::observed("2026-07-09T19:50:30Z", 6100),
                    FlounderTelemetryPoint::missing(
                        "2026-07-09T19:51:00Z",
                        FlounderTelemetryPointQuality::ServiceRestart,
                        "telemetry service restarted",
                    ),
                    FlounderTelemetryPoint::observed("2026-07-09T19:51:30Z", 6200),
                    FlounderTelemetryPoint::observed("2026-07-09T19:52:00Z", 6300),
                    FlounderTelemetryPoint::missing(
                        "2026-07-09T19:52:30Z",
                        FlounderTelemetryPointQuality::UnavailableCounter,
                        "memory counter unavailable",
                    ),
                ],
            }],
            bands: Vec::new(),
            missing_intervals: vec![FlounderTelemetryMissingInterval {
                start_utc: "2026-07-09T19:53:00Z".to_string(),
                end_utc: "2026-07-09T19:54:00Z".to_string(),
                reason: FlounderTelemetryMissingReason::CollectionError,
                label: "collector error".to_string(),
                affected_series_ids: vec!["memory_used".to_string()],
            }],
            small_multiples: Vec::new(),
        };

        let render_plan = chart.render_plan();
        let encoded = serde_json::to_value(&render_plan).expect("render plan serializes");

        assert_eq!(render_plan.line_segments.len(), 2);
        assert_eq!(render_plan.line_segments[0].points.len(), 2);
        assert_eq!(
            render_plan.line_segments[0].points[1].timestamp_utc,
            "2026-07-09T19:50:30Z"
        );
        assert_eq!(render_plan.line_segments[1].points.len(), 2);
        assert_eq!(render_plan.gap_labels.len(), 3);
        assert_eq!(
            render_plan.gap_labels[0].reason,
            FlounderTelemetryMissingReason::ServiceStopped
        );
        assert_eq!(
            render_plan.gap_labels[1].reason,
            FlounderTelemetryMissingReason::CounterUnavailable
        );
        assert_eq!(
            render_plan.gap_labels[2].reason,
            FlounderTelemetryMissingReason::CollectionError
        );
        assert_eq!(encoded["line_segments"][0]["points"][0]["value"], 6000);
        assert_eq!(
            encoded["gap_labels"][0]["label"],
            "telemetry service restarted"
        );
        assert_eq!(
            encoded["gap_labels"][1]["affected_series_ids"][0],
            "memory_used"
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
