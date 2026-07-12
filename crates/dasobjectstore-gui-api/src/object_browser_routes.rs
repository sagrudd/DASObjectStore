use crate::{
    daemon_bridge::{DaemonBridge, DaemonBridgeError},
    discover_local_user, AuthRouteError, AuthenticatedGuiActor, LocalAuthStore,
    LocalUserDiscoveryError, LocalUserMetadata,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::Response,
    routing::get,
    Extension, Json, Router,
};
use bytes::Bytes;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonRuntimeConfig, ObjectBrowserDelegatedActor,
    ObjectBrowserPageRequest, ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort,
    ObjectDownloadRequest, ObjectDownloadResponse, ObjectFolderDownloadRequest,
    ObjectFolderDownloadResponse, UnixSocketDaemonTransport,
};
use flate2::{write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

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
        .route(
            "/api/v1/object-stores/{endpoint}/objects/download/{*object_id}",
            get(object_store_object_download),
        )
        .route(
            "/api/v1/object-stores/{endpoint}/folders/download/{*prefix}",
            get(object_store_folder_download),
        )
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
}

#[derive(Clone)]
pub(crate) struct StandaloneObjectBrowserRouteState {
    auth_store: LocalAuthStore,
    object_browser_client: Option<Arc<dyn StandaloneObjectBrowserClient>>,
    local_user_provider: Arc<dyn ObjectBrowserLocalUserProvider>,
    daemon_bridge: Arc<DaemonBridge>,
}

impl StandaloneObjectBrowserRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            object_browser_client: Some(Arc::new(
                DaemonStandaloneObjectBrowserClient::default_packaged(),
            )),
            local_user_provider: Arc::new(SystemObjectBrowserLocalUserProvider),
            daemon_bridge: Arc::new(DaemonBridge::packaged()),
        }
    }
}

trait ObjectBrowserLocalUserProvider: Send + Sync {
    fn local_user(&self, username: &str) -> Result<LocalUserMetadata, LocalUserDiscoveryError>;
}

struct SystemObjectBrowserLocalUserProvider;

impl ObjectBrowserLocalUserProvider for SystemObjectBrowserLocalUserProvider {
    fn local_user(&self, username: &str) -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
        discover_local_user(username)
    }
}

pub(crate) trait StandaloneObjectBrowserClient: Send + Sync {
    fn object_browser(
        &self,
        request: ObjectBrowserRequest,
    ) -> Result<ObjectBrowserResponse, StandaloneObjectBrowserClientError>;

    fn object_download(
        &self,
        request: ObjectDownloadRequest,
    ) -> Result<ObjectDownloadResponse, StandaloneObjectBrowserClientError>;

    fn object_folder_download(
        &self,
        request: ObjectFolderDownloadRequest,
    ) -> Result<ObjectFolderDownloadResponse, StandaloneObjectBrowserClientError>;
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

    fn object_download(
        &self,
        request: ObjectDownloadRequest,
    ) -> Result<ObjectDownloadResponse, StandaloneObjectBrowserClientError> {
        self.client
            .object_download(request)
            .map_err(object_browser_client_error)
    }

    fn object_folder_download(
        &self,
        request: ObjectFolderDownloadRequest,
    ) -> Result<ObjectFolderDownloadResponse, StandaloneObjectBrowserClientError> {
        self.client
            .object_folder_download(request)
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
    actor: AuthenticatedGuiActor,
    Path(endpoint): Path<String>,
    Query(query): Query<ObjectBrowserQuery>,
) -> Result<Json<ObjectBrowserResponse>, (StatusCode, Json<AuthRouteError>)> {
    let delegated_actor = delegated_object_browser_actor(&state, &actor)?;
    let request = object_browser_request(endpoint, query, Some(delegated_actor))?;
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            err.to_string(),
        )
    })?;

    let client = state.object_browser_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_object_browser_unavailable",
            "daemon ObjectStore browser contract is not available",
        )
    })?;
    let client = Arc::clone(client);
    state
        .daemon_bridge
        .call(move || client.object_browser(request))
        .await
        .map(Json)
        .map_err(daemon_bridge_route_error)
}

async fn object_store_object_download(
    State(state): State<StandaloneObjectBrowserRouteState>,
    actor: AuthenticatedGuiActor,
    Path((endpoint, object_id)): Path<(String, String)>,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let delegated_actor = delegated_object_browser_actor(&state, &actor)?;
    let request = object_download_request(endpoint, object_id, Some(delegated_actor))?;
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_download_request",
            err.to_string(),
        )
    })?;
    let client = state.object_browser_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_object_download_unavailable",
            "daemon ObjectStore download contract is not available",
        )
    })?;
    let client = Arc::clone(client);
    let download = state
        .daemon_bridge
        .call(move || client.object_download(request))
        .await
        .map_err(daemon_bridge_route_error)?;

    let file = tokio::fs::File::open(&download.source_path)
        .await
        .map_err(|err| {
            route_error(
                StatusCode::CONFLICT,
                "object_download_unavailable",
                format!("object download source could not be opened: {err}"),
            )
        })?;
    let body = Body::from_stream(ReaderStream::new(file));
    let mut response = Response::new(body);
    *response.headers_mut() = object_download_headers(&download)?;
    Ok(response)
}

async fn object_store_folder_download(
    State(state): State<StandaloneObjectBrowserRouteState>,
    actor: AuthenticatedGuiActor,
    Path((endpoint, prefix)): Path<(String, String)>,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let delegated_actor = delegated_object_browser_actor(&state, &actor)?;
    let request = object_folder_download_request(endpoint, prefix, Some(delegated_actor))?;
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_folder_download_request",
            err.to_string(),
        )
    })?;
    let client = state.object_browser_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_object_folder_download_unavailable",
            "daemon ObjectStore folder download contract is not available",
        )
    })?;
    let client = Arc::clone(client);
    let download = state
        .daemon_bridge
        .call(move || client.object_folder_download(request))
        .await
        .map_err(daemon_bridge_route_error)?;

    let headers = object_folder_download_headers(&download)?;
    let archive_download = download.clone();
    let (sender, receiver) = mpsc::channel::<Result<Bytes, io::Error>>(4);
    tokio::task::spawn_blocking(move || stream_folder_archive(archive_download, sender));
    let body = Body::from_stream(ReceiverStream::new(receiver));
    let mut response = Response::new(body);
    *response.headers_mut() = headers;
    Ok(response)
}

fn daemon_bridge_route_error(error: DaemonBridgeError) -> (StatusCode, Json<AuthRouteError>) {
    match error {
        DaemonBridgeError::Client(error) => route_error(error.status, error.code, error.message),
        DaemonBridgeError::Busy => route_error(
            StatusCode::TOO_MANY_REQUESTS,
            "daemon_bridge_busy",
            "daemon control capacity is saturated; retry shortly",
        ),
        DaemonBridgeError::Deadline => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_bridge_timeout",
            "daemon control request exceeded its deadline; retry shortly",
        ),
        DaemonBridgeError::Join(message) => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_bridge_join_failed",
            message,
        ),
    }
}

fn object_browser_request(
    endpoint: String,
    query: ObjectBrowserQuery,
    delegated_actor: Option<ObjectBrowserDelegatedActor>,
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
        delegated_actor,
    })
}

fn object_download_request(
    endpoint: String,
    object_id: String,
    delegated_actor: Option<ObjectBrowserDelegatedActor>,
) -> Result<ObjectDownloadRequest, (StatusCode, Json<AuthRouteError>)> {
    let endpoint = StoreId::new(required_field("endpoint", endpoint)?).map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_download_request",
            err.to_string(),
        )
    })?;
    let object_id = ObjectId::new(required_field(
        "object_id",
        object_id.trim_start_matches('/').to_string(),
    )?)
    .map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_download_request",
            err.to_string(),
        )
    })?;
    Ok(ObjectDownloadRequest {
        endpoint,
        object_id,
        delegated_actor,
    })
}

fn object_folder_download_request(
    endpoint: String,
    prefix: String,
    delegated_actor: Option<ObjectBrowserDelegatedActor>,
) -> Result<ObjectFolderDownloadRequest, (StatusCode, Json<AuthRouteError>)> {
    let endpoint = StoreId::new(required_field("endpoint", endpoint)?).map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_folder_download_request",
            err.to_string(),
        )
    })?;
    Ok(ObjectFolderDownloadRequest {
        endpoint,
        prefix: required_field("prefix", prefix.trim_start_matches('/').to_string())?,
        delegated_actor,
    })
}

fn delegated_object_browser_actor(
    state: &StandaloneObjectBrowserRouteState,
    actor: &AuthenticatedGuiActor,
) -> Result<ObjectBrowserDelegatedActor, (StatusCode, Json<AuthRouteError>)> {
    if actor.authority != crate::AuthenticatedActorAuthority::LocalStandalone {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "standalone_local_user_required",
            "ObjectStore browser access requires a local standalone browser session",
        ));
    }
    let local_user = state
        .local_user_provider
        .local_user(&actor.subject_id)
        .map_err(|err| {
            route_error(
                StatusCode::FORBIDDEN,
                "local_user_discovery_failed",
                format!(
                    "ObjectStore browser access could not resolve local user {}: {err}",
                    actor.subject_id
                ),
            )
        })?;
    Ok(ObjectBrowserDelegatedActor {
        username: local_user.username,
        uid: None,
        primary_gid: None,
        groups: local_user.groups,
    })
}

fn object_download_headers(
    download: &ObjectDownloadResponse,
) -> Result<HeaderMap, (StatusCode, Json<AuthRouteError>)> {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&download.size_bytes.to_string()).map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "invalid_object_download_response",
                err.to_string(),
            )
        })?,
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(&download.file_name)).map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "invalid_object_download_response",
                err.to_string(),
            )
        })?,
    );
    Ok(headers)
}

fn object_folder_download_headers(
    download: &ObjectFolderDownloadResponse,
) -> Result<HeaderMap, (StatusCode, Json<AuthRouteError>)> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/gzip"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(&download.archive_name)).map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "invalid_object_folder_download_response",
                err.to_string(),
            )
        })?,
    );
    headers.insert(
        "x-dasobjectstore-archive-files",
        HeaderValue::from_str(&download.total_files.to_string()).map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "invalid_object_folder_download_response",
                err.to_string(),
            )
        })?,
    );
    headers.insert(
        "x-dasobjectstore-archive-source-bytes",
        HeaderValue::from_str(&download.total_source_bytes.to_string()).map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "invalid_object_folder_download_response",
                err.to_string(),
            )
        })?,
    );
    Ok(headers)
}

fn stream_folder_archive(
    download: ObjectFolderDownloadResponse,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let error_sender = sender.clone();
    if let Err(err) = write_folder_archive(download, sender) {
        let _ = error_sender.blocking_send(Err(err));
    }
}

fn write_folder_archive(
    download: ObjectFolderDownloadResponse,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
) -> io::Result<()> {
    let writer = ChannelWriter { sender };
    let encoder = GzEncoder::new(writer, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for entry in download.entries {
        archive.append_path_with_name(&entry.source_path, &entry.archive_path)?;
    }
    let encoder = archive.into_inner()?;
    encoder.finish()?;
    Ok(())
}

struct ChannelWriter {
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sender
            .blocking_send(Ok(Bytes::copy_from_slice(buf)))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "archive receiver closed"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn content_disposition(file_name: &str) -> String {
    let escaped = file_name
        .chars()
        .filter_map(|character| match character {
            '"' | '\\' | '/' | '\r' | '\n' => Some('_'),
            character if character.is_control() => None,
            character => Some(character),
        })
        .collect::<String>();
    let file_name = if escaped.trim().is_empty() {
        "object"
    } else {
        escaped.trim()
    };
    format!("attachment; filename=\"{file_name}\"")
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
            code: "invalid_daemon_object_request".to_string(),
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
        DaemonClientError::Api(api_error) => {
            let status = daemon_error_status(&api_error.code);
            StandaloneObjectBrowserClientError {
                status,
                code: api_error.code,
                message,
            }
        }
        _ => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_GATEWAY,
            code: "daemon_object_request_failed".to_string(),
            message,
        },
    }
}

fn daemon_error_status(code: &str) -> StatusCode {
    match code {
        "object_download_not_found" => StatusCode::NOT_FOUND,
        "object_download_unavailable" => StatusCode::CONFLICT,
        "object_folder_download_not_found" => StatusCode::NOT_FOUND,
        "object_folder_download_unavailable" => StatusCode::CONFLICT,
        _ => StatusCode::BAD_GATEWAY,
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
        standalone_object_browser_router_with_state, write_folder_archive,
        ObjectBrowserLocalUserProvider, StandaloneObjectBrowserClient,
        StandaloneObjectBrowserClientError, StandaloneObjectBrowserRouteState,
    };
    use crate::{
        daemon_bridge::DaemonBridge, LocalAuthStore, LocalUserDiscoveryError, LocalUserMetadata,
        STANDALONE_SESSION_TOKEN_HEADER, STANDALONE_USERNAME_HEADER,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::ObjectState;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_daemon::{
        ObjectBrowserDelegatedActor, ObjectBrowserFileNode, ObjectBrowserPageRequest,
        ObjectBrowserReadinessState, ObjectBrowserRequest, ObjectBrowserResponse,
        ObjectBrowserSort, ObjectDownloadRequest, ObjectDownloadResponse, ObjectFolderArchiveEntry,
        ObjectFolderDownloadRequest, ObjectFolderDownloadResponse,
    };
    use flate2::read::GzDecoder;
    use std::fs;
    use std::io::Read;
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
                delegated_actor: Some(expected_delegated_actor("admin")),
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

    #[tokio::test]
    async fn object_download_route_streams_authorized_daemon_source() {
        let root = temp_root("object-download");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let source_path = root.join("objects").join("payload");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source parent");
        fs::write(&source_path, b"download payload").expect("write source");
        let client = recording_browser_client();
        client.set_download(ObjectDownloadResponse {
            endpoint: StoreId::new("ena").expect("store id"),
            store_id: StoreId::new("ena").expect("store id"),
            object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object id"),
            file_name: "metadata.tsv".to_string(),
            source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id"),
            source_path,
            size_bytes: b"download payload".len() as u64,
        });
        let app = test_router(auth_store, client.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/objects/download/ENA/Xeno/metadata.tsv")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-disposition"],
            "attachment; filename=\"metadata.tsv\""
        );
        assert_eq!(response.headers()["content-length"], "16");
        assert_eq!(
            client.download_requests(),
            vec![ObjectDownloadRequest {
                endpoint: StoreId::new("ena").expect("store id"),
                object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object id"),
                delegated_actor: Some(expected_delegated_actor("admin")),
            }]
        );
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        assert_eq!(&body[..], b"download payload");

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_download_route_surfaces_daemon_permission_denial() {
        let root = temp_root("object-download-denied");
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
                    .uri("/api/v1/object-stores/ena/objects/download/ENA/Xeno/metadata.tsv")
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

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_download_route_surfaces_unavailable_degraded_source() {
        let root = temp_root("object-download-unavailable");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = Arc::new(RecordingObjectBrowserClient::with_error(
            StandaloneObjectBrowserClientError {
                status: StatusCode::CONFLICT,
                code: "object_download_unavailable".to_string(),
                message: "object `ENA/Xeno/degraded.fastq.gz` has no verified placement on a managed HDD root"
                    .to_string(),
            },
        ));
        let app = test_router(auth_store, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/objects/download/ENA/Xeno/degraded.fastq.gz")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "object_download_unavailable");
        assert!(encoded["message"]
            .as_str()
            .expect("message")
            .contains("no verified placement"));

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_folder_download_route_streams_tar_gz_archive() {
        let root = temp_root("object-folder-download");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let metadata_path = write_test_file(&root, "objects/metadata.tsv", b"metadata");
        let reads_path = write_test_file(&root, "objects/reads.fastq.gz", b"reads");
        let client = recording_browser_client();
        client.set_folder_download(ObjectFolderDownloadResponse {
            endpoint: StoreId::new("ena").expect("store id"),
            store_id: StoreId::new("ena").expect("store id"),
            prefix: "ENA/Xeno".to_string(),
            archive_name: "Xeno.tar.gz".to_string(),
            total_files: 2,
            total_source_bytes: b"metadata".len() as u64 + b"reads".len() as u64,
            entries: vec![
                ObjectFolderArchiveEntry {
                    object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object id"),
                    archive_path: "metadata.tsv".to_string(),
                    source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-a")
                        .expect("disk id"),
                    source_path: metadata_path,
                    size_bytes: b"metadata".len() as u64,
                },
                ObjectFolderArchiveEntry {
                    object_id: ObjectId::new("ENA/Xeno/reads.fastq.gz").expect("object id"),
                    archive_path: "reads.fastq.gz".to_string(),
                    source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-a")
                        .expect("disk id"),
                    source_path: reads_path,
                    size_bytes: b"reads".len() as u64,
                },
            ],
        });
        let app = test_router(auth_store, client.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/folders/download/ENA/Xeno")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-disposition"],
            "attachment; filename=\"Xeno.tar.gz\""
        );
        assert_eq!(response.headers()["x-dasobjectstore-archive-files"], "2");
        assert_eq!(
            client.folder_download_requests(),
            vec![ObjectFolderDownloadRequest {
                endpoint: StoreId::new("ena").expect("store id"),
                prefix: "ENA/Xeno".to_string(),
                delegated_actor: Some(expected_delegated_actor("admin")),
            }]
        );
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body bytes");
        assert_eq!(
            tar_gz_members(&body),
            vec![
                ("metadata.tsv".to_string(), b"metadata".to_vec()),
                ("reads.fastq.gz".to_string(), b"reads".to_vec()),
            ]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn object_folder_download_route_surfaces_unavailable_archive_source() {
        let root = temp_root("object-folder-download-unavailable");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = Arc::new(RecordingObjectBrowserClient::with_error(
            StandaloneObjectBrowserClientError {
                status: StatusCode::CONFLICT,
                code: "object_folder_download_unavailable".to_string(),
                message: "object `ENA/Xeno/degraded.fastq.gz` has no verified placement on a managed HDD root"
                    .to_string(),
            },
        ));
        let app = test_router(auth_store, client);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/folders/download/ENA/Xeno")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "object_folder_download_unavailable");
        assert!(encoded["message"]
            .as_str()
            .expect("message")
            .contains("no verified placement"));

        cleanup(&root);
    }

    #[test]
    fn folder_archive_stream_stops_when_receiver_is_interrupted() {
        let root = temp_root("object-folder-download-interrupted");
        let metadata_path = write_test_file(&root, "objects/metadata.tsv", b"metadata");
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        drop(receiver);

        let err = write_folder_archive(
            ObjectFolderDownloadResponse {
                endpoint: StoreId::new("ena").expect("store id"),
                store_id: StoreId::new("ena").expect("store id"),
                prefix: "ENA/Xeno".to_string(),
                archive_name: "Xeno.tar.gz".to_string(),
                total_files: 1,
                total_source_bytes: b"metadata".len() as u64,
                entries: vec![ObjectFolderArchiveEntry {
                    object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object id"),
                    archive_path: "metadata.tsv".to_string(),
                    source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-a")
                        .expect("disk id"),
                    source_path: metadata_path,
                    size_bytes: b"metadata".len() as u64,
                }],
            },
            sender,
        )
        .expect_err("closed receiver stops archive generation");

        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);

        cleanup(&root);
    }

    fn test_router(
        auth_store: LocalAuthStore,
        client: Arc<RecordingObjectBrowserClient>,
    ) -> axum::Router {
        standalone_object_browser_router_with_state(StandaloneObjectBrowserRouteState {
            auth_store,
            object_browser_client: Some(client),
            local_user_provider: Arc::new(FixedObjectBrowserLocalUserProvider),
            daemon_bridge: Arc::new(DaemonBridge::packaged()),
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

    struct FixedObjectBrowserLocalUserProvider;

    impl ObjectBrowserLocalUserProvider for FixedObjectBrowserLocalUserProvider {
        fn local_user(&self, username: &str) -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
            Ok(LocalUserMetadata::from_username_and_groups(
                username,
                vec!["mnemosyne".to_string(), "users".to_string()],
            ))
        }
    }

    fn expected_delegated_actor(username: &str) -> ObjectBrowserDelegatedActor {
        let user = LocalUserMetadata::from_username_and_groups(
            username,
            vec!["mnemosyne".to_string(), "users".to_string()],
        );
        ObjectBrowserDelegatedActor {
            username: user.username,
            uid: None,
            primary_gid: None,
            groups: user.groups,
        }
    }

    #[derive(Default)]
    struct RecordingObjectBrowserClient {
        requests: Mutex<Vec<ObjectBrowserRequest>>,
        download_requests: Mutex<Vec<ObjectDownloadRequest>>,
        folder_download_requests: Mutex<Vec<ObjectFolderDownloadRequest>>,
        download: Mutex<Option<ObjectDownloadResponse>>,
        folder_download: Mutex<Option<ObjectFolderDownloadResponse>>,
        error: Option<StandaloneObjectBrowserClientError>,
    }

    impl RecordingObjectBrowserClient {
        fn with_error(error: StandaloneObjectBrowserClientError) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                download_requests: Mutex::new(Vec::new()),
                folder_download_requests: Mutex::new(Vec::new()),
                download: Mutex::new(None),
                folder_download: Mutex::new(None),
                error: Some(error),
            }
        }

        fn requests(&self) -> Vec<ObjectBrowserRequest> {
            self.requests.lock().expect("requests lock").clone()
        }

        fn download_requests(&self) -> Vec<ObjectDownloadRequest> {
            self.download_requests
                .lock()
                .expect("download requests lock")
                .clone()
        }

        fn folder_download_requests(&self) -> Vec<ObjectFolderDownloadRequest> {
            self.folder_download_requests
                .lock()
                .expect("folder download requests lock")
                .clone()
        }

        fn set_download(&self, download: ObjectDownloadResponse) {
            *self.download.lock().expect("download lock") = Some(download);
        }

        fn set_folder_download(&self, download: ObjectFolderDownloadResponse) {
            *self.folder_download.lock().expect("folder download lock") = Some(download);
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

        fn object_download(
            &self,
            request: ObjectDownloadRequest,
        ) -> Result<ObjectDownloadResponse, StandaloneObjectBrowserClientError> {
            self.download_requests
                .lock()
                .expect("download requests lock")
                .push(request);
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            self.download
                .lock()
                .expect("download lock")
                .clone()
                .ok_or_else(|| StandaloneObjectBrowserClientError {
                    status: StatusCode::NOT_FOUND,
                    code: "object_download_not_found".to_string(),
                    message: "test download response not configured".to_string(),
                })
        }

        fn object_folder_download(
            &self,
            request: ObjectFolderDownloadRequest,
        ) -> Result<ObjectFolderDownloadResponse, StandaloneObjectBrowserClientError> {
            self.folder_download_requests
                .lock()
                .expect("folder download requests lock")
                .push(request);
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            self.folder_download
                .lock()
                .expect("folder download lock")
                .clone()
                .ok_or_else(|| StandaloneObjectBrowserClientError {
                    status: StatusCode::NOT_FOUND,
                    code: "object_folder_download_not_found".to_string(),
                    message: "test folder download response not configured".to_string(),
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

    fn write_test_file(root: &Path, relative_path: &str, bytes: &[u8]) -> PathBuf {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().expect("file parent")).expect("file parent");
        fs::write(&path, bytes).expect("write test file");
        path
    }

    fn tar_gz_members(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let decoder = GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        let mut members = Vec::new();
        for entry in archive.entries().expect("archive entries") {
            let mut entry = entry.expect("archive entry");
            let path = entry
                .path()
                .expect("entry path")
                .to_string_lossy()
                .to_string();
            let mut contents = Vec::new();
            entry
                .read_to_end(&mut contents)
                .expect("entry contents read");
            members.push((path, contents));
        }
        members
    }

    fn cleanup(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("cleanup temp root");
        }
    }
}
