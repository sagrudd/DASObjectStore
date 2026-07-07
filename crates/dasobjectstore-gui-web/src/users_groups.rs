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

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct UsersGroupsWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(UsersGroupsWorkspace)]
pub fn users_groups_workspace(props: &UsersGroupsWorkspaceProps) -> Html {
    let api_path = users_groups_workspace_api_path(&props.api_base_path);
    let create_group_action_path = create_local_group_action_api_path(&props.api_base_path);
    let assign_user_action_path = assign_local_user_to_group_action_api_path(&props.api_base_path);

    html! {
        <section class="dos-users-groups" data-api-route={api_path}>
            <header class="dos-users-groups__header">
                <h1>{ "Users/Groups" }</h1>
            </header>
            <div class="dos-users-groups__layout">
                <section class="dos-users-groups__panel" data-panel="current-os-authority">
                    <h2>{ "Current OS Authority" }</h2>
                </section>
                <section class="dos-users-groups__panel" data-panel="product-local-users">
                    <h2>{ "Product-Local Users" }</h2>
                </section>
                <section class="dos-users-groups__panel" data-panel="local-groups">
                    <h2>{ "Local Groups" }</h2>
                </section>
                <section
                    class="dos-users-groups__panel"
                    data-panel="create-local-group"
                    data-operation={CREATE_LOCAL_GROUP_OPERATION}
                    data-action-route={create_group_action_path}
                >
                    <h2>{ "Create Group" }</h2>
                </section>
                <section
                    class="dos-users-groups__panel"
                    data-panel="assign-local-user-to-group"
                    data-operation={ASSIGN_LOCAL_USER_TO_GROUP_OPERATION}
                    data-action-route={assign_user_action_path}
                >
                    <h2>{ "Assign User to Group" }</h2>
                </section>
                <section class="dos-users-groups__panel" data-panel="administrator-readiness">
                    <h2>{ "Administrator Readiness" }</h2>
                </section>
                <section class="dos-users-groups__panel" data-panel="writer-policy-readiness">
                    <h2>{ "Writer Policy Readiness" }</h2>
                </section>
            </div>
        </section>
    }
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
