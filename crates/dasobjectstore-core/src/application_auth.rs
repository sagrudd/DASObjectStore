//! Versioned application identities and least-privilege token contracts.
//!
//! These types deliberately describe claims and policy, never secret key
//! material. Cryptographic signing, key custody, token exchange, and daemon
//! authorization are layered above this path-free core contract.

use crate::ids::StoreId;
use crate::ingress::IngressOrigin;
use crate::object_type::ObjectType;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const APPLICATION_AUTH_SCHEMA_VERSION: &str = "dasobjectstore.application_auth.v1";
pub const MAX_ACCESS_TOKEN_TTL_SECONDS: u64 = 15 * 60;
pub const MAX_UPLOAD_COMPLETION_TTL_SECONDS: u64 = 15 * 60;
pub const MAX_DEVELOPMENT_ACCESS_TOKEN_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationEnvironment {
    Production,
    Development,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationCredentialKind {
    AsymmetricKey,
    MtlsCertificate,
    DevelopmentSelfSigned,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationKeyAlgorithm {
    Ed25519,
    EcdsaP256Sha256,
    MtlsCertificate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationKeyDescriptor {
    pub schema_version: String,
    pub application_id: String,
    pub key_id: String,
    pub algorithm: ApplicationKeyAlgorithm,
    pub public_key_fingerprint: String,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub active: bool,
}

impl ApplicationKeyDescriptor {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        validate_schema(&self.schema_version)?;
        validate_slug("application_id", &self.application_id)?;
        validate_slug("key_id", &self.key_id)?;
        validate_sha256_fingerprint(&self.public_key_fingerprint, "public_key_fingerprint")?;
        validate_lifetime(self.issued_at_unix_seconds, self.expires_at_unix_seconds)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationOperation {
    Read,
    Write,
    List,
    Verify,
    CompleteUpload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationScope {
    pub store_ids: Vec<StoreId>,
    #[serde(default)]
    pub prefixes: Vec<String>,
    #[serde(default)]
    pub object_types: Vec<ObjectType>,
    pub operations: Vec<ApplicationOperation>,
    pub ingress_origin: IngressOrigin,
    pub max_object_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
}

impl ApplicationScope {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        if self.store_ids.is_empty() {
            return Err(ApplicationAuthValidationError::EmptyScope("store_ids"));
        }
        if has_duplicates(&self.store_ids) {
            return Err(ApplicationAuthValidationError::DuplicateScope("store_ids"));
        }
        if self.operations.is_empty() {
            return Err(ApplicationAuthValidationError::EmptyScope("operations"));
        }
        if has_duplicates(&self.operations) {
            return Err(ApplicationAuthValidationError::DuplicateScope("operations"));
        }
        if has_duplicates(&self.object_types) {
            return Err(ApplicationAuthValidationError::DuplicateScope(
                "object_types",
            ));
        }
        for prefix in &self.prefixes {
            validate_logical_path("prefix", prefix)?;
        }
        if has_duplicates(&self.prefixes) {
            return Err(ApplicationAuthValidationError::DuplicateScope("prefixes"));
        }
        if let (Some(max_object), Some(max_total)) = (self.max_object_bytes, self.max_total_bytes) {
            if max_object > max_total {
                return Err(ApplicationAuthValidationError::Invalid(
                    "max_object_bytes cannot exceed max_total_bytes".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn contains(&self, requested: &Self) -> bool {
        requested
            .store_ids
            .iter()
            .all(|store| self.store_ids.iter().any(|allowed| allowed == store))
            && requested
                .operations
                .iter()
                .all(|operation| self.operations.iter().any(|allowed| allowed == operation))
            && list_scope_contains(
                &self.object_types,
                &requested.object_types,
                |allowed, value| allowed == value,
            )
            && list_scope_contains(&self.prefixes, &requested.prefixes, |allowed, value| {
                prefix_contains(allowed, value)
            })
            && (self.ingress_origin == requested.ingress_origin)
            && limit_contains(self.max_object_bytes, requested.max_object_bytes)
            && limit_contains(self.max_total_bytes, requested.max_total_bytes)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationIdentity {
    pub schema_version: String,
    pub application_id: String,
    pub owner: String,
    pub purpose: String,
    pub environment: ApplicationEnvironment,
    pub credential_kind: ApplicationCredentialKind,
    pub scope: ApplicationScope,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub active: bool,
}

impl ApplicationIdentity {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        validate_schema(&self.schema_version)?;
        validate_slug("application_id", &self.application_id)?;
        validate_text("owner", &self.owner)?;
        validate_text("purpose", &self.purpose)?;
        validate_lifetime(self.issued_at_unix_seconds, self.expires_at_unix_seconds)?;
        if self.credential_kind == ApplicationCredentialKind::DevelopmentSelfSigned
            && self.environment != ApplicationEnvironment::Development
        {
            return Err(ApplicationAuthValidationError::Invalid(
                "development self-signed credentials require development environment".to_string(),
            ));
        }
        self.scope.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessTokenClaims {
    pub schema_version: String,
    pub token_id: String,
    pub application_id: String,
    pub audience: String,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub scope: ApplicationScope,
}

impl AccessTokenClaims {
    pub fn validate_against(
        &self,
        identity: &ApplicationIdentity,
    ) -> Result<(), ApplicationAuthValidationError> {
        validate_schema(&self.schema_version)?;
        validate_slug("token_id", &self.token_id)?;
        validate_slug("application_id", &self.application_id)?;
        validate_text("audience", &self.audience)?;
        validate_lifetime(self.issued_at_unix_seconds, self.expires_at_unix_seconds)?;
        identity.validate()?;
        if !identity.active {
            return Err(ApplicationAuthValidationError::InactiveIdentity);
        }
        if self.application_id != identity.application_id {
            return Err(ApplicationAuthValidationError::IdentityMismatch);
        }
        if self.issued_at_unix_seconds < identity.issued_at_unix_seconds
            || self.expires_at_unix_seconds > identity.expires_at_unix_seconds
        {
            return Err(ApplicationAuthValidationError::LifetimeOutsideIdentity);
        }
        let max_ttl = if identity.environment == ApplicationEnvironment::Development {
            MAX_DEVELOPMENT_ACCESS_TOKEN_TTL_SECONDS
        } else {
            MAX_ACCESS_TOKEN_TTL_SECONDS
        };
        if self.expires_at_unix_seconds - self.issued_at_unix_seconds > max_ttl {
            return Err(ApplicationAuthValidationError::TokenTtlTooLong {
                max_seconds: max_ttl,
            });
        }
        self.scope.validate()?;
        if !identity.scope.contains(&self.scope) {
            return Err(ApplicationAuthValidationError::ScopeNotContained);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenewalTokenClaims {
    pub schema_version: String,
    pub token_id: String,
    pub application_id: String,
    pub audience: String,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub nonce: String,
}

impl RenewalTokenClaims {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        validate_schema(&self.schema_version)?;
        validate_slug("token_id", &self.token_id)?;
        validate_slug("application_id", &self.application_id)?;
        validate_text("audience", &self.audience)?;
        validate_slug("nonce", &self.nonce)?;
        validate_lifetime(self.issued_at_unix_seconds, self.expires_at_unix_seconds)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadCompletionCapability {
    pub schema_version: String,
    pub capability_id: String,
    pub application_id: String,
    pub session_id: String,
    pub upload_id: String,
    pub store_id: StoreId,
    pub object_key: String,
    pub expected_size_bytes: u64,
    pub expected_checksum: String,
    pub audience: String,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub nonce: String,
}

impl UploadCompletionCapability {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        validate_schema(&self.schema_version)?;
        for (field, value) in [
            ("capability_id", self.capability_id.as_str()),
            ("application_id", self.application_id.as_str()),
            ("session_id", self.session_id.as_str()),
            ("upload_id", self.upload_id.as_str()),
            ("nonce", self.nonce.as_str()),
        ] {
            validate_slug(field, value)?;
        }
        validate_text("audience", &self.audience)?;
        validate_logical_path("object_key", &self.object_key)?;
        if !self.expected_checksum.starts_with("sha256:")
            || self.expected_checksum.len() != "sha256:".len() + 64
            || !self.expected_checksum["sha256:".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ApplicationAuthValidationError::Invalid(
                "expected_checksum must be a sha256 digest".to_string(),
            ));
        }
        validate_lifetime(self.issued_at_unix_seconds, self.expires_at_unix_seconds)?;
        if self.expires_at_unix_seconds - self.issued_at_unix_seconds
            > MAX_UPLOAD_COMPLETION_TTL_SECONDS
        {
            return Err(ApplicationAuthValidationError::TokenTtlTooLong {
                max_seconds: MAX_UPLOAD_COMPLETION_TTL_SECONDS,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationAuthValidationError {
    UnsupportedSchema,
    EmptyField(&'static str),
    UnsafeField(&'static str),
    EmptyScope(&'static str),
    DuplicateScope(&'static str),
    InvalidLifetime,
    LifetimeOutsideIdentity,
    TokenTtlTooLong { max_seconds: u64 },
    InactiveIdentity,
    IdentityMismatch,
    ScopeNotContained,
    Invalid(String),
}

impl Display for ApplicationAuthValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema => formatter.write_str("unsupported application auth schema"),
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::UnsafeField(field) => write!(formatter, "{field} contains unsafe characters"),
            Self::EmptyScope(field) => write!(formatter, "scope {field} must not be empty"),
            Self::DuplicateScope(field) => write!(formatter, "scope {field} contains duplicates"),
            Self::InvalidLifetime => formatter.write_str("token lifetime is invalid"),
            Self::LifetimeOutsideIdentity => {
                formatter.write_str("token lifetime exceeds its application identity")
            }
            Self::TokenTtlTooLong { max_seconds } => {
                write!(formatter, "token TTL exceeds {max_seconds} seconds")
            }
            Self::InactiveIdentity => formatter.write_str("application identity is inactive"),
            Self::IdentityMismatch => formatter.write_str("token application identity mismatch"),
            Self::ScopeNotContained => {
                formatter.write_str("token scope exceeds its application identity")
            }
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ApplicationAuthValidationError {}

fn validate_schema(schema_version: &str) -> Result<(), ApplicationAuthValidationError> {
    if schema_version == APPLICATION_AUTH_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ApplicationAuthValidationError::UnsupportedSchema)
    }
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ApplicationAuthValidationError> {
    if value.trim().is_empty() {
        return Err(ApplicationAuthValidationError::EmptyField(field));
    }
    if value.chars().any(|character| character.is_control()) || value.len() > 256 {
        return Err(ApplicationAuthValidationError::UnsafeField(field));
    }
    Ok(())
}

fn validate_slug(field: &'static str, value: &str) -> Result<(), ApplicationAuthValidationError> {
    if value.is_empty() {
        return Err(ApplicationAuthValidationError::EmptyField(field));
    }
    if value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(ApplicationAuthValidationError::UnsafeField(field));
    }
    Ok(())
}

fn validate_logical_path(
    field: &'static str,
    value: &str,
) -> Result<(), ApplicationAuthValidationError> {
    if value.is_empty() {
        return Ok(());
    }
    if value.starts_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ApplicationAuthValidationError::UnsafeField(field));
    }
    Ok(())
}

fn validate_lifetime(
    issued_at_unix_seconds: u64,
    expires_at_unix_seconds: u64,
) -> Result<(), ApplicationAuthValidationError> {
    if expires_at_unix_seconds <= issued_at_unix_seconds {
        Err(ApplicationAuthValidationError::InvalidLifetime)
    } else {
        Ok(())
    }
}

fn validate_sha256_fingerprint(
    value: &str,
    field: &'static str,
) -> Result<(), ApplicationAuthValidationError> {
    if !value.starts_with("sha256:")
        || value.len() != "sha256:".len() + 64
        || !value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApplicationAuthValidationError::Invalid(format!(
            "{field} must be a sha256 fingerprint"
        )));
    }
    Ok(())
}

fn prefix_contains(allowed: &str, requested: &str) -> bool {
    allowed.is_empty() || requested == allowed || requested.starts_with(&format!("{allowed}/"))
}

fn list_scope_contains<T, F>(allowed: &[T], requested: &[T], contains: F) -> bool
where
    F: Fn(&T, &T) -> bool,
{
    allowed.is_empty()
        || (!requested.is_empty()
            && requested.iter().all(|requested_value| {
                allowed
                    .iter()
                    .any(|allowed_value| contains(allowed_value, requested_value))
            }))
}

fn limit_contains(allowed: Option<u64>, requested: Option<u64>) -> bool {
    match (allowed, requested) {
        (Some(allowed), Some(requested)) => requested <= allowed,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn has_duplicates<T: Eq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].iter().any(|previous| previous == value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> ApplicationScope {
        ApplicationScope {
            store_ids: vec![StoreId::new("codex").expect("store")],
            prefixes: vec!["analysis".to_string()],
            object_types: vec![ObjectType::Fastq],
            operations: vec![
                ApplicationOperation::Write,
                ApplicationOperation::CompleteUpload,
            ],
            ingress_origin: IngressOrigin::Synoptikon,
            max_object_bytes: Some(10_000),
            max_total_bytes: Some(100_000),
        }
    }

    fn identity() -> ApplicationIdentity {
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: "synoptikon-ingest".to_string(),
            owner: "mnemosyne".to_string(),
            purpose: "sequencing ingest".to_string(),
            environment: ApplicationEnvironment::Production,
            credential_kind: ApplicationCredentialKind::AsymmetricKey,
            scope: scope(),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        }
    }

    #[test]
    fn identity_and_scoped_access_token_round_trip_without_secrets_or_paths() {
        let identity = identity();
        identity.validate().expect("identity");
        let token = AccessTokenClaims {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            token_id: "access-1".to_string(),
            application_id: identity.application_id.clone(),
            audience: "dasobjectstore".to_string(),
            issued_at_unix_seconds: 2_000,
            expires_at_unix_seconds: 2_600,
            scope: identity.scope.clone(),
        };
        token.validate_against(&identity).expect("token");
        let encoded = serde_json::to_string(&token).expect("encode");
        assert!(!encoded.contains("/srv"));
        assert!(!encoded.contains("private_key"));
        let decoded: AccessTokenClaims = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, token);
    }

    #[test]
    fn access_token_scope_must_be_contained_and_short_lived() {
        let identity = identity();
        let mut token = AccessTokenClaims {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            token_id: "access-1".to_string(),
            application_id: identity.application_id.clone(),
            audience: "dasobjectstore".to_string(),
            issued_at_unix_seconds: 2_000,
            expires_at_unix_seconds: 2_600,
            scope: identity.scope.clone(),
        };
        token.scope.prefixes = vec!["other".to_string()];
        assert_eq!(
            token.validate_against(&identity),
            Err(ApplicationAuthValidationError::ScopeNotContained)
        );
        token.scope.prefixes = identity.scope.prefixes.clone();
        token.scope.object_types = Vec::new();
        assert_eq!(
            token.validate_against(&identity),
            Err(ApplicationAuthValidationError::ScopeNotContained)
        );
        token.scope = identity.scope.clone();
        token.expires_at_unix_seconds =
            token.issued_at_unix_seconds + MAX_ACCESS_TOKEN_TTL_SECONDS + 1;
        assert_eq!(
            token.validate_against(&identity),
            Err(ApplicationAuthValidationError::TokenTtlTooLong {
                max_seconds: MAX_ACCESS_TOKEN_TTL_SECONDS
            })
        );
    }

    #[test]
    fn renewal_claims_carry_no_storage_scope() {
        let renewal = RenewalTokenClaims {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            token_id: "renew-1".to_string(),
            application_id: "synoptikon-ingest".to_string(),
            audience: "dasobjectstore-token".to_string(),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 10_000,
            nonce: "nonce-1".to_string(),
        };
        renewal.validate().expect("renewal");
        let encoded = serde_json::to_string(&renewal).expect("encode");
        assert!(!encoded.contains("operations"));
        assert!(!encoded.contains("store_ids"));
    }

    #[test]
    fn completion_capability_is_upload_bound_and_has_short_lifetime() {
        let capability = UploadCompletionCapability {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            capability_id: "cap-1".to_string(),
            application_id: "synoptikon-ingest".to_string(),
            session_id: "session-1".to_string(),
            upload_id: "upload-1".to_string(),
            store_id: StoreId::new("codex").expect("store"),
            object_key: "analysis/run.fastq".to_string(),
            expected_size_bytes: 42,
            expected_checksum: format!("sha256:{}", "a".repeat(64)),
            audience: "dasobjectstore".to_string(),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 1_100,
            nonce: "nonce-1".to_string(),
        };
        capability.validate().expect("capability");
        let mut invalid = capability.clone();
        invalid.object_key = "/private/host/path".to_string();
        assert!(matches!(
            invalid.validate(),
            Err(ApplicationAuthValidationError::UnsafeField("object_key"))
        ));
    }

    #[test]
    fn self_signed_identity_requires_development_environment() {
        let mut identity = identity();
        identity.credential_kind = ApplicationCredentialKind::DevelopmentSelfSigned;
        assert!(matches!(
            identity.validate(),
            Err(ApplicationAuthValidationError::Invalid(message))
                if message.contains("development environment")
        ));
        identity.environment = ApplicationEnvironment::Development;
        identity.validate().expect("development identity");
    }

    #[test]
    fn inactive_identity_cannot_authorize_access_tokens() {
        let mut identity = identity();
        identity.active = false;
        let token = AccessTokenClaims {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            token_id: "access-1".to_string(),
            application_id: identity.application_id.clone(),
            audience: "dasobjectstore".to_string(),
            issued_at_unix_seconds: 2_000,
            expires_at_unix_seconds: 2_600,
            scope: identity.scope.clone(),
        };
        assert_eq!(
            token.validate_against(&identity),
            Err(ApplicationAuthValidationError::InactiveIdentity)
        );
    }

    #[test]
    fn scope_validation_rejects_duplicate_prefixes() {
        let mut identity = identity();
        identity.scope.prefixes.push("analysis".to_string());
        assert_eq!(
            identity.validate(),
            Err(ApplicationAuthValidationError::DuplicateScope("prefixes"))
        );
    }

    #[test]
    fn public_key_descriptor_is_rotatable_metadata_without_private_material() {
        let descriptor = ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: "synoptikon-ingest".to_string(),
            key_id: "key-2026-07".to_string(),
            algorithm: ApplicationKeyAlgorithm::Ed25519,
            public_key_fingerprint: format!("sha256:{}", "a".repeat(64)),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        };
        descriptor.validate().expect("descriptor");
        let encoded = serde_json::to_string(&descriptor).expect("encode");
        assert!(!encoded.contains("private_key"));
        assert!(!encoded.contains("secret"));
    }
}
