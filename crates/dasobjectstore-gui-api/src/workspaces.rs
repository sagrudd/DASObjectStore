use crate::dashboard::{
    CreateObjectStoreAffordanceView, DasEnclosureCardView, DashboardAttentionView,
    DashboardWarning, DestageQueueView, DiskHealthView, HomeDashboardView, IngestQueueView,
    ObjectStateView, ObjectStoreCardView, PoolStatusView, StorageGroupView,
};
use crate::endpoints::EndpointInventoryView;
use crate::{LocalUserMetadata, UserSummary, SUDO_ADMIN_GROUPS};
use prosopikon_core::{ProsopikonAuthenticationFramework, ProsopikonDeviceTokenRequirement};
use serde::{Deserialize, Serialize};

#[path = "workspaces_product.rs"]
mod product;
pub use product::*;

pub const OPERATIONS_WORKSPACES_SCHEMA_VERSION: &str = "dasobjectstore.operations_workspaces.v1";
pub const PRODUCT_WORKSPACES_SCHEMA_VERSION: &str = "dasobjectstore.product_workspaces.v1";

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
    pub users_groups: Option<UsersGroupsWorkspaceView>,
}

impl OperationsWorkspacesView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        active_workspace: OperationsWorkspaceKindView,
        overview: OverviewWorkspaceView,
        disks: DisksWorkspaceView,
        stores: StoresWorkspaceView,
        objects: ObjectsWorkspaceView,
        endpoints: EndpointsWorkspaceView,
        activity: ActivityWorkspaceView,
        users_groups: Option<UsersGroupsWorkspaceView>,
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
            users_groups,
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
    UsersGroups,
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
            Self::UsersGroups => "Local Access",
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
            Self::UsersGroups => "users-groups",
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
        OperationsWorkspaceKindView::UsersGroups,
    ]
    .into_iter()
    .map(|workspace| WorkspaceNavigationItemView::new(workspace, active))
    .collect()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OverviewWorkspaceView {
    pub home: Option<HomeDashboardView>,
    pub pool: Option<PoolStatusView>,
    pub ingest: Option<IngestQueueView>,
    pub destage: Option<DestageQueueView>,
    pub endpoints: Option<EndpointInventoryView>,
    pub attention: DashboardAttentionView,
}

impl OverviewWorkspaceView {
    pub fn empty() -> Self {
        Self {
            home: None,
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
    pub enclosures: Vec<DasEnclosureCardView>,
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
            enclosures: Vec::new(),
            selected_disk_id: None,
            warnings,
        }
    }

    pub fn with_enclosures(mut self, enclosures: Vec<DasEnclosureCardView>) -> Self {
        self.enclosures = enclosures;
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoresWorkspaceView {
    pub stores: Vec<StorePolicySummaryView>,
    pub object_store_cards: Vec<ObjectStoreCardView>,
    pub create_object_store: CreateObjectStoreAffordanceView,
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
            object_store_cards: Vec::new(),
            create_object_store: CreateObjectStoreAffordanceView::enabled(),
            selected_store_id: None,
            warnings: Vec::new(),
        }
    }

    pub fn with_object_store_cards(mut self, stores: Vec<ObjectStoreCardView>) -> Self {
        self.object_store_cards = stores;
        self
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

impl EndpointsWorkspaceView {
    pub fn empty() -> Self {
        Self {
            inventory: EndpointInventoryView::from_endpoints(Vec::new()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityWorkspaceView {
    pub ingest: Option<IngestQueueView>,
    pub destage: Option<DestageQueueView>,
    pub categories: Vec<ActivityCategoryView>,
    pub tasks: Vec<ActivityTaskView>,
    pub warnings: Vec<DashboardWarning>,
}

impl ActivityWorkspaceView {
    pub fn empty() -> Self {
        Self::from_sections(None, None, Vec::new())
    }

    pub fn bootstrap() -> Self {
        Self::empty().with_categories(default_activity_categories())
    }

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
            categories: Vec::new(),
            tasks,
            warnings,
        }
    }

    pub fn with_categories(mut self, categories: Vec<ActivityCategoryView>) -> Self {
        self.categories = categories;
        self
    }

    pub fn with_tasks(mut self, tasks: Vec<ActivityTaskView>) -> Self {
        self.tasks = tasks;
        self.warnings
            .extend(self.tasks.iter().flat_map(|task| task.warnings.clone()));
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityCategoryView {
    pub kind: ActivityTaskKindView,
    pub label: String,
    pub description: String,
}

impl ActivityCategoryView {
    fn new(kind: ActivityTaskKindView, label: &str, description: &str) -> Self {
        Self {
            kind,
            label: label.to_string(),
            description: description.to_string(),
        }
    }
}

pub fn default_activity_categories() -> Vec<ActivityCategoryView> {
    vec![
        ActivityCategoryView::new(
            ActivityTaskKindView::SystemAdministration,
            "Administrator jobs",
            "Local group, user, and privileged appliance administration submitted to the daemon.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::EnclosurePreparation,
            "Enclosure preparation",
            "Supported DAS detection, SSD/HDD selection, and destructive preparation jobs.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::ObjectStoreCreation,
            "ObjectStore creation",
            "Daemon-owned ObjectStore creation and policy materialization.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::SubObjectCreation,
            "SubObject creation",
            "Folder-level and nested object routing registrations for workflow-ready data.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::Ingest,
            "Ingest",
            "SSD-first file and folder upload jobs, including queued and active ingress.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::Destage,
            "Destage",
            "SSD-to-HDD settlement, verification, and protected-object queue movement.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::Repair,
            "Repair",
            "Disk repair, replacement, redownload, and redundancy restoration work.",
        ),
        ActivityCategoryView::new(
            ActivityTaskKindView::EndpointValidation,
            "Endpoint validation",
            "Object-service, S3, NAS/NFS, and Mnemosyne endpoint validation tasks.",
        ),
    ]
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityTaskView {
    pub task_id: String,
    pub kind: ActivityTaskKindView,
    pub state: ActivityTaskStateView,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ActivityTaskProgressView>,
    pub updated_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityTaskProgressView {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub percent_complete: Option<u8>,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTaskKindView {
    Ingest,
    Destage,
    SystemAdministration,
    EnclosurePreparation,
    ObjectStoreCreation,
    SubObjectCreation,
    Repair,
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
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsersGroupsWorkspaceView {
    pub host_mode: UsersGroupsHostModeView,
    pub authentication_framework: ProsopikonAuthenticationFramework,
    pub device_token_requirement: ProsopikonDeviceTokenRequirement,
    pub current_user: Option<LocalUserAuthorityView>,
    pub users: Vec<StandaloneUserAccountView>,
    pub groups: Vec<LocalGroupMembershipView>,
    pub groups_file_path: String,
    pub writer_groups: Vec<StorageGroupView>,
    pub operations: Vec<LocalGroupOperationView>,
    pub capabilities: UsersGroupsCapabilitiesView,
    pub selected_username: Option<String>,
    pub selected_group_name: Option<String>,
    pub warnings: Vec<DashboardWarning>,
}

impl UsersGroupsWorkspaceView {
    pub fn standalone(
        current_user: Option<LocalUserMetadata>,
        users: Vec<UserSummary>,
        groups_file_path: String,
        writer_groups: Vec<StorageGroupView>,
        mut warnings: Vec<DashboardWarning>,
    ) -> Self {
        let administrator_actions_enabled = current_user
            .as_ref()
            .is_some_and(|user| user.sudo_administrator);

        if current_user.is_some() && !administrator_actions_enabled {
            warnings.push(DashboardWarning::new(
                "standalone_admin_authority_missing",
                "Current OS user is not a sudo-derived DASObjectStore administrator.",
            ));
        }

        let groups = current_user
            .as_ref()
            .map(local_group_memberships)
            .unwrap_or_default();

        Self {
            host_mode: UsersGroupsHostModeView::Standalone,
            authentication_framework: ProsopikonAuthenticationFramework::Hybrid,
            device_token_requirement: ProsopikonDeviceTokenRequirement::NotRequired,
            current_user: current_user.map(LocalUserAuthorityView::from),
            users: users
                .into_iter()
                .map(StandaloneUserAccountView::from)
                .collect(),
            groups,
            groups_file_path,
            writer_groups,
            operations: local_group_operations(administrator_actions_enabled),
            capabilities: UsersGroupsCapabilitiesView {
                product_local_user_registration: true,
                os_local_user_management: administrator_actions_enabled,
                os_local_group_management: administrator_actions_enabled,
                administrator_actions_enabled,
            },
            selected_username: None,
            selected_group_name: None,
            warnings,
        }
    }

    pub fn synoptikon_integrated() -> Self {
        Self {
            host_mode: UsersGroupsHostModeView::SynoptikonIntegrated,
            authentication_framework: ProsopikonAuthenticationFramework::Prosopikon,
            device_token_requirement: ProsopikonDeviceTokenRequirement::NotRequired,
            current_user: None,
            users: Vec::new(),
            groups: Vec::new(),
            groups_file_path: "/opt/dasobjectstore/groups.json".to_string(),
            writer_groups: Vec::new(),
            operations: Vec::new(),
            capabilities: UsersGroupsCapabilitiesView {
                product_local_user_registration: false,
                os_local_user_management: false,
                os_local_group_management: false,
                administrator_actions_enabled: false,
            },
            selected_username: None,
            selected_group_name: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsersGroupsHostModeView {
    Standalone,
    SynoptikonIntegrated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalUserAuthorityView {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

impl From<LocalUserMetadata> for LocalUserAuthorityView {
    fn from(user: LocalUserMetadata) -> Self {
        Self {
            username: user.username,
            groups: user.groups,
            sudo_administrator: user.sudo_administrator,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneUserAccountView {
    pub username: String,
    pub registered: bool,
    pub created_at_unix_seconds: i64,
    pub registered_at_unix_seconds: Option<i64>,
    pub active_session_count: usize,
}

impl From<UserSummary> for StandaloneUserAccountView {
    fn from(user: UserSummary) -> Self {
        Self {
            username: user.username,
            registered: user.registered,
            created_at_unix_seconds: user.created_at_unix_seconds,
            registered_at_unix_seconds: user.registered_at_unix_seconds,
            active_session_count: user.active_session_count,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalGroupMembershipView {
    pub group_name: String,
    pub current_user_member: bool,
    pub sudo_administrator_group: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalGroupOperationView {
    pub kind: LocalGroupOperationKindView,
    pub label: String,
    pub requires_sudo_administrator: bool,
    pub enabled: bool,
    pub blocked_reason: Option<String>,
}

impl LocalGroupOperationView {
    fn sudo_gated(
        kind: LocalGroupOperationKindView,
        label: impl Into<String>,
        administrator_actions_enabled: bool,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            requires_sudo_administrator: true,
            enabled: administrator_actions_enabled,
            blocked_reason: (!administrator_actions_enabled).then(|| {
                "Current OS user must be a sudo-derived DASObjectStore administrator.".to_string()
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalGroupOperationKindView {
    CreateLocalGroup,
    AssignLocalUserToGroup,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsersGroupsCapabilitiesView {
    pub product_local_user_registration: bool,
    pub os_local_user_management: bool,
    pub os_local_group_management: bool,
    pub administrator_actions_enabled: bool,
}

fn local_group_memberships(user: &LocalUserMetadata) -> Vec<LocalGroupMembershipView> {
    user.groups
        .iter()
        .map(|group| LocalGroupMembershipView {
            group_name: group.clone(),
            current_user_member: true,
            sudo_administrator_group: SUDO_ADMIN_GROUPS.contains(&group.as_str()),
        })
        .collect()
}

fn local_group_operations(administrator_actions_enabled: bool) -> Vec<LocalGroupOperationView> {
    vec![
        LocalGroupOperationView::sudo_gated(
            LocalGroupOperationKindView::CreateLocalGroup,
            "Create local writer/admin group",
            administrator_actions_enabled,
        ),
        LocalGroupOperationView::sudo_gated(
            LocalGroupOperationKindView::AssignLocalUserToGroup,
            "Assign local user to group",
            administrator_actions_enabled,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        workspace_navigation, ActivityTaskKindView, ActivityTaskStateView, ActivityTaskView,
        ActivityWorkspaceView, DisksWorkspaceView, EndpointsWorkspaceView,
        LocalGroupMembershipView, LocalGroupOperationKindView, LocalGroupOperationView,
        LocalUserAuthorityView, ObjectInventoryRowView, ObjectsWorkspaceView,
        OperationsWorkspaceKindView, OperationsWorkspacesView, OverviewWorkspaceView,
        StandaloneUserAccountView, StorePolicySummaryView, StoresWorkspaceView,
        UsersGroupsCapabilitiesView, UsersGroupsHostModeView, UsersGroupsWorkspaceView,
        OPERATIONS_WORKSPACES_SCHEMA_VERSION,
    };
    use crate::dashboard::{DashboardAttentionView, ObjectStateView, StorageGroupView};
    use crate::endpoints::{EndpointInventoryItemView, EndpointInventoryView};
    use crate::{LocalUserMetadata, UserSummary};

    #[test]
    fn builds_navigation_for_all_operations_workspaces() {
        let navigation = workspace_navigation(OperationsWorkspaceKindView::Endpoints);

        assert_eq!(navigation.len(), 7);
        assert_eq!(navigation[0].route_segment, "overview");
        assert_eq!(
            navigation[4].workspace,
            OperationsWorkspaceKindView::Endpoints
        );
        assert!(navigation[4].selected);
        assert_eq!(navigation[6].route_segment, "users-groups");
    }

    #[test]
    fn serializes_navigation_workspace_names_as_snake_case() {
        let encoded =
            serde_json::to_value(workspace_navigation(OperationsWorkspaceKindView::Disks))
                .expect("navigation serializes");

        assert_eq!(encoded[0]["workspace"], "overview");
        assert_eq!(encoded[1]["workspace"], "disks");
        assert_eq!(encoded[1]["selected"], true);
        assert_eq!(encoded[6]["workspace"], "users_groups");
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
            home: None,
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
                progress: None,
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
            Some(UsersGroupsWorkspaceView::synoptikon_integrated()),
        );

        let encoded = serde_json::to_value(view).expect("workspace payload serializes");

        assert_eq!(
            encoded["schema_version"],
            OPERATIONS_WORKSPACES_SCHEMA_VERSION
        );
        assert_eq!(encoded["active_workspace"], "overview");
        assert_eq!(
            encoded["navigation"].as_array().expect("navigation").len(),
            7
        );
        assert_eq!(encoded["stores"]["stores"][0]["store_id"], "raw-public");
        assert_eq!(
            encoded["activity"]["tasks"][0]["kind"],
            "endpoint_validation"
        );
        assert_eq!(
            encoded["users_groups"]["host_mode"],
            "synoptikon_integrated"
        );
    }

    #[test]
    fn builds_empty_overview_workspace_for_api_bootstrap() {
        let overview = OverviewWorkspaceView::empty();
        let encoded = serde_json::to_value(overview).expect("overview serializes");

        assert_eq!(encoded["home"], serde_json::Value::Null);
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
        assert_eq!(
            encoded["enclosures"].as_array().expect("enclosures").len(),
            0
        );
        assert_eq!(encoded["selected_disk_id"], serde_json::Value::Null);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }

    #[test]
    fn builds_empty_stores_workspace_for_api_bootstrap() {
        let stores = StoresWorkspaceView::empty();
        let encoded = serde_json::to_value(stores).expect("stores serializes");

        assert_eq!(encoded["stores"].as_array().expect("stores").len(), 0);
        assert_eq!(
            encoded["object_store_cards"]
                .as_array()
                .expect("object store cards")
                .len(),
            0
        );
        assert_eq!(encoded["create_object_store"]["enabled"], true);
        assert_eq!(
            encoded["create_object_store"]["action_kind"],
            "store_create"
        );
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

    #[test]
    fn builds_empty_endpoints_workspace_for_api_bootstrap() {
        let endpoints = EndpointsWorkspaceView::empty();
        let encoded = serde_json::to_value(endpoints).expect("endpoints serializes");

        assert_eq!(encoded["inventory"]["endpoint_count"], 0);
        assert_eq!(encoded["inventory"]["degraded_endpoint_count"], 0);
        assert_eq!(encoded["inventory"]["binding_count"], 0);
        assert_eq!(
            encoded["inventory"]["endpoints"]
                .as_array()
                .expect("endpoints")
                .len(),
            0
        );
    }

    #[test]
    fn builds_empty_activity_workspace_for_api_bootstrap() {
        let activity = ActivityWorkspaceView::empty();
        let encoded = serde_json::to_value(activity).expect("activity serializes");

        assert_eq!(encoded["ingest"], serde_json::Value::Null);
        assert_eq!(encoded["destage"], serde_json::Value::Null);
        assert_eq!(encoded["tasks"].as_array().expect("tasks").len(), 0);
        assert_eq!(encoded["warnings"].as_array().expect("warnings").len(), 0);
    }

    #[test]
    fn builds_standalone_users_groups_workspace() {
        let view = UsersGroupsWorkspaceView::standalone(
            Some(LocalUserMetadata::from_username_and_groups(
                "operator",
                vec!["mnemosyne".to_string(), "sudo".to_string()],
            )),
            vec![UserSummary {
                username: "operator".to_string(),
                registered: true,
                created_at_unix_seconds: 10,
                registered_at_unix_seconds: Some(20),
                active_session_count: 1,
            }],
            "/opt/dasobjectstore/groups.json".to_string(),
            vec![StorageGroupView {
                group_name: "mnemosyne".to_string(),
                display_name: "Mnemosyne".to_string(),
                source: "local_os".to_string(),
                current_user_member: true,
            }],
            Vec::new(),
        );

        assert_eq!(view.host_mode, UsersGroupsHostModeView::Standalone);
        assert_eq!(
            view.authentication_framework,
            prosopikon_core::ProsopikonAuthenticationFramework::Hybrid
        );
        assert_eq!(
            view.device_token_requirement,
            prosopikon_core::ProsopikonDeviceTokenRequirement::NotRequired
        );
        assert_eq!(
            view.current_user,
            Some(LocalUserAuthorityView {
                username: "operator".to_string(),
                groups: vec!["mnemosyne".to_string(), "sudo".to_string()],
                sudo_administrator: true,
            })
        );
        assert_eq!(
            view.users,
            vec![StandaloneUserAccountView {
                username: "operator".to_string(),
                registered: true,
                created_at_unix_seconds: 10,
                registered_at_unix_seconds: Some(20),
                active_session_count: 1,
            }]
        );
        assert_eq!(
            view.groups,
            vec![
                LocalGroupMembershipView {
                    group_name: "mnemosyne".to_string(),
                    current_user_member: true,
                    sudo_administrator_group: false,
                },
                LocalGroupMembershipView {
                    group_name: "sudo".to_string(),
                    current_user_member: true,
                    sudo_administrator_group: true,
                }
            ]
        );
        assert_eq!(view.writer_groups[0].group_name, "mnemosyne");
        assert!(view.writer_groups[0].current_user_member);
        assert_eq!(
            view.capabilities,
            UsersGroupsCapabilitiesView {
                product_local_user_registration: true,
                os_local_user_management: true,
                os_local_group_management: true,
                administrator_actions_enabled: true,
            }
        );
        assert_eq!(
            view.operations,
            vec![
                LocalGroupOperationView {
                    kind: LocalGroupOperationKindView::CreateLocalGroup,
                    label: "Create local writer/admin group".to_string(),
                    requires_sudo_administrator: true,
                    enabled: true,
                    blocked_reason: None,
                },
                LocalGroupOperationView {
                    kind: LocalGroupOperationKindView::AssignLocalUserToGroup,
                    label: "Assign local user to group".to_string(),
                    requires_sudo_administrator: true,
                    enabled: true,
                    blocked_reason: None,
                }
            ]
        );
        assert!(view.warnings.is_empty());
    }

    #[test]
    fn standalone_users_groups_warns_for_non_admin_actor() {
        let view = UsersGroupsWorkspaceView::standalone(
            Some(LocalUserMetadata::from_username_and_groups(
                "viewer",
                vec!["users".to_string()],
            )),
            Vec::new(),
            "/opt/dasobjectstore/groups.json".to_string(),
            Vec::new(),
            Vec::new(),
        );

        assert!(!view.capabilities.administrator_actions_enabled);
        assert_eq!(view.warnings[0].code, "standalone_admin_authority_missing");
        assert_eq!(view.operations.len(), 2);
        assert!(view.operations.iter().all(|operation| {
            !operation.enabled
                && operation.requires_sudo_administrator
                && operation.blocked_reason.is_some()
        }));
        assert_eq!(
            view.operations[0].kind,
            LocalGroupOperationKindView::CreateLocalGroup
        );
        assert_eq!(
            view.operations[0].blocked_reason.as_deref(),
            Some("Current OS user must be a sudo-derived DASObjectStore administrator.")
        );
        assert_eq!(
            view.operations[1].kind,
            LocalGroupOperationKindView::AssignLocalUserToGroup
        );
        assert_eq!(
            view.operations[1].blocked_reason.as_deref(),
            Some("Current OS user must be a sudo-derived DASObjectStore administrator.")
        );
    }
}
