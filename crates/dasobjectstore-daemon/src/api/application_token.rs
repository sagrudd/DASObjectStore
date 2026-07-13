use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::application_auth::{AccessTokenClaims, AccessTokenExchangeRequest};
use serde::{Deserialize, Serialize};

/// Canonical HTTPS route for proof-bearing application access-token exchange.
/// Listener authentication (including mTLS where configured) and daemon
/// dispatch remain deployment-layer responsibilities; this constant keeps
/// clients and Web adapters on one versioned path without exposing secrets.
pub const APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE: &str = "/api/v1/application-auth/access-token";

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
        self.claims.validate().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ApplicationAccessTokenExchangeRequest, APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE};
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

    #[test]
    fn exchange_route_is_versioned_and_path_stable() {
        assert_eq!(
            APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE,
            "/api/v1/application-auth/access-token"
        );
    }

    #[test]
    fn exchange_response_rejects_malformed_claims() {
        let mut claims = sample_claims();
        claims.audience.clear();
        let response = super::ApplicationAccessTokenExchangeResponse { claims };
        assert!(response.validate().is_err());
    }

    fn sample_claims() -> dasobjectstore_core::application_auth::AccessTokenClaims {
        use dasobjectstore_core::application_auth::{
            AccessTokenClaims, ApplicationOperation, ApplicationScope,
        };
        use dasobjectstore_core::ids::StoreId;
        use dasobjectstore_core::ingress::IngressOrigin;
        AccessTokenClaims {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            token_id: "access-1".to_string(),
            application_id: "synoptikon".to_string(),
            audience: "dasobjectstore".to_string(),
            issued_at_unix_seconds: 10,
            expires_at_unix_seconds: 20,
            scope: ApplicationScope {
                store_ids: vec![StoreId::new("codex").expect("store")],
                prefixes: vec!["reads".to_string()],
                object_types: vec![],
                operations: vec![ApplicationOperation::Read],
                ingress_origin: IngressOrigin::Synoptikon,
                max_object_bytes: Some(10),
                max_total_bytes: Some(100),
            },
        }
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
