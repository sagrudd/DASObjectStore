use super::*;
use crate::runtime::discover_managed_hdd_roots;
use dasobjectstore_object_service::{
    StoreRegistryDeleteReport, SubObjectRegistryStoreDeleteReport,
};

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
        DaemonApiRequest::DiskRetire(request) => {
            match handler.disk_retire_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::DiskRetire(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::DiskForceRetire(request) => {
            match handler.disk_force_retire_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::DiskForceRetire(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::StoreInventory(request) => {
            match handler.store_inventory_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreInventory(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "store_inventory_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::StoreDrain(request) => {
            match handler.store_drain_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreDrain(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::StoreDelete(request) => {
            match handler.store_delete_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreDelete(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::ObjectPut(request) => {
            match handler.object_put_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::ObjectPut(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::IngestQueueDrain(request) => {
            match handler.ingest_queue_drain_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::IngestQueueDrain(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
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
            let trusted_web_peer = actor.username.as_deref() == Some(DEFAULT_DAEMON_SERVICE_USER)
                && request
                    .administrator_actor
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty());
            if !actor.is_administrator() && !trusted_web_peer {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "administrator_authorization_required",
                    "object-store ingest policy updates require root, sudo, dasobjectstore-admin membership, or the trusted authenticated Web service peer",
                )));
            }
            if actor.is_administrator() {
                request.administrator_actor = Some(actor.display_name());
            }
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
    fn store_drain_for_actor(
        &self,
        request: StoreDrainRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreDrainResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "store drain requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "store drain requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
            if !request.allow_store_drain {
                return Err((
                    "store_drain_not_allowed",
                    "store drain requires policy allowance".to_string(),
                ));
            }
        }
        let store_id = StoreId::new(request.store_id.clone())
            .map_err(|error| ("invalid_store_id", error.to_string()))?;
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| ("managed_hdd_discovery_failed", error.to_string()))?;
        let report =
            dasobjectstore_metadata::drain_store(&dasobjectstore_metadata::StoreDrainRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                store_id,
                disk_roots,
                dry_run: request.dry_run,
            })
            .map_err(|error| ("store_drain_failed", error.to_string()))?;
        Ok(StoreDrainResponse { report })
    }

    fn store_delete_for_actor(
        &self,
        request: StoreDeleteRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreDeleteResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "store delete requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "store delete requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
            if !request.allow_store_delete {
                return Err((
                    "store_delete_not_allowed",
                    "store delete requires policy allowance".to_string(),
                ));
            }
        }

        let store_id = StoreId::new(request.store_id.clone())
            .map_err(|error| ("invalid_store_id", error.to_string()))?;
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| ("managed_hdd_discovery_failed", error.to_string()))?;
        let metadata =
            dasobjectstore_metadata::delete_store(&dasobjectstore_metadata::StoreDeleteRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                store_id: store_id.clone(),
                disk_roots,
                dry_run: request.dry_run,
            })
            .map_err(|error| ("store_delete_failed", error.to_string()))?;
        let host_registry =
            delete_store_definition_maybe(&self.store_registry_path, &store_id, request.dry_run)
                .map_err(|error| ("store_registry_delete_failed", error.to_string()))?;
        let host_subobjects = delete_subobjects_for_store_maybe(
            &self.subobject_registry_path,
            &store_id,
            request.dry_run,
        )
        .map_err(|error| ("subobject_registry_delete_failed", error.to_string()))?;

        let (portable_registry, portable_subobjects) = if known_ssd_root(&default_ssd_root()) {
            let ssd_root = default_ssd_root();
            let portable_registry_path = portable_store_registry_path(&ssd_root);
            let portable_subobject_path = portable_subobject_registry_path(&ssd_root);
            (
                Some(
                    delete_store_definition_maybe(
                        &portable_registry_path,
                        &store_id,
                        request.dry_run,
                    )
                    .map_err(|error| {
                        ("portable_store_registry_delete_failed", error.to_string())
                    })?,
                ),
                Some(
                    delete_subobjects_for_store_maybe(
                        &portable_subobject_path,
                        &store_id,
                        request.dry_run,
                    )
                    .map_err(|error| {
                        (
                            "portable_subobject_registry_delete_failed",
                            error.to_string(),
                        )
                    })?,
                ),
            )
        } else {
            (None, None)
        };

        Ok(StoreDeleteResponse {
            report: StoreDeleteCommandReport {
                metadata,
                host_registry,
                portable_registry,
                host_subobjects,
                portable_subobjects,
            },
        })
    }

    fn object_put_for_actor(
        &self,
        request: ObjectPutRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<ObjectPutResponse, (&'static str, String)> {
        if actor.is_none() {
            return Err((
                "authentication_required",
                "object put requires an authenticated local actor".to_string(),
            ));
        }
        let object_id = dasobjectstore_core::ids::ObjectId::new(request.object_id.clone())
            .map_err(|error| ("invalid_object_id", error.to_string()))?;
        let disk_roots = parse_disk_copy_roots(&request.disk_roots)
            .map_err(|error| ("invalid_disk_root", error))?;
        let metadata_request = MetadataObjectPutRequest::new(
            object_id,
            request.source_path,
            request.ssd_root,
            disk_roots,
            request.copies,
        )
        .with_object_type(request.object_type);
        let report = put_object_ssd_first(&metadata_request)
            .map_err(|error| ("object_put_failed", error.to_string()))?;
        Ok(ObjectPutResponse { report })
    }

    fn disk_retire_for_actor(
        &self,
        request: DiskRetireRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<DiskRetireResponse, (&'static str, String)> {
        let Some(actor) = actor else {
            return Err((
                "administrator_authentication_required",
                "disk retirement requires an authenticated local administrator".to_string(),
            ));
        };
        if !actor.is_administrator() {
            return Err((
                "administrator_authorization_required",
                "disk retirement requires root, sudo, or dasobjectstore-admin membership"
                    .to_string(),
            ));
        }
        let disk_id = dasobjectstore_core::ids::DiskId::new(request.disk_id.clone())
            .map_err(|error| ("invalid_disk_id", error.to_string()))?;
        let report = dasobjectstore_metadata::request_disk_retirement(
            &self.live_sqlite_path,
            &disk_id,
            self.clock.now_utc(),
        )
        .map_err(|error| ("disk_retirement_failed", error.to_string()))?;
        Ok(DiskRetireResponse { report })
    }

    fn disk_force_retire_for_actor(
        &self,
        request: DiskForceRetireRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<DiskRetireResponse, (&'static str, String)> {
        let Some(actor) = actor else {
            return Err((
                "administrator_authentication_required",
                "disk force-retirement requires an authenticated local administrator".to_string(),
            ));
        };
        if !actor.is_administrator() {
            return Err((
                "administrator_authorization_required",
                "disk force-retirement requires root, sudo, or dasobjectstore-admin membership"
                    .to_string(),
            ));
        }
        if !request.allow_force_retire {
            return Err((
                "force_disk_retire_not_allowed",
                "disk force-retirement requires policy allowance".to_string(),
            ));
        }
        let disk_id = dasobjectstore_core::ids::DiskId::new(request.disk_id.clone())
            .map_err(|error| ("invalid_disk_id", error.to_string()))?;
        let report = dasobjectstore_metadata::force_retire_disk(
            &self.live_sqlite_path,
            &disk_id,
            self.clock.now_utc(),
            dasobjectstore_core::risk::RiskPolicy {
                allow_force_retire: true,
                ..Default::default()
            },
            &dasobjectstore_core::risk::ActionConfirmation::new(&request.confirmation_marker),
        )
        .map_err(|error| ("disk_force_retirement_failed", error.to_string()))?;
        Ok(DiskRetireResponse { report })
    }

    fn ingest_queue_drain_for_actor(
        &self,
        request: IngestQueueDrainRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<IngestQueueDrainResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "ingest queue drain requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "ingest queue drain requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
            if !request.allow_ingest_queue_drain {
                return Err((
                    "ingest_queue_drain_not_allowed",
                    "ingest queue drain requires policy allowance".to_string(),
                ));
            }
        }
        let store_id = StoreId::new(request.store_id.clone())
            .map_err(|error| ("invalid_store_id", error.to_string()))?;
        let report = dasobjectstore_metadata::drain_ingest_queue(
            &dasobjectstore_metadata::IngestQueueDrainRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                store_id,
                updated_at_utc: self.clock.now_utc(),
                reason: request.reason,
                dry_run: request.dry_run,
            },
        )
        .map_err(|error| ("ingest_queue_drain_failed", error.to_string()))?;
        Ok(IngestQueueDrainResponse { report })
    }

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

fn known_ssd_root(path: &Path) -> bool {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
        .map(|marker| marker.lines().any(|line| line == "role=ssd"))
        .unwrap_or(false)
}

fn parse_disk_copy_roots(entries: &[String]) -> Result<Vec<DiskCopyRoot>, String> {
    entries
        .iter()
        .map(|entry| {
            let (disk_id, root_path) = entry
                .split_once('=')
                .ok_or_else(|| format!("disk root must use disk-id=/path syntax: {entry}"))?;
            let disk_id = dasobjectstore_core::ids::DiskId::new(disk_id)
                .map_err(|error| format!("invalid disk id {disk_id}: {error}"))?;
            if root_path.is_empty() {
                return Err(format!("disk root path must not be empty: {entry}"));
            }
            Ok(DiskCopyRoot::new(disk_id, root_path))
        })
        .collect()
}

fn delete_store_definition_maybe(
    path: &Path,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<StoreRegistryDeleteReport, ObjectServiceError> {
    if dry_run {
        let removed = read_store_registry(path)?
            .iter()
            .any(|definition| &definition.store_id == store_id);
        return Ok(StoreRegistryDeleteReport {
            registry_path: path.to_path_buf(),
            store_id: store_id.clone(),
            removed,
        });
    }

    delete_store_definition(path, store_id)
}

fn delete_subobjects_for_store_maybe(
    path: &Path,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<SubObjectRegistryStoreDeleteReport, ObjectServiceError> {
    if dry_run {
        let mut removed_names = read_subobject_registry(path)?
            .iter()
            .filter(|definition| &definition.store_id == store_id)
            .map(|definition| definition.name.clone())
            .collect::<Vec<_>>();
        removed_names.sort();
        return Ok(SubObjectRegistryStoreDeleteReport {
            registry_path: path.to_path_buf(),
            store_id: store_id.clone(),
            removed_count: removed_names.len(),
            removed_names,
        });
    }

    delete_subobjects_for_store(path, store_id)
}
