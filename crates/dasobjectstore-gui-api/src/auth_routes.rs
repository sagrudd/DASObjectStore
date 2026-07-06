use crate::{
    LocalAuthStore, LocalAuthStoreError, LoginResponse, LogoutResponse, RegisterResponse,
    SessionCheckResponse,
};
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

pub fn standalone_gui_api_router(auth_store: LocalAuthStore) -> Router {
    crate::gui_api_router().merge(standalone_auth_router(auth_store))
}

pub fn standalone_auth_router(auth_store: LocalAuthStore) -> Router {
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/session", post(session))
        .with_state(auth_store)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterRequest {
    pub username: String,
    pub token: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub session_ttl_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LogoutRequest {
    pub username: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCheckRequest {
    pub username: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthRouteError {
    pub code: String,
    pub message: String,
}

async fn register(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .register_with_token(&request.username, &request.token, &request.password)
        .map(Json)
        .map_err(auth_route_error)
}

async fn login(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .login_with_session_ttl_seconds(
            &request.username,
            &request.password,
            request.session_ttl_seconds,
        )
        .map(Json)
        .map_err(auth_route_error)
}

async fn logout(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<LogoutRequest>,
) -> Result<Json<LogoutResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .logout(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

async fn session(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<SessionCheckRequest>,
) -> Result<Json<SessionCheckResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .verify_session(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

fn auth_route_error(err: LocalAuthStoreError) -> (StatusCode, Json<AuthRouteError>) {
    let status = match err {
        LocalAuthStoreError::UserNameRequired | LocalAuthStoreError::PasswordRequired => {
            StatusCode::BAD_REQUEST
        }
        LocalAuthStoreError::UserAlreadyExists { .. }
        | LocalAuthStoreError::UserAlreadyRegistered { .. } => StatusCode::CONFLICT,
        LocalAuthStoreError::UserNotFound { .. }
        | LocalAuthStoreError::UserNotRegistered { .. }
        | LocalAuthStoreError::InvalidRegistrationToken
        | LocalAuthStoreError::UsedRegistrationToken
        | LocalAuthStoreError::ExpiredRegistrationToken
        | LocalAuthStoreError::InvalidSessionToken
        | LocalAuthStoreError::ExpiredSessionToken
        | LocalAuthStoreError::InvalidPassword => StatusCode::UNAUTHORIZED,
        LocalAuthStoreError::Io { .. }
        | LocalAuthStoreError::Json(_)
        | LocalAuthStoreError::PasswordHash => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        Json(AuthRouteError {
            code: status.as_u16().to_string(),
            message: err.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        standalone_auth_router, LoginRequest, LogoutRequest, RegisterRequest, SessionCheckRequest,
    };
    use crate::{LocalAuthStore, LoginResponse};
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde::de::DeserializeOwned;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[tokio::test]
    async fn standalone_auth_routes_complete_login_session_logout_flow() {
        let root = temp_root("flow");
        let auth_store = LocalAuthStore::new(&root);
        auth_store.create_user("admin").expect("user created");
        let registration_token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("registration token issued");
        let app = standalone_auth_router(auth_store);

        let register = post_json::<crate::RegisterResponse>(
            app.clone(),
            "/api/register",
            &RegisterRequest {
                username: "admin".to_string(),
                token: registration_token,
                password: "secret".to_string(),
            },
        )
        .await;
        assert_eq!(register.username, "admin");

        let login = post_json::<LoginResponse>(
            app.clone(),
            "/api/login",
            &LoginRequest {
                username: "admin".to_string(),
                password: "secret".to_string(),
                session_ttl_seconds: Some(3_600),
            },
        )
        .await;

        let session = post_json::<crate::SessionCheckResponse>(
            app.clone(),
            "/api/session",
            &SessionCheckRequest {
                username: "admin".to_string(),
                session_token: login.session_token.clone(),
            },
        )
        .await;
        assert!(session.valid);

        let logout = post_json::<crate::LogoutResponse>(
            app.clone(),
            "/api/logout",
            &LogoutRequest {
                username: "admin".to_string(),
                session_token: login.session_token,
            },
        )
        .await;
        assert!(logout.disconnected);

        cleanup(&root);
    }

    #[tokio::test]
    async fn login_route_rejects_invalid_password() {
        let root = temp_root("invalid-password-route");
        let auth_store = LocalAuthStore::new(&root);
        auth_store.create_user("admin").expect("user created");
        let token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("token issued");
        auth_store
            .register_with_token("admin", &token, "secret")
            .expect("registered");
        let app = standalone_auth_router(auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LoginRequest {
                            username: "admin".to_string(),
                            password: "wrong".to_string(),
                            session_ttl_seconds: None,
                        })
                        .expect("request encodes"),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    async fn post_json<T>(app: axum::Router, path: &str, body: &impl serde::Serialize) -> T
    where
        T: DeserializeOwned,
    {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(body).expect("request encodes"),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        serde_json::from_slice(&bytes).expect("response decodes")
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-auth-routes-{label}-{}-{}",
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
}
