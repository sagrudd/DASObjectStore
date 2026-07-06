use crate::overview::{overview_workspace_api_path, OVERVIEW_WORKSPACE_ROUTE};

pub const POST_LOGIN_WORKSPACE_ID: &str = "overview";
pub const POST_LOGIN_WORKSPACE_ROUTE: &str = OVERVIEW_WORKSPACE_ROUTE;

pub fn post_login_workspace_api_path(api_base_path: &str) -> String {
    overview_workspace_api_path(api_base_path)
}

#[cfg(test)]
mod tests {
    use super::{
        post_login_workspace_api_path, POST_LOGIN_WORKSPACE_ID, POST_LOGIN_WORKSPACE_ROUTE,
    };

    #[test]
    fn post_login_entrypoint_is_operations_overview() {
        assert_eq!(POST_LOGIN_WORKSPACE_ID, "overview");
        assert_eq!(POST_LOGIN_WORKSPACE_ROUTE, "workspaces/overview");
        assert_eq!(
            post_login_workspace_api_path("/products/dasobjectstore/api/v1"),
            "/products/dasobjectstore/api/v1/workspaces/overview"
        );
    }

    #[test]
    fn post_login_entrypoint_is_not_a_landing_route() {
        for forbidden in ["landing", "home", "welcome", "marketing"] {
            assert!(!POST_LOGIN_WORKSPACE_ID.contains(forbidden));
            assert!(!POST_LOGIN_WORKSPACE_ROUTE.contains(forbidden));
        }
    }
}
