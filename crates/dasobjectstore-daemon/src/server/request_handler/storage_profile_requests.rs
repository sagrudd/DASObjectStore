use super::*;

pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::ProfileBrowser(request) => {
            storage_profiles::profile_browser(handler, request, actor)
        }
        DaemonApiRequest::ProfileS3List(request) => {
            storage_profiles::profile_s3_list(handler, request, actor)
        }
        DaemonApiRequest::ProfileCatalogueExport(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding))
                    if binding.manifest.deployment_profile == DeploymentProfile::Folder =>
                {
                    binding
                }
                Ok(Some(_)) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue export is available for bounded folder profiles only",
                    )));
                }
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue export requires a registered bounded folder profile",
                    )));
                }
            };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_catalogue_unavailable",
                    "profile capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_catalogue_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let catalogue = match crate::runtime::export_profile_catalogue(&store_id, &backend) {
                Ok(catalogue) => catalogue,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_export_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileCatalogueExport(
                crate::api::ProfileCatalogueExportResponse {
                    schema_version: crate::api::PROFILE_CATALOGUE_SCHEMA_VERSION.to_string(),
                    store_id,
                    catalogue,
                },
            ))
        }
        DaemonApiRequest::ProfileCatalogueImport(request) => {
            let store_id = match handler.authorize_endpoint_write(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding))
                    if binding.manifest.deployment_profile == DeploymentProfile::Folder =>
                {
                    binding
                }
                Ok(Some(_)) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue import is available for bounded folder profiles only",
                    )));
                }
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue import requires a registered bounded folder profile",
                    )));
                }
            };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_catalogue_unavailable",
                    "profile capacity policy is unavailable",
                )));
            };
            let handoff_root = binding
                .backend_root
                .join(".dasobjectstore/profile-catalogue-handoffs");
            let mut backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_catalogue_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let committed_at_utc = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => format_utc_timestamp_seconds(duration.as_secs() as i64),
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_import_failed",
                        format!("clock unavailable: {error}"),
                    )));
                }
            };
            let imported_objects = match crate::runtime::import_profile_catalogue_with_metadata(
                &store_id,
                &request.catalogue,
                &mut backend,
                &handler.live_sqlite_path,
                handoff_root,
                &request.transaction_id,
                &request.profile_namespace,
                &committed_at_utc,
            ) {
                Ok(imported_objects) => imported_objects,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_import_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileCatalogueImport(
                crate::api::ProfileCatalogueImportResponse {
                    schema_version: crate::api::PROFILE_CATALOGUE_SCHEMA_VERSION.to_string(),
                    store_id,
                    imported_objects,
                    source_retained: true,
                },
            ))
        }
        DaemonApiRequest::ProfileS3Delete(request) => {
            let store_id = match handler.authorize_endpoint_write(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            match dasobjectstore_metadata::read_s3_object_binding(
                &handler.live_sqlite_path,
                &store_id,
                &request.key.object_id,
                request.key.version,
            ) {
                Ok(Some(_)) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_native_delete_requires_management",
                        "catalogue-native objects cannot be removed through S3 DELETE; use the evidence-bound DASObjectStore deletion workflow",
                    )));
                }
                Ok(None) => {}
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_delete_failed",
                        error.to_string(),
                    )));
                }
            }
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 deletion requires a registered bounded folder profile",
                    )));
                }
            };
            let (backend_root, backend_manifest) =
                match crate::runtime::direct_s3_profile_backend(&binding) {
                    Ok(specification) => specification,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )))
                    }
                };
            let definition = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id),
                Err(_) => None,
            };
            let Some(definition) = definition else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let capacity = crate::runtime::direct_s3_profile_capacity(
                &binding,
                definition.policy.capacity.clone(),
            );
            let mut backend = match FolderBackend::open(backend_root, backend_manifest, capacity, 0)
            {
                Ok(backend) => backend,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        error.to_string(),
                    )));
                }
            };
            let Some(provider) = handler.service_orchestrator.capacity_provider() else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_delete_unavailable",
                    "profile S3 deletion requires daemon capacity admission",
                )));
            };
            let deleted = match storage_profiles::delete_profile_s3_object(
                handler,
                provider.as_ref(),
                &store_id,
                &mut backend,
                &request.key,
            ) {
                Ok(deleted) => deleted,
                Err(response) => return Ok(response),
            };
            Ok(DaemonApiResponse::ProfileS3Delete(
                crate::api::ProfileS3DeleteResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    key: request.key,
                    deleted,
                },
            ))
        }
        DaemonApiRequest::ProfileS3MultipartAbort(request) => {
            let authorized = match handler.authorize_endpoint_write_scope(actor, &request.store_id)
            {
                Ok(authorized) => authorized,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let store_id = authorized.store_id.clone();
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "multipart abort requires a registered profile",
                    )));
                }
            };
            let (backend_root, _) = match crate::runtime::direct_s3_profile_backend(&binding) {
                Ok(specification) => specification,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_multipart_abort_failed",
                        error.to_string(),
                    )));
                }
            };
            let qualified_key = authorized.qualify_object(&request.key);
            let journal = crate::runtime::MultipartPartJournal::open_for_completion(
                &backend_root,
                store_id.as_str(),
                &request.reservation_id,
                qualified_key,
                1,
            );
            let aborted = match journal {
                Ok(journal) => {
                    match journal.abort() {
                        Ok(()) => {}
                        Err(crate::runtime::MultipartPartJournalError::CompletionStarted)
                        | Err(crate::runtime::MultipartPartJournalError::AlreadyCommitted) => {
                            return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                                "profile_s3_multipart_completion_active",
                                "multipart completion has started and cannot be aborted",
                            )));
                        }
                        Err(error) => {
                            return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                                "profile_s3_multipart_abort_failed",
                                error.to_string(),
                            )));
                        }
                    }
                    if let Some(provider) = handler.service_orchestrator.capacity_provider() {
                        match authorized.subobject.as_deref() {
                            Some(subobject) => provider
                                .release_subobject(&store_id, subobject, &request.reservation_id)
                                .map_err(DaemonRequestHandlerError::ServiceRuntime)?,
                            None => provider
                                .release(&store_id, &request.reservation_id)
                                .map_err(DaemonRequestHandlerError::ServiceRuntime)?,
                        }
                    }
                    true
                }
                Err(crate::runtime::MultipartPartJournalError::Manifest(_)) => false,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_multipart_abort_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3MultipartAbort(
                crate::api::ProfileS3MultipartAbortResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    reservation_id: request.reservation_id,
                    aborted,
                },
            ))
        }
        DaemonApiRequest::ProfileS3MultipartComplete(request) => {
            let authorized = match handler.authorize_endpoint_write_scope(actor, &request.store_id)
            {
                Ok(authorized) => authorized,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let store_id = authorized.store_id.clone();
            let qualified_key = authorized.qualify_object(&request.key);
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "multipart completion requires a registered bounded folder profile",
                    )));
                }
            };
            let (backend_root, backend_manifest) =
                match crate::runtime::direct_s3_profile_backend(&binding) {
                    Ok(specification) => specification,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let definition = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id),
                Err(_) => None,
            };
            let Some(definition) = definition else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let capacity = crate::runtime::direct_s3_profile_capacity(
                &binding,
                definition.policy.capacity.clone(),
            );
            let mut journal = match crate::runtime::MultipartPartJournal::open_for_completion(
                &backend_root,
                request.store_id.as_str(),
                &request.reservation_id,
                qualified_key.clone(),
                request.expected_size_bytes,
            ) {
                Ok(journal) => journal,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_multipart_unavailable",
                        error.to_string(),
                    )));
                }
            };
            let journal_parts = journal.parts().collect::<Vec<_>>();
            let requested_parts = request
                .parts
                .iter()
                .map(|part| crate::runtime::MultipartPartRecord {
                    part_number: part.part_number,
                    size_bytes: part.size_bytes,
                    checksum: part.checksum.clone(),
                })
                .collect::<Vec<_>>();
            if journal_parts != requested_parts
                || journal.staged_bytes() != request.expected_size_bytes
            {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_multipart_incomplete",
                    "multipart completion does not match all verified staged parts",
                )));
            }
            let completion_claim = match journal.begin_completion(
                qualified_key.clone(),
                request.expected_size_bytes,
                requested_parts,
            ) {
                Ok(receipt) => receipt,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_multipart_completion_conflict",
                        error.to_string(),
                    )));
                }
            };
            if let crate::runtime::MultipartCompletionClaim::Committed(receipt) = &completion_claim
            {
                return Ok(DaemonApiResponse::ProfileS3MultipartComplete(
                    crate::api::ProfileS3MultipartCompletionResponse {
                        schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                        store_id,
                        reservation_id: request.reservation_id,
                        key: receipt.object.clone(),
                        committed: true,
                    },
                ));
            }
            let mut backend =
                match FolderBackend::open(&backend_root, backend_manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            if completion_claim == crate::runtime::MultipartCompletionClaim::Resuming {
                let recovered = match backend.records() {
                    Ok(records) => records
                        .into_iter()
                        .find(|record| record.key == qualified_key),
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_recovery_failed",
                            error.to_string(),
                        )));
                    }
                };
                if let Some(record) = recovered {
                    let assembled_checksum = match journal.assembled_checksum() {
                        Ok(checksum) => checksum,
                        Err(error) => {
                            return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                                "profile_s3_multipart_recovery_failed",
                                error.to_string(),
                            )));
                        }
                    };
                    if record.size_bytes != request.expected_size_bytes
                        || record.checksum != assembled_checksum
                    {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_completion_conflict",
                            "published backend object conflicts with the durable multipart intent",
                        )));
                    }
                    if let Err(error) = handler.commit_profile_s3_acceptance(
                        &definition,
                        &binding,
                        &backend,
                        &record,
                        &request.reservation_id,
                    ) {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_publication_failed",
                            error,
                        )));
                    }
                    if let Err(error) =
                        journal.mark_committed(crate::runtime::MultipartCompletionReceipt {
                            object: record.key.clone(),
                            size_bytes: record.size_bytes,
                            checksum: record.checksum.clone(),
                        })
                    {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_receipt_failed",
                            error.to_string(),
                        )));
                    }
                    return Ok(DaemonApiResponse::ProfileS3MultipartComplete(
                        crate::api::ProfileS3MultipartCompletionResponse {
                            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                            store_id,
                            reservation_id: request.reservation_id,
                            key: record.key,
                            committed: true,
                        },
                    ));
                }
            }
            let mut sources = Vec::with_capacity(request.parts.len());
            for part in &request.parts {
                let reader = match journal.open_part(part.part_number) {
                    Ok(reader) => reader,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_incomplete",
                            error.to_string(),
                        )));
                    }
                };
                sources.push(crate::runtime::ProfileS3MultipartPartSource {
                    part: crate::runtime::ProfileS3MultipartPart {
                        part_number: part.part_number,
                        size_bytes: part.size_bytes,
                        checksum: part.checksum.clone(),
                    },
                    reader: Box::new(reader),
                });
            }
            let Some(provider) = handler.service_orchestrator.capacity_provider() else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_multipart_unavailable",
                    "multipart completion requires daemon capacity admission",
                )));
            };
            let completion = crate::runtime::ProfileS3MultipartCompletion {
                reservation_id: request.reservation_id.clone(),
                key: qualified_key,
                expected_size_bytes: request.expected_size_bytes,
                parts: request
                    .parts
                    .iter()
                    .map(|part| crate::runtime::ProfileS3MultipartPart {
                        part_number: part.part_number,
                        size_bytes: part.size_bytes,
                        checksum: part.checksum.clone(),
                    })
                    .collect(),
            };
            let record =
                match crate::runtime::complete_profile_s3_multipart_with_admitted_capacity_scope(
                    provider.as_ref(),
                    store_id.as_str(),
                    authorized.subobject.as_deref(),
                    &mut backend,
                    &completion,
                    sources,
                ) {
                    Ok(record) => record,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_failed",
                            error.to_string(),
                        )));
                    }
                };
            if let Err(error) = handler.commit_profile_s3_acceptance(
                &definition,
                &binding,
                &backend,
                &record,
                &request.reservation_id,
            ) {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_multipart_publication_failed",
                    error,
                )));
            }
            if let Err(error) = journal.mark_committed(crate::runtime::MultipartCompletionReceipt {
                object: record.key.clone(),
                size_bytes: record.size_bytes,
                checksum: record.checksum.clone(),
            }) {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_multipart_receipt_failed",
                    error.to_string(),
                )));
            }
            let response = crate::api::ProfileS3MultipartCompletionResponse {
                schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                store_id,
                reservation_id: request.reservation_id,
                key: record.key,
                committed: true,
            };
            Ok(DaemonApiResponse::ProfileS3MultipartComplete(response))
        }
        DaemonApiRequest::ProfileS3Head(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            match dasobjectstore_metadata::read_s3_object_binding(
                &handler.live_sqlite_path,
                &store_id,
                &request.key.object_id,
                request.key.version,
            ) {
                Ok(Some(object)) => {
                    if let Err(error) = resolve_object_download_with_hdd_root(
                        &handler.live_sqlite_path,
                        &handler.hdd_root_path,
                        &store_id,
                        &ObjectDownloadRequest {
                            endpoint: store_id.clone(),
                            object_id: object.object_id,
                            delegated_actor: None,
                        },
                    ) {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            format!(
                                "catalogued object has no readable verified placement: {error}"
                            ),
                        )));
                    }
                    return Ok(DaemonApiResponse::ProfileS3Head(ProfileS3HeadResponse {
                        schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                        store_id,
                        object: ProfileS3ObjectView {
                            key: request.key,
                            size_bytes: object.size_bytes,
                            checksum: object.checksum,
                        },
                    }));
                }
                Ok(None) => {}
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_head_failed",
                        error.to_string(),
                    )));
                }
            }
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
            };
            let (backend_root, backend_manifest) =
                match crate::runtime::direct_s3_profile_backend(&binding) {
                    Ok(specification) => specification,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )))
                    }
                };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let capacity = crate::runtime::direct_s3_profile_capacity(&binding, capacity);
            let backend = match FolderBackend::open(backend_root, backend_manifest, capacity, 0) {
                Ok(backend) => backend,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        error.to_string(),
                    )));
                }
            };
            let object = match head_profile_object(&backend, &request.key) {
                Ok(object) => object,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_head_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Head(ProfileS3HeadResponse {
                schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                store_id,
                object: ProfileS3ObjectView {
                    key: object.key,
                    size_bytes: object.size_bytes,
                    checksum: object.checksum,
                },
            }))
        }
        DaemonApiRequest::ProfileS3Verify(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
            };
            let (backend_root, backend_manifest) =
                match crate::runtime::direct_s3_profile_backend(&binding) {
                    Ok(specification) => specification,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )))
                    }
                };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let capacity = crate::runtime::direct_s3_profile_capacity(&binding, capacity);
            let backend = match FolderBackend::open(backend_root, backend_manifest, capacity, 0) {
                Ok(backend) => backend,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        error.to_string(),
                    )));
                }
            };
            let object = match verify_profile_object(&backend, &request.key) {
                Ok(object) => object,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_verify_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Verify(
                ProfileS3VerifyResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    object: ProfileS3ObjectView {
                        key: object.key,
                        size_bytes: object.size_bytes,
                        checksum: object.checksum,
                    },
                    verified: true,
                },
            ))
        }
        DaemonApiRequest::ProfileS3Health(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
            };
            let (backend_root, backend_manifest) =
                match crate::runtime::direct_s3_profile_backend(&binding) {
                    Ok(specification) => specification,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )))
                    }
                };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let capacity = crate::runtime::direct_s3_profile_capacity(&binding, capacity);
            let backend = match FolderBackend::open(backend_root, backend_manifest, capacity, 0) {
                Ok(backend) => backend,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        error.to_string(),
                    )));
                }
            };
            let health = match profile_health(&backend) {
                Ok(health) => health,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_health_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Health(
                ProfileS3HealthResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    health,
                },
            ))
        }
        DaemonApiRequest::ProfileDiagnostics(request) => {
            storage_profiles::diagnostics(handler, request, actor)
        }
        _ => unreachable!("profile storage dispatcher received an unrelated request"),
    }
}
