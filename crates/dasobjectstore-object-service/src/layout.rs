//! Store-to-bucket service layout planning.

use crate::credentials::{credential_reference_for_store, StoreCredentialRequest};
use crate::custody::{custody_bucket_is_reserved, CustodyStoreProfileV1};
use crate::provider::{ObjectServiceError, StoreBucketBinding};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::{ExportPolicy, StorePolicy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const BUCKET_PREFIX: &str = "dos";
const MAX_BUCKET_NAME_LEN: usize = 63;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreServiceDefinition {
    pub store_id: StoreId,
    pub policy: StorePolicy,
    pub bucket_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer_group: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub public: bool,
    /// A separate, sealed custody profile.  It is deliberately excluded from
    /// the ordinary store layout: that path creates one owner-capable Garage
    /// key and is therefore not permitted for a custody store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custody_profile: Option<CustodyStoreProfileV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreServiceLayout {
    pub credential_requests: Vec<StoreCredentialRequest>,
    pub bucket_bindings: Vec<StoreBucketBinding>,
}

pub fn plan_store_service_layout(
    definitions: &[StoreServiceDefinition],
) -> Result<StoreServiceLayout, ObjectServiceError> {
    if definitions.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "at least one store definition is required".to_string(),
        ));
    }

    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    let mut credential_requests = Vec::new();
    let mut bucket_bindings = Vec::new();

    for definition in definitions {
        validate_custody_definition(definition)?;
        if definition.custody_profile.is_some() {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "custody store {} is excluded from the normal layout and owner-capable Garage provisioner",
                definition.store_id
            )));
        }
        if !store_ids.insert(definition.store_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate store definition: {}",
                definition.store_id
            )));
        }

        if definition.policy.export_policy != ExportPolicy::S3 {
            continue;
        }

        let bucket_name = bucket_name_for_definition(definition)?;
        if !bucket_names.insert(bucket_name.as_str().to_string()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate bucket name: {bucket_name}"
            )));
        }

        credential_requests.push(StoreCredentialRequest {
            store_id: definition.store_id.clone(),
            bucket_name: bucket_name.clone(),
        });
        bucket_bindings.push(StoreBucketBinding {
            store_id: definition.store_id.clone(),
            policy: definition.policy.clone(),
            bucket_name,
            credential_reference: credential_reference_for_store(&definition.store_id),
        });
    }

    if bucket_bindings.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "at least one S3-exported store definition is required".to_string(),
        ));
    }

    Ok(StoreServiceLayout {
        credential_requests,
        bucket_bindings,
    })
}

/// Reject configurations which could make the normal mutable store path look
/// like a custody path.  A custody bucket is provisioned only by the dedicated
/// custody workflow, with distinct reader and writer identities and without
/// an owner grant.
pub fn validate_custody_definition(
    definition: &StoreServiceDefinition,
) -> Result<(), ObjectServiceError> {
    let Some(profile) = &definition.custody_profile else {
        return Ok(());
    };

    profile.validate()?;
    if definition.policy.export_policy != ExportPolicy::S3 {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "custody store {} must use S3 export policy",
            definition.store_id
        )));
    }
    if definition.public {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "custody store {} must not be public",
            definition.store_id
        )));
    }
    if definition.reader_group.is_some() || definition.writer_group.is_some() {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "custody store {} must use dedicated custody reader and writer identities",
            definition.store_id
        )));
    }
    let bucket_name = definition.bucket_name.as_deref().ok_or_else(|| {
        ObjectServiceError::InvalidConfiguration(format!(
            "custody store {} must specify a fresh explicit bucket name",
            definition.store_id
        ))
    })?;
    if custody_bucket_is_reserved(bucket_name) {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "custody store {} cannot use the retired r237 bootstrap bucket {bucket_name}",
            definition.store_id
        )));
    }
    if definition.store_id.as_str() == "r237_s4_bootstrap_custody" {
        return Err(ObjectServiceError::InvalidConfiguration(
            "the retired r237 bootstrap store can never be adopted as custody".to_string(),
        ));
    }
    Ok(())
}

pub fn bucket_name_for_definition(
    definition: &StoreServiceDefinition,
) -> Result<String, ObjectServiceError> {
    match &definition.bucket_name {
        Some(bucket_name) => {
            validate_bucket_name(bucket_name)?;
            Ok(bucket_name.clone())
        }
        None => Ok(default_bucket_name(&definition.store_id)),
    }
}

fn default_bucket_name(store_id: &StoreId) -> String {
    let mut bucket = String::from(BUCKET_PREFIX);
    bucket.push('-');
    bucket.push_str(&sanitize_bucket_component(store_id.as_str()));
    bucket.truncate(MAX_BUCKET_NAME_LEN);
    bucket.trim_end_matches('-').to_string()
}

fn sanitize_bucket_component(value: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_hyphen = false;

    for character in value.chars().flat_map(char::to_lowercase) {
        let next = if character.is_ascii_alphanumeric() {
            character
        } else {
            '-'
        };

        if next == '-' {
            if !last_was_hyphen && !sanitized.is_empty() {
                sanitized.push(next);
            }
            last_was_hyphen = true;
        } else {
            sanitized.push(next);
            last_was_hyphen = false;
        }
    }

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "store".to_string()
    } else {
        sanitized.to_string()
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub(crate) fn validate_bucket_name(bucket_name: &str) -> Result<(), ObjectServiceError> {
    if bucket_name.len() < 3 || bucket_name.len() > MAX_BUCKET_NAME_LEN {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "bucket name `{bucket_name}` must be 3 to 63 characters"
        )));
    }

    if bucket_name.starts_with('-') || bucket_name.ends_with('-') {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "bucket name `{bucket_name}` must not start or end with hyphen"
        )));
    }

    if !bucket_name.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "bucket name `{bucket_name}` must contain only lowercase letters, digits, or hyphens"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{plan_store_service_layout, StoreServiceDefinition};
    use crate::custody::{
        CustodyAssuranceClass, CustodyRetentionMode, CustodyStoreProfileV1,
        CUSTODY_OVERLAY_SCHEMA_V1, CUSTODY_PROFILE_V1,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};

    #[test]
    fn maps_s3_store_definitions_to_bucket_bindings_and_credentials() {
        let definitions = vec![
            definition(
                "Generated_Data",
                StorePolicy::defaults_for(StoreClass::GeneratedData),
            ),
            definition(
                "Critical.Metadata",
                StorePolicy::defaults_for(StoreClass::CriticalMetadata),
            ),
        ];

        let layout = plan_store_service_layout(&definitions).expect("layout planned");

        assert_eq!(layout.credential_requests.len(), 2);
        assert_eq!(layout.bucket_bindings.len(), 2);
        assert_eq!(layout.bucket_bindings[0].bucket_name, "dos-generated-data");
        assert_eq!(
            layout.bucket_bindings[0].credential_reference,
            "secret://dasobjectstore/stores/Generated_Data/s3"
        );
        assert_eq!(
            layout.credential_requests[1].bucket_name,
            "dos-critical-metadata"
        );
    }

    #[test]
    fn skips_non_s3_exported_stores() {
        let definitions = vec![
            definition(
                "generated",
                StorePolicy::defaults_for(StoreClass::GeneratedData),
            ),
            definition(
                "export",
                StorePolicy::defaults_for(StoreClass::ExportBundle),
            ),
            definition(
                "staging",
                StorePolicy::defaults_for(StoreClass::IngestStaging),
            ),
        ];

        let layout = plan_store_service_layout(&definitions).expect("layout planned");

        assert_eq!(layout.bucket_bindings.len(), 1);
        assert_eq!(layout.bucket_bindings[0].store_id.as_str(), "generated");
    }

    #[test]
    fn accepts_valid_explicit_bucket_name() {
        let mut store = definition(
            "generated",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        store.bucket_name = Some("custom-generated-data".to_string());

        let layout = plan_store_service_layout(&[store]).expect("layout planned");

        assert_eq!(
            layout.bucket_bindings[0].bucket_name,
            "custom-generated-data"
        );
    }

    #[test]
    fn rejects_invalid_explicit_bucket_name() {
        let mut store = definition(
            "generated",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        store.bucket_name = Some("Invalid_Bucket".to_string());

        let err = plan_store_service_layout(&[store]).expect_err("invalid bucket rejected");

        assert!(err.to_string().contains("must contain only lowercase"));
    }

    #[test]
    fn rejects_duplicate_store_definitions() {
        let definitions = vec![
            definition(
                "generated",
                StorePolicy::defaults_for(StoreClass::GeneratedData),
            ),
            definition(
                "generated",
                StorePolicy::defaults_for(StoreClass::CriticalMetadata),
            ),
        ];

        let err = plan_store_service_layout(&definitions).expect_err("duplicate store rejected");

        assert!(err.to_string().contains("duplicate store definition"));
    }

    #[test]
    fn rejects_layout_without_s3_stores() {
        let definitions = vec![definition(
            "export",
            StorePolicy::defaults_for(StoreClass::ExportBundle),
        )];

        let err = plan_store_service_layout(&definitions).expect_err("missing s3 store rejected");

        assert!(err
            .to_string()
            .contains("at least one S3-exported store definition"));
    }

    #[test]
    fn custody_profile_is_hard_denied_before_normal_owner_credential_layout() {
        let mut store = definition(
            "formal-custody",
            StorePolicy::defaults_for(StoreClass::CriticalMetadata),
        );
        store.bucket_name = Some("dos-formal-custody".to_string());
        store.custody_profile = Some(custody_profile());

        let error = plan_store_service_layout(&[store])
            .expect_err("normal store layout must not issue a custody credential");
        assert!(error.to_string().contains("owner-capable"));
    }

    #[test]
    fn custody_profile_rejects_retired_bootstrap_namespace() {
        let mut store = definition(
            "r237_s4_bootstrap_custody",
            StorePolicy::defaults_for(StoreClass::CriticalMetadata),
        );
        store.bucket_name = Some("dos-r237-s4-bootstrap-custody".to_string());
        store.custody_profile = Some(custody_profile());

        assert!(plan_store_service_layout(&[store]).is_err());
    }

    fn definition(store_id: &str, policy: StorePolicy) -> StoreServiceDefinition {
        StoreServiceDefinition {
            store_id: StoreId::new(store_id).expect("store id"),
            policy,
            bucket_name: None,
            reader_group: None,
            writer_group: None,
            public: false,
            custody_profile: None,
        }
    }

    fn custody_profile() -> CustodyStoreProfileV1 {
        CustodyStoreProfileV1 {
            schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
            profile: CUSTODY_PROFILE_V1.to_string(),
            assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
            retention_mode: CustodyRetentionMode::LocalTrustedAdministratorOverlay,
            target_id: "nuc-192.168.0.193".to_string(),
            retention_until_utc: "2027-09-05T10:00:00Z".to_string(),
            legal_hold: true,
            writer_credential_reference: "secret://custody/writer".to_string(),
            reader_credential_reference: "secret://custody/reader".to_string(),
            reader_identity: "custody-reader-v1".to_string(),
        }
    }
}
