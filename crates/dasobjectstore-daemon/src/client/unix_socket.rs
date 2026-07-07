use super::{DaemonClientError, DaemonClientTransport};
use crate::api::{DaemonApiRequest, DaemonApiResponse};
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
    fn send(&self, _request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError> {
        Err(DaemonClientError::UnixSocketTransportPlanned {
            socket_path: self.socket_path.clone(),
        })
    }
}
