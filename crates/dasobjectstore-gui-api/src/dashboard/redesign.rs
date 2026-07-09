use crate::dashboard::{DashboardSeverity, DashboardWarning, DestageQueueView, IngestQueueView};
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
    pub ingest: Option<IngestQueueView>,
    pub destage: Option<DestageQueueView>,
    pub memory_stress: MemoryStressView,
    pub smart_warnings: SmartWarningsSummaryView,
    pub object_stores: Vec<ObjectStoreCardView>,
    pub create_object_store: CreateObjectStoreAffordanceView,
}

impl HomeDashboardView {
    pub fn bootstrap_fixture() -> Self {
        Self {
            schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
            generated_at_utc: "2026-07-08T08:00:00Z".to_string(),
            health: HealthSummaryView {
                state: DashboardHealthStateView::Watch,
                label: "Inventory pending".to_string(),
                warning_count: 1,
                critical_count: 0,
                action_count: 1,
                last_checked_at_utc: None,
            },
            drives: DriveCountSummaryView {
                total: 0,
                mounted: 0,
                healthy: 0,
                watch: 0,
                suspect: 0,
                failed: 0,
            },
            capacity: CapacitySummaryView {
                total_tib: "0.0".to_string(),
                used_tib: "0.0".to_string(),
                free_tib: "0.0".to_string(),
                used_percent_basis_points: 0,
            },
            mounted_enclosures: Vec::new(),
            throughput_7d: ThroughputSummaryView::bootstrap_fixture(),
            ingest: None,
            destage: None,
            memory_stress: MemoryStressView {
                state: MemoryStressStateView::Elevated,
                pressure_percent: 0,
                swap_used_percent: 0,
                page_cache_tib: "0.0".to_string(),
                warning: Some(DashboardWarning::new(
                    "memory_telemetry_pending",
                    "Memory pressure telemetry is pending daemon integration.",
                )),
            },
            smart_warnings: SmartWarningsSummaryView::from_warnings(Vec::new()),
            object_stores: Vec::new(),
            create_object_store: CreateObjectStoreAffordanceView::admin_required(),
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
            read_tib: "0.0".to_string(),
            written_tib: "0.0".to_string(),
            ingest_tib: "0.0".to_string(),
            avg_read_mib_s: 0,
            avg_write_mib_s: 0,
            daily: Vec::new(),
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
    pub add_enclosure: AddEnclosureAffordanceView,
    pub enclosures: Vec<DasEnclosureCardView>,
    pub selected_enclosure_id: Option<String>,
    pub details: Option<DasEnclosureDetailView>,
    pub warnings: Vec<DashboardWarning>,
}

impl EnclosuresPageView {
    pub fn bootstrap_fixture() -> Self {
        Self {
            schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
            generated_at_utc: "2026-07-08T08:00:00Z".to_string(),
            add_enclosure: AddEnclosureAffordanceView::admin_required(),
            enclosures: Vec::new(),
            selected_enclosure_id: None,
            details: None,
            warnings: vec![DashboardWarning::new(
                "enclosure_inventory_pending",
                "Live DAS enclosure inventory is pending daemon integration.",
            )],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AddEnclosureAffordanceView {
    pub enabled: bool,
    pub action_kind: String,
    pub label: String,
    pub state: String,
    pub administrator: bool,
    pub supported_enclosure_detected: bool,
    pub daemon_ready: bool,
    pub confirmation_required: bool,
    pub blocked_reason: Option<String>,
    pub next_step: String,
}

impl AddEnclosureAffordanceView {
    pub fn available() -> Self {
        Self {
            enabled: true,
            action_kind: "enclosure_add".to_string(),
            label: "Add enclosure".to_string(),
            state: "ready".to_string(),
            administrator: true,
            supported_enclosure_detected: true,
            daemon_ready: true,
            confirmation_required: true,
            blocked_reason: None,
            next_step: "Start supported DAS detection and preparation planning.".to_string(),
        }
    }

    pub fn admin_required() -> Self {
        Self::blocked(
            "admin_required",
            false,
            true,
            "Administrator capability is required before enclosure preparation is available.",
            "Sign in with an administrator-capable local account to prepare DAS hardware.",
        )
    }

    pub fn blocked(
        state: &str,
        administrator: bool,
        daemon_ready: bool,
        blocked_reason: impl Into<String>,
        next_step: impl Into<String>,
    ) -> Self {
        Self {
            enabled: false,
            action_kind: "enclosure_add".to_string(),
            label: "Add enclosure".to_string(),
            state: state.to_string(),
            administrator,
            supported_enclosure_detected: false,
            daemon_ready,
            confirmation_required: true,
            blocked_reason: Some(blocked_reason.into()),
            next_step: next_step.into(),
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosureDriveSlotView {
    pub slot_number: u8,
    pub drive_id: String,
    pub role: String,
    pub mount_path: String,
    pub device_path: Option<String>,
    pub filesystem: Option<String>,
    pub size_tib: String,
    pub health: String,
    pub mounted: bool,
    pub smart_warning_count: usize,
    pub actions_available: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoresPageView {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub groups_file_path: String,
    pub groups: Vec<StorageGroupView>,
    #[serde(default)]
    pub mounted_enclosures: Vec<DasEnclosureCardView>,
    pub stores: Vec<ObjectStoreCardView>,
    pub selected_store_id: Option<String>,
    pub create_object_store: CreateObjectStoreAffordanceView,
    pub warnings: Vec<DashboardWarning>,
}

impl ObjectStoresPageView {
    pub fn bootstrap_fixture() -> Self {
        Self {
            schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
            generated_at_utc: "2026-07-08T08:00:00Z".to_string(),
            groups_file_path: "/opt/dasobjectstore/groups.json".to_string(),
            groups: Vec::new(),
            mounted_enclosures: Vec::new(),
            selected_store_id: None,
            stores: Vec::new(),
            create_object_store: CreateObjectStoreAffordanceView::admin_required(),
            warnings: vec![DashboardWarning::new(
                "object_store_inventory_pending",
                "Live object-store inventory and group policy are pending daemon integration.",
            )],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoreCardView {
    pub store_id: String,
    pub display_name: String,
    pub store_class: String,
    pub object_type: String,
    pub health: DashboardHealthStateView,
    pub required_copies: u8,
    pub object_count: usize,
    pub capacity: CapacitySummaryView,
    pub placement_policy: String,
    pub endpoint_export_mode: String,
    pub writer_group: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub created_at_utc: String,
    pub last_ingested_at_utc: Option<String>,
    pub writer_policy: WriterPolicyReadinessView,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageGroupView {
    pub group_name: String,
    pub display_name: String,
    pub source: String,
    pub current_user_member: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WriterPolicyReadinessView {
    pub writer_group: Option<String>,
    pub group_defined: bool,
    pub current_user_member: bool,
    pub writeable_by_current_user: bool,
    pub state: String,
    pub message: String,
}

impl WriterPolicyReadinessView {
    pub fn without_writer_group() -> Self {
        Self {
            writer_group: None,
            group_defined: false,
            current_user_member: false,
            writeable_by_current_user: false,
            state: "no_writer_group".to_string(),
            message: "No writer group is assigned to this ObjectStore.".to_string(),
        }
    }
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
    pub fn admin_required() -> Self {
        let mut view = Self::enabled();
        view.enabled = false;
        view.blocked_reason = Some(
            "Current user must have sudo-derived DASObjectStore administrator rights.".to_string(),
        );
        view
    }

    pub fn enclosure_required() -> Self {
        let mut view = Self::enabled();
        view.enabled = false;
        view.blocked_reason =
            Some("A mounted DAS enclosure is required before creating an ObjectStore.".to_string());
        view
    }

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            action_kind: "store_create".to_string(),
            label: "Create ObjectStore".to_string(),
            required_fields: vec![
                CreateObjectStoreFieldView::new("store_id", "Store name"),
                CreateObjectStoreFieldView::new("writer_group", "Writer group"),
                CreateObjectStoreFieldView::new("enclosure_id", "Enclosure"),
            ],
            optional_fields: vec![
                CreateObjectStoreFieldView::new("store_copies", "Required copies"),
                CreateObjectStoreFieldView::new("object_type", "Object type"),
                CreateObjectStoreFieldView::new("store_class", "Store class"),
                CreateObjectStoreFieldView::new("endpoint_export_mode", "Export mode"),
                CreateObjectStoreFieldView::new("public", "Public visibility"),
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

#[cfg(test)]
mod tests {
    use super::{
        CapacitySummaryView, CreateObjectStoreAffordanceView, DashboardHealthStateView,
        EnclosuresPageView, HomeDashboardView, ObjectStoreCardView, ObjectStoresPageView,
        WriterPolicyReadinessView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
    };

    #[test]
    fn serializes_home_dashboard_redesign_contract() {
        let encoded =
            serde_json::to_value(HomeDashboardView::bootstrap_fixture()).expect("home serializes");

        assert_eq!(encoded["schema_version"], REDESIGN_DASHBOARD_SCHEMA_VERSION);
        assert_eq!(encoded["health"]["state"], "watch");
        assert_eq!(encoded["health"]["label"], "Inventory pending");
        assert_eq!(encoded["drives"]["total"], 0);
        assert_eq!(encoded["capacity"]["total_tib"], "0.0");
        assert_eq!(encoded["throughput_7d"]["window_days"], 7);
        assert_eq!(
            encoded["throughput_7d"]["daily"]
                .as_array()
                .expect("daily throughput")
                .len(),
            0
        );
        assert_eq!(encoded["memory_stress"]["state"], "elevated");
        assert_eq!(encoded["smart_warnings"]["affected_drive_count"], 0);
        assert_eq!(encoded["mounted_enclosures"].as_array().unwrap().len(), 0);
        assert_eq!(encoded["object_stores"].as_array().unwrap().len(), 0);
        assert_eq!(encoded["create_object_store"]["enabled"], false);
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
        assert_eq!(encoded["selected_enclosure_id"], serde_json::Value::Null);
        assert_eq!(
            encoded["enclosures"].as_array().expect("enclosures").len(),
            0
        );
        assert_eq!(encoded["details"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 1);
    }

    #[test]
    fn serializes_object_stores_page_redesign_contract() {
        let mut view = ObjectStoresPageView::bootstrap_fixture();
        view.stores = vec![ObjectStoreCardView {
            store_id: "zymo_fecal_2025.05".to_string(),
            display_name: "zymo_fecal_2025.05".to_string(),
            store_class: "generated_data".to_string(),
            object_type: "pod5".to_string(),
            health: DashboardHealthStateView::Healthy,
            required_copies: 2,
            object_count: 42,
            capacity: CapacitySummaryView {
                total_tib: "100.0".to_string(),
                used_tib: "12.5".to_string(),
                free_tib: "87.5".to_string(),
                used_percent_basis_points: 1250,
            },
            placement_policy: "fractional_free_space".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            writer_group: Some("bioinformatics".to_string()),
            public: false,
            writeable: true,
            created_at_utc: "2026-07-08T08:00:00Z".to_string(),
            last_ingested_at_utc: Some("2026-07-08T08:30:00Z".to_string()),
            writer_policy: WriterPolicyReadinessView {
                writer_group: Some("bioinformatics".to_string()),
                group_defined: true,
                current_user_member: true,
                writeable_by_current_user: true,
                state: "ready".to_string(),
                message: "Current user belongs to the writer group.".to_string(),
            },
            warnings: Vec::new(),
        }];
        let encoded = serde_json::to_value(view).expect("object stores serializes");

        assert_eq!(encoded["schema_version"], REDESIGN_DASHBOARD_SCHEMA_VERSION);
        assert_eq!(encoded["selected_store_id"], serde_json::Value::Null);
        assert_eq!(
            encoded["mounted_enclosures"]
                .as_array()
                .expect("mounted enclosures")
                .len(),
            0
        );
        assert_eq!(encoded["stores"].as_array().expect("stores").len(), 1);
        assert_eq!(encoded["stores"][0]["object_type"], "pod5");
        assert_eq!(encoded["stores"][0]["public"], false);
        assert_eq!(encoded["stores"][0]["writeable"], true);
        assert_eq!(encoded["stores"][0]["writer_policy"]["state"], "ready");
        assert_eq!(encoded["create_object_store"]["enabled"], false);
        assert_eq!(
            encoded["create_object_store"]["defaults"]["store_class"],
            "generated_data"
        );
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 1);
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
            .any(|field| field["name"] == "writer_group"));
        assert!(required_fields
            .iter()
            .any(|field| field["name"] == "enclosure_id"));
        assert!(encoded["optional_fields"]
            .as_array()
            .expect("optional fields")
            .iter()
            .any(|field| field["name"] == "store_class"));
    }
}
