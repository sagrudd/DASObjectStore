use super::{unexpected, DaemonClient, DaemonClientError, DaemonClientTransport};
use crate::api::{
    DaemonApiRequest, DaemonApiResponse, WorkspaceControlRequest, WorkspaceControlResponse,
};

impl<T> DaemonClient<T>
where
    T: DaemonClientTransport,
{
    pub fn workspace_control(
        &self,
        request: WorkspaceControlRequest,
    ) -> Result<WorkspaceControlResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::WorkspaceControl(request))? {
            DaemonApiResponse::WorkspaceControl(response) => Ok(response),
            response => Err(unexpected("workspace_control", response)),
        }
    }
}
