//! Generic Docker Compose rendering helpers.

use crate::provider::{
    ComposeRenderRequest, ObjectServiceError, ObjectServiceProviderId, RenderedCompose,
    StoreBucketBinding,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposeServiceConfig {
    pub provider_id: ObjectServiceProviderId,
    pub service_name: String,
    pub image: String,
    pub api_port: u16,
}

impl ComposeServiceConfig {
    pub fn new(
        provider_id: ObjectServiceProviderId,
        service_name: impl Into<String>,
        image: impl Into<String>,
        api_port: u16,
    ) -> Self {
        Self {
            provider_id,
            service_name: service_name.into(),
            image: image.into(),
            api_port,
        }
    }
}

pub fn render_compose(
    request: &ComposeRenderRequest,
    service: &ComposeServiceConfig,
) -> Result<RenderedCompose, ObjectServiceError> {
    validate_render_request(request)?;
    validate_service_config(service)?;

    let buckets = request
        .store_bindings
        .iter()
        .map(|binding| binding.bucket_name.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let mut yaml = String::new();
    yaml.push_str(&format!("name: {}\n", request.project_name));
    yaml.push_str("services:\n");
    yaml.push_str(&format!("  {}:\n", service.service_name));
    yaml.push_str(&format!("    image: {}\n", service.image));
    yaml.push_str("    restart: \"no\"\n");
    yaml.push_str("    ports:\n");
    yaml.push_str(&format!(
        "      - \"127.0.0.1:{port}:{port}\"\n",
        port = service.api_port
    ));
    yaml.push_str("    environment:\n");
    yaml.push_str(&format!(
        "      DASOBJECTSTORE_PROVIDER: {}\n",
        service.provider_id
    ));
    yaml.push_str(&format!("      DASOBJECTSTORE_BUCKETS: {}\n", buckets));
    yaml.push_str("    volumes:\n");
    yaml.push_str(&format!(
        "      - {}:/var/lib/dasobjectstore/meta\n",
        request.ssd_metadata_path
    ));
    yaml.push_str(&format!(
        "      - {}:/var/lib/dasobjectstore/data\n",
        request.hdd_data_path
    ));
    yaml.push_str("x-dasobjectstore:\n");
    yaml.push_str(&format!("  provider: {}\n", service.provider_id));
    yaml.push_str("  stores:\n");
    for binding in &request.store_bindings {
        yaml.push_str(&render_store_binding(binding));
    }

    Ok(RenderedCompose {
        provider_id: service.provider_id,
        compose_yaml: yaml,
    })
}

pub(crate) fn validate_render_request(
    request: &ComposeRenderRequest,
) -> Result<(), ObjectServiceError> {
    reject_blank("project_name", &request.project_name)?;
    reject_blank("ssd_metadata_path", &request.ssd_metadata_path)?;
    reject_blank("hdd_data_path", &request.hdd_data_path)?;

    if request.store_bindings.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "at least one store bucket binding is required".to_string(),
        ));
    }

    let mut bucket_names = BTreeSet::new();
    for binding in &request.store_bindings {
        reject_blank("bucket_name", &binding.bucket_name)?;
        reject_blank("credential_reference", &binding.credential_reference)?;
        if !bucket_names.insert(binding.bucket_name.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate bucket binding: {}",
                binding.bucket_name
            )));
        }
    }

    Ok(())
}

fn validate_service_config(service: &ComposeServiceConfig) -> Result<(), ObjectServiceError> {
    reject_blank("service_name", &service.service_name)?;
    reject_blank("image", &service.image)?;

    if service.api_port == 0 {
        return Err(ObjectServiceError::InvalidConfiguration(
            "api_port must be greater than zero".to_string(),
        ));
    }

    Ok(())
}

fn reject_blank(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.trim().is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "{field} must not be blank"
        )));
    }

    Ok(())
}

pub(crate) fn render_store_binding(binding: &StoreBucketBinding) -> String {
    format!(
        "    - store_id: {}\n      class: {}\n      bucket: {}\n      credential_reference: {}\n",
        binding.store_id,
        binding.policy.class.name(),
        binding.bucket_name,
        binding.credential_reference
    )
}

#[cfg(test)]
mod tests {
    use super::{render_compose, ComposeServiceConfig};
    use crate::provider::{ComposeRenderRequest, ObjectServiceProviderId, StoreBucketBinding};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};

    #[test]
    fn renders_compose_from_store_bindings() {
        let request = request(vec![binding(
            "generated",
            StoreClass::GeneratedData,
            "generated-data",
            "secret://generated",
        )]);
        let service = ComposeServiceConfig::new(
            ObjectServiceProviderId::Garage,
            "object-service",
            "example/object-service:1",
            3900,
        );

        let rendered = render_compose(&request, &service).expect("compose renders");

        assert_eq!(rendered.provider_id, ObjectServiceProviderId::Garage);
        assert!(rendered
            .compose_yaml
            .contains("name: dasobjectstore-test\n"));
        assert!(rendered
            .compose_yaml
            .contains("image: example/object-service:1\n"));
        assert!(rendered
            .compose_yaml
            .contains("DASOBJECTSTORE_BUCKETS: generated-data\n"));
        assert!(rendered.compose_yaml.contains("store_id: generated\n"));
        assert!(rendered.compose_yaml.contains("class: generated_data\n"));
    }

    #[test]
    fn rejects_duplicate_buckets() {
        let request = request(vec![
            binding("store-a", StoreClass::GeneratedData, "shared", "secret://a"),
            binding(
                "store-b",
                StoreClass::CriticalMetadata,
                "shared",
                "secret://b",
            ),
        ]);
        let service = ComposeServiceConfig::new(
            ObjectServiceProviderId::Rustfs,
            "rustfs",
            "rustfs:tag",
            9000,
        );

        let err = render_compose(&request, &service).expect_err("duplicate bucket rejected");

        assert!(err.to_string().contains("duplicate bucket binding"));
    }

    #[test]
    fn rejects_missing_store_bindings() {
        let request = request(Vec::new());
        let service = ComposeServiceConfig::new(
            ObjectServiceProviderId::Rustfs,
            "rustfs",
            "rustfs:tag",
            9000,
        );

        let err = render_compose(&request, &service).expect_err("empty bindings rejected");

        assert!(err
            .to_string()
            .contains("at least one store bucket binding"));
    }

    fn request(store_bindings: Vec<StoreBucketBinding>) -> ComposeRenderRequest {
        ComposeRenderRequest {
            project_name: "dasobjectstore-test".to_string(),
            ssd_metadata_path: "/ssd/meta".to_string(),
            hdd_data_path: "/hdd/data".to_string(),
            store_bindings,
        }
    }

    fn binding(
        store_id: &str,
        class: StoreClass,
        bucket_name: &str,
        credential_reference: &str,
    ) -> StoreBucketBinding {
        StoreBucketBinding {
            store_id: StoreId::new(store_id).expect("store id"),
            policy: StorePolicy::defaults_for(class),
            bucket_name: bucket_name.to_string(),
            credential_reference: credential_reference.to_string(),
        }
    }
}
