pub const USERS_GROUPS_WORKSPACE_ROUTE: &str = "workspaces/users-groups";

pub fn users_groups_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        USERS_GROUPS_WORKSPACE_ROUTE
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
                    data-operation="create_local_group"
                >
                    <h2>{ "Create Group" }</h2>
                </section>
                <section
                    class="dos-users-groups__panel"
                    data-panel="assign-local-user-to-group"
                    data-operation="assign_local_user_to_group"
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
    use super::users_groups_workspace_api_path;

    #[test]
    fn builds_users_groups_workspace_api_path() {
        assert_eq!(
            users_groups_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/users-groups"
        );
    }
}
