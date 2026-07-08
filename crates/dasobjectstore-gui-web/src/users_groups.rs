//! Compatibility helpers for the standalone Users/Groups route.
//!
//! Users and group readiness are now represented inside the coherent product
//! console, with ObjectStore writer-policy readiness surfaced on the canonical
//! ObjectStores page. These helpers remain for the authenticated API boundary
//! and future first-class Users/Groups work, but this module intentionally does
//! not expose a parallel Yew holder surface.

pub const USERS_GROUPS_WORKSPACE_ROUTE: &str = "workspaces/users-groups";
pub const CREATE_LOCAL_GROUP_ACTION_ROUTE: &str = "workspaces/users-groups/local-groups";
pub const ASSIGN_LOCAL_USER_TO_GROUP_ACTION_ROUTE: &str =
    "workspaces/users-groups/local-groups/members";
pub const CREATE_LOCAL_GROUP_OPERATION: &str = "create_local_group";
pub const ASSIGN_LOCAL_USER_TO_GROUP_OPERATION: &str = "assign_local_user_to_group";

pub fn users_groups_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        USERS_GROUPS_WORKSPACE_ROUTE
    )
}

pub fn create_local_group_action_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        CREATE_LOCAL_GROUP_ACTION_ROUTE
    )
}

pub fn assign_local_user_to_group_action_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ASSIGN_LOCAL_USER_TO_GROUP_ACTION_ROUTE
    )
}

#[cfg(test)]
mod tests {
    use super::{
        assign_local_user_to_group_action_api_path, create_local_group_action_api_path,
        users_groups_workspace_api_path, ASSIGN_LOCAL_USER_TO_GROUP_ACTION_ROUTE,
        ASSIGN_LOCAL_USER_TO_GROUP_OPERATION, CREATE_LOCAL_GROUP_ACTION_ROUTE,
        CREATE_LOCAL_GROUP_OPERATION,
    };

    #[test]
    fn builds_users_groups_workspace_api_path() {
        assert_eq!(
            users_groups_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/users-groups"
        );
    }

    #[test]
    fn exposes_group_management_operation_ids() {
        assert_eq!(CREATE_LOCAL_GROUP_OPERATION, "create_local_group");
        assert_eq!(
            ASSIGN_LOCAL_USER_TO_GROUP_OPERATION,
            "assign_local_user_to_group"
        );
    }

    #[test]
    fn exposes_group_management_action_routes() {
        assert_eq!(
            CREATE_LOCAL_GROUP_ACTION_ROUTE,
            "workspaces/users-groups/local-groups"
        );
        assert_eq!(
            ASSIGN_LOCAL_USER_TO_GROUP_ACTION_ROUTE,
            "workspaces/users-groups/local-groups/members"
        );
    }

    #[test]
    fn builds_create_local_group_action_api_path() {
        assert_eq!(
            create_local_group_action_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/users-groups/local-groups"
        );
    }

    #[test]
    fn builds_assign_local_user_to_group_action_api_path() {
        assert_eq!(
            assign_local_user_to_group_action_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/users-groups/local-groups/members"
        );
    }
}
