use super::*;
use dasobjectstore_core::backend::BackendObjectKey;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AuthorizedEndpointWrite {
    pub store_id: StoreId,
    pub subobject: Option<String>,
    object_prefix: Option<String>,
}

impl AuthorizedEndpointWrite {
    pub fn qualify_object(&self, object: &BackendObjectKey) -> BackendObjectKey {
        let Some(prefix) = &self.object_prefix else {
            return object.clone();
        };
        BackendObjectKey {
            object_id: format!("{prefix}/{}", object.object_id),
            version: object.version,
        }
    }
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    pub(super) fn authorize_synoptikon_projection_write(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &crate::api::ProviderStreamUploadOpenRequest,
    ) -> Result<AuthorizedEndpointWrite, ObjectBrowserAccessFailure> {
        let authority = request.synoptikon_projection.as_ref().ok_or(
            ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "Synoptikon projection intent is missing".to_owned(),
            },
        )?;
        let actor = actor.ok_or(ObjectBrowserAccessFailure::MissingActor)?;
        if actor.username.as_deref() != Some(crate::api::SYNOPTIKON_PROJECTION_FIXED_PEER_USER) {
            return Err(ObjectBrowserAccessFailure::DelegationNotAllowed {
                peer_actor: actor.display_name(),
            });
        }
        let intent = crate::runtime::projection_intent(
            &self.synoptikon_projection_ledger_path,
            &authority.intent_id,
        )
        .map_err(|error| ObjectBrowserAccessFailure::InvalidVerifiedSubject {
            message: error.to_string(),
        })?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(u64::MAX);
        let expected_provider_sha256 = format!("sha256:{}", intent.projection.source_sha256);
        if now >= intent.projection.expires_at_unix_seconds
            || request.upload_id != intent.intent_id
            || request.store_id.as_str() != intent.projection.object_store_id
            || request.object.object_id != intent.projection.object_key
            || request.object.version != intent.projection.object_version
            || request.expected_size_bytes != intent.projection.source_size_bytes
            || request.expected_sha256 != expected_provider_sha256
        {
            return Err(ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "upload differs from the durable Synoptikon intent".to_owned(),
            });
        }
        let store_id = resolve_authorization_store_id(
            &request.store_id,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )
        .map_err(ObjectBrowserAccessFailure::Endpoint)?;
        if store_id != request.store_id {
            return Err(ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "Synoptikon projection requires an exact ObjectStore".to_owned(),
            });
        }
        Ok(AuthorizedEndpointWrite {
            store_id,
            subobject: None,
            object_prefix: None,
        })
    }

    pub(super) fn authorize_synoptikon_projection_read(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &crate::api::ProviderStreamOpenRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        let authority = request.synoptikon_projection.as_ref().ok_or(
            ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "Synoptikon settlement is missing".to_owned(),
            },
        )?;
        let actor = actor.ok_or(ObjectBrowserAccessFailure::MissingActor)?;
        if actor.username.as_deref() != Some(crate::api::SYNOPTIKON_PROJECTION_FIXED_PEER_USER) {
            return Err(ObjectBrowserAccessFailure::DelegationNotAllowed {
                peer_actor: actor.display_name(),
            });
        }
        let intent = crate::runtime::verify_projection_settlement(
            &self.synoptikon_projection_ledger_path,
            &authority.settlement_id,
        )
        .map_err(|error| ObjectBrowserAccessFailure::InvalidVerifiedSubject {
            message: error.to_string(),
        })?;
        let expected_provider_sha256 = format!("sha256:{}", intent.projection.source_sha256);
        if request.range.is_some()
            || request.store_id.as_str() != intent.projection.object_store_id
            || request.object.object_id != intent.projection.object_key
            || request.object.version != intent.projection.object_version
            || request.condition.if_match_sha256.as_deref()
                != Some(expected_provider_sha256.as_str())
            || request.condition.if_none_match_sha256.is_some()
        {
            return Err(ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "readback differs from the terminal Synoptikon settlement".to_owned(),
            });
        }
        Ok(request.store_id.clone())
    }

    /// Authorize only the packaged Base Camp peer for the narrow retained
    /// dossier request. Serialized authority facts never substitute for the
    /// Unix peer credential supplied by the daemon transport.
    pub(super) fn authorize_expedition_retained_dossier_write(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &crate::api::ProviderStreamUploadOpenRequest,
    ) -> Result<AuthorizedEndpointWrite, ObjectBrowserAccessFailure> {
        let authority = request.retained_dossier.as_ref().ok_or(
            ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "retained dossier authority is missing".to_owned(),
            },
        )?;
        let actor = actor.ok_or(ObjectBrowserAccessFailure::MissingActor)?;
        if actor.username.as_deref() != Some("mnemosyne-expedition") {
            return Err(ObjectBrowserAccessFailure::DelegationNotAllowed {
                peer_actor: actor.display_name(),
            });
        }
        request
            .validate()
            .map_err(|error| ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: error.to_string(),
            })?;
        let now = chrono::DateTime::parse_from_rfc3339(&self.clock.now_utc()).map_err(|_| {
            ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "daemon clock is invalid".to_owned(),
            }
        })?;
        let expires = chrono::DateTime::parse_from_rfc3339(&authority.session_expires_at_utc)
            .map_err(|_| ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "session expiry is invalid".to_owned(),
            })?;
        if expires <= now {
            return Err(ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "verified Pistis session has expired".to_owned(),
            });
        }
        let store_id = resolve_authorization_store_id(
            &request.store_id,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )
        .map_err(ObjectBrowserAccessFailure::Endpoint)?;
        if store_id != request.store_id {
            return Err(ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: "retained dossier writes require an exact ObjectStore".to_owned(),
            });
        }
        Ok(AuthorizedEndpointWrite {
            store_id,
            subobject: None,
            object_prefix: None,
        })
    }

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
        // A Unix UID is transport provenance, not human authority. In
        // particular, root must not manufacture a delegated POSIX actor with
        // storage groups. The legacy envelope remains only for the fixed
        // packaged adapter while its verified-Pistis successor is rolled out.
        if peer_actor.username.as_deref() != Some(DEFAULT_DAEMON_SERVICE_USER) {
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
        // The packaged Web/S3 process is a trusted local adapter: the Unix
        // peer credential proves this dedicated service identity, while the
        // adapter has already bound the request to one authenticated bucket
        // credential. It is the only peer permitted to delegate end-user
        // actors, so this does not introduce a new trust principal.
        if actor.username.as_deref() == Some(DEFAULT_DAEMON_SERVICE_USER) {
            return Ok(store_id);
        }

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
        self.authorize_endpoint_write_scope(actor, endpoint)
            .map(|authorized| authorized.store_id)
    }

    pub(super) fn authorize_endpoint_write_scope(
        &self,
        actor: Option<&DaemonLocalActor>,
        endpoint: &StoreId,
    ) -> Result<AuthorizedEndpointWrite, ObjectBrowserAccessFailure> {
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
        if actor.username.as_deref() == Some(DEFAULT_DAEMON_SERVICE_USER) {
            let subobject = read_subobject_registry(&self.subobject_registry_path)?
                .into_iter()
                .find(|definition| definition.name == endpoint.as_str())
                .map(|definition| (definition.name, definition.path.join("/")));
            return Ok(AuthorizedEndpointWrite {
                store_id,
                subobject: subobject.as_ref().map(|(name, _)| name.clone()),
                object_prefix: subobject.map(|(_, path)| path),
            });
        }

        let mut policy = DaemonStoreAccessPolicy::new(store.store_id.clone());
        if let Some(reader_group) = store.reader_group {
            policy = policy.with_reader_group(reader_group);
        }
        if let Some(writer_group) = store.writer_group {
            policy = policy.with_writer_group(writer_group);
        }
        policy = policy.with_public_read(store.public);
        authorize_store_write(actor, &policy)?;
        let subobject = read_subobject_registry(&self.subobject_registry_path)?
            .into_iter()
            .find(|definition| definition.name == endpoint.as_str())
            .map(|definition| (definition.name, definition.path.join("/")));
        Ok(AuthorizedEndpointWrite {
            store_id,
            subobject: subobject.as_ref().map(|(name, _)| name.clone()),
            object_prefix: subobject.map(|(_, prefix)| prefix),
        })
    }

    pub(super) fn authorize_object_download(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &ObjectDownloadRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        if let Some(store_id) = self.authorize_verified_object_browser_subject(
            actor,
            request.verified_subject.as_ref(),
            &request.endpoint,
            Some(request.object_id.as_str()),
        )? {
            return Ok(store_id);
        }
        self.authorize_endpoint_read(actor, &request.endpoint)
    }

    pub(super) fn authorize_object_folder_download(
        &self,
        actor: Option<&DaemonLocalActor>,
        request: &ObjectFolderDownloadRequest,
    ) -> Result<StoreId, ObjectBrowserAccessFailure> {
        if let Some(store_id) = self.authorize_verified_object_browser_subject(
            actor,
            request.verified_subject.as_ref(),
            &request.endpoint,
            Some(&request.prefix),
        )? {
            return Ok(store_id);
        }
        self.authorize_endpoint_read(actor, &request.endpoint)
    }

    /// A verified browser subject is authority only when it arrived from the
    /// fixed packaged GUI/API peer.  The serialized `peer_identity` field is
    /// intentionally not sufficient on its own: the Unix peer credentials are
    /// the non-forgeable half of this boundary.  Legacy POSIX delegation stays
    /// separate and cannot coexist with this request shape.
    pub(super) fn authorize_verified_object_browser_subject(
        &self,
        peer_actor: Option<&DaemonLocalActor>,
        verified_subject: Option<&crate::api::ObjectBrowserVerifiedSubject>,
        endpoint: &StoreId,
        requested_path: Option<&str>,
    ) -> Result<Option<StoreId>, ObjectBrowserAccessFailure> {
        let Some(verified_subject) = verified_subject else {
            return Ok(None);
        };
        let peer_actor = peer_actor.ok_or(ObjectBrowserAccessFailure::MissingActor)?;
        if peer_actor.username.as_deref() != Some(DEFAULT_DAEMON_SERVICE_USER) {
            return Err(ObjectBrowserAccessFailure::DelegationNotAllowed {
                peer_actor: peer_actor.display_name(),
            });
        }
        verified_subject
            .validate_for_endpoint(endpoint, requested_path)
            .map_err(|error| ObjectBrowserAccessFailure::InvalidVerifiedSubject {
                message: error.to_string(),
            })?;
        let store_id = resolve_authorization_store_id(
            endpoint,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )
        .map_err(ObjectBrowserAccessFailure::Endpoint)?;
        Ok(Some(store_id))
    }
}

#[cfg(test)]
mod tests {
    use super::AuthorizedEndpointWrite;
    use dasobjectstore_core::{backend::BackendObjectKey, ids::StoreId};

    #[test]
    fn subobject_write_qualifies_the_backend_namespace() {
        let authorized = AuthorizedEndpointWrite {
            store_id: StoreId::new("store-main").expect("store id"),
            subobject: Some("project-media".to_string()),
            object_prefix: Some("projects/alpha/media".to_string()),
        };
        let key = BackendObjectKey {
            object_id: "frames/0001.raw".to_string(),
            version: 7,
        };

        assert_eq!(
            authorized.qualify_object(&key),
            BackendObjectKey {
                object_id: "projects/alpha/media/frames/0001.raw".to_string(),
                version: 7,
            }
        );
    }

    #[test]
    fn root_write_preserves_the_backend_namespace() {
        let authorized = AuthorizedEndpointWrite {
            store_id: StoreId::new("store-main").expect("store id"),
            subobject: None,
            object_prefix: None,
        };
        let key = BackendObjectKey {
            object_id: "frames/0001.raw".to_string(),
            version: 7,
        };

        assert_eq!(authorized.qualify_object(&key), key);
    }
}
