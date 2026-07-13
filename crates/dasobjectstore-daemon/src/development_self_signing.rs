//! Explicitly development-only self-signed certificate support.
//!
//! This module is behind the `development-self-signing` Cargo feature. The
//! production daemon and all RPM/DEB build commands use the default feature
//! set, so this code and its private-key material cannot be enabled by a
//! packaged configuration file.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use dasobjectstore_core::application_auth::{
    ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
    ApplicationKeyAlgorithm, ApplicationKeyDescriptor, ApplicationOperation, ApplicationScope,
    APPLICATION_AUTH_SCHEMA_VERSION,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::ingress::IngressOrigin;
use dasobjectstore_core::object_type::ObjectType;
use rcgen::generate_simple_self_signed;
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};

pub const DEVELOPMENT_SELF_SIGNING_MAX_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevelopmentSelfSignedPolicy {
    pub application_id: String,
    pub owner: String,
    pub purpose: String,
    pub synthetic_store_id: StoreId,
    pub synthetic_prefix: String,
    pub object_type: ObjectType,
    pub ingress_origin: IngressOrigin,
    pub max_object_bytes: u64,
    pub max_total_bytes: u64,
    pub token_ttl_seconds: u64,
    pub listener_host: String,
}

impl DevelopmentSelfSignedPolicy {
    pub fn validate(&self) -> Result<(), DevelopmentSelfSigningError> {
        if self.application_id.trim().is_empty()
            || self.owner.trim().is_empty()
            || self.purpose.trim().is_empty()
        {
            return Err(DevelopmentSelfSigningError::BlankField);
        }
        if self.synthetic_prefix.is_empty()
            || self.synthetic_prefix.starts_with('/')
            || self.synthetic_prefix.contains("..")
            || self.synthetic_prefix.contains('\\')
        {
            return Err(DevelopmentSelfSigningError::UnsafeSyntheticPrefix);
        }
        if !matches!(
            self.listener_host.as_str(),
            "127.0.0.1" | "localhost" | "::1"
        ) {
            return Err(DevelopmentSelfSigningError::NonLocalListener(
                self.listener_host.clone(),
            ));
        }
        if self.max_object_bytes == 0
            || self.max_total_bytes == 0
            || self.max_object_bytes > self.max_total_bytes
        {
            return Err(DevelopmentSelfSigningError::InvalidByteBudget);
        }
        if self.token_ttl_seconds == 0
            || self.token_ttl_seconds > DEVELOPMENT_SELF_SIGNING_MAX_TTL_SECONDS
        {
            return Err(DevelopmentSelfSigningError::TokenTtlTooLong);
        }
        Ok(())
    }

    pub fn application_identity(&self, issued_at_unix_seconds: u64) -> ApplicationIdentity {
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: self.application_id.clone(),
            owner: self.owner.clone(),
            purpose: self.purpose.clone(),
            environment: ApplicationEnvironment::Development,
            credential_kind: ApplicationCredentialKind::DevelopmentSelfSigned,
            scope: ApplicationScope {
                store_ids: vec![self.synthetic_store_id.clone()],
                prefixes: vec![self.synthetic_prefix.clone()],
                object_types: vec![self.object_type],
                operations: vec![
                    ApplicationOperation::Read,
                    ApplicationOperation::Write,
                    ApplicationOperation::List,
                    ApplicationOperation::Verify,
                ],
                ingress_origin: self.ingress_origin,
                max_object_bytes: Some(self.max_object_bytes),
                max_total_bytes: Some(self.max_total_bytes),
            },
            issued_at_unix_seconds,
            expires_at_unix_seconds: issued_at_unix_seconds.saturating_add(self.token_ttl_seconds),
            active: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevelopmentSelfSignedMaterial {
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub public_key_fingerprint: String,
    pub public_key_material: String,
}

impl DevelopmentSelfSignedMaterial {
    pub fn generate(
        policy: &DevelopmentSelfSignedPolicy,
    ) -> Result<Self, DevelopmentSelfSigningError> {
        policy.validate()?;
        let certified_key = generate_simple_self_signed(vec!["localhost".to_string()])
            .map_err(|error| DevelopmentSelfSigningError::Certificate(error.to_string()))?;
        let fingerprint = Sha256::digest(certified_key.signing_key.public_key_raw());
        Ok(Self {
            certificate_pem: certified_key.cert.pem(),
            private_key_pem: certified_key.signing_key.serialize_pem(),
            public_key_fingerprint: format!("sha256:{fingerprint:x}"),
            public_key_material: BASE64.encode(certified_key.signing_key.public_key_raw()),
        })
    }

    pub fn key_descriptor(
        &self,
        policy: &DevelopmentSelfSignedPolicy,
        key_id: impl Into<String>,
        issued_at_unix_seconds: u64,
    ) -> Result<ApplicationKeyDescriptor, DevelopmentSelfSigningError> {
        policy.validate()?;
        let descriptor = ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: policy.application_id.clone(),
            key_id: key_id.into(),
            algorithm: ApplicationKeyAlgorithm::EcdsaP256Sha256,
            public_key_fingerprint: self.public_key_fingerprint.clone(),
            public_key_material: Some(self.public_key_material.clone()),
            issued_at_unix_seconds,
            expires_at_unix_seconds: issued_at_unix_seconds
                .saturating_add(policy.token_ttl_seconds),
            active: true,
        };
        descriptor
            .validate()
            .map_err(|error| DevelopmentSelfSigningError::Certificate(error.to_string()))?;
        Ok(descriptor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DevelopmentSelfSigningError {
    BlankField,
    UnsafeSyntheticPrefix,
    NonLocalListener(String),
    InvalidByteBudget,
    TokenTtlTooLong,
    Certificate(String),
}

impl Display for DevelopmentSelfSigningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField => {
                formatter.write_str("development identity fields must not be blank")
            }
            Self::UnsafeSyntheticPrefix => {
                formatter.write_str("development synthetic prefix must be a relative logical path")
            }
            Self::NonLocalListener(host) => {
                write!(
                    formatter,
                    "development self-signing listener must be local: {host}"
                )
            }
            Self::InvalidByteBudget => formatter.write_str("development byte budget is invalid"),
            Self::TokenTtlTooLong => formatter.write_str("development token TTL exceeds 24 hours"),
            Self::Certificate(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DevelopmentSelfSigningError {}

#[cfg(test)]
mod tests {
    use super::{
        DevelopmentSelfSignedMaterial, DevelopmentSelfSignedPolicy,
        DEVELOPMENT_SELF_SIGNING_MAX_TTL_SECONDS,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::ingress::IngressOrigin;
    use dasobjectstore_core::object_type::ObjectType;

    fn policy() -> DevelopmentSelfSignedPolicy {
        DevelopmentSelfSignedPolicy {
            application_id: "codex-dev".to_string(),
            owner: "codex".to_string(),
            purpose: "generated-data tests".to_string(),
            synthetic_store_id: StoreId::new("codex-dev").expect("store"),
            synthetic_prefix: "fixtures".to_string(),
            object_type: ObjectType::Fastq,
            ingress_origin: IngressOrigin::Synoptikon,
            max_object_bytes: 1024,
            max_total_bytes: 8192,
            token_ttl_seconds: 900,
            listener_host: "127.0.0.1".to_string(),
        }
    }

    #[test]
    fn development_certificate_is_bounded_and_never_production_scoped() {
        let policy = policy();
        let material = DevelopmentSelfSignedMaterial::generate(&policy).expect("certificate");
        assert!(material.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(material.private_key_pem.contains("BEGIN PRIVATE KEY"));
        let identity = policy.application_identity(1_000);
        identity.validate().expect("identity");
        assert_eq!(
            identity.environment,
            dasobjectstore_core::application_auth::ApplicationEnvironment::Development
        );
        assert_eq!(
            identity.expires_at_unix_seconds,
            1_000 + policy.token_ttl_seconds
        );
    }

    #[test]
    fn key_descriptor_uses_requested_bounded_ttl() {
        let policy = policy();
        let material = DevelopmentSelfSignedMaterial::generate(&policy).expect("certificate");
        let descriptor = material
            .key_descriptor(&policy, "codex-key", 2_000)
            .expect("descriptor");

        assert_eq!(
            descriptor.expires_at_unix_seconds,
            2_000 + policy.token_ttl_seconds
        );
        assert!(
            descriptor.expires_at_unix_seconds < 2_000 + DEVELOPMENT_SELF_SIGNING_MAX_TTL_SECONDS
        );
    }

    #[test]
    fn non_local_listener_and_unbounded_ttl_are_rejected() {
        let mut policy = policy();
        policy.listener_host = "192.0.2.10".to_string();
        assert!(policy.validate().is_err());
        policy.listener_host = "127.0.0.1".to_string();
        policy.token_ttl_seconds = DEVELOPMENT_SELF_SIGNING_MAX_TTL_SECONDS + 1;
        assert!(policy.validate().is_err());
    }
}
