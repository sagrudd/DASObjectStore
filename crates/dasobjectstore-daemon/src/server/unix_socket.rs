use crate::api::{DaemonApiErrorResponse, DaemonApiRequest, DaemonApiResponse};
use crate::server::{DaemonRequestHandler, DaemonRequestHandlerError, DaemonServiceOrchestrator};
use crate::DaemonClock;
use std::fmt::{self, Display};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const SOCKET_MODE: u32 = 0o660;

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

    pub fn serve_forever(&self) -> Result<(), UnixSocketDaemonServerError> {
        let listener = bind_listener(&self.socket_path)?;
        for stream in listener.incoming() {
            let stream = stream.map_err(UnixSocketDaemonServerError::Accept)?;
            self.handle_stream(stream)?;
        }
        Ok(())
    }

    pub fn handle_stream(&self, stream: UnixStream) -> Result<(), UnixSocketDaemonServerError> {
        handle_stream(stream, &self.handler)
    }
}

pub trait DaemonApiHandler {
    fn handle_api_request(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, UnixSocketDaemonServerError>;
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
        }
    }
}

impl std::error::Error for UnixSocketDaemonServerError {}

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
    mut stream: UnixStream,
    handler: &impl DaemonApiHandler,
) -> Result<(), UnixSocketDaemonServerError> {
    let mut line = String::new();
    BufReader::new(
        stream
            .try_clone()
            .map_err(UnixSocketDaemonServerError::Io)?,
    )
    .read_line(&mut line)
    .map_err(UnixSocketDaemonServerError::Io)?;

    let response = match serde_json::from_str::<DaemonApiRequest>(&line) {
        Ok(request) => handler.handle_api_request(request)?,
        Err(error) => DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "bad_request",
            format!("failed to decode daemon request: {error}"),
        )),
    };
    serde_json::to_writer(&mut stream, &response).map_err(UnixSocketDaemonServerError::Encode)?;
    stream
        .write_all(b"\n")
        .map_err(UnixSocketDaemonServerError::Io)?;
    stream.flush().map_err(UnixSocketDaemonServerError::Io)
}

#[cfg(test)]
mod tests {
    use super::UnixSocketDaemonServer;
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, DaemonServiceStatusResponse, StoreInventoryRequest,
    };
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

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
}
