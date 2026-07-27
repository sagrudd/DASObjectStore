//! RFC 8785-compatible proof verification for the governed v2 exchange.
//!
//! The admitted exchange schema contains only strings, booleans, arrays,
//! objects, and integral JSON numbers. The canonicalizer deliberately rejects
//! floating-point values so its bounded implementation cannot silently diverge
//! from JCS number serialization.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use dasobjectstore_core::application_auth::{ApplicationKeyAlgorithm, ApplicationKeyDescriptor};
use dasobjectstore_core::application_auth_v2::ErgasterionExchangeProofV1;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn verify_ergasterion_ed25519_proof(
    domain: &str,
    proof_free_value: &Value,
    proof: &ErgasterionExchangeProofV1,
    key: &ApplicationKeyDescriptor,
) -> Result<String, String> {
    if key.algorithm != ApplicationKeyAlgorithm::Ed25519 || !key.active {
        return Err("registered application key is not an active Ed25519 key".to_string());
    }
    let encoded_key = key
        .public_key_material
        .as_deref()
        .ok_or_else(|| "registered application key has no public material".to_string())?;
    let public_key = STANDARD
        .decode(encoded_key)
        .map_err(|_| "registered application public key is malformed".to_string())?;
    if public_key.len() != 32 {
        return Err("registered application public key has invalid length".to_string());
    }
    let fingerprint = format!("sha256:{}", hex_sha256(&public_key));
    if fingerprint != key.public_key_fingerprint {
        return Err("registered application public key fingerprint mismatch".to_string());
    }
    let signature = URL_SAFE_NO_PAD
        .decode(proof.signature.as_bytes())
        .map_err(|_| "application proof is not base64url without padding".to_string())?;
    if signature.len() != 64 {
        return Err("application proof has invalid length".to_string());
    }
    let canonical = canonical_json(proof_free_value)?;
    let payload = [domain.as_bytes(), canonical.as_bytes()].concat();
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload, &signature)
        .map_err(|_| "application proof is invalid".to_string())?;
    Ok(hex_sha256(&payload))
}

pub fn canonical_value_sha256(value: &Value) -> Result<String, String> {
    canonical_json(value).map(|canonical| hex_sha256(canonical.as_bytes()))
}

fn canonical_json(value: &Value) -> Result<String, String> {
    let mut output = String::new();
    append_canonical(value, &mut output)?;
    Ok(output)
}

fn append_canonical(value: &Value, output: &mut String) -> Result<(), String> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            output.push_str(&number.to_string());
        }
        Value::Number(_) => {
            return Err("floating-point values are not admitted by this contract".to_string())
        }
        Value::String(value) => output.push_str(
            &serde_json::to_string(value)
                .map_err(|_| "application proof string cannot be canonicalized".to_string())?,
        ),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                append_canonical(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output
                    .push_str(&serde_json::to_string(key).map_err(|_| {
                        "application proof key cannot be canonicalized".to_string()
                    })?);
                output.push(':');
                append_canonical(&values[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_json, canonical_value_sha256, hex_sha256, verify_ergasterion_ed25519_proof,
    };
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use base64::Engine;
    use dasobjectstore_core::application_auth::{
        ApplicationKeyAlgorithm, ApplicationKeyDescriptor, APPLICATION_AUTH_SCHEMA_VERSION,
    };
    use dasobjectstore_core::application_auth_v2::{
        ErgasterionExchangeProofAlgorithmV1, ErgasterionExchangeProofV1,
    };
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[test]
    fn canonicalization_is_independent_of_object_member_order() {
        let left = serde_json::json!({"z": [3, 2, 1], "a": {"b": true, "a": "x"}});
        let right = serde_json::json!({"a": {"a": "x", "b": true}, "z": [3, 2, 1]});
        assert_eq!(
            canonical_json(&left).expect("left"),
            canonical_json(&right).expect("right")
        );
        assert_eq!(
            canonical_value_sha256(&left).expect("left digest"),
            canonical_value_sha256(&right).expect("right digest")
        );
    }

    #[test]
    fn floating_point_values_fail_closed() {
        assert!(canonical_json(&serde_json::json!({"value": 1.5})).is_err());
    }

    #[test]
    fn registered_ed25519_key_verifies_the_exact_domain_and_canonical_payload() {
        let key_pair = Ed25519KeyPair::from_pkcs8(
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .expect("PKCS8 generation")
                .as_ref(),
        )
        .expect("key pair");
        let public_key = key_pair.public_key().as_ref();
        let key = ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: "app-7e4a31c9b260".to_string(),
            key_id: "ergasterion-ed25519-2026-07-19".to_string(),
            algorithm: ApplicationKeyAlgorithm::Ed25519,
            public_key_fingerprint: format!("sha256:{}", hex_sha256(public_key)),
            public_key_material: Some(STANDARD.encode(public_key)),
            issued_at_unix_seconds: 1,
            expires_at_unix_seconds: 2,
            active: true,
        };
        let value = serde_json::json!({"z": 3, "a": ["governed", true]});
        let domain = "dasobjectstore.test-domain.v1\n";
        let canonical = canonical_json(&value).expect("canonical payload");
        let payload = [domain.as_bytes(), canonical.as_bytes()].concat();
        let proof = ErgasterionExchangeProofV1 {
            algorithm: ErgasterionExchangeProofAlgorithmV1::Ed25519,
            signature: URL_SAFE_NO_PAD.encode(key_pair.sign(&payload).as_ref()),
        };

        assert_eq!(
            verify_ergasterion_ed25519_proof(domain, &value, &proof, &key)
                .expect("valid signature"),
            hex_sha256(&payload)
        );
        assert!(verify_ergasterion_ed25519_proof(
            "dasobjectstore.wrong-domain.v1\n",
            &value,
            &proof,
            &key
        )
        .is_err());
    }
}
