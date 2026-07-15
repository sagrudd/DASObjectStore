//! Adapter boundary for product-owned storage-policy templates.

use dasobjectstore_core::{StoragePolicyTemplate, StoragePolicyTemplateValidationError};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const PRODUCT_POLICY_TEMPLATE_SCHEMA_VERSION: &str =
    "dasobjectstore.product_policy_template.v1";

/// Product identities with an approved policy-adapter boundary. These
/// identities select ownership validation only; products still provide their
/// own policy values and provisioning decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductPolicyAdapterKind {
    Synoptikon,
    Mneion,
    Mnemosyne,
    Standalone,
}

impl ProductPolicyAdapterKind {
    pub const fn adapter_id(self) -> &'static str {
        match self {
            Self::Synoptikon => "synoptikon",
            Self::Mneion => "mneion",
            Self::Mnemosyne => "mnemosyne",
            Self::Standalone => "standalone",
        }
    }
}

/// Identifies the product adapter validating a template. It does not provide
/// defaults or provisioning; those remain owned by the calling product.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductPolicyTemplateAdapter {
    adapter_id: String,
}

impl ProductPolicyTemplateAdapter {
    pub fn new(adapter_id: impl Into<String>) -> Result<Self, ProductPolicyTemplateAdapterError> {
        let adapter_id = adapter_id.into();
        validate_slug(&adapter_id)?;
        Ok(Self { adapter_id })
    }

    pub fn adapter_id(&self) -> &str {
        &self.adapter_id
    }

    pub fn for_product(kind: ProductPolicyAdapterKind) -> Self {
        Self {
            adapter_id: kind.adapter_id().to_string(),
        }
    }

    pub fn adapt(
        &self,
        template: StoragePolicyTemplate,
    ) -> Result<ProductPolicyTemplateEnvelope, ProductPolicyTemplateAdapterError> {
        template
            .validate()
            .map_err(ProductPolicyTemplateAdapterError::InvalidTemplate)?;
        if template.owner_product != self.adapter_id {
            return Err(ProductPolicyTemplateAdapterError::OwnerMismatch {
                adapter_id: self.adapter_id.clone(),
                owner_product: template.owner_product,
            });
        }
        Ok(ProductPolicyTemplateEnvelope {
            schema_version: PRODUCT_POLICY_TEMPLATE_SCHEMA_VERSION.to_string(),
            adapter_id: self.adapter_id.clone(),
            template,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPolicyTemplateEnvelope {
    pub schema_version: String,
    pub adapter_id: String,
    pub template: StoragePolicyTemplate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProductPolicyTemplateAdapterError {
    InvalidAdapterId,
    InvalidTemplate(StoragePolicyTemplateValidationError),
    OwnerMismatch {
        adapter_id: String,
        owner_product: String,
    },
}

impl Display for ProductPolicyTemplateAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAdapterId => formatter.write_str(
                "adapter ID must be a lowercase ASCII slug containing letters, digits, '.', '_' or '-'",
            ),
            Self::InvalidTemplate(error) => write!(formatter, "invalid policy template: {error}"),
            Self::OwnerMismatch {
                adapter_id,
                owner_product,
            } => write!(
                formatter,
                "adapter `{adapter_id}` cannot adapt template owned by `{owner_product}`"
            ),
        }
    }
}

impl std::error::Error for ProductPolicyTemplateAdapterError {}

fn validate_slug(value: &str) -> Result<(), ProductPolicyTemplateAdapterError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || (!bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit())
        || (!bytes[bytes.len() - 1].is_ascii_lowercase()
            && !bytes[bytes.len() - 1].is_ascii_digit())
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(byte))
    {
        return Err(ProductPolicyTemplateAdapterError::InvalidAdapterId);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ProductPolicyAdapterKind, ProductPolicyTemplateAdapter, ProductPolicyTemplateAdapterError,
        PRODUCT_POLICY_TEMPLATE_SCHEMA_VERSION,
    };
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ingress::IngressOrigin;
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::CapacityPolicy;
    use dasobjectstore_core::StoragePolicyTemplate;

    fn template(owner_product: &str) -> StoragePolicyTemplate {
        StoragePolicyTemplate {
            template_id: "default-bounded".to_string(),
            owner_product: owner_product.to_string(),
            profile: DeploymentProfile::Folder,
            host_mode: HostMode::Integrated,
            protection: ProtectionPolicy::Reproducible,
            capacity: CapacityPolicy::bounded(10_000, 100),
            copies: 1,
            ingress_origin: IngressOrigin::WebUpload,
        }
    }

    #[test]
    fn adapts_owned_template_without_inventing_product_defaults() {
        let adapter = ProductPolicyTemplateAdapter::new("synoptikon").expect("adapter ID is valid");
        let envelope = adapter
            .adapt(template("synoptikon"))
            .expect("template adapts");
        assert_eq!(
            envelope.schema_version,
            PRODUCT_POLICY_TEMPLATE_SCHEMA_VERSION
        );
        assert_eq!(envelope.adapter_id, "synoptikon");
        assert_eq!(envelope.template.capacity.logical_limit_bytes, Some(10_000));
    }

    #[test]
    fn rejects_owner_mismatch_and_invalid_adapter_ids() {
        let adapter = ProductPolicyTemplateAdapter::new("mneion").expect("adapter ID is valid");
        assert!(matches!(
            adapter.adapt(template("synoptikon")),
            Err(ProductPolicyTemplateAdapterError::OwnerMismatch { .. })
        ));
        assert_eq!(
            ProductPolicyTemplateAdapter::new("../mneion"),
            Err(ProductPolicyTemplateAdapterError::InvalidAdapterId)
        );
    }

    #[test]
    fn envelope_serializes_as_strict_versioned_shape() {
        let adapter = ProductPolicyTemplateAdapter::new("mnemosyne").expect("adapter ID is valid");
        let encoded = serde_json::to_value(
            adapter
                .adapt(template("mnemosyne"))
                .expect("template adapts"),
        )
        .expect("envelope serializes");
        assert_eq!(
            encoded["schema_version"],
            PRODUCT_POLICY_TEMPLATE_SCHEMA_VERSION
        );
        assert_eq!(encoded["adapter_id"], "mnemosyne");
        assert_eq!(encoded["template"]["profile"], "folder");
    }

    #[test]
    fn typed_product_adapters_preserve_explicit_ownership_without_defaults() {
        for kind in [
            ProductPolicyAdapterKind::Synoptikon,
            ProductPolicyAdapterKind::Mneion,
            ProductPolicyAdapterKind::Mnemosyne,
            ProductPolicyAdapterKind::Standalone,
        ] {
            let adapter = ProductPolicyTemplateAdapter::for_product(kind);
            assert_eq!(adapter.adapter_id(), kind.adapter_id());
            let envelope = adapter
                .adapt(template(kind.adapter_id()))
                .expect("typed product adapter accepts owned template");
            assert_eq!(envelope.template.owner_product, kind.adapter_id());
            assert_eq!(envelope.template.template_id, "default-bounded");
        }
    }
}
