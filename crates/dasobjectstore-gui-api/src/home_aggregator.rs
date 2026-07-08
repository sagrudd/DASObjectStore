use crate::dashboard::{
    CapacitySummaryView, CreateObjectStoreAffordanceView, DasEnclosureCardView,
    DashboardHealthStateView, DashboardWarning, DriveCountSummaryView, EnclosureConnectionView,
    HealthSummaryView, HomeDashboardView, MemoryStressStateView, MemoryStressView,
    ObjectStoreCardView, SmartWarningView, SmartWarningsSummaryView, ThroughputDayView,
    ThroughputSummaryView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
};
use dasobjectstore_core::store::{ExportPolicy, MutabilityPolicy, PlacementStrategy};
use dasobjectstore_object_service::{default_store_registry_path, read_store_registry};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const DEFAULT_SSD_ROOT: &str = "/srv/dasobjectstore/ssd";
pub(crate) const DEFAULT_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";
const DEFAULT_THROUGHPUT_PATH: &str = "/var/lib/dasobjectstore/telemetry/throughput-7d.json";
const DEFAULT_SMART_WARNINGS_PATH: &str = "/var/lib/dasobjectstore/health/smart-warnings.json";
const DEFAULT_MEMINFO_PATH: &str = "/proc/meminfo";

#[derive(Clone, Debug)]
struct HomeDashboardAggregatorConfig {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    store_registry_path: PathBuf,
    throughput_path: PathBuf,
    smart_warnings_path: PathBuf,
    meminfo_path: PathBuf,
}

impl HomeDashboardAggregatorConfig {
    fn from_env() -> Self {
        Self {
            ssd_root: env_path("DASOBJECTSTORE_SSD_ROOT", DEFAULT_SSD_ROOT),
            hdd_root: env_path("DASOBJECTSTORE_HDD_ROOT", DEFAULT_HDD_ROOT),
            store_registry_path: default_store_registry_path(),
            throughput_path: env_path(
                "DASOBJECTSTORE_WEB_THROUGHPUT_PATH",
                DEFAULT_THROUGHPUT_PATH,
            ),
            smart_warnings_path: env_path(
                "DASOBJECTSTORE_WEB_SMART_WARNINGS_PATH",
                DEFAULT_SMART_WARNINGS_PATH,
            ),
            meminfo_path: env_path("DASOBJECTSTORE_WEB_MEMINFO_PATH", DEFAULT_MEMINFO_PATH),
        }
    }
}

pub(crate) fn live_home_dashboard() -> HomeDashboardView {
    build_home_dashboard(HomeDashboardAggregatorConfig::from_env())
}

fn build_home_dashboard(config: HomeDashboardAggregatorConfig) -> HomeDashboardView {
    let generated_at_utc = now_utc_string();
    let mut source_warnings = Vec::new();

    let ssd_capacity = capacity_for_root(&config.ssd_root);
    if !config.ssd_root.exists() {
        source_warnings.push(DashboardWarning::new(
            "ssd_root_missing",
            format!(
                "Managed SSD root is not present at {}.",
                config.ssd_root.display()
            ),
        ));
    } else if ssd_capacity.is_none() {
        source_warnings.push(DashboardWarning::new(
            "ssd_capacity_unavailable",
            format!(
                "The Web API could not measure managed SSD capacity at {}.",
                config.ssd_root.display()
            ),
        ));
    }

    let hdd_roots = discover_hdd_roots(&config.hdd_root, &mut source_warnings);
    let hdd_capacities = hdd_roots
        .iter()
        .filter_map(|root| capacity_for_root(root))
        .collect::<Vec<_>>();
    if hdd_roots
        .iter()
        .any(|root| capacity_for_root(root).is_none())
    {
        source_warnings.push(DashboardWarning::new(
            "hdd_capacity_partial",
            "One or more managed HDD roots could not be measured by the Web API.",
        ));
    }

    let all_capacities = ssd_capacity
        .iter()
        .chain(hdd_capacities.iter())
        .copied()
        .collect::<Vec<_>>();
    let capacity = capacity_summary(&all_capacities);
    let drive_count = drive_count_summary(ssd_capacity.is_some(), hdd_capacities.len());
    let mounted_enclosures = enclosure_cards(
        &config.hdd_root,
        &hdd_roots,
        &hdd_capacities,
        &generated_at_utc,
        &source_warnings,
    );
    let smart_warnings =
        read_smart_warnings(&config.smart_warnings_path).unwrap_or_else(|warning| {
            if config.smart_warnings_path.exists() {
                source_warnings.push(warning);
            }
            Vec::new()
        });
    let object_stores = registry_object_stores(&config.store_registry_path, &mut source_warnings);
    let memory_stress = memory_stress(&config.meminfo_path, &mut source_warnings);
    let throughput_7d = read_throughput_7d(&config.throughput_path).unwrap_or_else(|| {
        source_warnings.push(DashboardWarning::new(
            "throughput_telemetry_unavailable",
            "Seven-day throughput telemetry has not yet been written for the Web dashboard.",
        ));
        ThroughputSummaryView::bootstrap_fixture()
    });

    let warning_count = source_warnings.len() + smart_warnings.len();
    let health_state = if warning_count == 0 {
        DashboardHealthStateView::Healthy
    } else if hdd_capacities.is_empty() {
        DashboardHealthStateView::Degraded
    } else {
        DashboardHealthStateView::Watch
    };

    HomeDashboardView {
        schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
        generated_at_utc: generated_at_utc.clone(),
        health: HealthSummaryView {
            state: health_state,
            label: health_label(health_state, hdd_capacities.len(), object_stores.len())
                .to_string(),
            warning_count,
            critical_count: usize::from(matches!(health_state, DashboardHealthStateView::Critical)),
            action_count: source_warnings.len(),
            last_checked_at_utc: Some(generated_at_utc),
        },
        drives: drive_count,
        capacity,
        mounted_enclosures,
        throughput_7d,
        ingest: None,
        destage: None,
        memory_stress,
        smart_warnings: SmartWarningsSummaryView::from_warnings(smart_warnings),
        object_stores,
        create_object_store: CreateObjectStoreAffordanceView::admin_required(),
    }
}

pub(crate) fn env_path(name: &str, default: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

pub(crate) fn discover_hdd_roots(
    hdd_root: &Path,
    warnings: &mut Vec<DashboardWarning>,
) -> Vec<PathBuf> {
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            warnings.push(DashboardWarning::new(
                "hdd_root_missing",
                format!("Managed HDD root is not present at {}.", hdd_root.display()),
            ));
            return Vec::new();
        }
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "hdd_root_unreadable",
                format!(
                    "Managed HDD root {} could not be read: {error}.",
                    hdd_root.display()
                ),
            ));
            return Vec::new();
        }
    };

    let mut roots = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| is_managed_hdd_root(path))
        .collect::<Vec<_>>();
    roots.sort();

    if roots.is_empty() {
        warnings.push(DashboardWarning::new(
            "hdd_inventory_empty",
            "No managed HDD roots were detected for the Home dashboard.",
        ));
    }

    roots
}

fn is_managed_hdd_root(path: &Path) -> bool {
    let marker = path.join(".dasobjectstore").join("device.env");
    fs::read_to_string(marker)
        .map(|contents| contents.lines().any(|line| line.starts_with("role=hdd:")))
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FilesystemCapacity {
    pub(crate) total_bytes: u64,
    pub(crate) available_bytes: u64,
}

#[cfg(unix)]
pub(crate) fn capacity_for_root(path: &Path) -> Option<FilesystemCapacity> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let encoded = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(encoded.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let fragment_size = stat.f_frsize as u64;
    Some(FilesystemCapacity {
        total_bytes: (stat.f_blocks as u64).saturating_mul(fragment_size),
        available_bytes: (stat.f_bavail as u64).saturating_mul(fragment_size),
    })
}

#[cfg(not(unix))]
pub(crate) fn capacity_for_root(_path: &Path) -> Option<FilesystemCapacity> {
    None
}

pub(crate) fn capacity_summary(capacities: &[FilesystemCapacity]) -> CapacitySummaryView {
    let total = capacities
        .iter()
        .map(|capacity| capacity.total_bytes)
        .sum::<u64>();
    let available = capacities
        .iter()
        .map(|capacity| capacity.available_bytes)
        .sum::<u64>();
    let used = total.saturating_sub(available);

    CapacitySummaryView {
        total_tib: format_tib(total),
        used_tib: format_tib(used),
        free_tib: format_tib(available),
        used_percent_basis_points: percent_basis_points(used, total),
    }
}

pub(crate) fn drive_count_summary(ssd_present: bool, hdd_count: usize) -> DriveCountSummaryView {
    let mounted = usize::from(ssd_present) + hdd_count;
    DriveCountSummaryView {
        total: mounted,
        mounted,
        healthy: mounted,
        watch: 0,
        suspect: 0,
        failed: 0,
    }
}

fn enclosure_cards(
    hdd_root: &Path,
    hdd_roots: &[PathBuf],
    hdd_capacities: &[FilesystemCapacity],
    generated_at_utc: &str,
    source_warnings: &[DashboardWarning],
) -> Vec<DasEnclosureCardView> {
    if hdd_roots.is_empty() {
        return Vec::new();
    }

    vec![DasEnclosureCardView {
        enclosure_id: "managed-das-hdd-roots".to_string(),
        display_name: "Managed DAS HDD roots".to_string(),
        mount_path: hdd_root.display().to_string(),
        connection: EnclosureConnectionView {
            bus: "managed-root".to_string(),
            protocol: "filesystem".to_string(),
            link_speed: "host reported".to_string(),
        },
        health: if source_warnings.is_empty() {
            DashboardHealthStateView::Healthy
        } else {
            DashboardHealthStateView::Watch
        },
        drive_count: drive_count_summary(false, hdd_roots.len()),
        capacity: capacity_summary(hdd_capacities),
        last_seen_at_utc: generated_at_utc.to_string(),
        warnings: source_warnings.to_vec(),
    }]
}

fn registry_object_stores(
    registry_path: &Path,
    warnings: &mut Vec<DashboardWarning>,
) -> Vec<ObjectStoreCardView> {
    let definitions = match read_store_registry(registry_path) {
        Ok(definitions) => definitions,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "store_registry_unreadable",
                format!(
                    "ObjectStore registry {} could not be read: {error}.",
                    registry_path.display()
                ),
            ));
            return Vec::new();
        }
    };

    definitions
        .into_iter()
        .map(|definition| {
            let policy = definition.policy;
            ObjectStoreCardView {
                store_id: definition.store_id.to_string(),
                display_name: definition.store_id.to_string(),
                store_class: policy.class.name().to_string(),
                object_type: "naive".to_string(),
                health: DashboardHealthStateView::Healthy,
                required_copies: policy.copies,
                object_count: 0,
                capacity: capacity_summary(&[]),
                placement_policy: placement_strategy_label(policy.placement_strategy).to_string(),
                endpoint_export_mode: export_policy_label(policy.export_policy).to_string(),
                writer_group: definition.writer_group,
                public: false,
                writeable: policy.mutability_policy == MutabilityPolicy::Mutable
                    || policy.export_policy != ExportPolicy::Disabled,
                created_at_utc: "registry-managed".to_string(),
                last_ingested_at_utc: None,
                warnings: vec![DashboardWarning::new(
                    "store_usage_pending",
                    "Object counts and per-store used capacity require daemon metadata aggregation.",
                )],
            }
        })
        .collect()
}

fn placement_strategy_label(strategy: PlacementStrategy) -> &'static str {
    match strategy {
        PlacementStrategy::WeightedHealthCapacityPerformance => {
            "weighted_health_capacity_performance"
        }
    }
}

fn export_policy_label(policy: ExportPolicy) -> &'static str {
    match policy {
        ExportPolicy::S3 => "s3_bucket",
        ExportPolicy::ReadOnlyFileExport => "read_only_file_export",
        ExportPolicy::Disabled => "disabled",
    }
}

fn memory_stress(path: &Path, warnings: &mut Vec<DashboardWarning>) -> MemoryStressView {
    let meminfo = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "memory_telemetry_unavailable",
                format!(
                    "Memory telemetry could not be read from {}: {error}.",
                    path.display()
                ),
            ));
            return MemoryStressView {
                state: MemoryStressStateView::Elevated,
                pressure_percent: 0,
                swap_used_percent: 0,
                page_cache_tib: "0.0".to_string(),
                warning: Some(DashboardWarning::new(
                    "memory_telemetry_unavailable",
                    "Memory telemetry could not be read by the Web dashboard.",
                )),
            };
        }
    };

    let total = meminfo_kib(&meminfo, "MemTotal").unwrap_or(0);
    let available = meminfo_kib(&meminfo, "MemAvailable").unwrap_or(0);
    let cached = meminfo_kib(&meminfo, "Cached").unwrap_or(0);
    let swap_total = meminfo_kib(&meminfo, "SwapTotal").unwrap_or(0);
    let swap_free = meminfo_kib(&meminfo, "SwapFree").unwrap_or(0);
    let pressure_percent = percent_u8(total.saturating_sub(available), total);
    let swap_used_percent = percent_u8(swap_total.saturating_sub(swap_free), swap_total);
    let state = match pressure_percent {
        0..=69 => MemoryStressStateView::Nominal,
        70..=84 => MemoryStressStateView::Elevated,
        85..=94 => MemoryStressStateView::High,
        _ => MemoryStressStateView::Critical,
    };
    let warning = matches!(
        state,
        MemoryStressStateView::Elevated
            | MemoryStressStateView::High
            | MemoryStressStateView::Critical
    )
    .then(|| {
        DashboardWarning::new(
            "memory_pressure",
            format!("Memory pressure is {pressure_percent}% on this appliance."),
        )
    });

    MemoryStressView {
        state,
        pressure_percent,
        swap_used_percent,
        page_cache_tib: format_tib(cached.saturating_mul(1024)),
        warning,
    }
}

fn meminfo_kib(contents: &str, key: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        if name != key {
            return None;
        }
        rest.split_whitespace().next()?.parse::<u64>().ok()
    })
}

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

fn read_throughput_7d(path: &Path) -> Option<ThroughputSummaryView> {
    let contents = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<ThroughputJson>(&contents).ok()?;
    Some(ThroughputSummaryView {
        window_days: parsed.window_days,
        read_tib: format_tib(parsed.read_bytes),
        written_tib: format_tib(parsed.written_bytes),
        ingest_tib: format_tib(parsed.ingest_bytes),
        avg_read_mib_s: mib_per_second(parsed.avg_read_bytes_per_second),
        avg_write_mib_s: mib_per_second(parsed.avg_write_bytes_per_second),
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

fn read_smart_warnings(path: &Path) -> Result<Vec<SmartWarningView>, DashboardWarning> {
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

fn health_label(
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

fn percent_basis_points(used: u64, total: u64) -> u16 {
    if total == 0 {
        return 0;
    }
    ((u128::from(used) * 10_000) / u128::from(total)).min(u128::from(u16::MAX)) as u16
}

fn percent_u8(used: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    ((u128::from(used) * 100) / u128::from(total)).min(100) as u8
}

fn format_tib(bytes: u64) -> String {
    const TIB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;
    format!("{:.1}", bytes as f64 / TIB)
}

fn mib_per_second(bytes_per_second: u64) -> u32 {
    (bytes_per_second / (1024 * 1024)).min(u64::from(u32::MAX)) as u32
}

pub(crate) fn now_utc_string() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|output| output.status.success().then_some(output.stdout))
        .and_then(|stdout| String::from_utf8(stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            format!("unix:{seconds}")
        })
}

#[cfg(test)]
mod tests {
    use super::{build_home_dashboard, HomeDashboardAggregatorConfig};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn home_aggregator_uses_managed_roots_registry_memory_and_throughput() {
        let root = temp_root("home-aggregator-live");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let disk_a = hdd_root.join("qnap-1057");
        fs::create_dir_all(ssd_root.join(".dasobjectstore")).expect("ssd root");
        fs::create_dir_all(disk_a.join(".dasobjectstore")).expect("hdd root");
        fs::write(
            disk_a.join(".dasobjectstore/device.env"),
            "role=hdd:qnap-1057\n",
        )
        .expect("hdd marker");
        let registry_path = root.join("stores.json");
        let store = StoreServiceDefinition {
            store_id: StoreId::new("zymo").expect("store id"),
            policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
            bucket_name: Some("dos-zymo".to_string()),
            writer_group: Some("bioinformatics".to_string()),
        };
        fs::write(
            &registry_path,
            serde_json::to_string_pretty(&vec![store]).expect("registry json"),
        )
        .expect("registry write");
        let meminfo_path = root.join("meminfo");
        fs::write(
            &meminfo_path,
            "MemTotal:       1000000 kB\nMemAvailable:    750000 kB\nCached:          250000 kB\nSwapTotal:            0 kB\nSwapFree:             0 kB\n",
        )
        .expect("meminfo");
        let throughput_path = root.join("throughput.json");
        fs::write(
            &throughput_path,
            r#"{"window_days":7,"read_bytes":1099511627776,"written_bytes":2199023255552,"ingest_bytes":3298534883328,"avg_read_bytes_per_second":104857600,"avg_write_bytes_per_second":209715200,"daily":[{"date":"2026-07-08","read_bytes":1099511627776,"written_bytes":2199023255552,"ingest_bytes":3298534883328}]}"#,
        )
        .expect("throughput");

        let view = build_home_dashboard(HomeDashboardAggregatorConfig {
            ssd_root,
            hdd_root,
            store_registry_path: registry_path,
            throughput_path,
            smart_warnings_path: root.join("missing-smart.json"),
            meminfo_path,
        });

        assert_eq!(view.health.label, "Live inventory healthy");
        assert_eq!(view.drives.mounted, 2);
        assert_eq!(view.mounted_enclosures.len(), 1);
        assert_eq!(view.object_stores.len(), 1);
        assert_eq!(view.object_stores[0].store_id, "zymo");
        assert_eq!(
            view.object_stores[0].writer_group.as_deref(),
            Some("bioinformatics")
        );
        assert_eq!(view.memory_stress.pressure_percent, 25);
        assert_eq!(view.throughput_7d.avg_write_mib_s, 200);
        assert_eq!(view.throughput_7d.daily.len(), 1);
    }

    #[test]
    fn home_aggregator_reports_missing_managed_storage_without_bootstrap_fixture() {
        let root = temp_root("home-aggregator-missing");

        let view = build_home_dashboard(HomeDashboardAggregatorConfig {
            ssd_root: root.join("missing-ssd"),
            hdd_root: root.join("missing-hdd"),
            store_registry_path: root.join("missing-stores.json"),
            throughput_path: root.join("missing-throughput.json"),
            smart_warnings_path: root.join("missing-smart.json"),
            meminfo_path: root.join("missing-meminfo"),
        });

        assert_eq!(
            view.health.state,
            crate::dashboard::DashboardHealthStateView::Degraded
        );
        assert_eq!(view.health.label, "Managed storage unavailable");
        assert_ne!(view.health.label, "Inventory pending");
        assert_eq!(view.drives.total, 0);
        assert!(view.health.warning_count >= 3);
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
