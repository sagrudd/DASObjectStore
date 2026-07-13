use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::application_auth::{AccessTokenClaims, AccessTokenExchangeRequest};
use serde::{Deserialize, Serialize};

/// Public daemon boundary for proof-verified application access-token
/// exchange. Identity and key registries remain daemon-owned; callers submit
/// only the signed, path-free request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationAccessTokenExchangeRequest {
    pub exchange: AccessTokenExchangeRequest,
}

impl ApplicationAccessTokenExchangeRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        self.exchange.validate_shape().map_err(|error| {
            DaemonRequestValidationError::InvalidPolicy {
                message: error.to_string(),
            }
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplicationAccessTokenExchangeResponse {
    pub claims: AccessTokenClaims,
}

impl ApplicationAccessTokenExchangeResponse {
    pub fn validate(&self) -> Result<(), String> {
        self.claims
            .schema_version
            .eq(dasobjectstore_core::application_auth::APPLICATION_AUTH_SCHEMA_VERSION)
            .then_some(())
            .ok_or_else(|| "unsupported application access-token schema".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::ApplicationAccessTokenExchangeRequest;
    use dasobjectstore_core::application_auth::{
        AccessTokenExchangeRequest, APPLICATION_AUTH_SCHEMA_VERSION,
    };

    #[test]
    fn exchange_request_validates_shape_before_registry_lookup() {
        let request = ApplicationAccessTokenExchangeRequest {
            exchange: AccessTokenExchangeRequest {
                schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
                application_id: "synoptikon".to_string(),
                key_id: "key-1".to_string(),
                audience: "dasobjectstore".to_string(),
                requested_issued_at_unix_seconds: 10,
                requested_expires_at_unix_seconds: 20,
                scope: sample_scope(),
                proof: "proof".to_string(),
            },
        };
        request.validate().expect("shape validates");
    }

    fn sample_scope() -> dasobjectstore_core::application_auth::ApplicationScope {
        use dasobjectstore_core::application_auth::{ApplicationOperation, ApplicationScope};
        use dasobjectstore_core::ids::StoreId;
        use dasobjectstore_core::ingress::IngressOrigin;
        ApplicationScope {
            store_ids: vec![StoreId::new("codex").expect("store")],
            prefixes: vec![],
            object_types: vec![],
            operations: vec![ApplicationOperation::Read],
            ingress_origin: IngressOrigin::Synoptikon,
            max_object_bytes: Some(10),
            max_total_bytes: Some(100),
        }
    }
}
