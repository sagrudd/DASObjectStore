//! Axum composition for host-authenticated DASObjectStore product routes.

use crate::{
    accept_monas_host_session, accept_synoptikon_host_session, MonasHostSessionIssue,
    SynoptikonIntegratedSessionIssue, SynoptikonLiveSessionVerifier,
};
use axum::{
    body::Body,
    extract::{OriginalUri, Request, State},
    http::{
        header::{ACCEPT, COOKIE},
        HeaderName, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use dasobjectstore_gui_api::{federated_gui_api_router, LocalAuthStore};
use prosopikon_core::ProsopikonAuthStore;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const MONAS_SESSION_COOKIE: &str = "monas_session";
pub const FEDERATED_CSRF_HEADER: HeaderName = HeaderName::from_static("x-dasobjectstore-csrf");

#[derive(Clone, Debug)]
pub struct SynoptikonHostRequestAuthentication {
    pub issue: SynoptikonIntegratedSessionIssue,
    pub csrf_binding_sha256: String,
}

pub fn monas_federated_router(router: Router, auth_store: ProsopikonAuthStore) -> Router {
    router.layer(middleware::from_fn_with_state(
        MonasFederatedAuthState { auth_store },
        authenticate_monas_request,
    ))
}

pub fn monas_dasobjectstore_api_router(auth_store: ProsopikonAuthStore) -> Router {
    monas_dasobjectstore_router(Router::new(), auth_store)
}

/// Mount host-owned Web routes and the DASObjectStore operational API behind
/// one Monas session boundary.
pub fn monas_dasobjectstore_router(
    host_product_routes: Router,
    auth_store: ProsopikonAuthStore,
) -> Router {
    let product_router =
        federated_gui_api_router(LocalAuthStore::from_prosopikon(auth_store.clone()))
            .merge(host_product_routes);
    monas_federated_router(product_router, auth_store)
}

pub fn synoptikon_federated_router(
    router: Router,
    verifier: Arc<dyn SynoptikonLiveSessionVerifier + Send + Sync>,
) -> Router {
    router.layer(middleware::from_fn_with_state(
        SynoptikonFederatedAuthState { verifier },
        authenticate_synoptikon_request,
    ))
}

#[derive(Clone)]
struct MonasFederatedAuthState {
    auth_store: ProsopikonAuthStore,
}

#[derive(Clone)]
struct SynoptikonFederatedAuthState {
    verifier: Arc<dyn SynoptikonLiveSessionVerifier + Send + Sync>,
}

async fn authenticate_monas_request(
    State(state): State<MonasFederatedAuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some((username, session_token)) = parse_monas_session_cookie(&request) else {
        return monas_unauthorized(&request);
    };
    let issue = MonasHostSessionIssue {
        username,
        csrf_binding_sha256: csrf_binding(&session_token),
        session_token,
        correlation_id: format!("monas:{}", Uuid::new_v4()),
    };
    let Ok(verified) = accept_monas_host_session(&state.auth_store, &issue, unix_now()) else {
        return monas_unauthorized(&request);
    };
    if !csrf_is_valid(&request, verified.context().csrf_binding_sha256.as_str()) {
        return csrf_rejected();
    }
    request.extensions_mut().insert(verified);
    next.run(request).await
}

async fn authenticate_synoptikon_request(
    State(state): State<SynoptikonFederatedAuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some(authentication) = request
        .extensions()
        .get::<SynoptikonHostRequestAuthentication>()
        .cloned()
    else {
        return unauthorized();
    };
    let Ok(verified) = accept_synoptikon_host_session(
        &authentication.issue,
        authentication.csrf_binding_sha256,
        unix_now(),
        state.verifier.as_ref(),
    ) else {
        return unauthorized();
    };
    if !csrf_is_valid(&request, verified.context().csrf_binding_sha256.as_str()) {
        return csrf_rejected();
    }
    request.extensions_mut().insert(verified);
    next.run(request).await
}

fn parse_monas_session_cookie(request: &Request<Body>) -> Option<(String, String)> {
    let cookie_header = request.headers().get(COOKIE)?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let (name, value) = cookie.trim().split_once('=')?;
        if name != MONAS_SESSION_COOKIE {
            continue;
        }
        let (username, session_token) = value.split_once(':')?;
        return Some((cookie_unescape(username), cookie_unescape(session_token)));
    }
    None
}

fn cookie_unescape(value: &str) -> String {
    value.replace("%3A", ":").replace("%25", "%")
}

fn csrf_binding(session_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"dasobjectstore:monas:csrf-binding:v1\0");
    hasher.update(session_token.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(-1)
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "host_authentication_required").into_response()
}

fn csrf_is_valid(request: &Request<Body>, expected: &str) -> bool {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return true;
    }
    request
        .headers()
        .get(&FEDERATED_CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        == Some(expected)
}

fn csrf_rejected() -> Response {
    (StatusCode::FORBIDDEN, "host_csrf_validation_failed").into_response()
}

fn monas_unauthorized(request: &Request<Body>) -> Response {
    let accepts_html = request
        .headers()
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|item| item.trim().starts_with("text/html"))
        });
    if request.method() == Method::GET && accepts_html {
        let return_to = request
            .extensions()
            .get::<OriginalUri>()
            .map(|original| original.0.path())
            .unwrap_or_else(|| request.uri().path());
        return axum::response::Redirect::to(&format!("/login?return_to={return_to}"))
            .into_response();
    }
    unauthorized()
}
