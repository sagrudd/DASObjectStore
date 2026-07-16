use super::*;
use crate::api::{
    DaemonApiErrorResponse, DaemonApiResponse, ProviderStreamChunkHeader,
    ProviderStreamMultipartPartUploadOpenRequest, ProviderStreamMultipartPartUploadResponse,
    ProviderStreamOpenRequest, ProviderStreamUploadOpenRequest, ProviderStreamUploadResponse,
    ProviderStreamVerifier,
};
use crate::server::unix_socket::UnixSocketDaemonServerError;
use dasobjectstore_core::backend::ObjectStoreBackend;
use std::io::{self, Read};

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
    pub(super) fn publish_profile_s3_catalogue(
        &self,
        store_id: &StoreId,
        backend: &FolderBackend,
    ) -> Result<(), dasobjectstore_core::backend::BackendError> {
        let profile_namespace = format!("profile-s3:{}", store_id.as_str());
        crate::runtime::publish_profile_catalogue_with_metadata(
            store_id,
            backend,
            &self.live_sqlite_path,
            backend
                .root()
                .join(".dasobjectstore/profile-catalogue-handoffs"),
            &profile_namespace,
            &self.clock.now_utc(),
        )
        .map(|_| ())
    }

    pub(crate) fn handle_provider_stream_multipart_part_upload_for_actor(
        &self,
        request: ProviderStreamMultipartPartUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let store_id = match self.authorize_endpoint_write(actor, &request.store_id) {
            Ok(store_id) => store_id,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )))
            }
        };
        let binding =
            match read_profile_binding(&self.profile_binding_registry_path, store_id.as_str()) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        "multipart part staging requires a registered bounded folder profile",
                    )))
                }
            };
        if binding.manifest.deployment_profile != DeploymentProfile::Folder {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "multipart part staging is available for bounded folder profiles only",
            )));
        }
        let mut journal =
            match crate::runtime::MultipartPartJournal::open(binding.backend_root, &request) {
                Ok(journal) => journal,
                Err(error) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_multipart_failed",
                        error.to_string(),
                    )))
                }
            };
        let admitted = journal.staged_bytes() != 0;
        if !admitted {
            let Some(provider) = self.service_orchestrator.capacity_provider() else {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_unavailable",
                    "multipart part staging requires daemon capacity admission",
                )));
            };
            let admission = provider.admit_remote_upload(
                store_id.as_str(),
                request.reservation_size_bytes,
                &request.reservation_id,
            );
            let admission = match admission {
                Ok(admission) => admission,
                Err(error) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_multipart_failed",
                        error.to_string(),
                    )))
                }
            };
            if admission.decision != crate::api::CapacityAdmissionDecision::Admitted {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_rejected",
                    admission
                        .message
                        .unwrap_or_else(|| "multipart capacity admission rejected".to_string()),
                )));
            }
        }
        let part = match journal.stage_part(&request, &mut || {
            read_frame()
                .map_err(|error| crate::runtime::MultipartPartJournalError::Io(error.to_string()))
        }) {
            Ok(part) => part,
            Err(error) => {
                if !admitted {
                    if let Some(provider) = self.service_orchestrator.capacity_provider() {
                        let _ = provider.release(&store_id, &request.reservation_id);
                    }
                }
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_multipart_failed",
                    error.to_string(),
                )));
            }
        };
        emit_response(DaemonApiResponse::ProviderStreamMultipartPartUpload(
            ProviderStreamMultipartPartUploadResponse {
                schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request.request_id,
                reservation_id: request.reservation_id,
                part_number: part.part_number,
                store_id,
                object: request.object,
                size_bytes: part.size_bytes,
                sha256: part.checksum,
            },
        ))
    }

    pub(crate) fn handle_provider_stream_upload_for_actor(
        &self,
        request: ProviderStreamUploadOpenRequest,
        actor: Option<&DaemonLocalActor>,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
        emit_response: &mut dyn FnMut(DaemonApiResponse) -> Result<(), UnixSocketDaemonServerError>,
    ) -> Result<(), UnixSocketDaemonServerError> {
        let store_id = match self.authorize_endpoint_write(actor, &request.store_id) {
            Ok(store_id) => store_id,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )))
            }
        };
        let binding =
            match read_profile_binding(&self.profile_binding_registry_path, store_id.as_str()) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        "provider stream upload requires a registered bounded folder profile",
                    )))
                }
            };
        if binding.manifest.deployment_profile != DeploymentProfile::Folder {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "provider stream upload is available for bounded folder profiles only",
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
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_unavailable",
                "profile capacity policy is unavailable",
            )));
        };
        let mut backend =
            match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                Ok(backend) => backend,
                Err(error) => {
                    return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "provider_stream_unavailable",
                        error.to_string(),
                    )))
                }
            };
        let Some(provider) = self.service_orchestrator.capacity_provider() else {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_upload_unavailable",
                "provider stream upload requires daemon capacity admission",
            )));
        };
        let mut source = ProviderUploadReader::new(&request, read_frame);
        let record = crate::runtime::put_profile_object_with_capacity_provider(
            provider.as_ref(),
            store_id.as_str(),
            &mut backend,
            &request.upload_id,
            &request.object,
            &mut source,
            request.expected_size_bytes,
        );
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "provider_stream_upload_failed",
                    error.to_string(),
                )))
            }
        };
        if let Err(error) = self.publish_profile_s3_catalogue(&store_id, &backend) {
            return emit_response(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "provider_stream_catalogue_publication_failed",
                error.to_string(),
            )));
        }
        emit_response(DaemonApiResponse::ProviderStreamUpload(
            ProviderStreamUploadResponse::from_record(request.upload_id, store_id, &record),
        ))
    }

    /// Open a catalogue-authoritative profile object for the Unix-socket
    /// provider stream. The returned reader never exposes a backend path; the
    /// transport owns chunking and cumulative verification.
    pub(crate) fn open_provider_stream(
        &self,
        request: &ProviderStreamOpenRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<ProviderStreamSource, DaemonApiResponse> {
        let delegated_actor =
            match self.delegated_object_browser_actor(actor, request.delegated_actor.as_ref()) {
                Ok(actor) => actor,
                Err(error) => {
                    return Err(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )))
                }
            };
        let effective_actor = delegated_actor.as_ref().or(actor);
        let store_id = match self.authorize_endpoint_read(effective_actor, &request.store_id) {
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

struct ProviderUploadReader<'a> {
    expected_size_bytes: u64,
    expected_sha256: String,
    next_frame: &'a mut dyn FnMut() -> Result<
        (ProviderStreamChunkHeader, Vec<u8>),
        UnixSocketDaemonServerError,
    >,
    verifier: Option<ProviderStreamVerifier>,
    pending: Vec<u8>,
    pending_offset: usize,
    final_frame_seen: bool,
}

impl<'a> ProviderUploadReader<'a> {
    fn new(
        request: &ProviderStreamUploadOpenRequest,
        next_frame: &'a mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            UnixSocketDaemonServerError,
        >,
    ) -> Self {
        Self {
            expected_size_bytes: request.expected_size_bytes,
            expected_sha256: request.expected_sha256.clone(),
            next_frame,
            verifier: Some(
                ProviderStreamVerifier::new(request.request_id.clone())
                    .expect("validated provider upload request has a request id"),
            ),
            pending: Vec::new(),
            pending_offset: 0,
            final_frame_seen: false,
        }
    }

    fn invalid(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.into())
    }
}

impl Read for ProviderUploadReader<'_> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if destination.is_empty() {
            return Ok(0);
        }
        loop {
            if self.pending_offset < self.pending.len() {
                let count = (self.pending.len() - self.pending_offset).min(destination.len());
                destination[..count].copy_from_slice(
                    &self.pending[self.pending_offset..self.pending_offset + count],
                );
                self.pending_offset += count;
                if self.pending_offset == self.pending.len() {
                    self.pending.clear();
                    self.pending_offset = 0;
                }
                return Ok(count);
            }
            if self.final_frame_seen {
                return Ok(0);
            }

            let (header, payload) = (self.next_frame)()
                .map_err(|error| io::Error::new(io::ErrorKind::UnexpectedEof, error.to_string()))?;
            let verifier = self
                .verifier
                .as_mut()
                .ok_or_else(|| Self::invalid("provider upload verifier already consumed"))?;
            if header.final_chunk {
                let total_size = self
                    .verifier
                    .take()
                    .expect("provider upload verifier present")
                    .finish(&header, &payload)
                    .map_err(|error| Self::invalid(error.to_string()))?;
                let checksum = header
                    .sha256
                    .as_deref()
                    .ok_or_else(|| Self::invalid("provider upload final checksum is missing"))?;
                if total_size != self.expected_size_bytes
                    || !checksum.eq_ignore_ascii_case(&self.expected_sha256)
                {
                    return Err(Self::invalid(
                        "provider upload differs from its declared size or checksum",
                    ));
                }
                self.pending = payload;
                self.final_frame_seen = true;
                continue;
            }
            verifier
                .push(&header, &payload)
                .map_err(|error| Self::invalid(error.to_string()))?;
            let end = header
                .offset
                .checked_add(payload.len() as u64)
                .ok_or_else(|| Self::invalid("provider upload size overflow"))?;
            if end > self.expected_size_bytes {
                return Err(Self::invalid("provider upload exceeds its declared size"));
            }
            self.pending = payload;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn request(expected_size_bytes: u64, expected_sha256: &str) -> ProviderStreamUploadOpenRequest {
        ProviderStreamUploadOpenRequest {
            schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "request-1".to_string(),
            upload_id: "upload-1".to_string(),
            store_id: "codex".parse().expect("store id"),
            object: dasobjectstore_core::backend::BackendObjectKey {
                object_id: "reads/example.txt".to_string(),
                version: 1,
            },
            expected_size_bytes,
            expected_sha256: expected_sha256.to_string(),
            chunk_size_bytes: 4,
        }
    }

    fn frame(
        request_id: &str,
        offset: u64,
        payload: &[u8],
        final_chunk: bool,
        total_size: Option<u64>,
        sha256: Option<&str>,
    ) -> (ProviderStreamChunkHeader, Vec<u8>) {
        (
            ProviderStreamChunkHeader {
                schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request_id.to_string(),
                offset,
                payload_len: payload.len() as u32,
                final_chunk,
                total_size,
                sha256: sha256.map(ToOwned::to_owned),
            },
            payload.to_vec(),
        )
    }

    #[test]
    fn reader_streams_frames_and_verifies_terminal_metadata() {
        let checksum = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let request = request(5, checksum);
        let frames = RefCell::new(vec![
            frame("request-1", 0, b"hello", false, None, None),
            frame("request-1", 5, b"", true, Some(5), Some(checksum)),
        ]);
        let mut next_frame = || {
            frames.borrow_mut().drain(..1).next().ok_or_else(|| {
                UnixSocketDaemonServerError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "test frame missing",
                ))
            })
        };
        let mut reader = ProviderUploadReader::new(&request, &mut next_frame);
        let mut payload = Vec::new();
        reader.read_to_end(&mut payload).expect("verified payload");
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn reader_rejects_declared_checksum_mismatch() {
        let request = request(
            5,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let frames = RefCell::new(vec![frame(
            "request-1",
            0,
            b"hello",
            true,
            Some(5),
            Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        )]);
        let mut next_frame = || {
            frames.borrow_mut().drain(..1).next().ok_or_else(|| {
                UnixSocketDaemonServerError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "test frame missing",
                ))
            })
        };
        let mut reader = ProviderUploadReader::new(&request, &mut next_frame);
        let mut payload = Vec::new();
        let error = reader
            .read_to_end(&mut payload)
            .expect_err("checksum mismatch");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
