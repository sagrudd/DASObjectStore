use crate::{AuthRouteError, AuthenticatedGuiActor, LocalAuthStore};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Extension, Json, Router,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonRuntimeConfig, ObjectBrowserPageRequest,
    ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort, UnixSocketDaemonTransport,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn standalone_object_browser_router(auth_store: LocalAuthStore) -> Router {
    standalone_object_browser_router_with_state(StandaloneObjectBrowserRouteState::system(
        auth_store,
    ))
}

pub(crate) fn standalone_object_browser_router_with_state(
    state: StandaloneObjectBrowserRouteState,
) -> Router {
    Router::new()
        .route(
            "/api/v1/object-stores/{endpoint}/browser",
            get(object_store_browser),
        )
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
}

#[derive(Clone)]
pub(crate) struct StandaloneObjectBrowserRouteState {
    auth_store: LocalAuthStore,
    object_browser_client: Option<Arc<dyn StandaloneObjectBrowserClient>>,
}

impl StandaloneObjectBrowserRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            object_browser_client: Some(Arc::new(
                DaemonStandaloneObjectBrowserClient::default_packaged(),
            )),
        }
    }
}

pub(crate) trait StandaloneObjectBrowserClient: Send + Sync {
    fn object_browser(
        &self,
        request: ObjectBrowserRequest,
    ) -> Result<ObjectBrowserResponse, StandaloneObjectBrowserClientError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StandaloneObjectBrowserClientError {
    status: StatusCode,
    code: String,
    message: String,
}

impl StandaloneObjectBrowserClientError {
    #[cfg(test)]
    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "daemon_object_browser_denied".to_string(),
            message: message.into(),
        }
    }
}

struct DaemonStandaloneObjectBrowserClient {
    client: DaemonClient<UnixSocketDaemonTransport>,
}

impl DaemonStandaloneObjectBrowserClient {
    fn default_packaged() -> Self {
        Self {
            client: DaemonClient::new(UnixSocketDaemonTransport::new(
                DaemonRuntimeConfig::default_packaged().socket_path,
            )),
        }
    }
}

impl StandaloneObjectBrowserClient for DaemonStandaloneObjectBrowserClient {
    fn object_browser(
        &self,
        request: ObjectBrowserRequest,
    ) -> Result<ObjectBrowserResponse, StandaloneObjectBrowserClientError> {
        self.client
            .object_browser(request)
            .map_err(object_browser_client_error)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct ObjectBrowserQuery {
    prefix: Option<String>,
    search: Option<String>,
    sort: Option<String>,
    cursor: Option<String>,
    limit: Option<u16>,
    include_placement: Option<bool>,
}

async fn object_store_browser(
    State(state): State<StandaloneObjectBrowserRouteState>,
    _actor: AuthenticatedGuiActor,
    Path(endpoint): Path<String>,
    Query(query): Query<ObjectBrowserQuery>,
) -> Result<Json<ObjectBrowserResponse>, (StatusCode, Json<AuthRouteError>)> {
    let request = object_browser_request(endpoint, query)?;
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            err.to_string(),
        )
    })?;

    state
        .object_browser_client
        .as_ref()
        .ok_or_else(|| {
            route_error(
                StatusCode::NOT_IMPLEMENTED,
                "daemon_object_browser_unavailable",
                "daemon ObjectStore browser contract is not available",
            )
        })?
        .object_browser(request)
        .map(Json)
        .map_err(|err| route_error(err.status, err.code, err.message))
}

fn object_browser_request(
    endpoint: String,
    query: ObjectBrowserQuery,
) -> Result<ObjectBrowserRequest, (StatusCode, Json<AuthRouteError>)> {
    let endpoint = StoreId::new(required_field("endpoint", endpoint)?).map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            err.to_string(),
        )
    })?;
    Ok(ObjectBrowserRequest {
        endpoint,
        prefix: query.prefix,
        search: query.search,
        sort: parse_object_browser_sort(query.sort.as_deref())?,
        page: ObjectBrowserPageRequest {
            cursor: query.cursor,
            limit: query
                .limit
                .unwrap_or_else(|| ObjectBrowserPageRequest::default().limit),
        },
        include_placement: query.include_placement.unwrap_or(false),
    })
}

fn parse_object_browser_sort(
    value: Option<&str>,
) -> Result<ObjectBrowserSort, (StatusCode, Json<AuthRouteError>)> {
    match value.unwrap_or("name_asc").trim() {
        "name_asc" => Ok(ObjectBrowserSort::NameAsc),
        "name_desc" => Ok(ObjectBrowserSort::NameDesc),
        "size_asc" => Ok(ObjectBrowserSort::SizeAsc),
        "size_desc" => Ok(ObjectBrowserSort::SizeDesc),
        "modified_asc" => Ok(ObjectBrowserSort::ModifiedAsc),
        "modified_desc" => Ok(ObjectBrowserSort::ModifiedDesc),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            format!(
                "sort must be name_asc, name_desc, size_asc, size_desc, modified_asc, or modified_desc: {other}"
            ),
        )),
    }
}

fn required_field(
    field: &'static str,
    value: String,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            format!("{field} must not be blank"),
        ));
    }
    Ok(value)
}

fn object_browser_client_error(err: DaemonClientError) -> StandaloneObjectBrowserClientError {
    let message = err.to_string();
    match err {
        DaemonClientError::RequestValidation(_) => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_object_browser_request".to_string(),
            message,
        },
        DaemonClientError::Api(api_error)
            if matches!(
                api_error.code.as_str(),
                "permission_denied" | "authorization_failed" | "forbidden"
            ) =>
        {
            StandaloneObjectBrowserClientError {
                status: StatusCode::FORBIDDEN,
                code: "daemon_object_browser_denied".to_string(),
                message,
            }
        }
        _ => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_GATEWAY,
            code: "daemon_object_browser_failed".to_string(),
            message,
        },
    }
}

fn route_error(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<AuthRouteError>) {
    (
        status,
        Json(AuthRouteError {
            code: code.into(),
            message: message.into(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        standalone_object_browser_router_with_state, StandaloneObjectBrowserClient,
        StandaloneObjectBrowserClientError, StandaloneObjectBrowserRouteState,
    };
    use crate::{LocalAuthStore, STANDALONE_SESSION_TOKEN_HEADER, STANDALONE_USERNAME_HEADER};
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::ObjectState;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_daemon::{
        ObjectBrowserFileNode, ObjectBrowserPageRequest, ObjectBrowserReadinessState,
        ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[tokio::test]
    async fn object_browser_route_requires_session() {
        let root = temp_root("object-browser-auth");
        let auth_store = registered_auth_store(&root);
        let app = test_router(auth_store, recording_browser_client());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/browser")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_browser_route_forwards_typed_request_to_daemon_client() {
        let root = temp_root("object-browser-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_browser_client();
        let app = test_router(auth_store, client.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/browser?prefix=ENA%2FXeno&search=vervet&sort=size_desc&cursor=25&limit=50&include_placement=true")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;
        assert_eq!(encoded["endpoint"], "ena");
        assert_eq!(encoded["files"][0]["name"], "metadata.tsv");
        assert_eq!(
            client.requests(),
            vec![ObjectBrowserRequest {
                endpoint: StoreId::new("ena").expect("store id"),
                prefix: Some("ENA/Xeno".to_string()),
                search: Some("vervet".to_string()),
                sort: ObjectBrowserSort::SizeDesc,
                page: ObjectBrowserPageRequest {
                    cursor: Some("25".to_string()),
                    limit: 50,
                },
                include_placement: true,
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_browser_route_rejects_invalid_query_before_daemon() {
        let root = temp_root("object-browser-invalid");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_browser_client();
        let app = test_router(auth_store, client.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/browser?sort=random")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "invalid_object_browser_request");
        assert!(client.requests().is_empty());

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_browser_route_surfaces_daemon_permission_denial() {
        let root = temp_root("object-browser-denied");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = Arc::new(RecordingObjectBrowserClient::with_error(
            StandaloneObjectBrowserClientError::forbidden(
                "current user cannot read ObjectStore ena",
            ),
        ));
        let app = test_router(auth_store, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/browser")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "daemon_object_browser_denied");
        assert!(encoded["message"]
            .as_str()
            .expect("message")
            .contains("cannot read"));

        cleanup(&root);
    }

    fn test_router(
        auth_store: LocalAuthStore,
        client: Arc<RecordingObjectBrowserClient>,
    ) -> axum::Router {
        standalone_object_browser_router_with_state(StandaloneObjectBrowserRouteState {
            auth_store,
            object_browser_client: Some(client),
        })
    }

    fn registered_auth_store(root: &Path) -> LocalAuthStore {
        let auth_store = LocalAuthStore::new(root);
        auth_store.create_user("admin").expect("user created");
        let token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("token issued");
        auth_store
            .register_with_token("admin", &token, "secret")
            .expect("registered");
        auth_store
    }

    fn recording_browser_client() -> Arc<RecordingObjectBrowserClient> {
        Arc::new(RecordingObjectBrowserClient::default())
    }

    #[derive(Default)]
    struct RecordingObjectBrowserClient {
        requests: Mutex<Vec<ObjectBrowserRequest>>,
        error: Option<StandaloneObjectBrowserClientError>,
    }

    impl RecordingObjectBrowserClient {
        fn with_error(error: StandaloneObjectBrowserClientError) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                error: Some(error),
            }
        }

        fn requests(&self) -> Vec<ObjectBrowserRequest> {
            self.requests.lock().expect("requests lock").clone()
        }
    }

    impl StandaloneObjectBrowserClient for RecordingObjectBrowserClient {
        fn object_browser(
            &self,
            request: ObjectBrowserRequest,
        ) -> Result<ObjectBrowserResponse, StandaloneObjectBrowserClientError> {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            Ok(ObjectBrowserResponse {
                endpoint: request.endpoint,
                prefix: request.prefix.unwrap_or_default(),
                breadcrumbs: Vec::new(),
                folders: Vec::new(),
                files: vec![ObjectBrowserFileNode {
                    object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object id"),
                    name: "metadata.tsv".to_string(),
                    path: "ENA/Xeno/metadata.tsv".to_string(),
                    object_type: ObjectType::Naive,
                    size_bytes: 1024,
                    modified_at_utc: Some("2026-07-09T09:48:51Z".to_string()),
                    checksum: None,
                    readiness: ObjectBrowserReadinessState::Available,
                    lifecycle_state: ObjectState::Protected,
                    copy_count: 1,
                    placements: Vec::new(),
                }],
                next_cursor: None,
                total_entries: Some(1),
            })
        }
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        serde_json::from_slice(&body).unwrap_or_else(|err| {
            panic!(
                "response decodes as JSON: {err}; body={}",
                String::from_utf8_lossy(&body)
            )
        })
    }

    fn temp_root(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-gui-browser-{name}-{suffix}"))
    }

    fn cleanup(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("cleanup temp root");
        }
    }
}
