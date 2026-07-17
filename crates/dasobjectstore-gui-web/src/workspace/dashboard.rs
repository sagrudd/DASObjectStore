use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardMetric {
    pub label: String,
    pub value: String,
    pub detail: String,
    pub state: String,
}

impl DashboardMetric {
    pub(super) fn new(
        label: &str,
        value: impl Into<String>,
        detail: impl Into<String>,
        state: &str,
    ) -> Self {
        Self {
            label: label.to_string(),
            value: value.into(),
            detail: detail.into(),
            state: state.to_string(),
        }
    }
}

pub fn home_dashboard_metrics(view: &HomeDashboardResponse) -> Vec<DashboardMetric> {
    vec![
        DashboardMetric::new(
            "Drives",
            view.drives.total.to_string(),
            format!(
                "{} mounted; {} healthy; {} watch; {} suspect; {} failed",
                view.drives.mounted,
                view.drives.healthy,
                view.drives.watch,
                view.drives.suspect,
                view.drives.failed
            ),
            &view.health.label,
        ),
        DashboardMetric::new(
            "DAS enclosures",
            format!("{} mounted", view.mounted_enclosures.len()),
            "Supported enclosure inventory from daemon dashboard API",
            &view.health.label,
        ),
        DashboardMetric::new(
            "Capacity",
            format!("{} TiB free", view.capacity.free_tib),
            format!(
                "{} TiB used of {} TiB total",
                view.capacity.used_tib, view.capacity.total_tib
            ),
            &format!(
                "{:.1}% used",
                f64::from(view.capacity.used_percent_basis_points) / 100.0
            ),
        ),
        DashboardMetric::new(
            "Throughput",
            format!("{} TiB written", view.throughput_7d.written_tib),
            format!(
                "Physical disk IO: {} MiB/s write avg; {} MiB/s read avg",
                view.throughput_7d.avg_write_mib_s, view.throughput_7d.avg_read_mib_s
            ),
            &view.telemetry_window.selected_label,
        ),
        DashboardMetric::new(
            "Disk IO",
            if view.disk_io.available {
                format!("{} MiB/s write", view.disk_io.write_mib_s)
            } else {
                "Unavailable".to_string()
            },
            if view.disk_io.available {
                format!(
                    "{} MiB/s read; {} write ops/s; {} read ops/s",
                    view.disk_io.read_mib_s, view.disk_io.write_ops_s, view.disk_io.read_ops_s
                )
            } else {
                view.disk_io
                    .message
                    .clone()
                    .unwrap_or_else(|| "Disk IO telemetry is not available yet.".to_string())
            },
            &view.disk_io.state,
        ),
        DashboardMetric::new(
            "CPU",
            view.cpu_usage
                .usage_percent
                .map(|percent| format!("{percent}%"))
                .unwrap_or_else(|| "Unavailable".to_string()),
            if view.cpu_usage.available {
                format!(
                    "load {}; {} logical core(s)",
                    view.cpu_usage
                        .load_average_1m
                        .as_deref()
                        .unwrap_or("unknown"),
                    view.cpu_usage.logical_core_count.unwrap_or(0)
                )
            } else {
                view.cpu_usage
                    .message
                    .clone()
                    .unwrap_or_else(|| "CPU telemetry is not available yet.".to_string())
            },
            &view.cpu_usage.state,
        ),
        DashboardMetric::new(
            "Logged-in users",
            if view.active_users.available {
                view.active_users.distinct_logged_in_users.to_string()
            } else {
                "Unavailable".to_string()
            },
            if view.active_users.available {
                format!(
                    "{} active session(s); {} admin; {} remote",
                    view.active_users.active_sessions,
                    view.active_users.administrator_sessions,
                    view.active_users.remote_agent_sessions
                )
            } else {
                view.active_users
                    .message
                    .clone()
                    .unwrap_or_else(|| "Session telemetry is not available yet.".to_string())
            },
            &view.active_users.state,
        ),
        DashboardMetric::new(
            "S3 service",
            if view.object_service.remote_ready {
                view.object_service
                    .remote_url
                    .clone()
                    .unwrap_or_else(|| format!("port {}", view.object_service.port))
            } else if view.object_service.active {
                "Loopback only".to_string()
            } else {
                "Offline".to_string()
            },
            format!(
                "bind {}:{}; {}",
                view.object_service.bind_address,
                view.object_service.port,
                view.object_service
                    .service_state
                    .as_deref()
                    .unwrap_or("service state unavailable")
            ),
            if view.object_service.remote_ready {
                "remote ready"
            } else if view.object_service.active {
                "loopback"
            } else {
                "offline"
            },
        ),
        DashboardMetric::new(
            "Memory stress",
            format!("{}%", view.memory_stress.pressure_percent),
            format!(
                "{}% swap; {} TiB page cache",
                view.memory_stress.swap_used_percent, view.memory_stress.page_cache_tib
            ),
            &view.memory_stress.state,
        ),
        DashboardMetric::new(
            "SMART warnings",
            view.smart_warnings.warning_count.to_string(),
            format!(
                "{} affected drive(s)",
                view.smart_warnings.affected_drive_count
            ),
            if view.smart_warnings.warning_count == 0 {
                "clear"
            } else {
                "review"
            },
        ),
        DashboardMetric::new(
            "ObjectStores",
            view.object_stores.len().to_string(),
            "Registered object stores visible to this appliance",
            &view.health.label,
        ),
    ]
}

#[derive(Clone, Debug, PartialEq)]
pub struct HomeThroughputChartPoint {
    pub date: String,
    pub ingest_tib: Option<f64>,
    pub x: f64,
    pub y: Option<f64>,
}

pub fn home_throughput_chart_points(view: &HomeDashboardResponse) -> Vec<HomeThroughputChartPoint> {
    throughput_chart_points(&view.throughput_7d.daily)
}

pub fn home_throughput_source_label(source: &str) -> &'static str {
    match source {
        "daemon_disk_io" => "Daemon telemetry",
        "legacy_file" => "Legacy file telemetry",
        "bootstrap_fixture" => "Bootstrap fixture",
        _ => "Unavailable telemetry",
    }
}

pub fn home_throughput_source_class(source: &str) -> &'static str {
    match source {
        "daemon_disk_io" => "dos-telemetry-source-daemon",
        "legacy_file" => "dos-telemetry-source-legacy",
        "bootstrap_fixture" => "dos-telemetry-source-fixture",
        _ => "dos-telemetry-source-unavailable",
    }
}

pub fn home_throughput_chart_polyline(points: &[HomeThroughputChartPoint]) -> String {
    points
        .iter()
        .filter_map(|point| point.y.map(|y| format!("{:.1},{:.1}", point.x, y)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn home_throughput_chart_segments(points: &[HomeThroughputChartPoint]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    for point in points {
        match point.y {
            Some(y) => current.push(format!("{:.1},{:.1}", point.x, y)),
            None if !current.is_empty() => {
                segments.push(current.join(" "));
                current.clear();
            }
            None => {}
        }
    }
    if !current.is_empty() {
        segments.push(current.join(" "));
    }
    segments
}

pub fn home_throughput_chart_max_tib(points: &[HomeThroughputChartPoint]) -> String {
    let max_tib = points
        .iter()
        .filter_map(|point| point.ingest_tib)
        .fold(0.0_f64, f64::max);
    if max_tib > 0.0 && max_tib < 0.1 {
        format!("{:.0} GiB", max_tib * 1024.0)
    } else if max_tib < 10.0 {
        format!("{max_tib:.1} TiB")
    } else {
        format!("{max_tib:.0} TiB")
    }
}

pub(super) fn throughput_chart_points(
    days: &[ThroughputDayResponse],
) -> Vec<HomeThroughputChartPoint> {
    if days.is_empty() {
        return Vec::new();
    }

    let observed_max_tib = days
        .iter()
        .filter_map(|day| parse_tib_value(&day.ingest_tib))
        .fold(0.0_f64, f64::max);
    let chart_scale_tib = if observed_max_tib > 0.0 {
        observed_max_tib
    } else {
        1.0
    };
    let span = days.len().saturating_sub(1).max(1) as f64;
    days.into_iter()
        .enumerate()
        .map(|(index, day)| {
            let ingest_tib = parse_tib_value(&day.ingest_tib);
            let x = HOME_THROUGHPUT_CHART_LEFT
                + ((HOME_THROUGHPUT_CHART_RIGHT - HOME_THROUGHPUT_CHART_LEFT)
                    * (index as f64 / span));
            let y = ingest_tib.map(|ingest_tib| {
                HOME_THROUGHPUT_CHART_BOTTOM
                    - ((HOME_THROUGHPUT_CHART_BOTTOM - HOME_THROUGHPUT_CHART_TOP)
                        * (ingest_tib / chart_scale_tib))
            });
            HomeThroughputChartPoint {
                date: day.date.clone(),
                ingest_tib,
                x,
                y,
            }
        })
        .collect()
}

pub(super) fn parse_tib_value(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let numeric = value
        .strip_suffix("TiB")
        .unwrap_or(value)
        .trim()
        .parse::<f64>()
        .ok()?;
    numeric.is_finite().then_some(numeric.max(0.0))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardAttentionItem {
    pub title: String,
    pub detail: String,
    pub state: String,
}

impl DashboardAttentionItem {
    fn new(title: impl Into<String>, detail: impl Into<String>, state: &str) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            state: state.to_string(),
        }
    }
}

pub fn home_dashboard_attention(view: &HomeDashboardResponse) -> Vec<DashboardAttentionItem> {
    let mut items = Vec::new();
    if view.health.action_count > 0
        || view.health.warning_count > 0
        || view.health.critical_count > 0
    {
        items.push(DashboardAttentionItem::new(
            "Appliance attention",
            format!(
                "{} required action(s), {} warning(s), {} critical condition(s)",
                view.health.action_count, view.health.warning_count, view.health.critical_count
            ),
            &view.health.state,
        ));
    }
    if view.drives.failed > 0 || view.drives.suspect > 0 {
        items.push(DashboardAttentionItem::new(
            "Drive health",
            format!(
                "{} failed drive(s), {} suspect drive(s), {} watch drive(s)",
                view.drives.failed, view.drives.suspect, view.drives.watch
            ),
            if view.drives.failed > 0 {
                "critical"
            } else {
                "warning"
            },
        ));
    }
    if let Some(capacity_item) = capacity_attention_item(view) {
        items.push(capacity_item);
    }
    if let Some(ingest) = &view.ingest {
        if ingest.failed_jobs > 0
            || ingest.active_jobs > 0
            || ingest.queued_jobs > 0
            || ingest.pressure != "normal"
            || !ingest.warnings.is_empty()
        {
            let detail = ingest
                .warnings
                .first()
                .map(|warning| warning.message.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{} queued, {} active, {} failed ingest job(s).",
                        ingest.queued_jobs, ingest.active_jobs, ingest.failed_jobs
                    )
                });
            items.push(DashboardAttentionItem::new(
                "Ingest queue",
                detail,
                queue_attention_state(&ingest.pressure, ingest.failed_jobs),
            ));
        }
    }
    if let Some(destage) = &view.destage {
        if destage.pending_objects > 0
            || destage.copying_objects > 0
            || !destage.warnings.is_empty()
        {
            let detail = destage
                .warnings
                .first()
                .map(|warning| warning.message.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{} pending, {} copying, {} verified destage object(s).",
                        destage.pending_objects, destage.copying_objects, destage.verified_objects
                    )
                });
            items.push(DashboardAttentionItem::new(
                "Destage queue",
                detail,
                if destage.warnings.is_empty() {
                    "watch"
                } else {
                    "warning"
                },
            ));
        }
    }
    if let Some(warning) = &view.memory_stress.warning {
        items.push(DashboardAttentionItem::new(
            "Memory stress",
            warning.message.clone(),
            &view.memory_stress.state,
        ));
    }
    if !view.object_service.remote_ready {
        items.push(DashboardAttentionItem::new(
            "S3 object service",
            view.object_service.message.clone().unwrap_or_else(|| {
                format!(
                    "S3-compatible object service is not remote-ready on {}:{}.",
                    view.object_service.bind_address, view.object_service.port
                )
            }),
            if view.object_service.active {
                "warning"
            } else {
                "critical"
            },
        ));
    }
    for enclosure in view.mounted_enclosures.iter().filter(|enclosure| {
        !enclosure.warnings.is_empty() || !matches_health_clear(&enclosure.health)
    }) {
        let state = if !enclosure.warnings.is_empty() && matches_health_clear(&enclosure.health) {
            "warning"
        } else {
            enclosure.health.as_str()
        };
        let detail = enclosure
            .warnings
            .first()
            .map(|warning| warning.message.clone())
            .unwrap_or_else(|| {
                format!(
                    "{} reports {} health at {}.",
                    enclosure.display_name, enclosure.health, enclosure.mount_path
                )
            });
        items.push(DashboardAttentionItem::new(
            format!("Enclosure {}", enclosure.display_name),
            detail,
            state,
        ));
    }
    for warning in view.smart_warnings.warnings.iter().take(3) {
        items.push(DashboardAttentionItem::new(
            format!("SMART {}", warning.drive_id),
            format!("{}: {}", warning.attribute, warning.message),
            &warning.severity,
        ));
    }
    for store in view.object_stores.iter().filter(|store| {
        !store.warnings.is_empty()
            || !matches_health_clear(&store.health)
            || store.endpoint_export_mode.is_none()
    }) {
        let state = if (!store.warnings.is_empty() || store.endpoint_export_mode.is_none())
            && matches_health_clear(&store.health)
        {
            "warning"
        } else {
            store.health.as_str()
        };
        let detail = store
            .warnings
            .first()
            .map(|warning| warning.message.clone())
            .unwrap_or_else(|| {
                if store.endpoint_export_mode.is_none() {
                    format!(
                        "{} has no daemon-reported object-service export mode.",
                        store.display_name
                    )
                } else {
                    format!(
                        "{} reports {} health with {} object(s).",
                        store.display_name, store.health, store.object_count
                    )
                }
            });
        items.push(DashboardAttentionItem::new(
            format!("ObjectStore {}", store.display_name),
            detail,
            state,
        ));
    }
    if view.mounted_enclosures.is_empty() {
        items.push(DashboardAttentionItem::new(
            "Enclosure inventory",
            "The daemon dashboard payload did not report any mounted supported DAS enclosures.",
            "watch",
        ));
    }
    if view.object_stores.is_empty() {
        items.push(DashboardAttentionItem::new(
            "ObjectStore inventory",
            "The daemon dashboard payload did not report any object stores visible to this user.",
            "watch",
        ));
    }
    if items.is_empty() {
        items.push(DashboardAttentionItem::new(
            "No operator attention required",
            "The daemon dashboard payload reports clear drive, ingest, destage, capacity, memory, SMART, enclosure, and ObjectStore state.",
            "clear",
        ));
    }
    items
}

pub(super) fn capacity_attention_item(
    view: &HomeDashboardResponse,
) -> Option<DashboardAttentionItem> {
    if view.capacity.total_tib == "0.0" && view.health.action_count > 0 {
        return Some(DashboardAttentionItem::new(
            "Capacity telemetry",
            "The daemon dashboard payload has not reported usable capacity yet.",
            "watch",
        ));
    }

    match view.capacity.used_percent_basis_points {
        9000..=u16::MAX => Some(DashboardAttentionItem::new(
            "Capacity pressure",
            format!(
                "{} TiB used of {} TiB total; {} TiB remains free.",
                view.capacity.used_tib, view.capacity.total_tib, view.capacity.free_tib
            ),
            "critical",
        )),
        8000..=8999 => Some(DashboardAttentionItem::new(
            "Capacity pressure",
            format!(
                "{} TiB used of {} TiB total; {} TiB remains free.",
                view.capacity.used_tib, view.capacity.total_tib, view.capacity.free_tib
            ),
            "warning",
        )),
        _ => None,
    }
}

pub(super) fn matches_health_clear(health: &str) -> bool {
    matches!(health, "healthy" | "nominal" | "clear")
}

pub(super) fn queue_attention_state(pressure: &str, failed_jobs: usize) -> &'static str {
    if failed_jobs > 0 || pressure == "critical" {
        "critical"
    } else if pressure == "normal" {
        "watch"
    } else {
        "warning"
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityCategorySummary {
    pub kind: String,
    pub label: String,
    pub description: String,
    pub active_count: usize,
    pub waiting_count: usize,
    pub failed_count: usize,
    pub complete_count: usize,
    pub state: String,
}

pub fn activity_category_summaries(
    view: &ActivityWorkspaceResponse,
) -> Vec<ActivityCategorySummary> {
    view.categories
        .iter()
        .map(|category| {
            let matching_tasks = view.tasks.iter().filter(|task| task.kind == category.kind);
            let mut active_count = 0;
            let mut waiting_count = 0;
            let mut failed_count = 0;
            let mut complete_count = 0;

            for task in matching_tasks {
                match task.state.as_str() {
                    "failed" => failed_count += 1,
                    "complete" | "cancelled" => complete_count += 1,
                    "queued" | "waiting" => waiting_count += 1,
                    _ => active_count += 1,
                }
            }

            let state = if failed_count > 0 {
                "critical"
            } else if active_count > 0 {
                "running"
            } else if waiting_count > 0 {
                "waiting"
            } else {
                "idle"
            };

            ActivityCategorySummary {
                kind: category.kind.clone(),
                label: category.label.clone(),
                description: category.description.clone(),
                active_count,
                waiting_count,
                failed_count,
                complete_count,
                state: state.to_string(),
            }
        })
        .collect()
}

pub fn activity_queue_summary(view: &ActivityWorkspaceResponse) -> Vec<DashboardMetric> {
    let ingest = view
        .ingest
        .as_ref()
        .map(|ingest| {
            DashboardMetric::new(
                "Ingest",
                format!("{} active", ingest.active_jobs),
                format!(
                    "{} queued; {} failed; pressure {}",
                    ingest.queued_jobs, ingest.failed_jobs, ingest.pressure
                ),
                queue_attention_state(&ingest.pressure, ingest.failed_jobs),
            )
        })
        .unwrap_or_else(|| {
            DashboardMetric::new(
                "Ingest",
                "No queue",
                "No daemon ingest queue payload has been reported.",
                "idle",
            )
        });
    let destage = view
        .destage
        .as_ref()
        .map(|destage| {
            DashboardMetric::new(
                "Destage",
                format!("{} copying", destage.copying_objects),
                format!(
                    "{} pending; {} verified",
                    destage.pending_objects, destage.verified_objects
                ),
                if destage.copying_objects > 0 {
                    "running"
                } else if destage.pending_objects > 0 {
                    "waiting"
                } else {
                    "idle"
                },
            )
        })
        .unwrap_or_else(|| {
            DashboardMetric::new(
                "Destage",
                "No queue",
                "No daemon destage queue payload has been reported.",
                "idle",
            )
        });

    vec![ingest, destage]
}
