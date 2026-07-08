use crate::dashboard::{DashboardSeverity, DashboardWarning};
use serde::{Deserialize, Serialize};

pub const REDESIGN_DASHBOARD_SCHEMA_VERSION: &str = "dasobjectstore.web_redesign.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HomeDashboardView {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub health: HealthSummaryView,
    pub drives: DriveCountSummaryView,
    pub capacity: CapacitySummaryView,
    pub mounted_enclosures: Vec<DasEnclosureCardView>,
    pub throughput_7d: ThroughputSummaryView,
    pub memory_stress: MemoryStressView,
    pub smart_warnings: SmartWarningsSummaryView,
    pub object_stores: Vec<ObjectStoreCardView>,
    pub create_object_store: CreateObjectStoreAffordanceView,
}

impl HomeDashboardView {
    pub fn bootstrap_fixture() -> Self {
        let enclosures = bootstrap_enclosure_cards();
        let object_stores = bootstrap_object_store_cards();
        let smart_warnings = SmartWarningsSummaryView::from_warnings(vec![SmartWarningView {
            drive_id: "das-enc-a-slot-04".to_string(),
            enclosure_id: Some("das-enc-a".to_string()),
            severity: DashboardSeverity::Warning,
            attribute: "reallocated_sector_count".to_string(),
            message: "SMART reallocation count is above the watch threshold.".to_string(),
            observed_at_utc: "2026-07-08T07:00:00Z".to_string(),
        }]);

        Self {
            schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
            generated_at_utc: "2026-07-08T08:00:00Z".to_string(),
            health: HealthSummaryView {
                state: DashboardHealthStateView::Watch,
                label: "Watch".to_string(),
                warning_count: 2,
                critical_count: 0,
                action_count: 1,
                last_checked_at_utc: Some("2026-07-08T07:45:00Z".to_string()),
            },
            drives: DriveCountSummaryView {
                total: 18,
                mounted: 18,
                healthy: 16,
                watch: 2,
                suspect: 0,
                failed: 0,
            },
            capacity: CapacitySummaryView {
                total_tib: "174.6".to_string(),
                used_tib: "91.2".to_string(),
                free_tib: "83.4".to_string(),
                used_percent_basis_points: 5223,
            },
            mounted_enclosures: enclosures,
            throughput_7d: ThroughputSummaryView::bootstrap_fixture(),
            memory_stress: MemoryStressView {
                state: MemoryStressStateView::Nominal,
                pressure_percent: 34,
                swap_used_percent: 0,
                page_cache_tib: "0.8".to_string(),
                warning: None,
            },
            smart_warnings,
            object_stores,
            create_object_store: CreateObjectStoreAffordanceView::enabled(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthSummaryView {
    pub state: DashboardHealthStateView,
    pub label: String,
    pub warning_count: usize,
    pub critical_count: usize,
    pub action_count: usize,
    pub last_checked_at_utc: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardHealthStateView {
    Healthy,
    Watch,
    Degraded,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStressStateView {
    Nominal,
    Elevated,
    High,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveCountSummaryView {
    pub total: usize,
    pub mounted: usize,
    pub healthy: usize,
    pub watch: usize,
    pub suspect: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacitySummaryView {
    pub total_tib: String,
    pub used_tib: String,
    pub free_tib: String,
    pub used_percent_basis_points: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThroughputSummaryView {
    pub window_days: u8,
    pub read_tib: String,
    pub written_tib: String,
    pub ingest_tib: String,
    pub avg_read_mib_s: u32,
    pub avg_write_mib_s: u32,
    pub daily: Vec<ThroughputDayView>,
}

impl ThroughputSummaryView {
    pub fn bootstrap_fixture() -> Self {
        Self {
            window_days: 7,
            read_tib: "18.4".to_string(),
            written_tib: "11.7".to_string(),
            ingest_tib: "9.3".to_string(),
            avg_read_mib_s: 31,
            avg_write_mib_s: 20,
            daily: vec![
                ThroughputDayView::new("2026-07-02", "2.4", "1.2", "1.0"),
                ThroughputDayView::new("2026-07-03", "2.0", "1.6", "1.3"),
                ThroughputDayView::new("2026-07-04", "2.7", "1.4", "1.2"),
                ThroughputDayView::new("2026-07-05", "3.3", "2.1", "1.8"),
                ThroughputDayView::new("2026-07-06", "2.9", "1.9", "1.5"),
                ThroughputDayView::new("2026-07-07", "2.5", "1.7", "1.4"),
                ThroughputDayView::new("2026-07-08", "2.6", "1.8", "1.1"),
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThroughputDayView {
    pub date: String,
    pub read_tib: String,
    pub written_tib: String,
    pub ingest_tib: String,
}

impl ThroughputDayView {
    fn new(date: &str, read_tib: &str, written_tib: &str, ingest_tib: &str) -> Self {
        Self {
            date: date.to_string(),
            read_tib: read_tib.to_string(),
            written_tib: written_tib.to_string(),
            ingest_tib: ingest_tib.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryStressView {
    pub state: MemoryStressStateView,
    pub pressure_percent: u8,
    pub swap_used_percent: u8,
    pub page_cache_tib: String,
    pub warning: Option<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SmartWarningsSummaryView {
    pub warning_count: usize,
    pub affected_drive_count: usize,
    pub warnings: Vec<SmartWarningView>,
}

impl SmartWarningsSummaryView {
    pub fn from_warnings(warnings: Vec<SmartWarningView>) -> Self {
        let mut affected_drives = Vec::new();
        for warning in &warnings {
            if !affected_drives.contains(&warning.drive_id) {
                affected_drives.push(warning.drive_id.clone());
            }
        }

        Self {
            warning_count: warnings.len(),
            affected_drive_count: affected_drives.len(),
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SmartWarningView {
    pub drive_id: String,
    pub enclosure_id: Option<String>,
    pub severity: DashboardSeverity,
    pub attribute: String,
    pub message: String,
    pub observed_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosuresPageView {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub enclosures: Vec<DasEnclosureCardView>,
    pub selected_enclosure_id: Option<String>,
    pub details: Option<DasEnclosureDetailView>,
    pub warnings: Vec<DashboardWarning>,
}

impl EnclosuresPageView {
    pub fn bootstrap_fixture() -> Self {
        let enclosures = bootstrap_enclosure_cards();
        let selected_enclosure_id = enclosures
            .first()
            .map(|enclosure| enclosure.enclosure_id.clone());
        let details = selected_enclosure_id
            .as_deref()
            .map(DasEnclosureDetailView::bootstrap_fixture);
        let warnings = enclosures
            .iter()
            .flat_map(|enclosure| enclosure.warnings.clone())
            .collect();

        Self {
            schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
            generated_at_utc: "2026-07-08T08:00:00Z".to_string(),
            enclosures,
            selected_enclosure_id,
            details,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DasEnclosureCardView {
    pub enclosure_id: String,
    pub display_name: String,
    pub mount_path: String,
    pub connection: EnclosureConnectionView,
    pub health: DashboardHealthStateView,
    pub drive_count: DriveCountSummaryView,
    pub capacity: CapacitySummaryView,
    pub last_seen_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosureConnectionView {
    pub bus: String,
    pub protocol: String,
    pub link_speed: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DasEnclosureDetailView {
    pub enclosure_id: String,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    pub firmware: Option<String>,
    pub slots: Vec<EnclosureDriveSlotView>,
}

impl DasEnclosureDetailView {
    fn bootstrap_fixture(enclosure_id: &str) -> Self {
        Self {
            enclosure_id: enclosure_id.to_string(),
            vendor: "DASObjectStore Lab".to_string(),
            model: "USB4 JBOD 8".to_string(),
            serial: "DAS-BOOTSTRAP-001".to_string(),
            firmware: Some("1.4.2".to_string()),
            slots: vec![
                EnclosureDriveSlotView::new(1, "das-enc-a-slot-01", "10.9", "healthy", true),
                EnclosureDriveSlotView::new(2, "das-enc-a-slot-02", "10.9", "healthy", true),
                EnclosureDriveSlotView::new(3, "das-enc-a-slot-03", "10.9", "healthy", true),
                EnclosureDriveSlotView::new(4, "das-enc-a-slot-04", "10.9", "watch", true),
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosureDriveSlotView {
    pub slot_number: u8,
    pub drive_id: String,
    pub size_tib: String,
    pub health: String,
    pub mounted: bool,
}

impl EnclosureDriveSlotView {
    fn new(slot_number: u8, drive_id: &str, size_tib: &str, health: &str, mounted: bool) -> Self {
        Self {
            slot_number,
            drive_id: drive_id.to_string(),
            size_tib: size_tib.to_string(),
            health: health.to_string(),
            mounted,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoresPageView {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub stores: Vec<ObjectStoreCardView>,
    pub selected_store_id: Option<String>,
    pub create_object_store: CreateObjectStoreAffordanceView,
    pub warnings: Vec<DashboardWarning>,
}

impl ObjectStoresPageView {
    pub fn bootstrap_fixture() -> Self {
        let stores = bootstrap_object_store_cards();
        let warnings = stores
            .iter()
            .flat_map(|store| store.warnings.clone())
            .collect();

        Self {
            schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
            generated_at_utc: "2026-07-08T08:00:00Z".to_string(),
            selected_store_id: stores.first().map(|store| store.store_id.clone()),
            stores,
            create_object_store: CreateObjectStoreAffordanceView::enabled(),
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoreCardView {
    pub store_id: String,
    pub display_name: String,
    pub store_class: String,
    pub health: DashboardHealthStateView,
    pub required_copies: u8,
    pub object_count: usize,
    pub capacity: CapacitySummaryView,
    pub placement_policy: String,
    pub endpoint_export_mode: String,
    pub writer_group: Option<String>,
    pub created_at_utc: String,
    pub last_ingested_at_utc: Option<String>,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreAffordanceView {
    pub enabled: bool,
    pub action_kind: String,
    pub label: String,
    pub required_fields: Vec<CreateObjectStoreFieldView>,
    pub optional_fields: Vec<CreateObjectStoreFieldView>,
    pub defaults: CreateObjectStoreDefaultsView,
    pub store_class_options: Vec<StoreClassOptionView>,
    pub copy_count_options: Vec<u8>,
    pub confirmation_required: bool,
    pub blocked_reason: Option<String>,
}

impl CreateObjectStoreAffordanceView {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            action_kind: "store_create".to_string(),
            label: "Create ObjectStore".to_string(),
            required_fields: vec![
                CreateObjectStoreFieldView::new("store_id", "Store ID"),
                CreateObjectStoreFieldView::new("store_class", "Store class"),
            ],
            optional_fields: vec![
                CreateObjectStoreFieldView::new("store_copies", "Required copies"),
                CreateObjectStoreFieldView::new("bucket", "S3 bucket"),
                CreateObjectStoreFieldView::new("writer_group", "Writer group"),
                CreateObjectStoreFieldView::new("ssd_root", "SSD root"),
            ],
            defaults: CreateObjectStoreDefaultsView {
                store_class: "generated_data".to_string(),
                required_copies: 2,
                endpoint_export_mode: "s3_bucket".to_string(),
            },
            store_class_options: vec![
                StoreClassOptionView::new(
                    "generated_data",
                    "Generated data",
                    "Protected, non-evictable outputs that should survive disk loss.",
                ),
                StoreClassOptionView::new(
                    "reproducible_cache",
                    "Reproducible cache",
                    "Evictable data that can be rebuilt from an external source.",
                ),
                StoreClassOptionView::new(
                    "archive",
                    "Archive",
                    "Cold data optimized for durability over write speed.",
                ),
            ],
            copy_count_options: vec![1, 2, 3],
            confirmation_required: true,
            blocked_reason: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreFieldView {
    pub name: String,
    pub label: String,
}

impl CreateObjectStoreFieldView {
    fn new(name: &str, label: &str) -> Self {
        Self {
            name: name.to_string(),
            label: label.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreDefaultsView {
    pub store_class: String,
    pub required_copies: u8,
    pub endpoint_export_mode: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreClassOptionView {
    pub value: String,
    pub label: String,
    pub description: String,
}

impl StoreClassOptionView {
    fn new(value: &str, label: &str, description: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
            description: description.to_string(),
        }
    }
}

fn bootstrap_enclosure_cards() -> Vec<DasEnclosureCardView> {
    vec![
        DasEnclosureCardView {
            enclosure_id: "das-enc-a".to_string(),
            display_name: "Primary DAS enclosure".to_string(),
            mount_path: "/srv/dasobjectstore/enclosures/primary".to_string(),
            connection: EnclosureConnectionView {
                bus: "usb4".to_string(),
                protocol: "uas".to_string(),
                link_speed: "40 Gbit/s".to_string(),
            },
            health: DashboardHealthStateView::Watch,
            drive_count: DriveCountSummaryView {
                total: 8,
                mounted: 8,
                healthy: 7,
                watch: 1,
                suspect: 0,
                failed: 0,
            },
            capacity: CapacitySummaryView {
                total_tib: "87.3".to_string(),
                used_tib: "48.1".to_string(),
                free_tib: "39.2".to_string(),
                used_percent_basis_points: 5510,
            },
            last_seen_at_utc: "2026-07-08T07:59:00Z".to_string(),
            warnings: vec![DashboardWarning::new(
                "enclosure_smart_warning",
                "One mounted drive has a SMART warning.",
            )],
        },
        DasEnclosureCardView {
            enclosure_id: "das-enc-b".to_string(),
            display_name: "Expansion DAS enclosure".to_string(),
            mount_path: "/srv/dasobjectstore/enclosures/expansion".to_string(),
            connection: EnclosureConnectionView {
                bus: "usb3".to_string(),
                protocol: "uas".to_string(),
                link_speed: "10 Gbit/s".to_string(),
            },
            health: DashboardHealthStateView::Healthy,
            drive_count: DriveCountSummaryView {
                total: 10,
                mounted: 10,
                healthy: 9,
                watch: 1,
                suspect: 0,
                failed: 0,
            },
            capacity: CapacitySummaryView {
                total_tib: "87.3".to_string(),
                used_tib: "43.1".to_string(),
                free_tib: "44.2".to_string(),
                used_percent_basis_points: 4937,
            },
            last_seen_at_utc: "2026-07-08T07:59:00Z".to_string(),
            warnings: Vec::new(),
        },
    ]
}

fn bootstrap_object_store_cards() -> Vec<ObjectStoreCardView> {
    vec![
        ObjectStoreCardView {
            store_id: "generated-data".to_string(),
            display_name: "Generated data".to_string(),
            store_class: "generated_data".to_string(),
            health: DashboardHealthStateView::Healthy,
            required_copies: 2,
            object_count: 1_248,
            capacity: CapacitySummaryView {
                total_tib: "120.0".to_string(),
                used_tib: "72.6".to_string(),
                free_tib: "47.4".to_string(),
                used_percent_basis_points: 6050,
            },
            placement_policy: "ssd_first_then_parallel_hdd".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            writer_group: Some("mnemosyne".to_string()),
            created_at_utc: "2026-06-11T09:30:00Z".to_string(),
            last_ingested_at_utc: Some("2026-07-08T06:40:00Z".to_string()),
            warnings: Vec::new(),
        },
        ObjectStoreCardView {
            store_id: "raw-public".to_string(),
            display_name: "Raw public data".to_string(),
            store_class: "reproducible_cache".to_string(),
            health: DashboardHealthStateView::Watch,
            required_copies: 1,
            object_count: 382,
            capacity: CapacitySummaryView {
                total_tib: "54.6".to_string(),
                used_tib: "18.6".to_string(),
                free_tib: "36.0".to_string(),
                used_percent_basis_points: 3407,
            },
            placement_policy: "evictable_cache".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            writer_group: Some("research".to_string()),
            created_at_utc: "2026-06-15T14:10:00Z".to_string(),
            last_ingested_at_utc: Some("2026-07-07T21:05:00Z".to_string()),
            warnings: vec![DashboardWarning::new(
                "store_copy_count_at_minimum",
                "Store is operating at the configured minimum copy count.",
            )],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        CreateObjectStoreAffordanceView, EnclosuresPageView, HomeDashboardView,
        ObjectStoresPageView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
    };

    #[test]
    fn serializes_home_dashboard_redesign_contract() {
        let encoded =
            serde_json::to_value(HomeDashboardView::bootstrap_fixture()).expect("home serializes");

        assert_eq!(encoded["schema_version"], REDESIGN_DASHBOARD_SCHEMA_VERSION);
        assert_eq!(encoded["health"]["state"], "watch");
        assert_eq!(encoded["drives"]["total"], 18);
        assert_eq!(encoded["capacity"]["total_tib"], "174.6");
        assert_eq!(encoded["throughput_7d"]["window_days"], 7);
        assert_eq!(
            encoded["throughput_7d"]["daily"]
                .as_array()
                .expect("daily throughput")
                .len(),
            7
        );
        assert_eq!(encoded["memory_stress"]["state"], "nominal");
        assert_eq!(encoded["smart_warnings"]["affected_drive_count"], 1);
        assert_eq!(
            encoded["create_object_store"]["action_kind"],
            "store_create"
        );
    }

    #[test]
    fn serializes_enclosures_page_redesign_contract() {
        let encoded = serde_json::to_value(EnclosuresPageView::bootstrap_fixture())
            .expect("enclosures serializes");

        assert_eq!(encoded["schema_version"], REDESIGN_DASHBOARD_SCHEMA_VERSION);
        assert_eq!(encoded["selected_enclosure_id"], "das-enc-a");
        assert_eq!(
            encoded["enclosures"].as_array().expect("enclosures").len(),
            2
        );
        assert_eq!(
            encoded["enclosures"][0]["mount_path"],
            "/srv/dasobjectstore/enclosures/primary"
        );
        assert_eq!(encoded["details"]["slots"][0]["mounted"], true);
    }

    #[test]
    fn serializes_object_stores_page_redesign_contract() {
        let encoded = serde_json::to_value(ObjectStoresPageView::bootstrap_fixture())
            .expect("object stores serializes");

        assert_eq!(encoded["schema_version"], REDESIGN_DASHBOARD_SCHEMA_VERSION);
        assert_eq!(encoded["selected_store_id"], "generated-data");
        assert_eq!(encoded["stores"][0]["required_copies"], 2);
        assert_eq!(
            encoded["stores"][0]["placement_policy"],
            "ssd_first_then_parallel_hdd"
        );
        assert_eq!(
            encoded["create_object_store"]["defaults"]["store_class"],
            "generated_data"
        );
    }

    #[test]
    fn create_object_store_affordance_advertises_required_inputs() {
        let encoded = serde_json::to_value(CreateObjectStoreAffordanceView::enabled())
            .expect("affordance serializes");

        let required_fields = encoded["required_fields"]
            .as_array()
            .expect("required fields");

        assert_eq!(encoded["enabled"], true);
        assert_eq!(encoded["confirmation_required"], true);
        assert!(required_fields
            .iter()
            .any(|field| field["name"] == "store_id"));
        assert!(required_fields
            .iter()
            .any(|field| field["name"] == "store_class"));
    }
}
