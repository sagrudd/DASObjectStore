use super::*;

impl<T: DaemonClientTransport> DaemonClient<T> {
    pub fn remote_easyconnect_pairing_status(
        &self,
        request: RemoteEasyconnectPairingStatusRequest,
    ) -> Result<RemoteEasyconnectPairingStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectPairingStatus(request))? {
            DaemonApiResponse::RemoteEasyconnectPairingStatus(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_pairing_status", response)),
        }
    }
}
