use crate::{LocalAuthStore, LocalAuthStoreError};
use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

pub const STANDALONE_USERNAME_HEADER: &str = "x-dasobjectstore-username";
pub const STANDALONE_SESSION_TOKEN_HEADER: &str = "x-dasobjectstore-session-token";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticatedActorAuthority {
    LocalStandalone,
    MonasStandalone,
    SynoptikonIntegrated,
}

impl AuthenticatedActorAuthority {
    /// Whether this authority identifies a username in the appliance-local OS
    /// namespace. Host roles are deliberately ignored; authorization is
    /// derived afresh from the local user's groups and sudo status.
    pub(crate) fn uses_local_os_policy(self) -> bool {
        matches!(self, Self::LocalStandalone | Self::MonasStandalone)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticatedGuiActor {
    pub subject_id: String,
    pub authority: AuthenticatedActorAuthority,
    pub roles: Vec<String>,
    pub expires_at_unix_seconds: Option<i64>,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FederatedHostSessionResponse {
    pub subject_id: String,
    pub authority: AuthenticatedActorAuthority,
    pub roles: Vec<String>,
    pub expires_at_unix_seconds: Option<i64>,
    pub correlation_id: Option<String>,
    /// Same-origin mutation token bound to the live host session. This is not
    /// a bearer credential and grants no storage authority on its own.
    pub csrf_token: String,
}

impl FederatedHostSessionResponse {
    pub fn from_host_actor(actor: AuthenticatedGuiActor, csrf_token: String) -> Self {
        Self {
            subject_id: actor.subject_id,
            authority: actor.authority,
            roles: actor.roles,
            expires_at_unix_seconds: actor.expires_at_unix_seconds,
            correlation_id: actor.correlation_id,
            csrf_token,
        }
    }
}

impl AuthenticatedGuiActor {
    pub fn local_standalone(username: impl Into<String>, expires_at_unix_seconds: i64) -> Self {
        Self {
            subject_id: username.into(),
            authority: AuthenticatedActorAuthority::LocalStandalone,
            roles: Vec::new(),
            expires_at_unix_seconds: Some(expires_at_unix_seconds),
            correlation_id: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthGuardError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthGuardRejection {
    pub status: StatusCode,
    pub error: AuthGuardError,
}

impl IntoResponse for AuthGuardRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}

impl<S> FromRequestParts<S> for AuthenticatedGuiActor
where
    S: Send + Sync,
{
    type Rejection = AuthGuardRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(verified) = parts
            .extensions
            .get::<crate::VerifiedHostAuthenticatedContext>()
        {
            let context = verified.context();
            let authority = match context.authority {
                crate::HostAuthenticationAuthority::MonasStandalone => {
                    AuthenticatedActorAuthority::MonasStandalone
                }
                crate::HostAuthenticationAuthority::SynoptikonIntegrated => {
                    AuthenticatedActorAuthority::SynoptikonIntegrated
                }
            };
            return Ok(Self {
                subject_id: context.subject_id.clone(),
                authority,
                roles: context.roles.clone(),
                expires_at_unix_seconds: Some(context.expires_at_unix_seconds),
                correlation_id: Some(context.correlation_id.clone()),
            });
        }
        let auth_store = parts
            .extensions
            .get::<LocalAuthStore>()
            .ok_or_else(missing_auth_context)?;
        let username = required_header(&parts.headers, STANDALONE_USERNAME_HEADER)?;
        let session_token = standalone_session_token(&parts.headers)?;
        let session = auth_store
            .verify_session(username, session_token)
            .map_err(local_auth_rejection)?;

        Ok(Self::local_standalone(
            session.username,
            session.expires_at_unix_seconds,
        ))
    }
}

fn standalone_session_token(headers: &HeaderMap) -> Result<&str, AuthGuardRejection> {
    if let Some(session_token) = optional_header(headers, STANDALONE_SESSION_TOKEN_HEADER)? {
        return Ok(session_token);
    }

    let authorization = required_header(headers, AUTHORIZATION.as_str())?;
    authorization
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(invalid_authorization_header)
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, AuthGuardRejection> {
    optional_header(headers, name)?.ok_or_else(missing_credentials)
}

fn optional_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a str>, AuthGuardRejection> {
    headers
        .get(name)
        .map(|value| value.to_str().map_err(|_| invalid_header(name)))
        .transpose()
}

fn local_auth_rejection(err: LocalAuthStoreError) -> AuthGuardRejection {
    match err {
        LocalAuthStoreError::Io { .. }
        | LocalAuthStoreError::Json(_)
        | LocalAuthStoreError::ProsopikonStore(_)
        | LocalAuthStoreError::PasswordHash => rejection(
            StatusCode::INTERNAL_SERVER_ERROR,
            "auth_store_error",
            "local authentication store failed",
        ),
        _ => rejection(
            StatusCode::UNAUTHORIZED,
            "invalid_session",
            "authentication session is invalid or expired",
        ),
    }
}

fn missing_auth_context() -> AuthGuardRejection {
    rejection(
        StatusCode::UNAUTHORIZED,
        "missing_auth_context",
        "authenticated actor context is required",
    )
}

fn missing_credentials() -> AuthGuardRejection {
    rejection(
        StatusCode::UNAUTHORIZED,
        "missing_credentials",
        "authentication credentials are required",
    )
}

fn invalid_authorization_header() -> AuthGuardRejection {
    rejection(
        StatusCode::UNAUTHORIZED,
        "invalid_authorization_header",
        "authorization header must use Bearer authentication",
    )
}

fn invalid_header(name: &str) -> AuthGuardRejection {
    rejection(
        StatusCode::BAD_REQUEST,
        "invalid_header",
        format!("header {name} must be valid UTF-8"),
    )
}

fn rejection(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> AuthGuardRejection {
    AuthGuardRejection {
        status,
        error: AuthGuardError {
            code: code.into(),
            message: message.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuthenticatedActorAuthority, AuthenticatedGuiActor, STANDALONE_SESSION_TOKEN_HEADER,
        STANDALONE_USERNAME_HEADER,
    };
    use crate::{
        accept_host_authenticated_context, HostAuthenticatedContext, HostAuthenticationAuthority,
        HostAuthenticationContextVerifier, LocalAuthStore, HOST_AUTH_AUDIENCE,
        HOST_AUTH_CONTEXT_SCHEMA_VERSION,
    };
    use axum::{
        body::Body,
        extract::Extension,
        http::{header::AUTHORIZATION, Request, StatusCode},
        response::Json,
        routing::get,
        Router,
    };
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[tokio::test]
    async fn extractor_accepts_valid_standalone_session() {
        let root = temp_root("standalone-valid");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = protected_router().layer(Extension(auth_store));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/protected")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(AUTHORIZATION, format!("Bearer {}", login.session_token))
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);

        cleanup(&root);
    }

    #[tokio::test]
    async fn extractor_accepts_explicit_session_token_header() {
        let root = temp_root("standalone-token-header");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = protected_router().layer(Extension(auth_store));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/protected")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);

        cleanup(&root);
    }

    #[tokio::test]
    async fn extractor_rejects_missing_credentials() {
        let root = temp_root("standalone-missing");
        let auth_store = registered_auth_store(&root);
        let app = protected_router().layer(Extension(auth_store));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/protected")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn extractor_rejects_invalid_bearer_session() {
        let root = temp_root("standalone-invalid");
        let auth_store = registered_auth_store(&root);
        let app = protected_router().layer(Extension(auth_store));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/protected")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(AUTHORIZATION, "Bearer wrong")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn extractor_accepts_verified_monas_and_synoptikon_contexts() {
        for authority in [
            HostAuthenticationAuthority::MonasStandalone,
            HostAuthenticationAuthority::SynoptikonIntegrated,
        ] {
            let context = host_context(authority);
            let verified = accept_host_authenticated_context(context, 1_500, &LiveVerifier)
                .expect("live host session accepted");
            let app = protected_router().layer(Extension(verified));

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/protected")
                        .body(Body::empty())
                        .expect("request builds"),
                )
                .await
                .expect("request completes");

            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn extractor_rejects_unverified_host_context() {
        let app = protected_router().layer(Extension(host_context(
            HostAuthenticationAuthority::MonasStandalone,
        )));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/protected")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    async fn protected(actor: AuthenticatedGuiActor) -> Json<ProtectedResponse> {
        Json(ProtectedResponse {
            subject_id: actor.subject_id,
            authority: actor.authority,
        })
    }

    fn protected_router() -> Router {
        Router::new().route("/protected", get(protected))
    }

    fn registered_auth_store(root: &Path) -> LocalAuthStore {
        let auth_store = LocalAuthStore::new(root);
        auth_store.create_user("admin").expect("user created");
        let token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("registration token issued");
        auth_store
            .register_with_token("admin", &token, "secret")
            .expect("user registered");
        auth_store
    }

    struct LiveVerifier;

    impl HostAuthenticationContextVerifier for LiveVerifier {
        fn verify_live_session(&self, _context: &HostAuthenticatedContext) -> Result<(), String> {
            Ok(())
        }
    }

    fn host_context(authority: HostAuthenticationAuthority) -> HostAuthenticatedContext {
        HostAuthenticatedContext {
            schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_string(),
            authority,
            issuer: authority.issuer().to_string(),
            audience: HOST_AUTH_AUDIENCE.to_string(),
            subject_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
            roles: vec!["storage_operator".to_string()],
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 2_000,
            correlation_id: "corr-1".to_string(),
            csrf_binding_sha256: format!("sha256:{}", "a".repeat(64)),
        }
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct ProtectedResponse {
        subject_id: String,
        authority: AuthenticatedActorAuthority,
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-auth-guard-{label}-{}-{}",
            std::process::id(),
            unix_now_nanos()
        ))
    }

    fn unix_now_nanos() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos()
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_standalone_authorities_use_appliance_local_os_policy() {
        assert!(AuthenticatedActorAuthority::LocalStandalone.uses_local_os_policy());
        assert!(AuthenticatedActorAuthority::MonasStandalone.uses_local_os_policy());
        assert!(!AuthenticatedActorAuthority::SynoptikonIntegrated.uses_local_os_policy());
    }
}
