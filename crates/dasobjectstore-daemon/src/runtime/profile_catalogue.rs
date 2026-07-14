use dasobjectstore_core::backend::{
    BackendError, BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
    ObjectStoreBackend,
};
use dasobjectstore_core::ids::{ObjectId, PlacementId, StoreId};
use dasobjectstore_core::object_catalogue::{
    ObjectDigest, PortableLifecycleState, PortableObjectCatalogue, PortableObjectVersion,
    PortablePlacement, PortablePlacementLocation, PortableProtectionState, PortableProvenance,
    PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
};
use dasobjectstore_core::protection::ProtectionPolicy;

pub trait ProfileCatalogueBackend: ObjectStoreBackend + ObjectCatalogueAuthority {}

impl<T> ProfileCatalogueBackend for T where T: ObjectStoreBackend + ObjectCatalogueAuthority {}

/// Convert a daemon-authoritative backend catalogue into profile-neutral
/// metadata. The payload itself is never copied by this function.
pub fn export_profile_catalogue(
    store_id: &StoreId,
    authority: &dyn ObjectCatalogueAuthority,
) -> Result<PortableObjectCatalogue, BackendError> {
    let mut objects = authority.records()?;
    objects.sort_by(|left, right| {
        left.key
            .object_id
            .cmp(&right.key.object_id)
            .then(left.key.version.cmp(&right.key.version))
    });
    let objects = objects
        .into_iter()
        .map(|record| portable_object(store_id, record))
        .collect::<Result<Vec<_>, _>>()?;
    let catalogue = PortableObjectCatalogue {
        schema_version: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
        store_id: store_id.clone(),
        objects,
    };
    catalogue.validate().map_err(|error| {
        BackendError::InvalidRequest(format!("portable catalogue export: {error}"))
    })?;
    Ok(catalogue)
}

/// Verify destination payloads before committing imported catalogue rows.
/// This is intentionally metadata-only: source retirement remains a separate
/// operator-confirmed migration transition.
pub fn import_profile_catalogue(
    store_id: &StoreId,
    catalogue: &PortableObjectCatalogue,
    backend: &mut dyn ProfileCatalogueBackend,
) -> Result<u64, BackendError> {
    catalogue.validate().map_err(|error| {
        BackendError::InvalidRequest(format!("portable catalogue import: {error}"))
    })?;
    if catalogue.store_id != *store_id {
        return Err(BackendError::InvalidRequest(
            "portable catalogue store identity does not match destination".to_string(),
        ));
    }
    let mut records = Vec::with_capacity(catalogue.objects.len());
    for object in &catalogue.objects {
        let Some(placement) = object.placements.first() else {
            return Err(BackendError::InvalidRequest(format!(
                "portable object {} has no placement",
                object.object_id
            )));
        };
        let key = BackendObjectKey {
            object_id: object.object_id.to_string(),
            version: object.version,
        };
        let verified = backend.verify(&key)?;
        let expected_checksum = digest_value(&object.checksum)?;
        let placement_checksum = digest_value(&placement.checksum)?;
        if verified.size_bytes != object.size_bytes
            || verified.checksum != expected_checksum
            || placement_checksum != verified.checksum
        {
            return Err(BackendError::InvalidRequest(format!(
                "portable object {}:{} does not match destination payload",
                key.object_id, key.version
            )));
        }
        records.push(verified);
    }
    backend.commit_batch(&records)?;
    Ok(records.len() as u64)
}

fn portable_object(
    store_id: &StoreId,
    record: BackendObjectRecord,
) -> Result<PortableObjectVersion, BackendError> {
    let object_id = ObjectId::new(record.key.object_id.clone())
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    let placement_id = PlacementId::new(format!("{}-{}", record.key.object_id, record.key.version))
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    let digest = parse_digest(&record.checksum)?;
    Ok(PortableObjectVersion {
        object_id,
        version: record.key.version,
        size_bytes: record.size_bytes,
        checksum: digest.clone(),
        provenance: PortableProvenance {
            source_kind: "profile_backend".to_string(),
            locator: Some(format!("{store_id}/{}", record.key.object_id)),
            revision: Some(record.key.version.to_string()),
        },
        lifecycle: PortableLifecycleState::HashVerified,
        protection_policy: ProtectionPolicy::LocalOnly,
        protection_state: PortableProtectionState::Verified,
        placements: vec![PortablePlacement {
            placement_id,
            location: PortablePlacementLocation::Folder {
                relative_path: record.key.object_id,
            },
            checksum: digest,
            verified_at_utc: None,
        }],
    })
}

fn parse_digest(value: &str) -> Result<ObjectDigest, BackendError> {
    let Some((algorithm, digest)) = value.split_once(':') else {
        return Err(BackendError::InvalidRequest(
            "portable catalogue checksums must use algorithm:value form".to_string(),
        ));
    };
    if algorithm.trim().is_empty() || digest.trim().is_empty() {
        return Err(BackendError::InvalidRequest(
            "portable catalogue checksum must not be blank".to_string(),
        ));
    }
    Ok(ObjectDigest {
        algorithm: algorithm.to_string(),
        value: digest.to_string(),
    })
}

fn digest_value(digest: &ObjectDigest) -> Result<String, BackendError> {
    if digest.algorithm.trim().is_empty() || digest.value.trim().is_empty() {
        return Err(BackendError::InvalidRequest(
            "portable catalogue checksum must not be blank".to_string(),
        ));
    }
    Ok(format!("{}:{}", digest.algorithm, digest.value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::backend::{BackendCapabilities, BackendHealth};
    use dasobjectstore_core::manifest::ObjectStoreManifest;
    use std::io::{Cursor, Read};

    struct FakeBackend {
        record: BackendObjectRecord,
    }

    impl ObjectStoreBackend for FakeBackend {
        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities::complete()
        }
        fn validate_manifest(&self, _: &ObjectStoreManifest) -> Result<(), BackendError> {
            Ok(())
        }
        fn reserve(&mut self, _: &str, _: u64) -> Result<(), BackendError> {
            Ok(())
        }
        fn stage(
            &mut self,
            _: &str,
            _: &BackendObjectKey,
            _: &mut dyn Read,
        ) -> Result<BackendObjectRecord, BackendError> {
            Ok(self.record.clone())
        }
        fn finalize(
            &mut self,
            staged: BackendObjectRecord,
        ) -> Result<BackendObjectRecord, BackendError> {
            Ok(staged)
        }
        fn read(&self, _: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError> {
            Ok(Box::new(Cursor::new(Vec::<u8>::new())))
        }
        fn enumerate(&self, _: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError> {
            Ok(vec![self.record.clone()])
        }
        fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError> {
            if key == &self.record.key {
                Ok(self.record.clone())
            } else {
                Err(BackendError::NotFound(key.object_id.clone()))
            }
        }
        fn reconcile(&mut self) -> Result<Vec<BackendObjectRecord>, BackendError> {
            Ok(vec![self.record.clone()])
        }
        fn remove(&mut self, _: &BackendObjectKey) -> Result<(), BackendError> {
            Ok(())
        }
        fn health(&self) -> Result<BackendHealth, BackendError> {
            Ok(BackendHealth {
                state: "ready".to_string(),
                message: None,
            })
        }
    }

    impl ObjectCatalogueAuthority for FakeBackend {
        fn records(&self) -> Result<Vec<BackendObjectRecord>, BackendError> {
            Ok(vec![self.record.clone()])
        }
        fn commit_batch(&mut self, records: &[BackendObjectRecord]) -> Result<(), BackendError> {
            assert_eq!(records, &[self.record.clone()]);
            Ok(())
        }
        fn remove_record(&mut self, _: &BackendObjectKey) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[test]
    fn export_then_import_verifies_destination_and_retains_source() {
        let store_id = StoreId::new("codex").expect("store");
        let record = BackendObjectRecord {
            key: BackendObjectKey {
                object_id: "reads/a.txt".to_string(),
                version: 1,
            },
            size_bytes: 4,
            checksum: "sha256:abcd".to_string(),
            location: ".dasobjectstore/objects/reads/a.txt".to_string(),
        };
        let mut backend = FakeBackend { record };
        let catalogue = export_profile_catalogue(&store_id, &backend).expect("export");
        assert_eq!(catalogue.objects.len(), 1);
        let imported =
            import_profile_catalogue(&store_id, &catalogue, &mut backend).expect("import");
        assert_eq!(imported, 1);
        assert_eq!(catalogue.store_id, store_id);
    }
}
