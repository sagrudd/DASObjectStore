use crate::dashboard::{
    DashboardAttentionView, DashboardWarning, DestageQueueView, DiskHealthView, IngestQueueView,
    ObjectStateView, PoolStatusView,
};
use crate::endpoints::EndpointInventoryView;
use serde::{Deserialize, Serialize};

pub const OPERATIONS_WORKSPACES_SCHEMA_VERSION: &str = "dasobjectstore.operations_workspaces.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsWorkspacesView {
    pub schema_version: String,
    pub active_workspace: OperationsWorkspaceKindView,
    pub navigation: Vec<WorkspaceNavigationItemView>,
    pub overview: OverviewWorkspaceView,
    pub disks: DisksWorkspaceView,
    pub stores: StoresWorkspaceView,
    pub objects: ObjectsWorkspaceView,
    pub endpoints: EndpointsWorkspaceView,
    pub activity: ActivityWorkspaceView,
}

impl OperationsWorkspacesView {
    pub fn new(
        active_workspace: OperationsWorkspaceKindView,
        overview: OverviewWorkspaceView,
        disks: DisksWorkspaceView,
        stores: StoresWorkspaceView,
        objects: ObjectsWorkspaceView,
        endpoints: EndpointsWorkspaceView,
        activity: ActivityWorkspaceView,
    ) -> Self {
        Self {
            schema_version: OPERATIONS_WORKSPACES_SCHEMA_VERSION.to_string(),
            active_workspace,
            navigation: workspace_navigation(active_workspace),
            overview,
            disks,
            stores,
            objects,
            endpoints,
            activity,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationsWorkspaceKindView {
    Overview,
    Disks,
    Stores,
    Objects,
    Endpoints,
    Activity,
}

impl OperationsWorkspaceKindView {
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Disks => "Disks",
            Self::Stores => "Stores",
            Self::Objects => "Objects",
            Self::Endpoints => "Endpoints",
            Self::Activity => "Activity",
        }
    }

    fn route_segment(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Disks => "disks",
            Self::Stores => "stores",
            Self::Objects => "objects",
            Self::Endpoints => "endpoints",
            Self::Activity => "activity",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceNavigationItemView {
    pub workspace: OperationsWorkspaceKindView,
    pub label: String,
    pub route_segment: String,
    pub selected: bool,
    pub attention_count: usize,
}

impl WorkspaceNavigationItemView {
    fn new(workspace: OperationsWorkspaceKindView, active: OperationsWorkspaceKindView) -> Self {
        Self {
            workspace,
            label: workspace.label().to_string(),
            route_segment: workspace.route_segment().to_string(),
            selected: workspace == active,
            attention_count: 0,
        }
    }
}

pub fn workspace_navigation(
    active: OperationsWorkspaceKindView,
) -> Vec<WorkspaceNavigationItemView> {
    [
        OperationsWorkspaceKindView::Overview,
        OperationsWorkspaceKindView::Disks,
        OperationsWorkspaceKindView::Stores,
        OperationsWorkspaceKindView::Objects,
        OperationsWorkspaceKindView::Endpoints,
        OperationsWorkspaceKindView::Activity,
    ]
    .into_iter()
    .map(|workspace| WorkspaceNavigationItemView::new(workspace, active))
    .collect()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OverviewWorkspaceView {
    pub pool: Option<PoolStatusView>,
    pub ingest: Option<IngestQueueView>,
    pub destage: Option<DestageQueueView>,
    pub endpoints: Option<EndpointInventoryView>,
    pub attention: DashboardAttentionView,
}

impl OverviewWorkspaceView {
    pub fn empty() -> Self {
        Self {
            pool: None,
            ingest: None,
            destage: None,
            endpoints: Some(EndpointInventoryView::from_endpoints(Vec::new())),
            attention: DashboardAttentionView::from_sections(None, &[], None, None),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DisksWorkspaceView {
    pub disks: Vec<DiskHealthView>,
    pub selected_disk_id: Option<String>,
    pub warnings: Vec<DashboardWarning>,
}

impl DisksWorkspaceView {
    pub fn empty() -> Self {
        Self::from_disks(Vec::new())
    }

    pub fn from_disks(disks: Vec<DiskHealthView>) -> Self {
        let warnings = disks
            .iter()
            .flat_map(|disk| disk.warnings.clone())
            .collect();

        Self {
            disks,
            selected_disk_id: None,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoresWorkspaceView {
    pub stores: Vec<StorePolicySummaryView>,
    pub selected_store_id: Option<String>,
    pub warnings: Vec<DashboardWarning>,
}

impl StoresWorkspaceView {
    pub fn empty() -> Self {
        Self::from_stores(Vec::new())
    }

    pub fn from_stores(stores: Vec<StorePolicySummaryView>) -> Self {
        Self {
            stores,
            selected_store_id: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorePolicySummaryView {
    pub store_id: String,
    pub display_name: String,
    pub store_class: String,
    pub ingest_mode: String,
    pub required_copies: u8,
    pub object_count: usize,
    pub used_bytes: u64,
    pub capacity_behavior: String,
    pub endpoint_export_mode: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectsWorkspaceView {
    pub objects: Vec<ObjectInventoryRowView>,
    pub selected_object_id: Option<String>,
    pub filters: ObjectInventoryFiltersView,
    pub warnings: Vec<DashboardWarning>,
}

impl ObjectsWorkspaceView {
    pub fn empty() -> Self {
        Self::from_objects(Vec::new())
    }

    pub fn from_objects(objects: Vec<ObjectInventoryRowView>) -> Self {
        let warnings = objects
            .iter()
            .flat_map(|object| object.warnings.clone())
            .collect();

        Self {
            objects,
            selected_object_id: None,
            filters: ObjectInventoryFiltersView::default(),
            warnings,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectInventoryFiltersView {
    pub store_id: Option<String>,
    pub state: Option<ObjectStateView>,
    pub search: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectInventoryRowView {
    pub object_id: String,
    pub store_id: String,
    pub state: ObjectStateView,
    pub size_bytes: Option<u64>,
    pub content_hash: Option<String>,
    pub copy_count: usize,
    pub required_copies: u8,
    pub updated_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointsWorkspaceView {
    pub inventory: EndpointInventoryView,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityWorkspaceView {
    pub ingest: Option<IngestQueueView>,
    pub destage: Option<DestageQueueView>,
    pub tasks: Vec<ActivityTaskView>,
    pub warnings: Vec<DashboardWarning>,
}

impl ActivityWorkspaceView {
    pub fn from_sections(
        ingest: Option<IngestQueueView>,
        destage: Option<DestageQueueView>,
        tasks: Vec<ActivityTaskView>,
    ) -> Self {
        let mut warnings = Vec::new();
        if let Some(ingest) = &ingest {
            warnings.extend(ingest.warnings.clone());
        }
        if let Some(destage) = &destage {
            warnings.extend(destage.warnings.clone());
        }
        warnings.extend(tasks.iter().flat_map(|task| task.warnings.clone()));

        Self {
            ingest,
            destage,
            tasks,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityTaskView {
    pub task_id: String,
    pub kind: ActivityTaskKindView,
    pub state: ActivityTaskStateView,
    pub label: String,
    pub updated_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTaskKindView {
    Ingest,
    Destage,
    HealthCheck,
    DiskDrain,
    DiskReplace,
    EndpointValidation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTaskStateView {
    Queued,
    Running,
    Waiting,
    Complete,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{
        workspace_navigation, ActivityTaskKindView, ActivityTaskStateView, ActivityTaskView,
        ActivityWorkspaceView, DisksWorkspaceView, EndpointsWorkspaceView, ObjectInventoryRowView,
        ObjectsWorkspaceView, OperationsWorkspaceKindView, OperationsWorkspacesView,
        OverviewWorkspaceView, StorePolicySummaryView, StoresWorkspaceView,
        OPERATIONS_WORKSPACES_SCHEMA_VERSION,
    };
    use crate::dashboard::{DashboardAttentionView, ObjectStateView};
    use crate::endpoints::{EndpointInventoryItemView, EndpointInventoryView};

    #[test]
    fn builds_navigation_for_all_operations_workspaces() {
        let navigation = workspace_navigation(OperationsWorkspaceKindView::Endpoints);

        assert_eq!(navigation.len(), 6);
        assert_eq!(navigation[0].route_segment, "overview");
        assert_eq!(
            navigation[4].workspace,
            OperationsWorkspaceKindView::Endpoints
        );
        assert!(navigation[4].selected);
    }

    #[test]
    fn serializes_navigation_workspace_names_as_snake_case() {
        let encoded =
            serde_json::to_value(workspace_navigation(OperationsWorkspaceKindView::Disks))
                .expect("navigation serializes");

        assert_eq!(encoded[0]["workspace"], "overview");
        assert_eq!(encoded[1]["workspace"], "disks");
        assert_eq!(encoded[1]["selected"], true);
    }

    #[test]
    fn builds_operations_workspace_payload() {
        let endpoints =
            EndpointInventoryView::from_endpoints(vec![EndpointInventoryItemView::new(
                "endpoint-a",
                "DAS endpoint",
                crate::EndpointKindView::DasobjectstoreDas,
                "https://127.0.0.1:9443",
                crate::EndpointValidationView::new(crate::EndpointValidationStateView::Validated),
            )]);
        let overview = OverviewWorkspaceView {
            pool: None,
            ingest: None,
            destage: None,
            endpoints: Some(endpoints.clone()),
            attention: DashboardAttentionView::from_sections(None, &[], None, None),
        };
        let stores = StoresWorkspaceView::from_stores(vec![StorePolicySummaryView {
            store_id: "raw-public".to_string(),
            display_name: "Raw public data".to_string(),
            store_class: "reproducible_cache".to_string(),
            ingest_mode: "direct_to_hdd".to_string(),
            required_copies: 1,
            object_count: 12,
            used_bytes: 4096,
            capacity_behavior: "evictable".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
        }]);
        let objects = ObjectsWorkspaceView::from_objects(vec![ObjectInventoryRowView {
            object_id: "object-a".to_string(),
            store_id: "raw-public".to_string(),
            state: ObjectStateView::Protected,
            size_bytes: Some(4096),
            content_hash: Some("sha256:abc".to_string()),
            copy_count: 1,
            required_copies: 1,
            updated_at_utc: "2026-07-06T11:00:00Z".to_string(),
            warnings: Vec::new(),
        }]);
        let activity = ActivityWorkspaceView::from_sections(
            None,
            None,
            vec![ActivityTaskView {
                task_id: "task-a".to_string(),
                kind: ActivityTaskKindView::EndpointValidation,
                state: ActivityTaskStateView::Complete,
                label: "Validate endpoint".to_string(),
                updated_at_utc: "2026-07-06T11:00:00Z".to_string(),
                warnings: Vec::new(),
            }],
        );

        let view = OperationsWorkspacesView::new(
            OperationsWorkspaceKindView::Overview,
            overview,
            DisksWorkspaceView::from_disks(Vec::new()),
            stores,
            objects,
            EndpointsWorkspaceView {
                inventory: endpoints,
            },
            activity,
        );

        let encoded = serde_json::to_value(view).expect("workspace payload serializes");

        assert_eq!(
            encoded["schema_version"],
            OPERATIONS_WORKSPACES_SCHEMA_VERSION
        );
        assert_eq!(encoded["active_workspace"], "overview");
        assert_eq!(
            encoded["navigation"].as_array().expect("navigation").len(),
            6
        );
        assert_eq!(encoded["stores"]["stores"][0]["store_id"], "raw-public");
        assert_eq!(
            encoded["activity"]["tasks"][0]["kind"],
            "endpoint_validation"
        );
    }

    #[test]
    fn builds_empty_overview_workspace_for_api_bootstrap() {
        let overview = OverviewWorkspaceView::empty();
        let encoded = serde_json::to_value(overview).expect("overview serializes");

        assert_eq!(encoded["pool"], serde_json::Value::Null);
        assert_eq!(encoded["ingest"], serde_json::Value::Null);
        assert_eq!(encoded["destage"], serde_json::Value::Null);
        assert_eq!(encoded["endpoints"]["endpoint_count"], 0);
        assert_eq!(encoded["attention"]["warning_count"], 0);
    }

    #[test]
    fn builds_empty_disks_workspace_for_api_bootstrap() {
        let disks = DisksWorkspaceView::empty();
        let encoded = serde_json::to_value(disks).expect("disks serializes");

        assert_eq!(encoded["disks"].as_array().expect("disks").len(), 0);
        assert_eq!(encoded["selected_disk_id"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }

    #[test]
    fn builds_empty_stores_workspace_for_api_bootstrap() {
        let stores = StoresWorkspaceView::empty();
        let encoded = serde_json::to_value(stores).expect("stores serializes");

        assert_eq!(encoded["stores"].as_array().expect("stores").len(), 0);
        assert_eq!(encoded["selected_store_id"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }

    #[test]
    fn builds_empty_objects_workspace_for_api_bootstrap() {
        let objects = ObjectsWorkspaceView::empty();
        let encoded = serde_json::to_value(objects).expect("objects serializes");

        assert_eq!(encoded["objects"].as_array().expect("objects").len(), 0);
        assert_eq!(encoded["selected_object_id"], serde_json::Value::Null);
        assert_eq!(encoded["filters"]["store_id"], serde_json::Value::Null);
        assert_eq!(encoded["filters"]["state"], serde_json::Value::Null);
        assert_eq!(encoded["filters"]["search"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }
}
