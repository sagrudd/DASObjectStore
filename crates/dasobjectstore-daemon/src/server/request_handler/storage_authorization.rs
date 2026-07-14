use super::*;

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    pub(super) fn authorize_ingest_files(
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

    pub(super) fn appliance_telemetry_for_actor(
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

    pub(super) fn delegated_object_browser_actor(
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

    pub(super) fn authorize_endpoint_read(
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

    pub(super) fn authorize_endpoint_write(
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
        authorize_store_write(actor, &policy)?;
        Ok(store_id)
    }

    pub(super) fn authorize_object_download(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &ObjectDownloadRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        self.authorize_endpoint_read(actor, &request.endpoint)
    }

    pub(super) fn authorize_object_folder_download(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &ObjectFolderDownloadRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        self.authorize_endpoint_read(actor, &request.endpoint)
    }
}
