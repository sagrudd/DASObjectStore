#[cfg(any(target_arch = "wasm32", test))]
use crate::api::BioinformaticsWorkspaceResponse;
#[cfg(target_arch = "wasm32")]
use crate::api::ObjectBrowserResponse;
#[cfg(any(target_arch = "wasm32", test))]
use crate::api::{
    ActivityWorkspaceResponse, EnclosureDriveSlotResponse, EnclosuresPageResponse,
    HomeDashboardResponse, ObjectBrowserPlacementResponse, ObjectStoresPageResponse,
    ThroughputDayResponse, UsersGroupsWorkspaceResponse,
};
#[cfg(target_arch = "wasm32")]
use crate::api::{
    AdminJobCancelRequest, AdminJobCancelResponse, AdminJobStatusResponse, AdminJobSummary,
    AssignLocalUserToGroupRequest, CreateLocalGroupRequest, CreateObjectStoreRequest,
    CreateObjectStoreResponse, DasEnclosureCardResponse, DasEnclosureDetailResponse,
    EnclosurePrepareHddDevice, EnclosurePrepareRequest, EnclosurePrepareResponse,
    GuiActionPlanRequest, GuiActionPlanResponse, LocalGroupAdminResponse, ObjectStoreCardResponse,
    ObjectStoreIngestPolicyRequest, ObjectStoreIngestPolicyResponse,
    RemoteUploadIngressPolicyResponse, RemoteUploadObjectStoreResponse,
    RemoteUploadWorkspaceResponse,
};
#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::api::{
    AdminJobCancelResponse, AdminJobStatusResponse, AdminJobSummary, DasEnclosureCardResponse,
    EnclosurePrepareResponse,
};
#[cfg(any(target_arch = "wasm32", test))]
use crate::api::{ObjectBrowserFileNodeResponse, ObjectBrowserFolderNodeResponse};
use crate::mount::FrontendHost;
#[cfg(target_arch = "wasm32")]
use gloo_timers::callback::{Interval, Timeout};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use web_sys::{
    Blob, BlobPropertyBag, DragEvent, File, HtmlAnchorElement, HtmlInputElement, HtmlSelectElement,
    Url,
};
#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

pub const HOME_WORKSPACE_ROUTE: &str = "dashboard/home";
pub const ENCLOSURES_WORKSPACE_ROUTE: &str = "dashboard/enclosures";
pub const OBJECTSTORES_WORKSPACE_ROUTE: &str = "dashboard/object-stores";
#[cfg(target_arch = "wasm32")]
const HOME_DASHBOARD_REFRESH_MS: u32 = 30_000;
#[cfg(target_arch = "wasm32")]
const HOME_THROUGHPUT_CHART_WIDTH: f64 = 640.0;
#[cfg(target_arch = "wasm32")]
const HOME_THROUGHPUT_CHART_HEIGHT: f64 = 180.0;
#[cfg(any(target_arch = "wasm32", test))]
const HOME_THROUGHPUT_CHART_LEFT: f64 = 48.0;
#[cfg(any(target_arch = "wasm32", test))]
const HOME_THROUGHPUT_CHART_RIGHT: f64 = 616.0;
#[cfg(any(target_arch = "wasm32", test))]
const HOME_THROUGHPUT_CHART_TOP: f64 = 24.0;
#[cfg(any(target_arch = "wasm32", test))]
const HOME_THROUGHPUT_CHART_BOTTOM: f64 = 144.0;
pub const ACTIVITY_WORKSPACE_ROUTE: &str = crate::activity::ACTIVITY_WORKSPACE_ROUTE;
pub const ENDPOINTS_WORKSPACE_ROUTE: &str = crate::endpoints::ENDPOINTS_WORKSPACE_ROUTE;
pub const BIOINFORMATICS_WORKSPACE_ROUTE: &str = "workspaces/bioinformatics";
pub const REMOTE_UPLOAD_WORKSPACE_ROUTE: &str = "workspaces/remote-upload";
pub const USERS_GROUPS_WORKSPACE_ROUTE: &str = crate::users_groups::USERS_GROUPS_WORKSPACE_ROUTE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspacePage {
    Home,
    Enclosures,
    ObjectStores,
    Activity,
    RemoteUpload,
    Endpoints,
    UsersGroups,
    Bioinformatics,
}

impl WorkspacePage {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Enclosures => "enclosures",
            Self::ObjectStores => "objectstores",
            Self::Activity => "activity",
            Self::RemoteUpload => "remote-upload",
            Self::Endpoints => "endpoints",
            Self::UsersGroups => "users-groups",
            Self::Bioinformatics => "bioinformatics",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Enclosures => "Enclosures",
            Self::ObjectStores => "ObjectStores",
            Self::Activity => "Activity",
            Self::RemoteUpload => "Remote Upload",
            Self::Endpoints => "Connections",
            Self::UsersGroups => "Local Access",
            Self::Bioinformatics => "Bioinformatics",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Enclosures => "Enclosures",
            Self::ObjectStores => "ObjectStores",
            Self::Activity => "Activity",
            Self::RemoteUpload => "Remote Upload",
            Self::Endpoints => "Connections",
            Self::UsersGroups => "Local Access",
            Self::Bioinformatics => "Bioinformatics",
        }
    }

    pub fn api_path(self, api_base_path: &str) -> String {
        match self {
            Self::Home => home_workspace_api_path(api_base_path),
            Self::Enclosures => enclosures_workspace_api_path(api_base_path),
            Self::ObjectStores => objectstores_workspace_api_path(api_base_path),
            Self::Activity => activity_workspace_api_path(api_base_path),
            Self::RemoteUpload => remote_upload_workspace_base_api_path(api_base_path),
            Self::Endpoints => endpoints_workspace_api_path(api_base_path),
            Self::UsersGroups => users_groups_workspace_api_path(api_base_path),
            Self::Bioinformatics => bioinformatics_workspace_api_path(api_base_path),
        }
    }
}

pub const PRIMARY_NAVIGATION: [WorkspacePage; 7] = [
    WorkspacePage::Home,
    WorkspacePage::Enclosures,
    WorkspacePage::ObjectStores,
    WorkspacePage::Endpoints,
    WorkspacePage::Activity,
    WorkspacePage::UsersGroups,
    WorkspacePage::Bioinformatics,
];

pub const INTEGRATED_PRIMARY_NAVIGATION: [WorkspacePage; 5] = [
    WorkspacePage::Home,
    WorkspacePage::Enclosures,
    WorkspacePage::ObjectStores,
    WorkspacePage::Activity,
    WorkspacePage::Bioinformatics,
];

pub fn primary_navigation_for_host(host: FrontendHost) -> &'static [WorkspacePage] {
    match host {
        FrontendHost::Standalone => &PRIMARY_NAVIGATION,
        FrontendHost::Monas | FrontendHost::Synoptikon => &INTEGRATED_PRIMARY_NAVIGATION,
    }
}

pub fn home_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        HOME_WORKSPACE_ROUTE
    )
}

pub fn home_dashboard_api_path_with_window(api_base_path: &str, telemetry_window: &str) -> String {
    format!(
        "{}?telemetry_window={}",
        home_workspace_api_path(api_base_path),
        telemetry_window
    )
}

pub fn enclosures_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENCLOSURES_WORKSPACE_ROUTE
    )
}

pub fn objectstores_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        OBJECTSTORES_WORKSPACE_ROUTE
    )
}

pub fn activity_workspace_api_path(api_base_path: &str) -> String {
    crate::activity::activity_workspace_api_path(api_base_path)
}

fn remote_upload_workspace_base_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        REMOTE_UPLOAD_WORKSPACE_ROUTE
    )
}

pub fn remote_upload_workspace_api_path(api_base_path: &str, store_id: &str) -> String {
    format!(
        "{}?store_id={}",
        remote_upload_workspace_base_api_path(api_base_path),
        crate::encoding::percent_encode(store_id.trim())
    )
}

pub fn endpoints_workspace_api_path(api_base_path: &str) -> String {
    crate::endpoints::endpoints_workspace_api_path(api_base_path)
}

pub fn bioinformatics_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        BIOINFORMATICS_WORKSPACE_ROUTE
    )
}

pub fn users_groups_workspace_api_path(api_base_path: &str) -> String {
    crate::users_groups::users_groups_workspace_api_path(api_base_path)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiLoadState<T> {
    Loading,
    Success(T),
    Empty(String),
    PermissionDenied(String),
    TransportError(String),
    StaleData { value: T, message: String },
}

impl<T> ApiLoadState<T> {
    pub fn success(value: T) -> Self {
        Self::Success(value)
    }

    pub fn empty(message: impl Into<String>) -> Self {
        Self::Empty(message.into())
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied(message.into())
    }

    pub fn transport_error(message: impl Into<String>) -> Self {
        Self::TransportError(message.into())
    }

    pub fn stale_data(value: T, message: impl Into<String>) -> Self {
        Self::StaleData {
            value,
            message: message.into(),
        }
    }

    pub const fn state_name(&self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Success(_) => "success",
            Self::Empty(_) => "empty",
            Self::PermissionDenied(_) => "permission-denied",
            Self::TransportError(_) => "transport-error",
            Self::StaleData { .. } => "stale-data",
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn page_load_state_from_result<T, F>(
    result: Result<T, crate::api::ApiError>,
    empty_message: F,
) -> ApiLoadState<T>
where
    F: FnOnce(&T) -> Option<String>,
{
    match result {
        Ok(view) => match empty_message(&view) {
            Some(message) => ApiLoadState::empty(message),
            None => ApiLoadState::success(view),
        },
        Err(error) if error.is_permission_denied() => {
            ApiLoadState::permission_denied(error.message)
        }
        Err(error) => ApiLoadState::transport_error(error.message),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn page_load_state_from_result_with_stale<T, F>(
    previous: &ApiLoadState<T>,
    result: Result<T, crate::api::ApiError>,
    empty_message: F,
) -> ApiLoadState<T>
where
    T: Clone,
    F: FnOnce(&T) -> Option<String>,
{
    match result {
        Ok(view) => match empty_message(&view) {
            Some(message) => ApiLoadState::empty(message),
            None => ApiLoadState::success(view),
        },
        Err(error) if error.is_permission_denied() => {
            ApiLoadState::permission_denied(error.message)
        }
        Err(error) => match previous {
            ApiLoadState::Success(value) | ApiLoadState::StaleData { value, .. } => {
                ApiLoadState::stale_data(
                    value.clone(),
                    format!("Live dashboard refresh failed: {}. Showing the last successful snapshot; retry shortly.", error.message),
                )
            }
            _ => ApiLoadState::transport_error(error.message),
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
mod activity;
#[cfg(any(target_arch = "wasm32", test))]
mod activity_data;
#[cfg(any(target_arch = "wasm32", test))]
mod bioinformatics;
#[cfg(any(target_arch = "wasm32", test))]
mod components;
#[cfg(any(target_arch = "wasm32", test))]
mod dashboard;
#[cfg(any(target_arch = "wasm32", test))]
mod enclosures;
#[cfg(any(target_arch = "wasm32", test))]
mod home;
#[cfg(any(target_arch = "wasm32", test))]
mod object_browser;
#[cfg(any(target_arch = "wasm32", test))]
mod object_data;
#[cfg(any(target_arch = "wasm32", test))]
mod object_store_configure;
#[cfg(any(target_arch = "wasm32", test))]
mod object_store_create;
#[cfg(any(target_arch = "wasm32", test))]
mod object_stores;
#[cfg(any(target_arch = "wasm32", test))]
mod remote_upload;
#[cfg(any(target_arch = "wasm32", test))]
mod subobjects;
#[cfg(any(target_arch = "wasm32", test))]
mod users_groups;

#[cfg(any(target_arch = "wasm32", test))]
use activity::*;
#[cfg(any(target_arch = "wasm32", test))]
use activity_data::*;
#[cfg(any(target_arch = "wasm32", test))]
pub use activity_data::{EnclosureCardSummary, ObjectStoreCardSummary};
#[cfg(any(target_arch = "wasm32", test))]
use bioinformatics::*;
#[cfg(target_arch = "wasm32")]
use components::*;
#[cfg(any(target_arch = "wasm32", test))]
pub use dashboard::DashboardMetric;
#[cfg(any(target_arch = "wasm32", test))]
use dashboard::*;
#[cfg(any(target_arch = "wasm32", test))]
use enclosures::*;
#[cfg(target_arch = "wasm32")]
use home::*;
#[cfg(any(target_arch = "wasm32", test))]
use object_browser::*;
#[cfg(any(target_arch = "wasm32", test))]
use object_data::*;
#[cfg(target_arch = "wasm32")]
use object_store_configure::*;
#[cfg(target_arch = "wasm32")]
use object_store_create::*;
#[cfg(target_arch = "wasm32")]
use object_stores::*;
#[cfg(any(target_arch = "wasm32", test))]
use subobjects::*;
#[cfg(any(target_arch = "wasm32", test))]
use users_groups::*;

#[cfg(target_arch = "wasm32")]
pub use activity::ActivityPage;
#[cfg(target_arch = "wasm32")]
pub use bioinformatics::BioinformaticsPage;
#[cfg(target_arch = "wasm32")]
pub use enclosures::EnclosuresPage;
#[cfg(target_arch = "wasm32")]
pub use home::HomeDashboard;
#[cfg(target_arch = "wasm32")]
pub use object_stores::ObjectStoresPage;
#[cfg(target_arch = "wasm32")]
pub use remote_upload::RemoteUploadPage;
#[cfg(target_arch = "wasm32")]
pub use users_groups::UsersGroupsPage;

#[cfg(test)]
#[path = "workspace/tests.rs"]
mod tests;
