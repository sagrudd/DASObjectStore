use super::{unexpected, DaemonClientError, DaemonClientTransport};
use crate::api::{
    read_provider_stream_frame, DaemonApiRequest, DaemonApiResponse, DaemonIngestProgressEvent,
    ProviderStreamChunkHeader, ProviderStreamOpenRequest, ProviderStreamUploadOpenRequest,
    ProviderStreamVerifier,
};
use crate::runtime::DEFAULT_DAEMON_GROUP;
use serde::Serialize;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::io::{Error as IoError, ErrorKind};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnixSocketDaemonTransport {
    socket_path: PathBuf,
    idle_timeout: Option<Duration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStreamReceipt {
    pub total_size: u64,
    pub sha256: String,
}

impl UnixSocketDaemonTransport {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            idle_timeout: None,
        }
    }

    /// Construct a transport for bounded GUI calls. Progress frames reset the
    /// deadline, while a stalled daemon response eventually releases the
    /// bridge's blocking worker and semaphore permit.
    pub fn for_bounded_bridge(socket_path: impl Into<PathBuf>) -> Self {
        Self::new(socket_path).with_idle_timeout(Duration::from_millis(1_500))
    }

    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Consume a path-free provider stream from the daemon. The first server
    /// response is either a newline-delimited JSON error or the fixed binary
    /// frame magic; payload bytes are never buffered beyond one bounded frame
    /// and are delivered only after cumulative offset/request/checksum checks.
    pub fn stream_provider(
        &self,
        request: ProviderStreamOpenRequest,
        mut emit_frame: impl FnMut(&ProviderStreamChunkHeader, &[u8]) -> Result<(), DaemonClientError>,
    ) -> Result<ProviderStreamReceipt, DaemonClientError> {
        request
            .validate()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            DaemonClientError::Transport(connect_error_message(&self.socket_path, &error))
        })?;
        if let Some(timeout) = self.idle_timeout {
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        }
        serde_json::to_writer(&mut stream, &request)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .write_all(b"\n")
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .flush()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;

        let mut reader = BufReader::new(stream);
        let mut prefix = [0_u8; 4];
        reader
            .read_exact(&mut prefix)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        if &prefix != b"DPS1" {
            let mut line = String::from_utf8_lossy(&prefix).into_owned();
            reader
                .read_line(&mut line)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            let response: DaemonApiResponse = serde_json::from_str(&line)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            return match response {
                DaemonApiResponse::Error(error) => Err(DaemonClientError::Api(error)),
                response => Err(unexpected("provider_stream", response)),
            };
        }

        let mut framed_reader = PrefixReader::new(prefix, reader);
        let mut verifier = ProviderStreamVerifier::new(request.request_id)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        loop {
            let (header, payload) = read_provider_stream_frame(&mut framed_reader)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            if header.final_chunk {
                let total_size = verifier
                    .finish(&header, &payload)
                    .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
                let sha256 = header.sha256.clone().ok_or_else(|| {
                    DaemonClientError::Transport(
                        "provider stream final frame omitted its checksum".to_string(),
                    )
                })?;
                emit_frame(&header, &payload)?;
                return Ok(ProviderStreamReceipt { total_size, sha256 });
            }
            verifier
                .push(&header, &payload)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            emit_frame(&header, &payload)?;
        }
    }

    /// Send a bounded client-to-daemon provider upload. The caller supplies
    /// one frame at a time; this method never buffers more than the current
    /// frame and verifies request identity, contiguous offsets, final size,
    /// and checksum before writing each frame. The daemon response is emitted
    /// only after the caller supplies a terminal frame.
    pub fn upload_provider(
        &self,
        request: ProviderStreamUploadOpenRequest,
        mut next_frame: impl FnMut() -> Result<
            Option<(ProviderStreamChunkHeader, Vec<u8>)>,
            DaemonClientError,
        >,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        request
            .validate()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        self.upload_provider_frames(
            &request,
            &request.request_id,
            request.expected_size_bytes,
            &request.expected_sha256,
            &mut next_frame,
        )
    }

    /// Send one reservation-bound multipart part through the same bounded
    /// binary framing and verifier as a complete-object upload. The JSON
    /// envelope differs so retries are addressed by reservation and part
    /// number rather than an upload capability.
    pub fn upload_multipart_part(
        &self,
        request: crate::api::ProviderStreamMultipartPartUploadOpenRequest,
        mut next_frame: impl FnMut() -> Result<
            Option<(ProviderStreamChunkHeader, Vec<u8>)>,
            DaemonClientError,
        >,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        request
            .validate()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        self.upload_provider_frames(
            &request,
            &request.request_id,
            request.expected_size_bytes,
            &request.expected_sha256,
            &mut next_frame,
        )
    }

    fn upload_provider_frames(
        &self,
        request: &impl Serialize,
        request_id: &str,
        expected_size_bytes: u64,
        expected_sha256: &str,
        next_frame: &mut dyn FnMut() -> Result<
            Option<(ProviderStreamChunkHeader, Vec<u8>)>,
            DaemonClientError,
        >,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            DaemonClientError::Transport(connect_error_message(&self.socket_path, &error))
        })?;
        if let Some(timeout) = self.idle_timeout {
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        }
        serde_json::to_writer(&mut stream, &request)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .write_all(b"\n")
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .flush()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;

        let mut verifier = ProviderStreamVerifier::new(request_id.to_string())
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        loop {
            let Some((header, payload)) = next_frame()? else {
                return Err(DaemonClientError::Transport(
                    "provider stream upload omitted its terminal frame".to_string(),
                ));
            };
            if header.final_chunk {
                let total_size = verifier
                    .finish(&header, &payload)
                    .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
                let checksum = header.sha256.as_deref().ok_or_else(|| {
                    DaemonClientError::Transport(
                        "provider stream upload final frame omitted its checksum".to_string(),
                    )
                })?;
                if total_size != expected_size_bytes
                    || !checksum.eq_ignore_ascii_case(expected_sha256)
                {
                    return Err(DaemonClientError::Transport(
                        "provider stream upload differs from its declared size or checksum"
                            .to_string(),
                    ));
                }
                crate::api::write_provider_stream_frame(&mut stream, &header, &payload)
                    .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
                break;
            } else {
                verifier
                    .push(&header, &payload)
                    .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
                crate::api::write_provider_stream_frame(&mut stream, &header, &payload)
                    .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            }
        }

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        if line.is_empty() {
            return Err(DaemonClientError::Transport(
                "daemon closed the upload connection without a final response".to_string(),
            ));
        }
        serde_json::from_str(&line).map_err(|error| DaemonClientError::Transport(error.to_string()))
    }
}

struct PrefixReader<R> {
    prefix: Cursor<[u8; 4]>,
    reader: R,
}

impl<R> PrefixReader<R> {
    fn new(prefix: [u8; 4], reader: R) -> Self {
        Self {
            prefix: Cursor::new(prefix),
            reader,
        }
    }
}

impl<R: Read> Read for PrefixReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.prefix.read(buffer)?;
        if read != 0 {
            return Ok(read);
        }
        self.reader.read(buffer)
    }
}

impl DaemonClientTransport for UnixSocketDaemonTransport {
    fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError> {
        self.send_with_progress(request, &mut |_| Ok(()))
    }

    fn send_with_progress(
        &self,
        request: DaemonApiRequest,
        progress: &mut dyn FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonClientError>,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        self.send_with_progress_and_heartbeat(request, progress, &mut || Ok(()))
    }

    fn send_with_progress_and_heartbeat(
        &self,
        request: DaemonApiRequest,
        progress: &mut dyn FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonClientError>,
        heartbeat: &mut dyn FnMut() -> Result<(), DaemonClientError>,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            DaemonClientError::Transport(connect_error_message(&self.socket_path, &error))
        })?;
        stream
            .set_read_timeout(Some(
                self.idle_timeout
                    .unwrap_or(Duration::from_secs(1))
                    .min(Duration::from_secs(1)),
            ))
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        serde_json::to_writer(&mut stream, &request)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .write_all(b"\n")
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .flush()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;

        let mut reader = BufReader::new(stream);
        let mut idle_deadline = self.idle_timeout.map(|timeout| {
            Instant::now()
                .checked_add(timeout)
                .unwrap_or_else(Instant::now)
        });
        loop {
            let mut line = String::new();
            let bytes_read = match reader.read_line(&mut line) {
                Ok(bytes_read) => bytes_read,
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) =>
                {
                    heartbeat()?;
                    if idle_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Err(DaemonClientError::Transport(
                            "daemon response exceeded bounded idle deadline".to_string(),
                        ));
                    }
                    continue;
                }
                Err(error) => return Err(DaemonClientError::Transport(error.to_string())),
            };
            if bytes_read == 0 {
                return Err(DaemonClientError::Transport(
                    "daemon closed the connection without a final response".to_string(),
                ));
            }
            if let Some(timeout) = self.idle_timeout {
                idle_deadline = Some(
                    Instant::now()
                        .checked_add(timeout)
                        .unwrap_or_else(Instant::now),
                );
            }
            let response: DaemonApiResponse = serde_json::from_str(&line)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            if let DaemonApiResponse::IngestProgress(event) = response {
                progress(event)?;
                continue;
            }
            return Ok(response);
        }
    }
}

fn connect_error_message(socket_path: &Path, error: &IoError) -> String {
    if error.kind() == ErrorKind::PermissionDenied {
        return format!(
            "failed to connect to {}: {error}. The packaged daemon socket is restricted to members of the `{}` group. Ask an administrator to run `sudo usermod -aG {} \"$USER\"`, then start a new login session and verify membership with `id -nG`.",
            socket_path.display(),
            DEFAULT_DAEMON_GROUP,
            DEFAULT_DAEMON_GROUP
        );
    }

    format!("failed to connect to {}: {error}", socket_path.display())
}

#[cfg(test)]
mod tests {
    use super::{connect_error_message, ProviderStreamReceipt, UnixSocketDaemonTransport};
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, DaemonServiceStatusRequest,
        DaemonServiceStatusResponse, ProviderStreamChunkHeader, ProviderStreamOpenRequest,
        ProviderStreamUploadOpenRequest, PROVIDER_STREAM_SCHEMA_VERSION,
    };
    use crate::client::{DaemonClient, DaemonClientError};
    use crate::server::{DaemonApiHandler, UnixSocketDaemonServer, UnixSocketDaemonServerError};
    use dasobjectstore_core::backend::BackendObjectKey;
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::fs;
    use std::io::{Error as IoError, ErrorKind};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn unix_socket_transport_round_trips_request() {
        let socket_path = unique_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("listener binds");
        let server = UnixSocketDaemonServer::new(&socket_path, |request| {
            assert!(matches!(request, DaemonApiRequest::ServiceStatus(_)));
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
        let join = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client connects");
            server.handle_stream(stream).expect("request handled");
        });

        let client = DaemonClient::new(UnixSocketDaemonTransport::new(&socket_path));
        let response = client
            .service_status(DaemonServiceStatusRequest {
                include_detail: false,
            })
            .expect("status response");

        assert_eq!(response.state, ServiceState::Running);
        join.join().expect("server thread joins");
        let _ = fs::remove_file(socket_path);
    }

    #[test]
    fn unix_socket_transport_reports_connect_failure() {
        let client = DaemonClient::new(UnixSocketDaemonTransport::new(unique_socket_path()));

        let error = client
            .service_status(DaemonServiceStatusRequest::default())
            .expect_err("missing socket rejected");

        assert!(matches!(error, DaemonClientError::Transport(_)));
    }

    #[test]
    fn unix_socket_transport_consumes_verified_provider_frames() {
        let socket_path = unique_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("listener binds");
        let server = UnixSocketDaemonServer::new(&socket_path, ProviderStreamHandler);
        let join = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client connects");
            server
                .handle_stream(stream)
                .expect("provider stream handled");
        });
        let request = ProviderStreamOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "client-stream-1".to_string(),
            store_id: StoreId::new("stream-store").expect("store id"),
            object: BackendObjectKey {
                object_id: "objects/hello.txt".to_string(),
                version: 1,
            },
            range: None,
            condition: Default::default(),
            chunk_size_bytes: 1024,
        };
        let client = UnixSocketDaemonTransport::new(&socket_path);
        let mut bytes = Vec::new();
        let receipt: ProviderStreamReceipt = client
            .stream_provider(request, |_, payload| {
                bytes.extend_from_slice(payload);
                Ok(())
            })
            .expect("provider stream receipt");
        assert_eq!(bytes, b"hello");
        assert_eq!(receipt.total_size, 5);
        assert_eq!(
            receipt.sha256,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        join.join().expect("server thread joins");
        let _ = fs::remove_file(socket_path);
    }

    #[test]
    fn unix_socket_transport_uploads_bounded_frames_and_reads_terminal_response() {
        struct ProviderUploadHandler;

        impl DaemonApiHandler for ProviderUploadHandler {
            fn handle_api_request(
                &self,
                _request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, UnixSocketDaemonServerError> {
                panic!("provider upload test should use the upload handler")
            }

            fn handle_provider_stream_upload_for_actor(
                &self,
                request: ProviderStreamUploadOpenRequest,
                _actor: Option<&crate::auth::DaemonLocalActor>,
                read_frame: &mut dyn FnMut() -> Result<
                    (ProviderStreamChunkHeader, Vec<u8>),
                    UnixSocketDaemonServerError,
                >,
                emit_response: &mut dyn FnMut(
                    DaemonApiResponse,
                )
                    -> Result<(), UnixSocketDaemonServerError>,
            ) -> Result<(), UnixSocketDaemonServerError> {
                assert_eq!(request.request_id, "upload-client-1");
                let (_, payload) = read_frame()?;
                assert_eq!(payload, b"hello");
                emit_response(DaemonApiResponse::Error(
                    crate::api::DaemonApiErrorResponse::new("upload_test", "accepted"),
                ))
            }
        }

        let socket_path = unique_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("listener binds");
        let server = UnixSocketDaemonServer::new(&socket_path, ProviderUploadHandler);
        let join = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client connects");
            server.handle_stream(stream).expect("upload handled");
        });
        let request = ProviderStreamUploadOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "upload-client-1".to_string(),
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
        };
        let client = UnixSocketDaemonTransport::new(&socket_path);
        let mut next = Some((
            ProviderStreamChunkHeader {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: "upload-client-1".to_string(),
                offset: 0,
                payload_len: 5,
                final_chunk: true,
                total_size: Some(5),
                sha256: Some(
                    "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_string(),
                ),
            },
            b"hello".to_vec(),
        ));
        let response = client
            .upload_provider(request, || Ok(next.take()))
            .expect("upload response");
        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "upload_test"
        ));
        join.join().expect("server thread joins");
        let _ = fs::remove_file(socket_path);
    }

    #[test]
    fn unix_socket_transport_explains_permission_denied() {
        let message = connect_error_message(
            PathBuf::from("/run/dasobjectstore/dasobjectstored.sock").as_path(),
            &IoError::from(ErrorKind::PermissionDenied),
        );

        assert!(message.contains("restricted to members of the `dasobjectstore` group"));
        assert!(message.contains("sudo usermod -aG dasobjectstore \"$USER\""));
        assert!(message.contains("start a new login session"));
    }

    #[test]
    fn bounded_bridge_transport_aborts_stalled_response() {
        let socket_path = unique_socket_path();
        let listener = UnixListener::bind(&socket_path).expect("listener binds");
        let join = thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("client connects");
            thread::sleep(Duration::from_millis(200));
        });

        let client = DaemonClient::new(
            UnixSocketDaemonTransport::for_bounded_bridge(&socket_path)
                .with_idle_timeout(Duration::from_millis(50)),
        );
        let error = client
            .service_status(DaemonServiceStatusRequest::default())
            .expect_err("stalled response rejected");

        assert!(matches!(
            error,
            DaemonClientError::Transport(message)
                if message.contains("bounded idle deadline")
        ));
        join.join().expect("server thread joins");
        let _ = fs::remove_file(socket_path);
    }

    fn unique_socket_path() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-{}-{now}.sock", std::process::id()))
    }

    struct ProviderStreamHandler;

    impl DaemonApiHandler for ProviderStreamHandler {
        fn handle_api_request(
            &self,
            _request: DaemonApiRequest,
        ) -> Result<DaemonApiResponse, UnixSocketDaemonServerError> {
            Ok(DaemonApiResponse::Error(
                crate::api::DaemonApiErrorResponse::new(
                    "not_implemented",
                    "provider stream test handler only",
                ),
            ))
        }

        fn handle_provider_stream_open_for_actor(
            &self,
            request: ProviderStreamOpenRequest,
            _actor: Option<&crate::auth::DaemonLocalActor>,
            _emit_response: &mut dyn FnMut(
                DaemonApiResponse,
            ) -> Result<(), UnixSocketDaemonServerError>,
            emit_frame: &mut dyn FnMut(
                &ProviderStreamChunkHeader,
                &[u8],
            ) -> Result<(), UnixSocketDaemonServerError>,
        ) -> Result<(), UnixSocketDaemonServerError> {
            let payload = b"hello";
            let header = ProviderStreamChunkHeader {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request.request_id.clone(),
                offset: 0,
                payload_len: payload.len() as u32,
                final_chunk: false,
                total_size: None,
                sha256: None,
            };
            emit_frame(&header, payload)?;
            let final_header = ProviderStreamChunkHeader {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request.request_id,
                offset: payload.len() as u64,
                payload_len: 0,
                final_chunk: true,
                total_size: Some(payload.len() as u64),
                sha256: Some(
                    "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_string(),
                ),
            };
            emit_frame(&final_header, &[])
        }
    }
}
