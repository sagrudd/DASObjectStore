//! Concrete production proof verification for application token exchange.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use dasobjectstore_core::application_auth::{
    AccessTokenExchangeRequest, ApplicationAuthValidationError, ApplicationExchangeProofVerifier,
    ApplicationKeyAlgorithm, ApplicationKeyDescriptor,
};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_ASN1, ED25519};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default)]
pub struct RingApplicationExchangeProofVerifier;

impl ApplicationExchangeProofVerifier for RingApplicationExchangeProofVerifier {
    fn verify(
        &self,
        request: &AccessTokenExchangeRequest,
        key: &ApplicationKeyDescriptor,
    ) -> Result<(), ApplicationAuthValidationError> {
        let encoded_key = key
            .public_key_material
            .as_deref()
            .ok_or(ApplicationAuthValidationError::ProofRejected)?;
        let public_key = BASE64
            .decode(encoded_key)
            .map_err(|_| ApplicationAuthValidationError::ProofRejected)?;
        let fingerprint = format!(
            "sha256:{}",
            Sha256::digest(&public_key)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        if fingerprint != key.public_key_fingerprint {
            return Err(ApplicationAuthValidationError::ProofRejected);
        }
        let signature = BASE64
            .decode(request.proof.as_bytes())
            .map_err(|_| ApplicationAuthValidationError::ProofRejected)?;
        let payload = request.signing_payload();
        let verified = match key.algorithm {
            ApplicationKeyAlgorithm::Ed25519 => {
                UnparsedPublicKey::new(&ED25519, public_key).verify(&payload, &signature)
            }
            ApplicationKeyAlgorithm::EcdsaP256Sha256 => {
                UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, public_key)
                    .verify(&payload, &signature)
            }
            ApplicationKeyAlgorithm::MtlsCertificate => {
                return Err(ApplicationAuthValidationError::ProofRejected)
            }
        };
        verified.map_err(|_| ApplicationAuthValidationError::ProofRejected)
    }
}

#[cfg(test)]
mod tests {
    use super::RingApplicationExchangeProofVerifier;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use dasobjectstore_core::application_auth::{
        AccessTokenExchangeRequest, ApplicationCredentialKind, ApplicationEnvironment,
        ApplicationExchangeProofVerifier, ApplicationIdentity, ApplicationKeyAlgorithm,
        ApplicationKeyDescriptor, ApplicationOperation, ApplicationScope,
        APPLICATION_AUTH_SCHEMA_VERSION,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::ingress::IngressOrigin;
    use dasobjectstore_core::object_type::ObjectType;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use sha2::{Digest, Sha256};

    fn identity() -> ApplicationIdentity {
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: "synoptikon-ingest".to_string(),
            owner: "mnemosyne".to_string(),
            purpose: "sequencing ingest".to_string(),
            environment: ApplicationEnvironment::Production,
            credential_kind: ApplicationCredentialKind::AsymmetricKey,
            scope: ApplicationScope {
                store_ids: vec![StoreId::new("codex").expect("store")],
                prefixes: vec!["analysis".to_string()],
                object_types: vec![ObjectType::Fastq],
                operations: vec![ApplicationOperation::Write],
                ingress_origin: IngressOrigin::Synoptikon,
                max_object_bytes: Some(10_000),
                max_total_bytes: Some(100_000),
            },
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        }
    }

    #[test]
    fn malformed_or_unbound_proofs_are_rejected() {
        let identity = identity();
        let key = ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: identity.application_id.clone(),
            key_id: "key-1".to_string(),
            algorithm: ApplicationKeyAlgorithm::Ed25519,
            public_key_fingerprint: format!("sha256:{}", "a".repeat(64)),
            public_key_material: Some(BASE64.encode([0u8; 32])),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        };
        let request = AccessTokenExchangeRequest {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: identity.application_id.clone(),
            key_id: key.key_id.clone(),
            audience: "dasobjectstore".to_string(),
            requested_issued_at_unix_seconds: 2_000,
            requested_expires_at_unix_seconds: 2_600,
            scope: identity.scope.clone(),
            proof: BASE64.encode([0u8; 64]),
        };
        assert!(request.validate_against(&identity, &key).is_ok());
        assert!(RingApplicationExchangeProofVerifier
            .verify(&request, &key)
            .is_err());
        let fingerprint = format!(
            "sha256:{}",
            Sha256::digest([0u8; 32])
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        assert_ne!(fingerprint, key.public_key_fingerprint);
    }

    #[test]
    fn ed25519_proof_is_verified_against_bound_public_key() {
        let identity = identity();
        let signing_key = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).expect("key");
        let public_key = signing_key.public_key().as_ref();
        let fingerprint = format!(
            "sha256:{}",
            Sha256::digest(public_key)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let key = ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: identity.application_id.clone(),
            key_id: "key-1".to_string(),
            algorithm: ApplicationKeyAlgorithm::Ed25519,
            public_key_fingerprint: fingerprint,
            public_key_material: Some(BASE64.encode(public_key)),
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        };
        let mut request = AccessTokenExchangeRequest {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: identity.application_id.clone(),
            key_id: key.key_id.clone(),
            audience: "dasobjectstore".to_string(),
            requested_issued_at_unix_seconds: 2_000,
            requested_expires_at_unix_seconds: 2_600,
            scope: identity.scope.clone(),
            proof: String::new(),
        };
        request.proof = BASE64.encode(signing_key.sign(&request.signing_payload()).as_ref());
        request
            .issue_access_token(
                &identity,
                &key,
                "access-verified".to_string(),
                &RingApplicationExchangeProofVerifier,
            )
            .expect("verified access token");
    }
}
