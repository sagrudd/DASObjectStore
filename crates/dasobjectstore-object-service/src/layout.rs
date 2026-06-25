//! Store-to-bucket service layout planning.

use crate::credentials::{credential_reference_for_store, StoreCredentialRequest};
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

fn bucket_name_for_definition(
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

fn validate_bucket_name(bucket_name: &str) -> Result<(), ObjectServiceError> {
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

    fn definition(store_id: &str, policy: StorePolicy) -> StoreServiceDefinition {
        StoreServiceDefinition {
            store_id: StoreId::new(store_id).expect("store id"),
            policy,
            bucket_name: None,
        }
    }
}
