mod model;
mod os_local;
mod store;
#[cfg(test)]
mod tests;
mod token;

pub use model::{
    AuthRegistry, AuthTokenResetReport, AuthenticatedUser, LoginResponse, LogoutResponse,
    RegisterResponse, RegistrationTokenRecord, SessionCheckResponse, SessionTokenRecord,
    UserSummary,
};
pub use os_local::{
    discover_current_local_user, local_user_metadata_from_unix_account_files,
    LocalUserDiscoveryError, LocalUserMetadata, SUDO_ADMIN_GROUPS,
};
pub use store::{LocalAuthStore, LocalAuthStoreError};
