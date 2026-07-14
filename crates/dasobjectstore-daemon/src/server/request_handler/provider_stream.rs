use super::*;
use crate::api::ProviderStreamOpenRequest;
use dasobjectstore_core::backend::ObjectStoreBackend;
use std::io::Read;

pub(crate) struct ProviderStreamSource {
    pub reader: Box<dyn Read + Send>,
    pub expected_size_bytes: u64,
    pub expected_checksum: Option<String>,
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    /// Open a catalogue-authoritative profile object for the Unix-socket
    /// provider stream. The returned reader never exposes a backend path; the
    /// transport owns chunking and cumulative verification.
    pub(crate) fn open_provider_stream(
        &self,
        request: &ProviderStreamOpenRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<ProviderStreamSource, DaemonApiResponse> {
        let store_id = match self.authorize_endpoint_read(actor, &request.store_id) {
            Ok(store_id) => store_id,
            Err(error) => {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )))
            }
        };
        let binding =
            match read_profile_binding(&self.profile_binding_registry_path, store_id.as_str()) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        "provider stream requires a registered bounded folder profile",
                    )))
                }
            };
        if binding.manifest.deployment_profile != DeploymentProfile::Folder {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "provider stream is available for bounded folder profiles only",
            )));
        }
        let capacity = match read_store_registry(&self.store_registry_path) {
            Ok(definitions) => definitions
                .into_iter()
                .find(|definition| definition.store_id == store_id)
                .map(|definition| definition.policy.capacity),
            Err(_) => None,
        };
        let Some(capacity) = capacity else {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "profile capacity policy is unavailable",
            )));
        };
        let backend = match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0)
        {
            Ok(backend) => backend,
            Err(error) => {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_unavailable",
                    error.to_string(),
                )))
            }
        };
        let object = match head_profile_object(&backend, &request.object) {
            Ok(object) => object,
            Err(error) => {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_head_failed",
                    error.to_string(),
                )))
            }
        };
        if request
            .condition
            .if_match_sha256
            .as_deref()
            .is_some_and(|checksum| !checksum.eq_ignore_ascii_case(&object.checksum))
        {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_precondition_failed",
                "if_match_sha256 does not match the catalogue checksum",
            )));
        }
        if request
            .condition
            .if_none_match_sha256
            .as_deref()
            .is_some_and(|checksum| checksum.eq_ignore_ascii_case(&object.checksum))
        {
            return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_not_modified",
                "if_none_match_sha256 matches the catalogue checksum",
            )));
        }
        let (reader, expected_size_bytes, expected_checksum) = if let Some(range) = request.range {
            if range.start > object.size_bytes {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_invalid_range",
                    "provider stream range starts beyond the catalogue object",
                )));
            }
            let end = range
                .end_exclusive
                .unwrap_or(object.size_bytes)
                .min(object.size_bytes);
            if end < range.start {
                return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_invalid_range",
                    "provider stream range ends before it starts",
                )));
            }
            let length = end - range.start;
            (
                backend
                    .read_range(&request.object, range.start, length)
                    .map_err(|error| {
                        DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "provider_stream_read_failed",
                            error.to_string(),
                        ))
                    })?,
                length,
                None,
            )
        } else {
            (
                backend.read(&request.object).map_err(|error| {
                    DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_read_failed",
                        error.to_string(),
                    ))
                })?,
                object.size_bytes,
                Some(object.checksum),
            )
        };
        Ok(ProviderStreamSource {
            reader,
            expected_size_bytes,
            expected_checksum,
        })
    }
}
