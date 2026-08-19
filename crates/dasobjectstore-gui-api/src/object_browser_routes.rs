use crate::{
    daemon_bridge::{DaemonBridge, DaemonBridgeError},
    AuthRouteError, AuthenticatedGuiActor, VerifiedHostAuthenticatedContext,
    VerifiedHostObjectPrefixScope, VerifiedHostStoreScope,
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
use dasobjectstore_daemon::api::{
    ObjectBrowserVerifiedSubject, OBJECT_BROWSER_GUI_API_PEER_IDENTITY,
    OBJECT_BROWSER_VERIFIED_SUBJECT_SCHEMA_VERSION,
};
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonRuntimeConfig, ObjectBrowserPageRequest,
    ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort, ObjectDownloadRequest,
    ObjectDownloadResponse, ObjectFolderDownloadRequest, ObjectFolderDownloadResponse,
    UnixSocketDaemonTransport,
};
use flate2::{write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, Semaphore};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

const MAX_ARCHIVE_WORKERS: usize = 2;

/// Map an already verified host session into the explicit Object Browser
/// daemon envelope.  This is intentionally only a mapping/validation
/// boundary: no host-composed route consumes it until the daemon's transport
/// peer binding and authorization migration land together.
///
/// The supplied scope must have been derived from the same verified host
/// session.  This helper emits no local username, UID, GID, group, password,
/// PAM, or sudo assertion and therefore cannot coexist with the legacy
/// `ObjectBrowserDelegatedActor` path.
pub fn verified_object_browser_subject(
    verified: &VerifiedHostAuthenticatedContext,
    scope: &VerifiedHostStoreScope,
    store_id: StoreId,
    canonical_prefix: String,
) -> Result<ObjectBrowserVerifiedSubject, (StatusCode, Json<AuthRouteError>)> {
    if !scope.permits(verified, store_id.as_str()) {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_store_scope_denied",
            "the verified host session is not authorised for this ObjectStore",
        ));
    }
    let subject = ObjectBrowserVerifiedSubject {
        schema_version: OBJECT_BROWSER_VERIFIED_SUBJECT_SCHEMA_VERSION.to_string(),
        peer_identity: OBJECT_BROWSER_GUI_API_PEER_IDENTITY.to_string(),
        subject_id: verified.context().subject_id.clone(),
        session_id: verified.context().session_id.clone(),
        correlation_id: verified.context().correlation_id.clone(),
        store_id: store_id.clone(),
        canonical_prefix,
    };
    subject
        .validate_for_endpoint(&store_id, Some(&subject.canonical_prefix))
        .map_err(|error| {
            route_error(
                StatusCode::BAD_REQUEST,
                "invalid_verified_object_browser_subject",
                error.to_string(),
            )
        })?;
    Ok(subject)
}

/// Map a verified host prefix grant into the same daemon browser envelope.
/// Unlike the exact-object route helper above, the envelope preserves the
/// host-issued prefix so the daemon can independently reject any attempted
/// provider-stream read outside it.
pub fn verified_object_browser_subject_for_prefix_scope(
    verified: &VerifiedHostAuthenticatedContext,
    scope: &VerifiedHostObjectPrefixScope,
    store_id: StoreId,
    requested_path: &str,
) -> Result<ObjectBrowserVerifiedSubject, (StatusCode, Json<AuthRouteError>)> {
    if !scope.permits(verified, store_id.as_str(), requested_path) {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_object_prefix_scope_denied",
            "the verified host session is not authorised for this ObjectStore object prefix",
        ));
    }
    let subject = ObjectBrowserVerifiedSubject {
        schema_version: OBJECT_BROWSER_VERIFIED_SUBJECT_SCHEMA_VERSION.to_string(),
        peer_identity: OBJECT_BROWSER_GUI_API_PEER_IDENTITY.to_string(),
        subject_id: verified.context().subject_id.clone(),
        session_id: verified.context().session_id.clone(),
        correlation_id: verified.context().correlation_id.clone(),
        store_id: store_id.clone(),
        canonical_prefix: scope.canonical_prefix().to_string(),
    };
    subject
        .validate_for_endpoint(&store_id, Some(requested_path))
        .map_err(|error| {
            route_error(
                StatusCode::BAD_REQUEST,
                "invalid_verified_object_browser_subject",
                error.to_string(),
            )
        })?;
    Ok(subject)
}

fn archive_worker_semaphore() -> &'static Arc<Semaphore> {
    static ARCHIVE_WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    ARCHIVE_WORKERS.get_or_init(|| Arc::new(Semaphore::new(MAX_ARCHIVE_WORKERS)))
}

/// Host-composed Object Browser routes.  Unlike the appliance-only router,
/// these routes receive the already verified Pistis actor and exact store
/// scope from Monas.  They never resolve a POSIX user or emit a legacy
/// delegated actor to the daemon.
pub(super) fn preverified_host_object_browser_router() -> Router {
    preverified_host_object_browser_router_with_state(
        PreverifiedHostObjectBrowserRouteState::packaged(),
    )
}

pub(crate) fn preverified_host_object_browser_router_with_state(
    state: PreverifiedHostObjectBrowserRouteState,
) -> Router {
    Router::new()
        .route(
            "/api/v1/object-stores/{endpoint}/browser",
            get(preverified_host_object_store_browser),
        )
        .route(
            "/api/v1/object-stores/{endpoint}/objects/download/{*object_id}",
            get(preverified_host_object_store_object_download),
        )
        .route(
            "/api/v1/object-stores/{endpoint}/folders/download/{*prefix}",
            get(preverified_host_object_store_folder_download),
        )
        .with_state(state)
}

#[derive(Clone)]
pub(crate) struct PreverifiedHostObjectBrowserRouteState {
    object_browser_client: Arc<dyn StandaloneObjectBrowserClient>,
    daemon_bridge: Arc<DaemonBridge>,
}

impl PreverifiedHostObjectBrowserRouteState {
    fn packaged() -> Self {
        Self {
            object_browser_client: Arc::new(DaemonStandaloneObjectBrowserClient::default_packaged()),
            daemon_bridge: Arc::new(DaemonBridge::packaged()),
        }
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
    pub(crate) status: StatusCode,
    pub(crate) code: String,
    pub(crate) message: String,
}

impl StandaloneObjectBrowserClientError {
    pub(crate) fn bridge_failure(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "daemon_bridge_client_failed".to_string(),
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
            client: DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
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

/// List an ObjectStore only after the embedding Monas route has supplied a
/// matching verified Pistis subject and exact store scope.  The verified
/// envelope is deliberately scoped to the requested canonical prefix, so a
/// daemon request cannot widen the browser view after this boundary.
async fn preverified_host_object_store_browser(
    State(state): State<PreverifiedHostObjectBrowserRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
    Path(endpoint): Path<String>,
    Query(query): Query<ObjectBrowserQuery>,
) -> Result<Json<ObjectBrowserResponse>, (StatusCode, Json<AuthRouteError>)> {
    crate::auth_routes::require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &endpoint,
    )?;
    let endpoint = StoreId::new(required_field("endpoint", endpoint)?).map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            err.to_string(),
        )
    })?;
    let canonical_prefix = query.prefix.clone().unwrap_or_default();
    let verified_subject = verified_object_browser_subject(
        &verified,
        &scope.expect("checked above").0,
        endpoint.clone(),
        canonical_prefix,
    )?;
    let request = ObjectBrowserRequest {
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
        delegated_actor: None,
        verified_subject: Some(verified_subject),
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_browser_request",
            err.to_string(),
        )
    })?;
    let client = Arc::clone(&state.object_browser_client);
    state
        .daemon_bridge
        .call(move || client.object_browser(request))
        .await
        .map(Json)
        .map_err(daemon_bridge_route_error)
}

/// Download one object only through the verified Pistis subject envelope.
/// The host route deliberately has no provider-stream fallback: that legacy
/// fallback carries the transitional delegated-OS actor contract and must not
/// be reachable from a host-composed authority boundary.
async fn preverified_host_object_store_object_download(
    State(state): State<PreverifiedHostObjectBrowserRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
    Path((endpoint, object_id)): Path<(String, String)>,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    crate::auth_routes::require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &endpoint,
    )?;
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
    let verified_subject = verified_object_browser_subject(
        &verified,
        &scope.expect("checked above").0,
        endpoint.clone(),
        object_id.as_str().to_string(),
    )?;
    let request = ObjectDownloadRequest {
        endpoint,
        object_id,
        delegated_actor: None,
        verified_subject: Some(verified_subject),
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_download_request",
            err.to_string(),
        )
    })?;
    let client = Arc::clone(&state.object_browser_client);
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
    let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
    *response.headers_mut() = object_download_headers(&download)?;
    Ok(response)
}

/// Stream a folder archive only through the verified Pistis subject envelope.
/// As with the individual object route, this host-composed route deliberately
/// has neither a delegated-OS actor nor a provider-stream fallback.  The
/// daemon revalidates the exact canonical prefix against the fixed GUI/API
/// service peer before it resolves any archive entry.
async fn preverified_host_object_store_folder_download(
    State(state): State<PreverifiedHostObjectBrowserRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
    Path((endpoint, prefix)): Path<(String, String)>,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    crate::auth_routes::require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &endpoint,
    )?;
    let endpoint = StoreId::new(required_field("endpoint", endpoint)?).map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_folder_download_request",
            err.to_string(),
        )
    })?;
    let prefix = required_field("prefix", prefix.trim_start_matches('/').to_string())?;
    let verified_subject = verified_object_browser_subject(
        &verified,
        &scope.expect("checked above").0,
        endpoint.clone(),
        prefix.clone(),
    )?;
    let request = ObjectFolderDownloadRequest {
        endpoint,
        prefix,
        delegated_actor: None,
        verified_subject: Some(verified_subject),
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_folder_download_request",
            err.to_string(),
        )
    })?;
    let client = Arc::clone(&state.object_browser_client);
    let download = state
        .daemon_bridge
        .call(move || client.object_folder_download(request))
        .await
        .map_err(daemon_bridge_route_error)?;

    let headers = object_folder_download_headers(&download)?;
    let archive_download = download.clone();
    let archive_permit = archive_worker_semaphore()
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            route_error(
                StatusCode::TOO_MANY_REQUESTS,
                "archive_worker_busy",
                "folder archive capacity is saturated; retry shortly",
            )
        })?;
    let (sender, receiver) = mpsc::channel::<Result<Bytes, io::Error>>(4);
    tokio::task::spawn_blocking(move || {
        let _archive_permit = archive_permit;
        stream_folder_archive(archive_download, sender);
    });
    let mut response = Response::new(Body::from_stream(ReceiverStream::new(receiver)));
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
        DaemonBridgeError::CircuitOpen => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_bridge_circuit_open",
            "daemon control is temporarily degraded; retry shortly",
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
        DaemonClientError::Transport(_) => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_GATEWAY,
            code: "daemon_bridge_transport_failed".to_string(),
            message,
        },
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
        preverified_host_object_browser_router_with_state, verified_object_browser_subject,
        verified_object_browser_subject_for_prefix_scope, write_folder_archive,
        PreverifiedHostObjectBrowserRouteState, StandaloneObjectBrowserClient,
        StandaloneObjectBrowserClientError,
    };
    use crate::{
        accept_host_authenticated_context, daemon_bridge::DaemonBridge,
        AuthenticatedActorAuthority, AuthenticatedGuiActor, HostAuthenticatedContext,
        HostAuthenticationAuthority, HostAuthenticationContextVerifier,
        VerifiedHostObjectPrefixScope, VerifiedHostStoreScope,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::Extension;
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::ObjectState;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_daemon::{
        ObjectBrowserFileNode, ObjectBrowserReadinessState, ObjectBrowserRequest,
        ObjectBrowserResponse, ObjectDownloadRequest, ObjectDownloadResponse,
        ObjectFolderArchiveEntry, ObjectFolderDownloadRequest, ObjectFolderDownloadResponse,
    };
    use flate2::read::GzDecoder;
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Semaphore;
    use tower::ServiceExt;

    #[test]
    fn archive_worker_capacity_is_bounded() {
        let workers = Semaphore::new(1);
        let permit = workers.try_acquire().expect("first worker admitted");
        assert!(workers.try_acquire().is_err());
        drop(permit);
        assert!(workers.try_acquire().is_ok());
    }

    struct AcceptingHostVerifier;

    impl HostAuthenticationContextVerifier for AcceptingHostVerifier {
        fn verify_live_session(&self, _context: &HostAuthenticatedContext) -> Result<(), String> {
            Ok(())
        }
    }

    fn verified_host_context() -> crate::VerifiedHostAuthenticatedContext {
        accept_host_authenticated_context(
            HostAuthenticatedContext {
                schema_version: crate::HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_string(),
                authority: HostAuthenticationAuthority::MonasStandalone,
                issuer: "monas".to_string(),
                audience: crate::HOST_AUTH_AUDIENCE.to_string(),
                subject_id: "pistis:stephen".to_string(),
                session_id: "session-1".to_string(),
                roles: vec!["storage_viewer".to_string()],
                issued_at_unix_seconds: 1_000,
                expires_at_unix_seconds: 2_000,
                correlation_id: "correlation-1".to_string(),
                csrf_binding_sha256:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
            },
            1_500,
            &AcceptingHostVerifier,
        )
        .expect("verified host context")
    }

    fn verified_host_actor() -> AuthenticatedGuiActor {
        AuthenticatedGuiActor {
            subject_id: "pistis:stephen".to_string(),
            authority: AuthenticatedActorAuthority::MonasStandalone,
            roles: vec!["storage_viewer".to_string()],
            expires_at_unix_seconds: Some(2_000),
            correlation_id: Some("correlation-1".to_string()),
        }
    }

    #[test]
    fn maps_verified_host_scope_to_the_versioned_daemon_subject_without_os_identity() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let subject = verified_object_browser_subject(
            &verified,
            &scope,
            StoreId::new("ena").expect("store"),
            "ENA/Xeno".to_string(),
        )
        .expect("subject maps");

        assert_eq!(subject.subject_id, "pistis:stephen");
        assert_eq!(subject.session_id, "session-1");
        assert_eq!(subject.correlation_id, "correlation-1");
        assert_eq!(subject.store_id.as_str(), "ena");
        assert_eq!(subject.canonical_prefix, "ENA/Xeno");
        assert!(serde_json::to_value(subject)
            .expect("serializes")
            .get("uid")
            .is_none());
    }

    #[test]
    fn verified_subject_mapping_rejects_a_store_outside_the_host_scope() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let error = verified_object_browser_subject(
            &verified,
            &scope,
            StoreId::new("other").expect("store"),
            String::new(),
        )
        .expect_err("out-of-scope store rejects");

        assert_eq!(error.0, StatusCode::FORBIDDEN);
        assert_eq!(error.1 .0.code, "host_store_scope_denied");
    }

    #[test]
    fn prefix_scope_mapping_preserves_the_authorised_prefix_for_daemon_validation() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostObjectPrefixScope::for_verified_context(&verified, "ena", "ENA/Xeno")
                .expect("scope");
        let subject = verified_object_browser_subject_for_prefix_scope(
            &verified,
            &scope,
            StoreId::new("ena").expect("store"),
            "ENA/Xeno/metadata.tsv",
        )
        .expect("subject maps");

        assert_eq!(subject.canonical_prefix, "ENA/Xeno");
        assert!(verified_object_browser_subject_for_prefix_scope(
            &verified,
            &scope,
            StoreId::new("ena").expect("store"),
            "ENA/other/metadata.tsv",
        )
        .is_err());
    }

    #[tokio::test]
    async fn preverified_host_browser_forwards_only_the_verified_subject() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let client = recording_browser_client();
        let app = preverified_host_object_browser_router_with_state(
            PreverifiedHostObjectBrowserRouteState {
                object_browser_client: client.clone(),
                daemon_bridge: Arc::new(DaemonBridge::with_capacity_and_deadline(
                    1,
                    std::time::Duration::from_secs(1),
                )),
            },
        )
        .layer(Extension(verified))
        .layer(Extension(scope))
        .layer(Extension(verified_host_actor()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/browser?prefix=ENA%2FXeno")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        let requests = client.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].delegated_actor.is_none());
        let subject = requests[0]
            .verified_subject
            .as_ref()
            .expect("verified subject forwards");
        assert_eq!(subject.subject_id, "pistis:stephen");
        assert_eq!(subject.canonical_prefix, "ENA/Xeno");
    }

    #[tokio::test]
    async fn preverified_host_browser_rejects_an_out_of_scope_store_before_daemon() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let client = recording_browser_client();
        let app = preverified_host_object_browser_router_with_state(
            PreverifiedHostObjectBrowserRouteState {
                object_browser_client: client.clone(),
                daemon_bridge: Arc::new(DaemonBridge::with_capacity_and_deadline(
                    1,
                    std::time::Duration::from_secs(1),
                )),
            },
        )
        .layer(Extension(verified))
        .layer(Extension(scope))
        .layer(Extension(verified_host_actor()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/other/browser")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(client.requests().is_empty());
    }

    #[tokio::test]
    async fn preverified_host_object_download_forwards_only_a_scoped_verified_subject() {
        let root = temp_root("preverified-host-object-download");
        let source_path = write_test_file(&root, "metadata.tsv", b"verified payload");
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let client = recording_browser_client();
        client.set_download(ObjectDownloadResponse {
            endpoint: StoreId::new("ena").expect("endpoint"),
            store_id: StoreId::new("ena").expect("store"),
            object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object"),
            file_name: "metadata.tsv".to_string(),
            source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-one").expect("disk"),
            source_path: source_path.clone(),
            size_bytes: b"verified payload".len() as u64,
        });
        let app = preverified_host_object_browser_router_with_state(
            PreverifiedHostObjectBrowserRouteState {
                object_browser_client: client.clone(),
                daemon_bridge: Arc::new(DaemonBridge::with_capacity_and_deadline(
                    1,
                    std::time::Duration::from_secs(1),
                )),
            },
        )
        .layer(Extension(verified))
        .layer(Extension(scope))
        .layer(Extension(verified_host_actor()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/objects/download/ENA/Xeno/metadata.tsv")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            client.download_requests(),
            vec![ObjectDownloadRequest {
                endpoint: StoreId::new("ena").expect("endpoint"),
                object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object"),
                delegated_actor: None,
                verified_subject: Some(
                    verified_object_browser_subject(
                        &verified_host_context(),
                        &VerifiedHostStoreScope::for_verified_context(
                            &verified_host_context(),
                            vec!["ena".to_string()],
                        )
                        .expect("scope"),
                        StoreId::new("ena").expect("endpoint"),
                        "ENA/Xeno/metadata.tsv".to_string(),
                    )
                    .expect("subject"),
                ),
            }]
        );
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        assert_eq!(&body[..], b"verified payload");
        cleanup(&root);
    }

    #[tokio::test]
    async fn preverified_host_object_download_rejects_an_out_of_scope_store_before_daemon() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let client = recording_browser_client();
        let app = preverified_host_object_browser_router_with_state(
            PreverifiedHostObjectBrowserRouteState {
                object_browser_client: client.clone(),
                daemon_bridge: Arc::new(DaemonBridge::with_capacity_and_deadline(
                    1,
                    std::time::Duration::from_secs(1),
                )),
            },
        )
        .layer(Extension(verified))
        .layer(Extension(scope))
        .layer(Extension(verified_host_actor()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/other/objects/download/ENA/Xeno/metadata.tsv")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(client.download_requests().is_empty());
    }

    #[tokio::test]
    async fn preverified_host_folder_download_forwards_only_a_scoped_verified_subject() {
        let root = temp_root("preverified-host-folder-download");
        let first = write_test_file(&root, "metadata.tsv", b"verified metadata");
        let second = write_test_file(&root, "reads.fastq.gz", b"verified reads");
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let client = recording_browser_client();
        client.set_folder_download(ObjectFolderDownloadResponse {
            endpoint: StoreId::new("ena").expect("endpoint"),
            store_id: StoreId::new("ena").expect("store"),
            prefix: "ENA/Xeno".to_string(),
            archive_name: "Xeno.tar.gz".to_string(),
            total_files: 2,
            total_source_bytes: (b"verified metadata".len() + b"verified reads".len()) as u64,
            entries: vec![
                ObjectFolderArchiveEntry {
                    object_id: ObjectId::new("ENA/Xeno/metadata.tsv").expect("object"),
                    archive_path: "metadata.tsv".to_string(),
                    source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-one")
                        .expect("disk"),
                    source_path: first,
                    size_bytes: b"verified metadata".len() as u64,
                },
                ObjectFolderArchiveEntry {
                    object_id: ObjectId::new("ENA/Xeno/reads.fastq.gz").expect("object"),
                    archive_path: "reads.fastq.gz".to_string(),
                    source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-one")
                        .expect("disk"),
                    source_path: second,
                    size_bytes: b"verified reads".len() as u64,
                },
            ],
        });
        let app = preverified_host_object_browser_router_with_state(
            PreverifiedHostObjectBrowserRouteState {
                object_browser_client: client.clone(),
                daemon_bridge: Arc::new(DaemonBridge::with_capacity_and_deadline(
                    1,
                    std::time::Duration::from_secs(1),
                )),
            },
        )
        .layer(Extension(verified))
        .layer(Extension(scope))
        .layer(Extension(verified_host_actor()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/ena/folders/download/ENA/Xeno")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-dasobjectstore-archive-files"], "2");
        assert_eq!(
            client.folder_download_requests(),
            vec![ObjectFolderDownloadRequest {
                endpoint: StoreId::new("ena").expect("endpoint"),
                prefix: "ENA/Xeno".to_string(),
                delegated_actor: None,
                verified_subject: Some(
                    verified_object_browser_subject(
                        &verified_host_context(),
                        &VerifiedHostStoreScope::for_verified_context(
                            &verified_host_context(),
                            vec!["ena".to_string()],
                        )
                        .expect("scope"),
                        StoreId::new("ena").expect("endpoint"),
                        "ENA/Xeno".to_string(),
                    )
                    .expect("subject"),
                ),
            }]
        );
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("archive bytes");
        assert_eq!(
            tar_gz_members(&body),
            vec![
                ("metadata.tsv".to_string(), b"verified metadata".to_vec()),
                ("reads.fastq.gz".to_string(), b"verified reads".to_vec()),
            ]
        );
        cleanup(&root);
    }

    #[tokio::test]
    async fn preverified_host_folder_download_rejects_an_out_of_scope_store_before_daemon() {
        let verified = verified_host_context();
        let scope =
            VerifiedHostStoreScope::for_verified_context(&verified, vec!["ena".to_string()])
                .expect("scope");
        let client = recording_browser_client();
        let app = preverified_host_object_browser_router_with_state(
            PreverifiedHostObjectBrowserRouteState {
                object_browser_client: client.clone(),
                daemon_bridge: Arc::new(DaemonBridge::with_capacity_and_deadline(
                    1,
                    std::time::Duration::from_secs(1),
                )),
            },
        )
        .layer(Extension(verified))
        .layer(Extension(scope))
        .layer(Extension(verified_host_actor()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/object-stores/other/folders/download/ENA/Xeno")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(client.folder_download_requests().is_empty());
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

    fn recording_browser_client() -> Arc<RecordingObjectBrowserClient> {
        Arc::new(RecordingObjectBrowserClient::default())
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
                    download_source: Some(
                        dasobjectstore_daemon::ObjectBrowserDownloadSource::HddSettled,
                    ),
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
