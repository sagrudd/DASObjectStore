use super::{unexpected, DaemonClient, DaemonClientError, DaemonClientTransport};
use crate::api::{
    DaemonApiRequest, DaemonApiResponse, ProfileS3DeleteRequest, ProfileS3DeleteResponse,
    ProfileS3HeadRequest, ProfileS3HeadResponse, ProfileS3HealthRequest, ProfileS3HealthResponse,
    ProfileS3ListRequest, ProfileS3ListResponse, ProfileS3MultipartAbortRequest,
    ProfileS3MultipartAbortResponse, ProfileS3MultipartCompletionRequest,
    ProfileS3MultipartCompletionResponse, ProfileS3MultipartUploadsRequest,
    ProfileS3MultipartUploadsResponse, ProfileS3VerifyRequest, ProfileS3VerifyResponse,
};

impl<T> DaemonClient<T>
where
    T: DaemonClientTransport,
{
    pub fn profile_s3_list(
        &self,
        request: ProfileS3ListRequest,
    ) -> Result<ProfileS3ListResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3List(request))? {
            DaemonApiResponse::ProfileS3List(response) => Ok(response),
            response => Err(unexpected("profile_s3_list", response)),
        }
    }

    pub fn profile_s3_head(
        &self,
        request: ProfileS3VerifyRequest,
    ) -> Result<ProfileS3HeadResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3Head(request))? {
            DaemonApiResponse::ProfileS3Head(response) => Ok(response),
            response => Err(unexpected("profile_s3_head", response)),
        }
    }

    pub fn profile_s3_delete(
        &self,
        request: ProfileS3DeleteRequest,
    ) -> Result<ProfileS3DeleteResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3Delete(request))? {
            DaemonApiResponse::ProfileS3Delete(response) => Ok(response),
            response => Err(unexpected("profile_s3_delete", response)),
        }
    }

    pub fn profile_s3_verify(
        &self,
        request: ProfileS3HeadRequest,
    ) -> Result<ProfileS3VerifyResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3Verify(request))? {
            DaemonApiResponse::ProfileS3Verify(response) => Ok(response),
            response => Err(unexpected("profile_s3_verify", response)),
        }
    }

    pub fn profile_s3_health(
        &self,
        request: ProfileS3HealthRequest,
    ) -> Result<ProfileS3HealthResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3Health(request))? {
            DaemonApiResponse::ProfileS3Health(response) => Ok(response),
            response => Err(unexpected("profile_s3_health", response)),
        }
    }

    pub fn profile_s3_multipart_complete(
        &self,
        request: ProfileS3MultipartCompletionRequest,
    ) -> Result<ProfileS3MultipartCompletionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3MultipartComplete(request))? {
            DaemonApiResponse::ProfileS3MultipartComplete(response) => Ok(response),
            response => Err(unexpected("profile_s3_multipart_complete", response)),
        }
    }

    pub fn profile_s3_multipart_uploads(
        &self,
        request: ProfileS3MultipartUploadsRequest,
    ) -> Result<ProfileS3MultipartUploadsResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3MultipartUploads(request))? {
            DaemonApiResponse::ProfileS3MultipartUploads(response) => Ok(response),
            response => Err(unexpected("profile_s3_multipart_uploads", response)),
        }
    }

    pub fn profile_s3_multipart_abort(
        &self,
        request: ProfileS3MultipartAbortRequest,
    ) -> Result<ProfileS3MultipartAbortResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileS3MultipartAbort(request))? {
            DaemonApiResponse::ProfileS3MultipartAbort(response) => Ok(response),
            response => Err(unexpected("profile_s3_multipart_abort", response)),
        }
    }
}
