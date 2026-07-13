//! Local Access workspace DTOs kept separate from the general Web API contracts.

use super::*;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StorageGroupResponse {
    pub group_name: String,
    pub display_name: String,
    pub source: String,
    pub current_user_member: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct UsersGroupsWorkspaceResponse {
    pub host_mode: String,
    pub authentication_framework: ProsopikonAuthenticationFramework,
    pub device_token_requirement: ProsopikonDeviceTokenRequirement,
    pub current_user: Option<LocalUserAuthorityResponse>,
    pub users: Vec<StandaloneUserAccountResponse>,
    pub groups: Vec<LocalGroupMembershipResponse>,
    pub groups_file_path: String,
    pub writer_groups: Vec<StorageGroupResponse>,
    pub operations: Vec<LocalGroupOperationResponse>,
    pub capabilities: UsersGroupsCapabilitiesResponse,
    pub selected_username: Option<String>,
    pub selected_group_name: Option<String>,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalUserAuthorityResponse {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StandaloneUserAccountResponse {
    pub username: String,
    pub registered: bool,
    pub created_at_unix_seconds: i64,
    pub registered_at_unix_seconds: Option<i64>,
    pub active_session_count: usize,
    #[serde(default)]
    pub qualification_state: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub sudo_administrator: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupMembershipResponse {
    pub group_name: String,
    pub current_user_member: bool,
    pub sudo_administrator_group: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupOperationResponse {
    pub kind: String,
    pub label: String,
    pub requires_sudo_administrator: bool,
    pub enabled: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct UsersGroupsCapabilitiesResponse {
    pub product_local_user_registration: bool,
    pub os_local_user_management: bool,
    pub os_local_group_management: bool,
    pub administrator_actions_enabled: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CreateLocalGroupRequest {
    pub group_name: String,
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssignLocalUserToGroupRequest {
    pub group_name: String,
    pub username: String,
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupAdminResponse {
    pub accepted: LocalGroupAdminAcceptedResponse,
    pub operation: String,
    pub group_name: String,
    pub username: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupAdminAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}
