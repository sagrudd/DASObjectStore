use crate::api::{
    write_provider_stream_frame, DaemonApiErrorResponse, DaemonApiRequest, DaemonApiResponse,
    ProviderStreamChunkHeader, ProviderStreamFrameError,
    ProviderStreamMultipartPartUploadOpenRequest, ProviderStreamOpenRequest,
    ProviderStreamUploadOpenRequest, ProviderStreamVerifier,
};
use crate::auth::DaemonLocalActor;
use crate::runtime::DaemonIngestFilesRuntimeError;
use crate::server::{DaemonRequestHandler, DaemonRequestHandlerError, DaemonServiceOrchestrator};
use crate::DaemonClock;
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

const SOCKET_MODE: u32 = 0o660;
const MAX_CONTROL_CONNECTIONS: usize = 8;
const MAX_PRIORITY_CONTROL_CONNECTIONS: usize = 2;
const MAX_INGEST_CONNECTIONS: usize = 2;
const MAX_REQUEST_LINE_BYTES: usize = 64 * 1024;

pub struct UnixSocketDaemonServer<H> {
    socket_path: PathBuf,
    handler: H,
}

impl<H> UnixSocketDaemonServer<H>
where
    H: DaemonApiHandler,
{
    pub fn new(socket_path: impl Into<PathBuf>, handler: H) -> Self {
        Self {
            socket_path: socket_path.into(),
            handler,
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn serve_forever(&self) -> Result<(), UnixSocketDaemonServerError>
    where
        H: Sync,
    {
        let listener = bind_listener(&self.socket_path)?;
        let control_connections = AtomicUsize::new(0);
        let priority_control_connections = AtomicUsize::new(0);
        let ingest_connections = AtomicUsize::new(0);

        thread::scope(|scope| {
            for stream in listener.incoming() {
                let stream = stream.map_err(UnixSocketDaemonServerError::Accept)?;
                let Some(pending) = receive_stream(stream)? else {
                    continue;
                };
                let active_connections = if pending.request.is_ingest_submission() {
                    (&ingest_connections, MAX_INGEST_CONNECTIONS)
                } else if pending.request.is_priority_control_request() {
                    (
                        &priority_control_connections,
                        MAX_PRIORITY_CONTROL_CONNECTIONS,
                    )
                } else {
                    (&control_connections, MAX_CONTROL_CONNECTIONS)
                };
                let Some(permit) =
                    ConnectionPermit::try_acquire(active_connections.0, active_connections.1)
                else {
                    let mut stream = pending.stream;
                    write_response_frame(
                        &mut stream,
                        &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "server_busy",
                            "daemon request capacity is currently reserved for active work; retry shortly",
                        )),
                    )?;
                    continue;
                };
                let handler = &self.handler;
                scope.spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_pending_stream(pending, handler) {
                        if !error.is_client_disconnect() {
                            eprintln!("daemon client request failed: {error}");
                        }
                    }
                });
            }
            Ok(())
        })
    }

    pub fn handle_stream(&self, stream: UnixStream) -> Result<(), UnixSocketDaemonServerError> {
        handle_stream(stream, &self.handler)
    }
}

struct ConnectionPermit<'a> {
    active_connections: &'a AtomicUsize,
}

impl<'a> ConnectionPermit<'a> {
    fn try_acquire(active_connections: &'a AtomicUsize, limit: usize) -> Option<Self> {
        let mut current = active_connections.load(Ordering::Acquire);
        loop {
            if current >= limit {
                return None;
            }
            match active_connections.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self { active_connections }),
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for ConnectionPermit<'_> {
    fn drop(&mut self) {
        self.active_connections.fetch_sub(1, Ordering::Release);
    }
}

pub trait DaemonApiHandler {
    fn handle_api_request(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, UnixSocketDaemonServerError>;

    fn handle_api_request_streaming(
        &self,
        request: DaemonApiRequest,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let response = self.handle_api_request(request)?;
        emit_response(response)
    }

    fn handle_api_request_streaming_for_actor(
        &self,
        request: DaemonApiRequest,
        actor: Option<&DaemonLocalActor>,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let _ = actor;
        self.handle_api_request_streaming(request, emit_response)
    }

    /// Handle the path-free provider-stream open envelope. Successful
    /// implementations emit bounded binary frames through `emit_frame`; a
    /// handler without a provider reader remains fail-closed by default.
    fn handle_provider_stream_open_for_actor(
        &self,
        _request: ProviderStreamOpenRequest,
        _actor: Option<&DaemonLocalActor>,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
        _emit_frame: &mut dyn FnMut(
            &ProviderStreamChunkHeader,
            &[u8],
        ) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "not_implemented",
            "provider stream reader is not wired into dasobjectstored yet",
        )))
    }

    /// Handle a bounded client-to-daemon provider upload. The default remains
    /// fail-closed: an envelope is not permission to stage bytes or mutate
    /// catalogue state. Implementations must consume frames one at a time and
    /// publish a terminal response only after daemon-owned staging and
    /// verification complete.
    fn handle_provider_stream_upload_for_actor(
        &self,
        request: ProviderStreamUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let _ = (request, actor, read_frame);
        emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "not_implemented",
            "provider stream upload writer is not wired into dasobjectstored yet",
        )))
    }

    /// Handle one reservation-bound multipart part. The default remains
    /// fail-closed until the runtime stages bytes under daemon ownership.
    fn handle_provider_stream_multipart_part_upload_for_actor(
        &self,
        request: ProviderStreamMultipartPartUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let _ = (request, actor, read_frame);
        emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "not_implemented",
            "multipart part staging is not wired into dasobjectstored yet",
        )))
    }
}

impl<S, C> DaemonApiHandler for DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    fn handle_api_request(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, UnixSocketDaemonServerError> {
        self.handle(request)
            .map_err(UnixSocketDaemonServerError::Handler)
    }

    fn handle_api_request_streaming_for_actor(
        &self,
        request: DaemonApiRequest,
        actor: Option<&DaemonLocalActor>,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let response = self
            .handle_with_progress_for_actor(request, actor, |event| {
                emit_response(DaemonApiResponse::IngestProgress(event)).map_err(|err| {
                    DaemonIngestFilesRuntimeError::ClientDisconnected(format!(
                        "upload cancelled because the client disconnected: {err}"
                    ))
                })
            })
            .map_err(UnixSocketDaemonServerError::Handler)?;
        emit_response(response)
    }

    fn handle_provider_stream_open_for_actor(
        &self,
        request: ProviderStreamOpenRequest,
        actor: Option<&DaemonLocalActor>,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
        emit_frame: &mut dyn FnMut(
            &ProviderStreamChunkHeader,
            &[u8],
        ) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let source = match self.open_provider_stream(&request, actor) {
            Ok(source) => source,
            Err(response) => return emit_response(response),
        };
        let mut verifier =
            ProviderStreamVerifier::new(request.request_id.clone()).map_err(|error| {
                UnixSocketDaemonServerError::Handler(DaemonRequestHandlerError::ServiceRuntime(
                    crate::runtime::DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: error.to_string(),
                    },
                ))
            })?;
        let cancellation = verifier.cancellation_token();
        let mut reader = source.reader;
        let mut hasher = Sha256::new();
        let mut offset = 0_u64;
        let mut buffer = vec![0_u8; request.chunk_size_bytes as usize];
        let mut emitted_frame = false;
        loop {
            if cancellation.is_cancelled() {
                return Ok(());
            }
            let read = match reader.read(&mut buffer) {
                Ok(read) => read,
                Err(error) => {
                    if !emitted_frame {
                        return emit_response(DaemonApiResponse::Error(
                            DaemonApiErrorResponse::new(
                                "provider_stream_read_failed",
                                error.to_string(),
                            ),
                        ));
                    }
                    return Err(UnixSocketDaemonServerError::Io(error));
                }
            };
            if read == 0 {
                let checksum = format!("sha256:{:x}", hasher.clone().finalize());
                let header = ProviderStreamChunkHeader {
                    schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                    request_id: request.request_id.clone(),
                    offset,
                    payload_len: 0,
                    final_chunk: true,
                    total_size: Some(offset),
                    sha256: Some(checksum.clone()),
                };
                if let Err(error) = verifier.finish(&header, &[]) {
                    if !emitted_frame {
                        return emit_response(DaemonApiResponse::Error(
                            DaemonApiErrorResponse::new(
                                "provider_stream_verification_failed",
                                error.to_string(),
                            ),
                        ));
                    }
                    return Err(UnixSocketDaemonServerError::Handler(
                        DaemonRequestHandlerError::ServiceRuntime(
                            crate::runtime::DaemonServiceRuntimeError::UnsupportedOperation {
                                operation: error.to_string(),
                            },
                        ),
                    ));
                }
                if offset != source.expected_size_bytes {
                    return Err(UnixSocketDaemonServerError::Handler(
                        DaemonRequestHandlerError::ServiceRuntime(
                            crate::runtime::DaemonServiceRuntimeError::UnsupportedOperation {
                                operation: format!(
                                    "provider stream size {} does not match expected {}",
                                    offset, source.expected_size_bytes
                                ),
                            },
                        ),
                    ));
                }
                if source
                    .expected_checksum
                    .as_deref()
                    .is_some_and(|expected| !expected.eq_ignore_ascii_case(&checksum))
                {
                    return Err(UnixSocketDaemonServerError::Handler(
                        DaemonRequestHandlerError::ServiceRuntime(
                            crate::runtime::DaemonServiceRuntimeError::UnsupportedOperation {
                                operation: "provider stream checksum differs from catalogue"
                                    .to_string(),
                            },
                        ),
                    ));
                }
                if let Err(error) = emit_frame(&header, &[]) {
                    cancellation.cancel();
                    return Err(error);
                }
                return Ok(());
            }
            let payload = &buffer[..read];
            hasher.update(payload);
            let header = ProviderStreamChunkHeader {
                schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request.request_id.clone(),
                offset,
                payload_len: read as u32,
                final_chunk: false,
                total_size: None,
                sha256: None,
            };
            if let Err(error) = verifier.push(&header, payload) {
                if !emitted_frame {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_verification_failed",
                        error.to_string(),
                    )));
                }
                return Err(UnixSocketDaemonServerError::Handler(
                    DaemonRequestHandlerError::ServiceRuntime(
                        crate::runtime::DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: error.to_string(),
                        },
                    ),
                ));
            }
            if let Err(error) = emit_frame(&header, payload) {
                cancellation.cancel();
                return Err(error);
            }
            emitted_frame = true;
            offset = offset.checked_add(read as u64).ok_or_else(|| {
                UnixSocketDaemonServerError::Handler(DaemonRequestHandlerError::ServiceRuntime(
                    crate::runtime::DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: "provider stream offset overflow".to_string(),
                    },
                ))
            })?;
        }
    }

    fn handle_provider_stream_upload_for_actor(
        &self,
        request: ProviderStreamUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        DaemonRequestHandler::<S, C>::handle_provider_stream_upload_for_actor(
            self,
            request,
            actor,
            read_frame,
            emit_response,
        )
    }

    fn handle_provider_stream_multipart_part_upload_for_actor(
        &self,
        request: ProviderStreamMultipartPartUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        DaemonRequestHandler::<S, C>::handle_provider_stream_multipart_part_upload_for_actor(
            self,
            request,
            actor,
            read_frame,
            emit_response,
        )
    }
}

impl<F> DaemonApiHandler for F
where
    F: Fn(DaemonApiRequest) -> Result<DaemonApiResponse, UnixSocketDaemonServerError>,
{
    fn handle_api_request(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, UnixSocketDaemonServerError> {
        self(request)
    }
}

#[derive(Debug)]
pub enum UnixSocketDaemonServerError {
    MissingParent { socket_path: PathBuf },
    CreateRuntimeDir(std::io::Error),
    RemoveStaleSocket(std::io::Error),
    Bind(std::io::Error),
    SetPermissions(std::io::Error),
    Accept(std::io::Error),
    Io(std::io::Error),
    Decode(serde_json::Error),
    Encode(serde_json::Error),
    Handler(DaemonRequestHandlerError),
    ProviderStreamFrame(ProviderStreamFrameError),
    PeerCredentials(std::io::Error),
    RequestLineTooLarge { max_bytes: usize },
    RequestLineInvalidUtf8,
}

impl Display for UnixSocketDaemonServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingParent { socket_path } => {
                write!(
                    formatter,
                    "daemon socket path has no parent directory: {}",
                    socket_path.display()
                )
            }
            Self::CreateRuntimeDir(error) => {
                write!(formatter, "failed to create runtime dir: {error}")
            }
            Self::RemoveStaleSocket(error) => {
                write!(formatter, "failed to remove stale daemon socket: {error}")
            }
            Self::Bind(error) => write!(formatter, "failed to bind daemon socket: {error}"),
            Self::SetPermissions(error) => write!(
                formatter,
                "failed to set daemon socket permissions: {error}"
            ),
            Self::Accept(error) => write!(
                formatter,
                "failed to accept daemon client connection: {error}"
            ),
            Self::Io(error) => write!(formatter, "daemon socket IO failed: {error}"),
            Self::Decode(error) => write!(formatter, "failed to decode daemon request: {error}"),
            Self::Encode(error) => write!(formatter, "failed to encode daemon response: {error}"),
            Self::Handler(error) => Display::fmt(error, formatter),
            Self::ProviderStreamFrame(error) => Display::fmt(error, formatter),
            Self::PeerCredentials(error) => {
                write!(
                    formatter,
                    "failed to read daemon client credentials: {error}"
                )
            }
            Self::RequestLineTooLarge { max_bytes } => write!(
                formatter,
                "daemon request envelope exceeds the {max_bytes}-byte limit"
            ),
            Self::RequestLineInvalidUtf8 => {
                formatter.write_str("daemon request envelope is not valid UTF-8")
            }
        }
    }
}

impl std::error::Error for UnixSocketDaemonServerError {}

impl UnixSocketDaemonServerError {
    fn is_client_disconnect(&self) -> bool {
        match self {
            Self::Io(error) => client_disconnect_kind(error.kind()),
            Self::Encode(error) => error.io_error_kind().is_some_and(client_disconnect_kind),
            Self::Handler(DaemonRequestHandlerError::IngestRuntime(
                DaemonIngestFilesRuntimeError::ClientDisconnected(_),
            )) => true,
            Self::ProviderStreamFrame(ProviderStreamFrameError::Io(error)) => {
                client_disconnect_kind(error.kind())
            }
            _ => false,
        }
    }
}

fn client_disconnect_kind(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::NotConnected
    )
}

fn bind_listener(socket_path: &Path) -> Result<UnixListener, UnixSocketDaemonServerError> {
    let runtime_dir =
        socket_path
            .parent()
            .ok_or_else(|| UnixSocketDaemonServerError::MissingParent {
                socket_path: socket_path.to_path_buf(),
            })?;
    fs::create_dir_all(runtime_dir).map_err(UnixSocketDaemonServerError::CreateRuntimeDir)?;
    if socket_path.exists() {
        fs::remove_file(socket_path).map_err(UnixSocketDaemonServerError::RemoveStaleSocket)?;
    }
    let listener = UnixListener::bind(socket_path).map_err(UnixSocketDaemonServerError::Bind)?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(SOCKET_MODE))
        .map_err(UnixSocketDaemonServerError::SetPermissions)?;
    Ok(listener)
}

fn handle_stream(
    stream: UnixStream,
    handler: &impl DaemonApiHandler,
) -> Result<(), UnixSocketDaemonServerError> {
    let Some(pending) = receive_stream(stream)? else {
        return Ok(());
    };
    let result = handle_pending_stream(pending, handler);
    if result
        .as_ref()
        .is_err_and(UnixSocketDaemonServerError::is_client_disconnect)
    {
        return Ok(());
    }
    result
}

struct PendingStream {
    stream: UnixStream,
    request: PendingRequest,
    actor: Option<DaemonLocalActor>,
}

enum PendingRequest {
    Api(DaemonApiRequest),
    ProviderStream(ProviderStreamOpenRequest),
    ProviderStreamUpload(ProviderStreamUploadOpenRequest),
    ProviderStreamMultipartPartUpload(ProviderStreamMultipartPartUploadOpenRequest),
}

fn receive_stream(
    mut stream: UnixStream,
) -> Result<Option<PendingStream>, UnixSocketDaemonServerError> {
    let actor = peer_actor_for_stream(&stream)?;
    // Do not use a buffered clone here. Upload requests are followed by
    // binary frames on the same socket; read-ahead would consume those bytes
    // into a dropped BufReader before the upload handler can dispatch them.
    let line = match read_request_line(&mut stream) {
        Ok(line) => line,
        Err(UnixSocketDaemonServerError::RequestLineTooLarge { max_bytes }) => {
            write_response_frame(
                &mut stream,
                &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "bad_request",
                    format!("daemon request envelope exceeds the {max_bytes}-byte limit"),
                )),
            )?;
            return Ok(None);
        }
        Err(UnixSocketDaemonServerError::RequestLineInvalidUtf8) => {
            write_response_frame(
                &mut stream,
                &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "bad_request",
                    "daemon request envelope must be valid UTF-8",
                )),
            )?;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    if let Ok(request) = serde_json::from_str::<DaemonApiRequest>(&line) {
        return Ok(Some(PendingStream {
            stream,
            request: PendingRequest::Api(request),
            actor,
        }));
    }
    if let Ok(request) = serde_json::from_str::<ProviderStreamOpenRequest>(&line) {
        if let Err(error) = request.validate() {
            write_response_frame(
                &mut stream,
                &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "bad_request",
                    format!("invalid provider stream request: {error}"),
                )),
            )?;
            return Ok(None);
        }
        return Ok(Some(PendingStream {
            stream,
            request: PendingRequest::ProviderStream(request),
            actor,
        }));
    }
    if let Ok(request) = serde_json::from_str::<ProviderStreamUploadOpenRequest>(&line) {
        if let Err(error) = request.validate() {
            write_response_frame(
                &mut stream,
                &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "bad_request",
                    format!("invalid provider stream upload request: {error}"),
                )),
            )?;
            return Ok(None);
        }
        return Ok(Some(PendingStream {
            stream,
            request: PendingRequest::ProviderStreamUpload(request),
            actor,
        }));
    }
    if let Ok(request) = serde_json::from_str::<ProviderStreamMultipartPartUploadOpenRequest>(&line)
    {
        if let Err(error) = request.validate() {
            write_response_frame(
                &mut stream,
                &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "bad_request",
                    format!("invalid multipart part upload request: {error}"),
                )),
            )?;
            return Ok(None);
        }
        return Ok(Some(PendingStream {
            stream,
            request: PendingRequest::ProviderStreamMultipartPartUpload(request),
            actor,
        }));
    }
    {
        let error = serde_json::from_str::<DaemonApiRequest>(&line).expect_err("request parse");
        write_response_frame(
            &mut stream,
            &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "bad_request",
                format!("failed to decode daemon request: {error}"),
            )),
        )?;
        Ok(None)
    }
}

fn read_request_line(stream: &mut UnixStream) -> Result<String, UnixSocketDaemonServerError> {
    let mut bytes = Vec::with_capacity(1024);
    let mut byte = [0_u8; 1];
    loop {
        let read = stream
            .read(&mut byte)
            .map_err(UnixSocketDaemonServerError::Io)?;
        if read == 0 {
            return Err(UnixSocketDaemonServerError::Io(std::io::Error::new(
                ErrorKind::UnexpectedEof,
                "daemon client closed before request envelope newline",
            )));
        }
        if byte[0] == b'\n' {
            return String::from_utf8(bytes)
                .map_err(|_| UnixSocketDaemonServerError::RequestLineInvalidUtf8);
        }
        bytes.push(byte[0]);
        if bytes.len() > MAX_REQUEST_LINE_BYTES {
            return Err(UnixSocketDaemonServerError::RequestLineTooLarge {
                max_bytes: MAX_REQUEST_LINE_BYTES,
            });
        }
    }
}

fn handle_pending_stream(
    mut pending: PendingStream,
    handler: &impl DaemonApiHandler,
) -> Result<(), UnixSocketDaemonServerError> {
    match pending.request {
        PendingRequest::Api(request) => {
            let mut emit_response = |response| write_response_frame(&mut pending.stream, &response);
            handler.handle_api_request_streaming_for_actor(
                request,
                pending.actor.as_ref(),
                &mut emit_response,
            )?;
        }
        PendingRequest::ProviderStream(request) => {
            let mut response_stream = pending
                .stream
                .try_clone()
                .map_err(UnixSocketDaemonServerError::Io)?;
            let mut frame_stream = pending
                .stream
                .try_clone()
                .map_err(UnixSocketDaemonServerError::Io)?;
            let mut emit_response =
                |response| write_response_frame(&mut response_stream, &response);
            let mut emit_frame = |header: &ProviderStreamChunkHeader, payload: &[u8]| {
                write_provider_stream_frame(&mut frame_stream, header, payload)
                    .map_err(UnixSocketDaemonServerError::ProviderStreamFrame)
            };
            handler.handle_provider_stream_open_for_actor(
                request,
                pending.actor.as_ref(),
                &mut emit_response,
                &mut emit_frame,
            )?;
        }
        PendingRequest::ProviderStreamUpload(request) => {
            let mut response_stream = pending
                .stream
                .try_clone()
                .map_err(UnixSocketDaemonServerError::Io)?;
            let mut emit_response =
                |response| write_response_frame(&mut response_stream, &response);
            let mut read_frame = || {
                crate::api::read_provider_stream_frame(&mut pending.stream)
                    .map_err(UnixSocketDaemonServerError::ProviderStreamFrame)
            };
            match handler.handle_provider_stream_upload_for_actor(
                request,
                pending.actor.as_ref(),
                &mut read_frame,
                &mut emit_response,
            ) {
                Err(UnixSocketDaemonServerError::ProviderStreamFrame(error))
                    if !matches!(
                        &error,
                        ProviderStreamFrameError::Io(io)
                            if client_disconnect_kind(io.kind())
                    ) =>
                {
                    write_response_frame(
                        &mut response_stream,
                        &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "bad_request",
                            format!("invalid provider stream frame: {error}"),
                        )),
                    )?;
                }
                result => result?,
            }
        }
        PendingRequest::ProviderStreamMultipartPartUpload(request) => {
            let mut response_stream = pending
                .stream
                .try_clone()
                .map_err(UnixSocketDaemonServerError::Io)?;
            let mut emit_response =
                |response| write_response_frame(&mut response_stream, &response);
            let mut read_frame = || {
                crate::api::read_provider_stream_frame(&mut pending.stream)
                    .map_err(UnixSocketDaemonServerError::ProviderStreamFrame)
            };
            match handler.handle_provider_stream_multipart_part_upload_for_actor(
                request,
                pending.actor.as_ref(),
                &mut read_frame,
                &mut emit_response,
            ) {
                Err(UnixSocketDaemonServerError::ProviderStreamFrame(error))
                    if !matches!(
                        &error,
                        ProviderStreamFrameError::Io(io)
                            if client_disconnect_kind(io.kind())
                    ) =>
                {
                    write_response_frame(
                        &mut response_stream,
                        &DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "bad_request",
                            format!("invalid multipart part frame: {error}"),
                        )),
                    )?;
                }
                result => result?,
            }
        }
    }
    Ok(())
}

trait DaemonApiRequestClass {
    fn is_ingest_submission(&self) -> bool;
    fn is_priority_control_request(&self) -> bool;
}

impl DaemonApiRequestClass for DaemonApiRequest {
    fn is_ingest_submission(&self) -> bool {
        matches!(
            self,
            Self::SubmitIngestFiles(_) | Self::RemoteEasyconnectSubmitAwsCliUpload(_)
        )
    }

    fn is_priority_control_request(&self) -> bool {
        matches!(self, Self::CancelJob(_))
    }
}

impl PendingRequest {
    fn is_ingest_submission(&self) -> bool {
        match self {
            Self::Api(request) => request.is_ingest_submission(),
            Self::ProviderStream(_) => false,
            Self::ProviderStreamUpload(_) => true,
            Self::ProviderStreamMultipartPartUpload(_) => true,
        }
    }

    fn is_priority_control_request(&self) -> bool {
        match self {
            Self::Api(request) => request.is_priority_control_request(),
            Self::ProviderStream(_) => false,
            Self::ProviderStreamUpload(_) => false,
            Self::ProviderStreamMultipartPartUpload(_) => false,
        }
    }
}

#[cfg(target_os = "linux")]
fn peer_actor_for_stream(
    stream: &UnixStream,
) -> Result<Option<DaemonLocalActor>, UnixSocketDaemonServerError> {
    crate::auth::read_linux_peer_actor(stream)
        .map(Some)
        .map_err(UnixSocketDaemonServerError::PeerCredentials)
}

#[cfg(not(target_os = "linux"))]
fn peer_actor_for_stream(
    _stream: &UnixStream,
) -> Result<Option<DaemonLocalActor>, UnixSocketDaemonServerError> {
    Ok(None)
}

fn write_response_frame(
    stream: &mut UnixStream,
    response: &DaemonApiResponse,
) -> Result<(), UnixSocketDaemonServerError> {
    serde_json::to_writer(&mut *stream, &response).map_err(UnixSocketDaemonServerError::Encode)?;
    stream
        .write_all(b"\n")
        .map_err(UnixSocketDaemonServerError::Io)?;
    stream.flush().map_err(UnixSocketDaemonServerError::Io)
}

#[cfg(test)]
mod tests {
    use super::{DaemonApiHandler, DaemonApiRequestClass, UnixSocketDaemonServer};
    use crate::api::read_provider_stream_frame;
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, DaemonIngestPipelineStage, DaemonIngestProgressEvent,
        DaemonIngestStage, DaemonJobCancelRequest, DaemonJobId, DaemonServiceStatusResponse,
        ProviderStreamChunkHeader, ProviderStreamOpenRequest, ProviderStreamUploadOpenRequest,
        StoreInventoryRequest, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
        PROVIDER_STREAM_SCHEMA_VERSION,
    };
    use dasobjectstore_core::backend::BackendObjectKey;
    use dasobjectstore_core::ids::{IngestJobId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{mpsc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn classifies_cancellation_as_priority_control() {
        let cancellation = DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
            job_id: DaemonJobId::new("reconcile-job-1").expect("job id"),
            reason: Some("operator requested cancellation".to_string()),
        });
        assert!(cancellation.is_priority_control_request());
        assert!(!cancellation.is_ingest_submission());

        let status = DaemonApiRequest::StoreInventory(StoreInventoryRequest::default());
        assert!(!status.is_priority_control_request());
        assert!(!status.is_ingest_submission());
    }

    #[test]
    fn handles_one_line_json_request() {
        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new("/tmp/dasobjectstored-test.sock", |request| {
            assert!(matches!(request, DaemonApiRequest::StoreInventory(_)));
            Ok(DaemonApiResponse::ServiceStatus(
                DaemonServiceStatusResponse {
                    provider_id: ObjectServiceProviderId::Garage,
                    state: ServiceState::Running,
                    endpoint: Some("http://127.0.0.1:3900".to_string()),
                    message: None,
                    detail: None,
                },
            ))
        });

        serde_json::to_writer(
            &mut client,
            &DaemonApiRequest::StoreInventory(StoreInventoryRequest::default()),
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");

        server.handle_stream(server_stream).expect("stream handled");

        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::ServiceStatus(DaemonServiceStatusResponse {
                state: ServiceState::Running,
                ..
            })
        ));
    }

    #[test]
    fn rejects_oversized_request_envelope_with_typed_bad_request() {
        let (client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new("/tmp/dasobjectstored-test.sock", |_request| {
            panic!("oversized request must not reach the handler")
        });
        let mut writer = client.try_clone().expect("client clone");
        let write_join = thread::spawn(move || {
            writer
                .write_all(&vec![b'x'; super::MAX_REQUEST_LINE_BYTES + 1])
                .expect("oversized request written");
        });

        server.handle_stream(server_stream).expect("stream handled");
        write_join.join().expect("writer thread joins");

        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "bad_request"
        ));
    }

    #[test]
    fn rejects_non_utf8_request_envelope_with_typed_bad_request() {
        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new("/tmp/dasobjectstored-test.sock", |_request| {
            panic!("non-UTF-8 request must not reach the handler")
        });

        client
            .write_all(&[0xff, b'\n'])
            .expect("invalid request written");
        server.handle_stream(server_stream).expect("stream handled");

        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "bad_request"
        ));
    }

    #[test]
    fn dispatches_standalone_provider_stream_open_to_bounded_frame_handler() {
        struct ProviderStreamingHandler;

        impl DaemonApiHandler for ProviderStreamingHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                panic!("provider stream test should use the stream handler")
            }

            fn handle_provider_stream_open_for_actor(
                &self,
                request: ProviderStreamOpenRequest,
                _actor: Option<&crate::auth::DaemonLocalActor>,
                _emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
                emit_frame: &mut dyn FnMut(
                    &ProviderStreamChunkHeader,
                    &[u8],
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                assert_eq!(request.request_id, "provider-stream-test");
                let payload = b"hello";
                emit_frame(
                    &ProviderStreamChunkHeader {
                        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                        request_id: request.request_id,
                        offset: 0,
                        payload_len: payload.len() as u32,
                        final_chunk: true,
                        total_size: Some(payload.len() as u64),
                        sha256: Some(
                            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                                .to_string(),
                        ),
                    },
                    payload,
                )
            }
        }

        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new(
            "/tmp/dasobjectstored-provider-stream-test.sock",
            ProviderStreamingHandler,
        );
        serde_json::to_writer(
            &mut client,
            &ProviderStreamOpenRequest {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: "provider-stream-test".to_string(),
                store_id: StoreId::new("stream-store").expect("store id"),
                object: BackendObjectKey {
                    object_id: "objects/hello.txt".to_string(),
                    version: 1,
                },
                range: None,
                condition: Default::default(),
                chunk_size_bytes: 1024,
            },
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");

        server.handle_stream(server_stream).expect("stream handled");

        let (header, payload) = read_provider_stream_frame(&mut client).expect("frame decoded");
        assert!(header.final_chunk);
        assert_eq!(header.request_id, "provider-stream-test");
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn dispatches_provider_stream_upload_to_bounded_frame_reader() {
        struct ProviderUploadHandler;

        impl DaemonApiHandler for ProviderUploadHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                panic!("provider upload test should use the upload handler")
            }

            fn handle_provider_stream_upload_for_actor(
                &self,
                request: ProviderStreamUploadOpenRequest,
                _actor: Option<&crate::auth::DaemonLocalActor>,
                read_frame: &mut dyn FnMut() -> Result<
                    (ProviderStreamChunkHeader, Vec<u8>),
                    super::UnixSocketDaemonServerError,
                >,
                emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                assert_eq!(request.upload_id, "capability-1");
                let (_, first) = read_frame()?;
                let (final_header, second) = read_frame()?;
                assert!(final_header.final_chunk);
                assert_eq!([first, second].concat(), b"hello");
                emit_response(DaemonApiResponse::Error(
                    crate::api::DaemonApiErrorResponse::new("upload_test", "frames consumed"),
                ))
            }
        }

        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new(
            "/tmp/dasobjectstored-provider-upload-test.sock",
            ProviderUploadHandler,
        );
        serde_json::to_writer(
            &mut client,
            &ProviderStreamUploadOpenRequest {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: "upload-stream-1".to_string(),
                upload_id: "capability-1".to_string(),
                store_id: StoreId::new("stream-store").expect("store id"),
                object: BackendObjectKey {
                    object_id: "objects/hello.txt".to_string(),
                    version: 1,
                },
                expected_size_bytes: 5,
                expected_sha256:
                    "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_string(),
                chunk_size_bytes: 1024,
            },
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");
        let first = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "upload-stream-1".to_string(),
            offset: 0,
            payload_len: 2,
            final_chunk: false,
            total_size: None,
            sha256: None,
        };
        crate::api::write_provider_stream_frame(&mut client, &first, b"he").expect("first frame");
        let final_header = ProviderStreamChunkHeader {
            offset: 2,
            payload_len: 3,
            final_chunk: true,
            total_size: Some(5),
            sha256: Some(
                "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            ),
            ..first
        };
        crate::api::write_provider_stream_frame(&mut client, &final_header, b"llo")
            .expect("final frame");

        server.handle_stream(server_stream).expect("stream handled");
        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "upload_test"
        ));
    }

    #[test]
    fn dispatches_reservation_bound_multipart_part_to_frame_handler() {
        struct MultipartPartHandler;

        impl DaemonApiHandler for MultipartPartHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                panic!("multipart part test should use the stream handler")
            }

            fn handle_provider_stream_multipart_part_upload_for_actor(
                &self,
                request: crate::api::ProviderStreamMultipartPartUploadOpenRequest,
                _actor: Option<&crate::auth::DaemonLocalActor>,
                read_frame: &mut dyn FnMut() -> Result<
                    (ProviderStreamChunkHeader, Vec<u8>),
                    super::UnixSocketDaemonServerError,
                >,
                emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                assert_eq!(request.reservation_id, "reservation-1");
                assert_eq!(request.part_number, 2);
                let (header, payload) = read_frame()?;
                assert!(header.final_chunk);
                assert_eq!(payload, b"hello");
                emit_response(DaemonApiResponse::ProviderStreamMultipartPartUpload(
                    crate::api::ProviderStreamMultipartPartUploadResponse {
                        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                        request_id: request.request_id,
                        reservation_id: request.reservation_id,
                        part_number: request.part_number,
                        store_id: request.store_id,
                        object: request.object,
                        size_bytes: 5,
                        sha256: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string(),
                    },
                ))
            }
        }

        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new(
            "/tmp/dasobjectstored-provider-multipart-part-test.sock",
            MultipartPartHandler,
        );
        serde_json::to_writer(
            &mut client,
            &crate::api::ProviderStreamMultipartPartUploadOpenRequest {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: "multipart-frame-1".to_string(),
                reservation_id: "reservation-1".to_string(),
                reservation_size_bytes: 5,
                part_number: 2,
                store_id: StoreId::new("stream-store").expect("store id"),
                object: BackendObjectKey {
                    object_id: "objects/hello.txt".to_string(),
                    version: 1,
                },
                expected_size_bytes: 5,
                expected_sha256:
                    "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_string(),
                chunk_size_bytes: 1024,
            },
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");
        let final_header = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "multipart-frame-1".to_string(),
            offset: 0,
            payload_len: 5,
            final_chunk: true,
            total_size: Some(5),
            sha256: Some(
                "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            ),
        };
        crate::api::write_provider_stream_frame(&mut client, &final_header, b"hello")
            .expect("part frame");

        server.handle_stream(server_stream).expect("stream handled");
        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::ProviderStreamMultipartPartUpload(response)
                if response.reservation_id == "reservation-1" && response.part_number == 2
        ));
    }

    #[test]
    fn translates_invalid_provider_upload_frame_to_typed_bad_request() {
        struct InvalidFrameHandler;

        impl DaemonApiHandler for InvalidFrameHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                panic!("invalid provider frame test should use the upload handler")
            }

            fn handle_provider_stream_upload_for_actor(
                &self,
                _request: ProviderStreamUploadOpenRequest,
                _actor: Option<&crate::auth::DaemonLocalActor>,
                read_frame: &mut dyn FnMut() -> Result<
                    (ProviderStreamChunkHeader, Vec<u8>),
                    super::UnixSocketDaemonServerError,
                >,
                _emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                let _ = read_frame()?;
                Ok(())
            }
        }

        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new(
            "/tmp/dasobjectstored-invalid-provider-upload-test.sock",
            InvalidFrameHandler,
        );
        serde_json::to_writer(
            &mut client,
            &ProviderStreamUploadOpenRequest {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: "invalid-upload-stream".to_string(),
                upload_id: "capability-invalid".to_string(),
                store_id: StoreId::new("stream-store").expect("store id"),
                object: BackendObjectKey {
                    object_id: "objects/invalid.txt".to_string(),
                    version: 1,
                },
                expected_size_bytes: 0,
                expected_sha256:
                    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                        .to_string(),
                chunk_size_bytes: 1024,
            },
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");
        client
            .write_all(&[0, 0, 0, 0])
            .expect("invalid frame written");

        server.handle_stream(server_stream).expect("stream handled");

        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "bad_request"
        ));
    }

    #[test]
    fn returns_api_error_for_invalid_json() {
        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new("/tmp/dasobjectstored-test.sock", |_request| {
            panic!("bad request should not reach handler")
        });

        client.write_all(b"not-json\n").expect("request written");

        server.handle_stream(server_stream).expect("stream handled");

        let mut line = String::new();
        BufReader::new(client)
            .read_line(&mut line)
            .expect("response line");
        let response: DaemonApiResponse = serde_json::from_str(&line).expect("response decoded");
        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "bad_request"
        ));
    }

    #[test]
    fn streams_progress_before_final_response() {
        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");
        let server = UnixSocketDaemonServer::new("/tmp/dasobjectstored-test.sock", |_request| {
            panic!("streaming handler should be used")
        });

        struct StreamingHandler;

        impl super::DaemonApiHandler for StreamingHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                panic!("streaming handler should be used")
            }

            fn handle_api_request_streaming(
                &self,
                _request: DaemonApiRequest,
                emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                emit_response(DaemonApiResponse::IngestProgress(
                    DaemonIngestProgressEvent {
                        job_id: IngestJobId::new("ingest-files-1").expect("job id"),
                        endpoint: StoreId::new("zymo").expect("store id"),
                        stage: DaemonIngestStage::SsdIngest,
                        pipeline_stage: Some(DaemonIngestPipelineStage::SsdStage),
                        work_bytes_done: 50,
                        work_bytes_total: Some(100),
                        source_bytes_done: Some(50),
                        source_bytes_total: Some(100),
                        stage_bytes_done: Some(50),
                        stage_bytes_total: Some(100),
                        files_done: 0,
                        files_total: Some(1),
                        current_object_id: None,
                        ssd_pressure: None,
                        telemetry: None,
                        active_hdd_transfers: Vec::new(),
                        resource_policy: None,
                        message: Some("copying".to_string()),
                    },
                ))?;
                emit_response(DaemonApiResponse::SubmitIngestFiles(
                    SubmitIngestFilesResponse {
                        job_id: IngestJobId::new("ingest-files-1").expect("job id"),
                        accepted_at_utc: "2026-07-07T10:27:12Z".to_string(),
                        dry_run: false,
                    },
                ))
            }
        }

        let server = UnixSocketDaemonServer::new(server.socket_path(), StreamingHandler);
        serde_json::to_writer(
            &mut client,
            &DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                endpoint: StoreId::new("zymo").expect("store id"),
                source_path: "/tmp/source".into(),
                object_type: ObjectType::Naive,
                copies: None,
                hdd_workers: None,
                ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                dry_run: false,
                client_request_id: None,
            }),
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");

        server.handle_stream(server_stream).expect("stream handled");

        let mut reader = BufReader::new(client);
        let mut first = String::new();
        let mut second = String::new();
        reader.read_line(&mut first).expect("progress frame");
        reader.read_line(&mut second).expect("final frame");
        let progress: DaemonApiResponse = serde_json::from_str(&first).expect("progress decoded");
        let final_response: DaemonApiResponse =
            serde_json::from_str(&second).expect("final decoded");
        assert!(matches!(
            progress,
            DaemonApiResponse::IngestProgress(DaemonIngestProgressEvent {
                work_bytes_done: 50,
                ..
            })
        ));
        assert!(matches!(
            final_response,
            DaemonApiResponse::SubmitIngestFiles(SubmitIngestFilesResponse { .. })
        ));
    }

    #[test]
    fn treats_streaming_client_disconnect_as_handled_stream() {
        let (mut client, server_stream) = UnixStream::pair().expect("socket pair");

        struct DisconnectingHandler;

        impl super::DaemonApiHandler for DisconnectingHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                panic!("streaming handler should be used")
            }

            fn handle_api_request_streaming(
                &self,
                _request: DaemonApiRequest,
                emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                emit_response(DaemonApiResponse::IngestProgress(
                    DaemonIngestProgressEvent {
                        job_id: IngestJobId::new("ingest-files-1").expect("job id"),
                        endpoint: StoreId::new("zymo").expect("store id"),
                        stage: DaemonIngestStage::SsdIngest,
                        pipeline_stage: Some(DaemonIngestPipelineStage::SsdStage),
                        work_bytes_done: 50,
                        work_bytes_total: Some(100),
                        source_bytes_done: Some(50),
                        source_bytes_total: Some(100),
                        stage_bytes_done: Some(50),
                        stage_bytes_total: Some(100),
                        files_done: 0,
                        files_total: Some(1),
                        current_object_id: None,
                        ssd_pressure: None,
                        telemetry: None,
                        active_hdd_transfers: Vec::new(),
                        resource_policy: None,
                        message: Some("copying".to_string()),
                    },
                ))
            }
        }

        let server =
            UnixSocketDaemonServer::new("/tmp/dasobjectstored-test.sock", DisconnectingHandler);
        serde_json::to_writer(
            &mut client,
            &DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                endpoint: StoreId::new("zymo").expect("store id"),
                source_path: "/tmp/source".into(),
                object_type: ObjectType::Naive,
                copies: None,
                hdd_workers: None,
                ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                dry_run: false,
                client_request_id: None,
            }),
        )
        .expect("request encoded");
        client.write_all(b"\n").expect("request newline");
        drop(client);

        server
            .handle_stream(server_stream)
            .expect("client disconnect does not fail the server stream");
    }

    #[test]
    fn serves_control_requests_while_an_ingest_stream_is_active() {
        struct BlockingHandler {
            entered: mpsc::Sender<()>,
            release: Mutex<mpsc::Receiver<()>>,
        }

        impl super::DaemonApiHandler for BlockingHandler {
            fn handle_api_request(
                &self,
                request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, super::UnixSocketDaemonServerError> {
                match request {
                    DaemonApiRequest::ServiceStatus(_) => Ok(DaemonApiResponse::ServiceStatus(
                        DaemonServiceStatusResponse {
                            provider_id: ObjectServiceProviderId::Garage,
                            state: ServiceState::Running,
                            endpoint: None,
                            message: None,
                            detail: None,
                        },
                    )),
                    _ => panic!("unexpected control request"),
                }
            }

            fn handle_api_request_streaming(
                &self,
                request: DaemonApiRequest,
                emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), super::UnixSocketDaemonServerError>,
            ) -> Result<(), super::UnixSocketDaemonServerError> {
                match request {
                    DaemonApiRequest::SubmitIngestFiles(_) => {
                        self.entered.send(()).expect("ingest entered signal");
                        self.release
                            .lock()
                            .expect("release lock")
                            .recv()
                            .expect("ingest release signal");
                        emit_response(DaemonApiResponse::SubmitIngestFiles(
                            SubmitIngestFilesResponse {
                                job_id: IngestJobId::new("ingest-files-1").expect("job id"),
                                accepted_at_utc: "2026-07-10T10:00:00Z".to_string(),
                                dry_run: false,
                            },
                        ))
                    }
                    request => self.handle_api_request(request).and_then(emit_response),
                }
            }
        }

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let socket_path =
            std::env::temp_dir().join(format!("dasobjectstored-control-{suffix}.sock"));
        let (entered_sender, entered_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let server = UnixSocketDaemonServer::new(
            &socket_path,
            BlockingHandler {
                entered: entered_sender,
                release: Mutex::new(release_receiver),
            },
        );
        thread::spawn(move || server.serve_forever().expect("server runs"));
        for _ in 0..20 {
            if socket_path.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        let connect_with_retry = || {
            let mut last_error = None;
            for _ in 0..20 {
                match UnixStream::connect(&socket_path) {
                    Ok(stream) => return Ok(stream),
                    Err(error) => {
                        last_error = Some(error);
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            Err(last_error.expect("at least one connection attempt"))
        };
        let mut ingest = connect_with_retry().expect("ingest connects");
        serde_json::to_writer(
            &mut ingest,
            &DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                endpoint: StoreId::new("zymo").expect("store id"),
                source_path: "/tmp/source".into(),
                object_type: ObjectType::Naive,
                copies: None,
                hdd_workers: None,
                ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                conflict_policy: crate::api::DaemonIngestConflictPolicy::Force,
                dry_run: false,
                client_request_id: None,
            }),
        )
        .expect("ingest request encoded");
        ingest.write_all(b"\n").expect("ingest request newline");
        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("ingest handler entered");

        let mut control = connect_with_retry().expect("control connects");
        control
            .set_read_timeout(Some(Duration::from_millis(250)))
            .expect("control timeout set");
        serde_json::to_writer(
            &mut control,
            &DaemonApiRequest::ServiceStatus(Default::default()),
        )
        .expect("control request encoded");
        control.write_all(b"\n").expect("control request newline");
        let mut line = String::new();
        BufReader::new(control)
            .read_line(&mut line)
            .expect("control response remains responsive");
        assert!(matches!(
            serde_json::from_str::<DaemonApiResponse>(&line).expect("control response decoded"),
            DaemonApiResponse::ServiceStatus(DaemonServiceStatusResponse {
                state: ServiceState::Running,
                ..
            })
        ));

        release_sender.send(()).expect("release ingest");
    }
}
