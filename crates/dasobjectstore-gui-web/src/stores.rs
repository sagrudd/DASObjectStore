//! Compatibility helpers for the legacy Stores workspace route.
//!
//! The redesigned browser console renders ObjectStore administration through
//! `workspace::ObjectStoresPage`. Keep these route helpers stable for API and
//! downstream compatibility, but do not reintroduce a standalone Yew holder
//! surface for `workspaces/stores`.

pub const STORES_WORKSPACE_ROUTE: &str = "workspaces/stores";
pub const STORE_CREATE_ACTION_ROUTE: &str = "actions/plan";
pub const SUBOBJECT_CREATE_ACTION_ROUTE: &str = "actions/plan";

pub fn stores_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        STORES_WORKSPACE_ROUTE
    )
}

pub fn store_create_action_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        STORE_CREATE_ACTION_ROUTE
    )
}

pub fn subobject_create_action_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        SUBOBJECT_CREATE_ACTION_ROUTE
    )
}

#[cfg(test)]
mod tests {
    use super::{
        store_create_action_api_path, stores_workspace_api_path, subobject_create_action_api_path,
    };

    #[test]
    fn builds_stores_workspace_api_path() {
        assert_eq!(
            stores_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/stores"
        );
    }

    #[test]
    fn builds_store_create_action_api_path() {
        assert_eq!(
            store_create_action_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/actions/plan"
        );
    }

    #[test]
    fn builds_subobject_create_action_api_path() {
        assert_eq!(
            subobject_create_action_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/actions/plan"
        );
    }
}
