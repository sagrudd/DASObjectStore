//! Yew frontend scaffold for Monas and Synoptikon delivery surfaces.

pub mod activity;
mod api;
pub mod components;
pub mod disks;
pub mod endpoints;
pub mod entrypoint;
pub mod mount;
pub mod objects;
pub mod overview;
#[cfg(target_arch = "wasm32")]
mod session;
#[cfg(target_arch = "wasm32")]
mod storage;
pub mod stores;
pub mod users_groups;
pub mod workspace;

#[cfg(target_arch = "wasm32")]
pub mod app;

pub use activity::{activity_workspace_api_path, ACTIVITY_WORKSPACE_ROUTE};
#[cfg(target_arch = "wasm32")]
pub use app::App;
pub use disks::{disks_workspace_api_path, DISKS_WORKSPACE_ROUTE};
pub use endpoints::{endpoints_workspace_api_path, ENDPOINTS_WORKSPACE_ROUTE};
pub use entrypoint::{
    post_login_workspace_api_path, POST_LOGIN_WORKSPACE_ID, POST_LOGIN_WORKSPACE_ROUTE,
};
pub use mount::{FrontendHost, FrontendMount};
pub use objects::{objects_workspace_api_path, OBJECTS_WORKSPACE_ROUTE};
pub use overview::{overview_workspace_api_path, OVERVIEW_WORKSPACE_ROUTE};
pub use stores::{
    store_create_action_api_path, stores_workspace_api_path, subobject_create_action_api_path,
    STORES_WORKSPACE_ROUTE, STORE_CREATE_ACTION_ROUTE, SUBOBJECT_CREATE_ACTION_ROUTE,
};
pub use users_groups::{
    assign_local_user_to_group_action_api_path, create_local_group_action_api_path,
    users_groups_workspace_api_path, ASSIGN_LOCAL_USER_TO_GROUP_ACTION_ROUTE,
    ASSIGN_LOCAL_USER_TO_GROUP_OPERATION, CREATE_LOCAL_GROUP_ACTION_ROUTE,
    CREATE_LOCAL_GROUP_OPERATION, USERS_GROUPS_WORKSPACE_ROUTE,
};
pub use workspace::{
    bioinformatics_workspace_api_path, enclosures_workspace_api_path, fallback_dashboard_metrics,
    fallback_enclosures, fallback_object_stores, home_workspace_api_path,
    objectstores_workspace_api_path, DashboardMetric, EnclosureSummary, ObjectStoreSummary,
    WorkspacePage, BIOINFORMATICS_WORKSPACE_ROUTE, ENCLOSURES_WORKSPACE_ROUTE,
    HOME_WORKSPACE_ROUTE, OBJECTSTORES_WORKSPACE_ROUTE, PRIMARY_NAVIGATION,
};

/// Returns the GUI web crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
