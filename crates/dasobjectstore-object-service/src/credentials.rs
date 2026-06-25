//! Per-store object service credential generation.

use crate::provider::ObjectServiceError;
use dasobjectstore_core::ids::StoreId;
use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::io::Read;

const ACCESS_KEY_RANDOM_BYTES: usize = 10;
const SECRET_KEY_RANDOM_BYTES: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreCredentialRequest {
    pub store_id: StoreId,
    pub bucket_name: String,
}

#[derive(Clone, Eq, PartialEq)]
pub struct StoreServiceCredential {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub credential_reference: String,
    pub access_key_id: String,
    pub secret_access_key: SecretAccessKey,
}

impl fmt::Debug for StoreServiceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoreServiceCredential")
            .field("store_id", &self.store_id)
            .field("bucket_name", &self.bucket_name)
            .field("credential_reference", &self.credential_reference)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &self.secret_access_key)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretAccessKey(String);

impl SecretAccessKey {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretAccessKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretAccessKey(REDACTED)")
    }
}

pub trait CredentialEntropy {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError>;
}

#[derive(Debug, Default)]
pub struct SystemCredentialEntropy;

impl CredentialEntropy for SystemCredentialEntropy {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError> {
        let mut device = File::open("/dev/urandom").map_err(|error| {
            ObjectServiceError::CommandFailed(format!("open random source: {error}"))
        })?;
        device.read_exact(bytes).map_err(|error| {
            ObjectServiceError::CommandFailed(format!("read random source: {error}"))
        })
    }
}

pub fn generate_per_store_credentials(
    requests: &[StoreCredentialRequest],
    entropy: &mut impl CredentialEntropy,
) -> Result<Vec<StoreServiceCredential>, ObjectServiceError> {
    validate_requests(requests)?;

    requests
        .iter()
        .map(|request| generate_store_credential(request, entropy))
        .collect()
}

fn validate_requests(requests: &[StoreCredentialRequest]) -> Result<(), ObjectServiceError> {
    if requests.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "at least one store credential request is required".to_string(),
        ));
    }

    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    for request in requests {
        reject_blank("bucket_name", &request.bucket_name)?;
        if !store_ids.insert(request.store_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate credential request for store: {}",
                request.store_id
            )));
        }
        if !bucket_names.insert(request.bucket_name.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate credential request for bucket: {}",
                request.bucket_name
            )));
        }
    }

    Ok(())
}

fn generate_store_credential(
    request: &StoreCredentialRequest,
    entropy: &mut impl CredentialEntropy,
) -> Result<StoreServiceCredential, ObjectServiceError> {
    let mut access_key_bytes = [0_u8; ACCESS_KEY_RANDOM_BYTES];
    let mut secret_key_bytes = [0_u8; SECRET_KEY_RANDOM_BYTES];
    entropy.fill(&mut access_key_bytes)?;
    entropy.fill(&mut secret_key_bytes)?;

    Ok(StoreServiceCredential {
        store_id: request.store_id.clone(),
        bucket_name: request.bucket_name.clone(),
        credential_reference: credential_reference_for(&request.store_id),
        access_key_id: format!("DOS{}", hex_upper(&access_key_bytes)),
        secret_access_key: SecretAccessKey(hex_lower(&secret_key_bytes)),
    })
}

fn credential_reference_for(store_id: &StoreId) -> String {
    format!("secret://dasobjectstore/stores/{store_id}/s3")
}

fn reject_blank(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.trim().is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "{field} must not be blank"
        )));
    }

    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    hex_encode(bytes, TABLE)
}

fn hex_upper(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789ABCDEF";
    hex_encode(bytes, TABLE)
}

fn hex_encode(bytes: &[u8], table: &[u8; 16]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(table[(byte >> 4) as usize] as char);
        encoded.push(table[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        generate_per_store_credentials, CredentialEntropy, ObjectServiceError,
        StoreCredentialRequest,
    };
    use dasobjectstore_core::ids::StoreId;

    #[test]
    fn generates_distinct_per_store_credentials() {
        let mut entropy = FixedEntropy::new();
        let credentials = generate_per_store_credentials(
            &[
                request("generated", "generated-data"),
                request("critical", "critical-metadata"),
            ],
            &mut entropy,
        )
        .expect("credentials generated");

        assert_eq!(credentials.len(), 2);
        assert_eq!(credentials[0].store_id.as_str(), "generated");
        assert_eq!(credentials[0].bucket_name, "generated-data");
        assert_eq!(
            credentials[0].credential_reference,
            "secret://dasobjectstore/stores/generated/s3"
        );
        assert!(credentials[0].access_key_id.starts_with("DOS"));
        assert_ne!(credentials[0].access_key_id, credentials[1].access_key_id);
        assert_ne!(
            credentials[0].secret_access_key.expose_secret(),
            credentials[1].secret_access_key.expose_secret()
        );
    }

    #[test]
    fn debug_output_redacts_secret_access_key() {
        let mut entropy = FixedEntropy::new();
        let credential =
            generate_per_store_credentials(&[request("generated", "generated-data")], &mut entropy)
                .expect("credentials generated")
                .remove(0);

        let debug = format!("{credential:?}");

        assert!(debug.contains("SecretAccessKey(REDACTED)"));
        assert!(!debug.contains(credential.secret_access_key.expose_secret()));
    }

    #[test]
    fn rejects_duplicate_store_requests() {
        let mut entropy = FixedEntropy::new();

        let err = generate_per_store_credentials(
            &[
                request("generated", "generated-data"),
                request("generated", "generated-data-alt"),
            ],
            &mut entropy,
        )
        .expect_err("duplicate store rejected");

        assert!(err
            .to_string()
            .contains("duplicate credential request for store"));
    }

    #[test]
    fn rejects_duplicate_bucket_requests() {
        let mut entropy = FixedEntropy::new();

        let err = generate_per_store_credentials(
            &[
                request("generated", "shared"),
                request("critical", "shared"),
            ],
            &mut entropy,
        )
        .expect_err("duplicate bucket rejected");

        assert!(err
            .to_string()
            .contains("duplicate credential request for bucket"));
    }

    fn request(store_id: &str, bucket_name: &str) -> StoreCredentialRequest {
        StoreCredentialRequest {
            store_id: StoreId::new(store_id).expect("store id"),
            bucket_name: bucket_name.to_string(),
        }
    }

    struct FixedEntropy {
        next: u8,
    }

    impl FixedEntropy {
        fn new() -> Self {
            Self { next: 1 }
        }
    }

    impl CredentialEntropy for FixedEntropy {
        fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError> {
            for byte in bytes {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
    }
}
