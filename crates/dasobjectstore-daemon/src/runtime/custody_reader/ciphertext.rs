//! Inspect only the protected encrypted source; never decrypt or select a key.
use super::{files::Directory, raw_sha256, ReaderError};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

/// Explicit companion-selected effective systemd protection. No automatic mode,
/// null key or fallback is admitted by this source adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialProtection {
    /// Existing system host secret.
    Host,
    /// Existing TPM2 HMAC protection.
    Tpm2,
    /// Both existing host secret and TPM2 HMAC protection.
    HostAndTpm2,
}

/// This is header classification, not authentication or decryption. Actual
/// systemd activation must authenticate the complete encrypted credential.
fn classify(raw: &[u8]) -> Result<CredentialProtection, ReaderError> {
    if raw.is_empty() || raw.len() > 2 * 1024 * 1024 {
        return Err(ReaderError::Boundary);
    }
    let compact: Vec<u8> = raw
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let bytes = STANDARD.decode(compact).map_err(|_| ReaderError::Format)?;
    if bytes.len() < 32 || bytes.len() > 1152 * 1024 {
        return Err(ReaderError::Format);
    }
    // systemd v259 creds-util.h IDs; scoped/PK variants remain unsupported.
    let protection = match hex_id(&bytes[..16]).as_str() {
        "5a1c6a86df9d4096b1d5a65e0862f19a" => CredentialProtection::Host,
        "0c7cc07b117645919c4b0bea08bc20fe" => CredentialProtection::Tpm2,
        "93a894094874449090caf2fc93cab553" => CredentialProtection::HostAndTpm2,
        _ => return Err(ReaderError::Format),
    };
    let word = |offset: usize| {
        u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("fixed checked header"),
        ) as usize
    };
    let (key, block, iv, tag) = (word(16), word(20), word(24), word(28));
    if key != 32 || block == 0 || block > 16384 || iv > 16384 || tag != 16 {
        return Err(ReaderError::Format);
    }
    let header = (32 + iv + 7) & !7;
    let tpm_header = if protection == CredentialProtection::Host {
        0
    } else {
        24
    };
    // Includes encrypted timestamp/name metadata and authentication tag. Systemd
    // remains responsible for TPM payload parsing and cryptographic validation.
    if bytes.len() < header + tpm_header + 24 + tag {
        return Err(ReaderError::Format);
    }
    Ok(protection)
}

fn hex_id(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn verify(
    path: &Path,
    manager_uid: u32,
    expected_sha256: &str,
    protection: CredentialProtection,
    deadline: dasobjectstore_object_service::custody::CustodyReadDeadline,
) -> Result<(), ReaderError> {
    deadline.remaining().map_err(|_| ReaderError::Read)?;
    let parent = path.parent().ok_or(ReaderError::Boundary)?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(ReaderError::Boundary)?;
    let directory = Directory::open(parent.to_path_buf(), manager_uid)?;
    let raw = directory.read_private(name, 2 * 1024 * 1024)?;
    if raw_sha256(&raw) != expected_sha256 || classify(&raw)? != protection {
        return Err(ReaderError::Binding);
    }
    deadline.remaining().map_err(|_| ReaderError::Read)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_headers_are_classification_only_and_no_fallback() {
        // Deliberately not valid ciphertext; a matching header cannot establish
        // systemd authentication or a successful continuation-load positive.
        let mut bytes = vec![0u8; 128];
        bytes[..16].copy_from_slice(&[
            0x5a, 0x1c, 0x6a, 0x86, 0xdf, 0x9d, 0x40, 0x96, 0xb1, 0xd5, 0xa6, 0x5e, 0x08, 0x62,
            0xf1, 0x9a,
        ]);
        for (offset, value) in [(16, 32u32), (20, 1), (24, 12), (28, 16)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            classify(STANDARD.encode(&bytes).as_bytes()),
            Ok(CredentialProtection::Host)
        );
        bytes[0] ^= 1;
        assert!(classify(STANDARD.encode(&bytes).as_bytes()).is_err());
        bytes[0] ^= 1;
        bytes[16] = 0;
        assert!(classify(STANDARD.encode(&bytes).as_bytes()).is_err());
        assert!(classify(b"AWS_ACCESS_KEY_ID=not-ciphertext").is_err());
    }

    #[test]
    fn every_supported_mode_and_malformed_header() {
        let ids = [
            (
                [
                    0x5a, 0x1c, 0x6a, 0x86, 0xdf, 0x9d, 0x40, 0x96, 0xb1, 0xd5, 0xa6, 0x5e, 0x08,
                    0x62, 0xf1, 0x9a,
                ],
                CredentialProtection::Host,
            ),
            (
                [
                    0x0c, 0x7c, 0xc0, 0x7b, 0x11, 0x76, 0x45, 0x91, 0x9c, 0x4b, 0x0b, 0xea, 0x08,
                    0xbc, 0x20, 0xfe,
                ],
                CredentialProtection::Tpm2,
            ),
            (
                [
                    0x93, 0xa8, 0x94, 0x09, 0x48, 0x74, 0x44, 0x90, 0x90, 0xca, 0xf2, 0xfc, 0x93,
                    0xca, 0xb5, 0x53,
                ],
                CredentialProtection::HostAndTpm2,
            ),
        ];
        for (id, mode) in ids {
            let mut bytes = vec![0u8; 128];
            bytes[..16].copy_from_slice(&id);
            for (offset, value) in [(16, 32u32), (20, 1), (24, 12), (28, 16)] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            assert_eq!(classify(STANDARD.encode(&bytes).as_bytes()), Ok(mode));
            for other in [
                CredentialProtection::Host,
                CredentialProtection::Tpm2,
                CredentialProtection::HostAndTpm2,
            ] {
                if other != mode {
                    assert_ne!(classify(STANDARD.encode(&bytes).as_bytes()), Ok(other));
                }
            }
            for length in [0, 15, 16, 31, 32, 47, 63] {
                assert!(classify(STANDARD.encode(&bytes[..length]).as_bytes()).is_err());
            }
            for (offset, value) in [(16, 31u32), (20, 0), (20, 16385), (24, 16385), (28, 15)] {
                let mut bad = bytes.clone();
                bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                assert!(classify(STANDARD.encode(bad).as_bytes()).is_err());
            }
            for forbidden in [
                [
                    0x05, 0x84, 0x69, 0xda, 0xf6, 0xf5, 0x43, 0x24, 0x80, 0x05, 0x49, 0xda, 0x0f,
                    0x8e, 0xa2, 0xfb,
                ],
                [
                    0x55, 0xb9, 0xed, 0x1d, 0x38, 0x59, 0x4d, 0x43, 0xa8, 0x31, 0x9d, 0x2e, 0xbb,
                    0x33, 0x2a, 0xc6,
                ],
                [0; 16],
            ] {
                bytes[..16].copy_from_slice(&forbidden);
                assert!(classify(STANDARD.encode(&bytes).as_bytes()).is_err());
            }
        }
        for bad in [
            b"!".to_vec(),
            b"AAAA=".to_vec(),
            vec![b'A'; 2 * 1024 * 1024 + 1],
        ] {
            assert!(classify(&bad).is_err());
        }
    }
}
