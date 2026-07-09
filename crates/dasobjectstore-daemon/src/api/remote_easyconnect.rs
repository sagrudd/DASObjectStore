use crate::auth::{
    authorize_store_read, authorize_store_write, DaemonLocalActor, DaemonStoreAccessPolicy,
};
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

pub const REMOTE_EASYCONNECT_DISCOVERY_ROUTE: &str = "/api/v1/remote/easyconnect/discovery";
pub const REMOTE_EASYCONNECT_PAIRINGS_ROUTE: &str = "/api/v1/remote/easyconnect/pairings";
pub const REMOTE_EASYCONNECT_PAIRING_APPROVAL_ROUTE_TEMPLATE: &str =
    "/api/v1/remote/easyconnect/pairings/{pairing_id}/approve";
pub const REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE: &str =
    "/api/v1/remote/easyconnect/pairings/exchange";
pub const REMOTE_EASYCONNECT_SESSIONS_ROUTE: &str = "/api/v1/remote/easyconnect/sessions";
pub const REMOTE_EASYCONNECT_SESSION_ROUTE_TEMPLATE: &str =
    "/api/v1/remote/easyconnect/sessions/{session_id}";
pub const REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE: &str =
    "/api/v1/remote/easyconnect/sessions/{session_id}/renew";
pub const REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS: u64 = 60;
pub const REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS: u64 = 8 * 60 * 60;
pub const REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS: u64 = 24 * 60 * 60;
pub const REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS: u64 = 60 * 60;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectDiscoveryRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectDiscoveryResponse {
    pub appliance_id: String,
    pub product_id: String,
    pub display_name: String,
    pub pairing_create_url: String,
    pub pairing_exchange_url: String,
    pub session_revoke_url_template: String,
    pub session_renew_url_template: String,
    pub default_session_lifetime_seconds: u64,
    pub session_policy: RemoteEasyconnectSessionPolicy,
    pub auth_providers: Vec<RemoteEasyconnectAuthProvider>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSessionPolicy {
    pub default_lifetime_seconds: u64,
    pub min_lifetime_seconds: u64,
    pub max_lifetime_seconds: u64,
    pub renewal_lead_seconds: u64,
    pub renewal_requires_password_reauthentication: bool,
    pub renewal_token_rotates: bool,
}

impl Default for RemoteEasyconnectSessionPolicy {
    fn default() -> Self {
        Self {
            default_lifetime_seconds: REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS,
            min_lifetime_seconds: REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS,
            max_lifetime_seconds: REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS,
            renewal_lead_seconds: REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS,
            renewal_requires_password_reauthentication: false,
            renewal_token_rotates: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEasyconnectAuthProvider {
    StandaloneLocalUser,
    Synoptikon,
    Mneion,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectCreatePairingRequest {
    pub client_name: String,
    pub callback_url: String,
    pub requested_object_store: Option<String>,
    pub requested_session_lifetime_seconds: Option<u64>,
    pub client_request_id: Option<String>,
}

impl RemoteEasyconnectCreatePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("client_name", &self.client_name)?;
        require_http_url("callback_url", &self.callback_url)?;
        validate_optional_non_blank(
            "requested_object_store",
            self.requested_object_store.as_deref(),
        )?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        validate_requested_lifetime(self.requested_session_lifetime_seconds)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectCreatePairingResponse {
    pub pairing_id: String,
    pub browser_login_url: String,
    pub callback_url: String,
    pub expires_at_utc: String,
    pub polling_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectApprovePairingRequest {
    pub pairing_id: String,
    pub approved_actor: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub allowed_object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
    pub approval_expires_at_utc: String,
}

impl RemoteEasyconnectApprovePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("approved_actor", &self.approved_actor)?;
        require_non_blank("approval_expires_at_utc", &self.approval_expires_at_utc)?;
        if self.allowed_object_stores.is_empty() {
            return Err(RemoteEasyconnectValidationError::EmptyObjectStoreGrants);
        }
        for grant in &self.allowed_object_stores {
            grant.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectApprovePairingResponse {
    pub pairing_id: String,
    pub exchange_code: String,
    pub callback_url: String,
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangePairingRequest {
    pub pairing_id: String,
    pub exchange_code: String,
    pub client_request_id: Option<String>,
}

impl RemoteEasyconnectExchangePairingRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("exchange_code", &self.exchange_code)?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectExchangePairingResponse {
    pub appliance_id: String,
    pub appliance_base_url: String,
    pub session: RemoteEasyconnectSession,
    pub object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRevokeSessionRequest {
    pub session_id: String,
    pub reason: Option<String>,
}

impl RemoteEasyconnectRevokeSessionRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("session_id", &self.session_id)?;
        validate_optional_non_blank("reason", self.reason.as_deref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRevokeSessionResponse {
    pub session_id: String,
    pub revoked: bool,
    pub revoked_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRenewSessionRequest {
    pub session_id: String,
    pub renewal_token: String,
    pub requested_lifetime_seconds: Option<u64>,
}

impl RemoteEasyconnectRenewSessionRequest {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("session_id", &self.session_id)?;
        require_non_blank("renewal_token", &self.renewal_token)?;
        validate_requested_lifetime(self.requested_lifetime_seconds)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectRenewSessionResponse {
    pub session: RemoteEasyconnectSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSession {
    pub session_id: String,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub credentials: RemoteEasyconnectSessionCredentials,
    pub renewal: RemoteEasyconnectSessionRenewal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSessionCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectSessionRenewal {
    pub renew_url: String,
    pub renew_after_utc: String,
    pub renewal_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectObjectStoreGrant {
    pub object_store: String,
    pub bucket: String,
    pub can_read: bool,
    pub can_write: bool,
    pub writer_group: Option<String>,
    pub object_type: String,
}

impl RemoteEasyconnectObjectStoreGrant {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("object_store", &self.object_store)?;
        require_non_blank("bucket", &self.bucket)?;
        validate_optional_non_blank("writer_group", self.writer_group.as_deref())?;
        require_non_blank("object_type", &self.object_type)?;
        if !self.can_read && !self.can_write {
            return Err(RemoteEasyconnectValidationError::GrantWithoutAccess {
                object_store: self.object_store.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectObjectStoreAccessPolicy {
    pub object_store: String,
    pub bucket: String,
    pub reader_group: Option<String>,
    pub writer_group: Option<String>,
    pub admin_group: Option<String>,
    pub public_read: bool,
    pub writable: bool,
    pub object_type: String,
}

impl RemoteEasyconnectObjectStoreAccessPolicy {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectValidationError> {
        require_non_blank("object_store", &self.object_store)?;
        require_non_blank("bucket", &self.bucket)?;
        validate_optional_non_blank("reader_group", self.reader_group.as_deref())?;
        validate_optional_non_blank("writer_group", self.writer_group.as_deref())?;
        validate_optional_non_blank("admin_group", self.admin_group.as_deref())?;
        require_non_blank("object_type", &self.object_type)
    }

    fn daemon_policy(&self) -> DaemonStoreAccessPolicy {
        let mut policy = DaemonStoreAccessPolicy::new(
            StoreId::new(self.object_store.clone())
                .expect("object_store was checked non-blank before building daemon access policy"),
        )
        .with_public_read(self.public_read);
        if let Some(reader_group) = &self.reader_group {
            policy = policy.with_reader_group(reader_group.clone());
        }
        if let Some(writer_group) = &self.writer_group {
            policy = policy.with_writer_group(writer_group.clone());
        }
        if let Some(admin_group) = &self.admin_group {
            policy = policy.with_admin_group(admin_group.clone());
        }
        policy
    }
}

pub fn remote_easyconnect_object_store_grants_for_actor(
    actor: &DaemonLocalActor,
    stores: &[RemoteEasyconnectObjectStoreAccessPolicy],
) -> Result<Vec<RemoteEasyconnectObjectStoreGrant>, RemoteEasyconnectValidationError> {
    let mut grants = Vec::new();

    for store in stores {
        store.validate()?;
        let policy = store.daemon_policy();
        let can_read = authorize_store_read(actor, &policy).is_ok();
        let can_write = store.writable && authorize_store_write(actor, &policy).is_ok();
        if can_read || can_write {
            grants.push(RemoteEasyconnectObjectStoreGrant {
                object_store: store.object_store.clone(),
                bucket: store.bucket.clone(),
                can_read,
                can_write,
                writer_group: store.writer_group.clone(),
                object_type: store.object_type.clone(),
            });
        }
    }

    Ok(grants)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteEasyconnectValidationError {
    BlankField { field: &'static str },
    InvalidUrl { field: &'static str, value: String },
    InvalidRequestedLifetime { seconds: u64 },
    EmptyObjectStoreGrants,
    GrantWithoutAccess { object_store: String },
}

pub fn resolve_remote_easyconnect_session_lifetime_seconds(
    requested_seconds: Option<u64>,
) -> Result<u64, RemoteEasyconnectValidationError> {
    validate_requested_lifetime(requested_seconds)?;
    Ok(requested_seconds.unwrap_or(REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS))
}

pub fn remote_easyconnect_renew_after_offset_seconds(
    lifetime_seconds: u64,
) -> Result<u64, RemoteEasyconnectValidationError> {
    validate_requested_lifetime(Some(lifetime_seconds))?;
    let lead_seconds = REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS.min(lifetime_seconds / 2);
    Ok(lifetime_seconds.saturating_sub(lead_seconds).max(1))
}

impl std::fmt::Display for RemoteEasyconnectValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidUrl { field, value } => {
                write!(
                    formatter,
                    "{field} must be an http or https URL, got {value}"
                )
            }
            Self::InvalidRequestedLifetime { seconds } => write!(
                formatter,
                "requested session lifetime must be between 60 and 86400 seconds, got {seconds}"
            ),
            Self::EmptyObjectStoreGrants => {
                formatter.write_str("at least one object store grant is required")
            }
            Self::GrantWithoutAccess { object_store } => write!(
                formatter,
                "object store grant for {object_store} must allow read or write access"
            ),
        }
    }
}

impl std::error::Error for RemoteEasyconnectValidationError {}

fn require_non_blank(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectValidationError> {
    if value.trim().is_empty() {
        return Err(RemoteEasyconnectValidationError::BlankField { field });
    }
    Ok(())
}

fn validate_optional_non_blank(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), RemoteEasyconnectValidationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(RemoteEasyconnectValidationError::BlankField { field });
    }
    Ok(())
}

fn require_http_url(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectValidationError> {
    require_non_blank(field, value)?;
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(())
    } else {
        Err(RemoteEasyconnectValidationError::InvalidUrl {
            field,
            value: value.to_string(),
        })
    }
}

fn validate_requested_lifetime(
    seconds: Option<u64>,
) -> Result<(), RemoteEasyconnectValidationError> {
    if let Some(seconds) = seconds {
        if !(REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS
            ..=REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS)
            .contains(&seconds)
        {
            return Err(RemoteEasyconnectValidationError::InvalidRequestedLifetime { seconds });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        remote_easyconnect_object_store_grants_for_actor,
        remote_easyconnect_renew_after_offset_seconds,
        resolve_remote_easyconnect_session_lifetime_seconds, RemoteEasyconnectAuthProvider,
        RemoteEasyconnectCreatePairingRequest, RemoteEasyconnectExchangePairingRequest,
        RemoteEasyconnectObjectStoreAccessPolicy, RemoteEasyconnectObjectStoreGrant,
        RemoteEasyconnectSessionPolicy, RemoteEasyconnectValidationError,
        REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS, REMOTE_EASYCONNECT_PAIRINGS_ROUTE,
        REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE, REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE,
    };
    use crate::auth::DaemonLocalActor;

    #[test]
    fn validates_create_pairing_contract() {
        let request = RemoteEasyconnectCreatePairingRequest {
            client_name: "macbook-pro".to_string(),
            callback_url:
                "http://127.0.0.1:49321/products/dasobjectstore/remote/easyconnect/callback"
                    .to_string(),
            requested_object_store: Some("zymo_fecal_2025.05".to_string()),
            requested_session_lifetime_seconds: Some(28_800),
            client_request_id: Some("request-1".to_string()),
        };

        request.validate().expect("request validates");

        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["client_name"], "macbook-pro");
        assert_eq!(encoded["requested_session_lifetime_seconds"], 28_800);
        assert_eq!(
            REMOTE_EASYCONNECT_PAIRINGS_ROUTE,
            "/api/v1/remote/easyconnect/pairings"
        );
    }

    #[test]
    fn rejects_invalid_callback_url() {
        let request = RemoteEasyconnectCreatePairingRequest {
            client_name: "macbook-pro".to_string(),
            callback_url: "127.0.0.1:49321/callback".to_string(),
            requested_object_store: None,
            requested_session_lifetime_seconds: None,
            client_request_id: None,
        };

        let err = request.validate().expect_err("invalid URL rejected");

        assert!(matches!(
            err,
            RemoteEasyconnectValidationError::InvalidUrl {
                field: "callback_url",
                ..
            }
        ));
    }

    #[test]
    fn defaults_session_lifetime_to_eight_hours_and_renews_one_hour_before_expiry() {
        let policy = RemoteEasyconnectSessionPolicy::default();

        assert_eq!(
            policy.default_lifetime_seconds,
            REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS
        );
        assert_eq!(
            resolve_remote_easyconnect_session_lifetime_seconds(None).expect("default resolves"),
            28_800
        );
        assert_eq!(
            resolve_remote_easyconnect_session_lifetime_seconds(Some(3_600))
                .expect("requested resolves"),
            3_600
        );
        assert_eq!(
            remote_easyconnect_renew_after_offset_seconds(28_800).expect("renewal offset"),
            25_200
        );
        assert!(!policy.renewal_requires_password_reauthentication);
        assert!(policy.renewal_token_rotates);
    }

    #[test]
    fn short_sessions_become_renewable_halfway_through() {
        assert_eq!(
            remote_easyconnect_renew_after_offset_seconds(60).expect("minimum lifetime valid"),
            30
        );
    }

    #[test]
    fn serializes_auth_provider_names() {
        let encoded = serde_json::to_value(RemoteEasyconnectAuthProvider::StandaloneLocalUser)
            .expect("provider serializes");

        assert_eq!(encoded, "standalone_local_user");
    }

    #[test]
    fn validates_exchange_pairing_contract() {
        let request = RemoteEasyconnectExchangePairingRequest {
            pairing_id: "pair-1".to_string(),
            exchange_code: "code-1".to_string(),
            client_request_id: None,
        };

        request.validate().expect("request validates");
        assert_eq!(
            REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE,
            "/api/v1/remote/easyconnect/pairings/exchange"
        );
        assert_eq!(
            REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE,
            "/api/v1/remote/easyconnect/sessions/{session_id}/renew"
        );
    }

    #[test]
    fn rejects_grant_without_access() {
        let grant = RemoteEasyconnectObjectStoreGrant {
            object_store: "zymo".to_string(),
            bucket: "dos-zymo".to_string(),
            can_read: false,
            can_write: false,
            writer_group: None,
            object_type: "fastq".to_string(),
        };

        let err = grant.validate().expect_err("access required");

        assert!(matches!(
            err,
            RemoteEasyconnectValidationError::GrantWithoutAccess { .. }
        ));
    }

    #[test]
    fn filters_remote_grants_to_actor_read_and_write_membership() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne", "readers"]);
        let stores = vec![
            store("generated", "dos-generated")
                .with_reader_group("readers")
                .with_writer_group("mnemosyne"),
            store("archive", "dos-archive")
                .with_reader_group("readers")
                .with_writer_group("archive-writers"),
            store("private", "dos-private").with_writer_group("private-writers"),
        ];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 2);
        assert_eq!(grants[0].object_store, "generated");
        assert!(grants[0].can_read);
        assert!(grants[0].can_write);
        assert_eq!(grants[1].object_store, "archive");
        assert!(grants[1].can_read);
        assert!(!grants[1].can_write);
    }

    #[test]
    fn public_read_does_not_grant_remote_write_without_writer_membership() {
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);
        let stores = vec![store("public-cache", "dos-public-cache")
            .with_public_read(true)
            .with_writer_group("cache-writers")];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].object_store, "public-cache");
        assert!(grants[0].can_read);
        assert!(!grants[0].can_write);
    }

    #[test]
    fn locked_store_never_grants_remote_write_even_for_writer_member() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne"]);
        let stores = vec![store("locked", "dos-locked")
            .with_writer_group("mnemosyne")
            .with_writable(false)];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 1);
        assert!(grants[0].can_read);
        assert!(!grants[0].can_write);
    }

    #[test]
    fn admin_group_can_receive_remote_write_grant() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("operator")
            .with_groups(["dasobjectstore-admin"]);
        let stores = vec![store("generated", "dos-generated")
            .with_writer_group("mnemosyne")
            .with_admin_group("dasobjectstore-admin")];

        let grants =
            remote_easyconnect_object_store_grants_for_actor(&actor, &stores).expect("grants");

        assert_eq!(grants.len(), 1);
        assert!(grants[0].can_read);
        assert!(grants[0].can_write);
    }

    fn store(object_store: &str, bucket: &str) -> RemoteEasyconnectObjectStoreAccessPolicy {
        RemoteEasyconnectObjectStoreAccessPolicy {
            object_store: object_store.to_string(),
            bucket: bucket.to_string(),
            reader_group: None,
            writer_group: None,
            admin_group: None,
            public_read: false,
            writable: true,
            object_type: "fastq".to_string(),
        }
    }

    trait StorePolicyFixture {
        fn with_reader_group(self, group: &str) -> Self;
        fn with_writer_group(self, group: &str) -> Self;
        fn with_admin_group(self, group: &str) -> Self;
        fn with_public_read(self, public_read: bool) -> Self;
        fn with_writable(self, writable: bool) -> Self;
    }

    impl StorePolicyFixture for RemoteEasyconnectObjectStoreAccessPolicy {
        fn with_reader_group(mut self, group: &str) -> Self {
            self.reader_group = Some(group.to_string());
            self
        }

        fn with_writer_group(mut self, group: &str) -> Self {
            self.writer_group = Some(group.to_string());
            self
        }

        fn with_admin_group(mut self, group: &str) -> Self {
            self.admin_group = Some(group.to_string());
            self
        }

        fn with_public_read(mut self, public_read: bool) -> Self {
            self.public_read = public_read;
            self
        }

        fn with_writable(mut self, writable: bool) -> Self {
            self.writable = writable;
            self
        }
    }
}
