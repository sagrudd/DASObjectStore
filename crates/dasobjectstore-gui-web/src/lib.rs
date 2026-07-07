//! Yew frontend scaffold for Monas and Synoptikon delivery surfaces.

pub mod activity;
pub mod components;
pub mod disks;
pub mod endpoints;
pub mod entrypoint;
pub mod mount;
pub mod objects;
pub mod overview;
pub mod stores;
pub mod users_groups;

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
    users_groups_workspace_api_path, ASSIGN_LOCAL_USER_TO_GROUP_OPERATION,
    CREATE_LOCAL_GROUP_OPERATION, USERS_GROUPS_WORKSPACE_ROUTE,
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
        assert_eq!(version(), "0.0.0");
    }
}
