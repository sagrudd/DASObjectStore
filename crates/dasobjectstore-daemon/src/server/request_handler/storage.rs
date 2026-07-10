use super::*;

/// Handles storage inventory, telemetry, ingest, and object browser requests.
pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
    emit_progress: &mut impl FnMut(
        DaemonIngestProgressEvent,
    ) -> Result<(), DaemonIngestFilesRuntimeError>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::StoreInventory(request) => {
            match handler.store_inventory_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreInventory(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "store_inventory_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::UpdateObjectStoreIngestPolicy(mut request) => {
            let Some(actor) = actor else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "administrator_authentication_required",
                    "object-store ingest policy updates require an authenticated local administrator",
                )));
            };
            if !actor.is_administrator() {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "administrator_authorization_required",
                    "object-store ingest policy updates require root, sudo, or dasobjectstore-admin membership",
                )));
            }
            request.administrator_actor = Some(actor.display_name());
            let now = handler.clock.now_utc();
            let response = match handler.update_object_store_ingest_policy(request, &now) {
                Ok(response) => response,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "store_policy_update_failed",
                        error.to_string(),
                    )))
                }
            };
            handler.record_admin_job(daemon_job_summary_from_update_object_store_ingest_policy(
                &response,
            ))?;
            Ok(DaemonApiResponse::UpdateObjectStoreIngestPolicy(response))
        }
        DaemonApiRequest::ApplianceTelemetry(request) => {
            match handler.appliance_telemetry_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::ApplianceTelemetry(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::SubmitIngestFiles(request) => {
            if let Some(actor) = actor {
                if let Err(error) = handler.authorize_ingest_files(actor, &request) {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            }
            match handler.service_orchestrator.submit_ingest_files(
                request,
                &handler.clock.now_utc(),
                emit_progress,
            ) {
                Ok(response) => Ok(DaemonApiResponse::SubmitIngestFiles(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "ingest_files_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::ObjectBrowser(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_endpoint_read(effective_actor, &request.endpoint)
            {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let entries = match read_object_browser_metadata(&handler.live_sqlite_path, store_id) {
                Ok(entries) => entries,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "object_browser_metadata_failed",
                        error.to_string(),
                    )));
                }
            };
            query_object_browser_metadata(&request, &entries)
                .map(DaemonApiResponse::ObjectBrowser)
                .map_err(Into::into)
        }
        DaemonApiRequest::ObjectDownload(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_object_download(effective_actor, &request) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            match resolve_object_download_with_hdd_root(
                &handler.live_sqlite_path,
                &handler.hdd_root_path,
                &store_id,
                &request,
            ) {
                Ok(response) => Ok(DaemonApiResponse::ObjectDownload(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::ObjectFolderDownload(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_object_folder_download(effective_actor, &request)
            {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            match resolve_object_folder_download_with_hdd_root(
                &handler.live_sqlite_path,
                &handler.hdd_root_path,
                &store_id,
                &request,
            ) {
                Ok(response) => Ok(DaemonApiResponse::ObjectFolderDownload(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        _ => unreachable!("storage dispatcher received an unrelated request"),
    }
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    fn store_inventory_for_actor(
        &self,
        request: StoreInventoryRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreInventoryResponse, ObjectServiceError> {
        if let Some(session_id) = request.remote_easyconnect_session_id.as_deref() {
            return self
                .store_inventory_for_remote_easyconnect_session(&request, session_id)
                .map_err(|error| ObjectServiceError::CommandFailed(error.to_string()));
        }
        let stores = read_store_registry(&self.store_registry_path)?;
        let mut inventory = Vec::new();
        for definition in stores {
            let bucket_name = if definition.policy.export_policy == ExportPolicy::S3 {
                Some(bucket_name_for_definition(&definition)?)
            } else {
                None
            };
            let mut access_policy = DaemonStoreAccessPolicy::new(definition.store_id.clone());
            if let Some(reader_group) = &definition.reader_group {
                access_policy = access_policy.with_reader_group(reader_group.clone());
            }
            if let Some(writer_group) = &definition.writer_group {
                access_policy = access_policy.with_writer_group(writer_group.clone());
            }
            access_policy = access_policy.with_public_read(definition.public);
            let visible = match actor {
                Some(actor) => authorize_store_read(actor, &access_policy).is_ok(),
                None => definition.public,
            };
            if !visible {
                continue;
            }
            let mut policy = definition.policy.clone();
            if !request.include_policy {
                policy = dasobjectstore_core::store::StorePolicy::defaults_for(policy.class);
            }
            inventory.push(StoreInventoryItem {
                store_id: definition.store_id,
                policy,
                bucket_name,
                reader_group: definition.reader_group,
                writer_group: definition.writer_group,
                public: definition.public,
                writable: definition.policy.export_policy == ExportPolicy::S3,
            });
        }

        Ok(StoreInventoryResponse { stores: inventory })
    }

    fn store_inventory_for_remote_easyconnect_session(
        &self,
        request: &StoreInventoryRequest,
        session_id: &str,
    ) -> Result<StoreInventoryResponse, RemoteEasyconnectStoreInventoryError> {
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
        let session = session_store.get(session_id)?.ok_or_else(|| {
            RemoteEasyconnectPairedSessionStoreError::SessionNotFound {
                session_id: session_id.to_string(),
            }
        })?;
        let actor = DaemonLocalActor::new(0).with_username(session.approved_actor.clone());
        let stores = read_store_registry(&self.store_registry_path).map_err(|error| {
            RemoteEasyconnectPairedSessionStoreError::Json {
                path: self.store_registry_path.clone(),
                message: error.to_string(),
            }
        })?;
        let mut inventory = Vec::new();
        for definition in stores {
            let Some(grant) = session
                .object_stores
                .iter()
                .find(|grant| grant.object_store == definition.store_id.as_str())
            else {
                continue;
            };
            if request.remote_upload_writable_only {
                session_store.authorize_write(
                    session_id,
                    definition.store_id.as_str(),
                    &actor,
                    &self.clock.now_utc(),
                )?;
                if definition.writer_group.is_none() {
                    return Err(RemoteEasyconnectStoreInventoryError::MissingWriterGroup {
                        object_store: definition.store_id.to_string(),
                    });
                }
                if definition.policy.export_policy != ExportPolicy::S3 {
                    return Err(
                        RemoteEasyconnectStoreInventoryError::StoreNotRemoteWritable {
                            object_store: definition.store_id.to_string(),
                            export_policy: format!("{:?}", definition.policy.export_policy),
                        },
                    );
                }
            } else if !grant.can_read && !grant.can_write {
                continue;
            }
            let bucket_name = if definition.policy.export_policy == ExportPolicy::S3 {
                Some(bucket_name_for_definition(&definition).map_err(|error| {
                    RemoteEasyconnectPairedSessionStoreError::Json {
                        path: self.store_registry_path.clone(),
                        message: error.to_string(),
                    }
                })?)
            } else {
                None
            };
            let mut policy = definition.policy.clone();
            if !request.include_policy {
                policy = dasobjectstore_core::store::StorePolicy::defaults_for(policy.class);
            }
            inventory.push(StoreInventoryItem {
                store_id: definition.store_id,
                policy,
                bucket_name,
                reader_group: definition.reader_group,
                writer_group: definition.writer_group,
                public: definition.public,
                writable: definition.policy.export_policy == ExportPolicy::S3 && grant.can_write,
            });
        }

        Ok(StoreInventoryResponse { stores: inventory })
    }

    fn authorize_ingest_files(
        &self,
        actor: &DaemonLocalActor,
        request: &SubmitIngestFilesRequest,
    ) -> Result<(), IngestAuthorizationFailure> {
        let store_id = resolve_authorization_store_id(
            &request.endpoint,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )?;
        let stores = read_store_registry(&self.store_registry_path)?;
        let store = stores
            .into_iter()
            .find(|definition| definition.store_id == store_id)
            .ok_or_else(|| IngestAuthorizationFailure::MissingStore {
                store_id: store_id.clone(),
                store_registry_path: self.store_registry_path.clone(),
            })?;

        let mut policy = DaemonStoreAccessPolicy::new(store.store_id.clone());
        if let Some(reader_group) = store.reader_group {
            policy = policy.with_reader_group(reader_group);
        }
        if let Some(writer_group) = store.writer_group {
            policy = policy.with_writer_group(writer_group);
        }
        policy = policy.with_public_read(store.public);
        authorize_store_write(actor, &policy)?;
        Ok(())
    }

    fn appliance_telemetry_for_actor(
        &self,
        request: ApplianceTelemetryRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<ApplianceTelemetryResponse, ApplianceTelemetryAccessFailure> {
        if actor.is_none() {
            return Err(ApplianceTelemetryAccessFailure::MissingActor);
        }
        match fs::read_to_string(&self.appliance_telemetry_state_path) {
            Ok(contents) => {
                let sample_set: ApplianceTelemetrySampleSet = serde_json::from_str(&contents)
                    .map_err(|error| ApplianceTelemetryAccessFailure::InvalidState {
                        path: self.appliance_telemetry_state_path.clone(),
                        message: error.to_string(),
                    })?;
                Ok(query_appliance_telemetry(&sample_set, &request))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(ApplianceTelemetryResponse::missing(request.window))
            }
            Err(error) => Err(ApplianceTelemetryAccessFailure::ReadState {
                path: self.appliance_telemetry_state_path.clone(),
                message: error.to_string(),
            }),
        }
    }

    fn delegated_object_browser_actor(
        &self,
        peer_actor: Option<&DaemonLocalActor>,
        delegated_actor: Option<&ObjectBrowserDelegatedActor>,
    ) -> Result<Option<DaemonLocalActor>, ObjectBrowserAccessFailure> {
        let Some(delegated_actor) = delegated_actor else {
            return Ok(None);
        };
        let peer_actor = peer_actor.ok_or(ObjectBrowserAccessFailure::MissingActor)?;
        if peer_actor.uid != 0
            && peer_actor.username.as_deref() != Some(DEFAULT_DAEMON_SERVICE_USER)
        {
            return Err(ObjectBrowserAccessFailure::DelegationNotAllowed {
                peer_actor: peer_actor.display_name(),
            });
        }
        let mut actor = DaemonLocalActor::new(delegated_actor.uid.unwrap_or(peer_actor.uid))
            .with_username(delegated_actor.username.clone())
            .with_groups(delegated_actor.groups.clone());
        if let Some(primary_gid) = delegated_actor.primary_gid {
            actor = actor.with_primary_gid(primary_gid);
        }
        Ok(Some(actor))
    }

    fn authorize_endpoint_read(
        &self,
        actor: Option<&DaemonLocalActor>,
        endpoint: &StoreId,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        let actor = actor.ok_or(ObjectBrowserAccessFailure::MissingActor)?;
        let store_id = resolve_authorization_store_id(
            endpoint,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )
        .map_err(ObjectBrowserAccessFailure::Endpoint)?;
        let stores = read_store_registry(&self.store_registry_path)?;
        let store = stores
            .into_iter()
            .find(|definition| definition.store_id == store_id)
            .ok_or_else(|| ObjectBrowserAccessFailure::MissingStore {
                store_id: store_id.clone(),
                store_registry_path: self.store_registry_path.clone(),
            })?;

        let mut policy = DaemonStoreAccessPolicy::new(store.store_id.clone());
        if let Some(reader_group) = store.reader_group {
            policy = policy.with_reader_group(reader_group);
        }
        if let Some(writer_group) = store.writer_group {
            policy = policy.with_writer_group(writer_group);
        }
        policy = policy.with_public_read(store.public);
        authorize_store_read(actor, &policy)?;
        Ok(store_id)
    }

    fn authorize_object_download(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &ObjectDownloadRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        self.authorize_endpoint_read(actor, &request.endpoint)
    }

    fn authorize_object_folder_download(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &ObjectFolderDownloadRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        self.authorize_endpoint_read(actor, &request.endpoint)
    }
}
