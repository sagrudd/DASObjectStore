mod model;
mod os_local;
mod store;
#[cfg(test)]
mod tests;

pub use model::{
    AuthRegistry, AuthTokenResetReport, AuthenticatedUser, LoginResponse, LogoutResponse,
    RegisterResponse, RegistrationTokenRecord, SessionCheckResponse, SessionTokenRecord,
    UserSummary,
};
pub use os_local::{
    discover_current_local_user, discover_local_user, local_user_metadata_from_unix_account_files,
    LocalPasswordAuthError, LocalUserDiscoveryError, LocalUserMetadata,
    PamLocalPasswordAuthenticator, SUDO_ADMIN_GROUPS,
};
#[cfg(target_os = "linux")]
pub use os_local::{
    DEFAULT_DASOBJECTSTORE_LOCAL_AUTH_HELPER_PATH, DEFAULT_PROSOPIKON_LOCAL_AUTH_HELPER_PATH,
    PROSOPIKON_LOCAL_AUTH_HELPER_BYPASS_ENV, PROSOPIKON_LOCAL_AUTH_HELPER_ENV,
};
pub use store::{LocalAuthStore, LocalAuthStoreError};
