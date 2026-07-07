use super::{DaemonClientError, DaemonClientTransport};
use crate::api::{DaemonApiRequest, DaemonApiResponse, DaemonIngestProgressEvent};
use crate::runtime::DEFAULT_DAEMON_GROUP;
use std::io::{BufRead, BufReader, Write};
use std::io::{Error as IoError, ErrorKind};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnixSocketDaemonTransport {
    socket_path: PathBuf,
}

impl UnixSocketDaemonTransport {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl DaemonClientTransport for UnixSocketDaemonTransport {
    fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError> {
        self.send_with_progress(request, &mut |_| {})
    }

    fn send_with_progress(
        &self,
        request: DaemonApiRequest,
        progress: &mut dyn FnMut(DaemonIngestProgressEvent),
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|error| {
            DaemonClientError::Transport(connect_error_message(&self.socket_path, &error))
        })?;
        serde_json::to_writer(&mut stream, &request)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .write_all(b"\n")
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
        stream
            .flush()
            .map_err(|error| DaemonClientError::Transport(error.to_string()))?;

        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            let bytes_read = reader
                .read_line(&mut line)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            if bytes_read == 0 {
                return Err(DaemonClientError::Transport(
                    "daemon closed the connection without a final response".to_string(),
                ));
            }
            let response: DaemonApiResponse = serde_json::from_str(&line)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))?;
            if let DaemonApiResponse::IngestProgress(event) = response {
                progress(event);
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
    use super::{connect_error_message, UnixSocketDaemonTransport};
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, DaemonServiceStatusRequest,
        DaemonServiceStatusResponse,
    };
    use crate::client::{DaemonClient, DaemonClientError};
    use crate::server::UnixSocketDaemonServer;
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::fs;
    use std::io::{Error as IoError, ErrorKind};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn unix_socket_transport_explains_permission_denied() {
        let message = connect_error_message(
            PathBuf::from("/run/dasobjectstore/dasobjectstored.sock").as_path(),
            &IoError::from(ErrorKind::PermissionDenied),
        );

        assert!(message.contains("restricted to members of the `dasobjectstore` group"));
        assert!(message.contains("sudo usermod -aG dasobjectstore \"$USER\""));
        assert!(message.contains("start a new login session"));
    }

    fn unique_socket_path() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-{}-{now}.sock", std::process::id()))
    }
}
