use super::{DaemonClientError, DaemonClientTransport};
use crate::api::{DaemonApiRequest, DaemonApiResponse};

pub struct InProcessDaemonTransport<H> {
    handler: H,
}

impl<H> InProcessDaemonTransport<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }
}

impl<H> DaemonClientTransport for InProcessDaemonTransport<H>
where
    H: Fn(DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError>,
{
    fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError> {
        (self.handler)(request)
    }
}
