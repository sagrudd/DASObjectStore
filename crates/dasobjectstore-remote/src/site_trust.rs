//! Verified, process-local Site-root trust for Monas-managed remote clients.
//!
//! A remote machine receives the short-lived PXCE/v1 envelope through an
//! independently authenticated provisioning channel. `trust provision`
//! verifies that envelope once and persists only public Site-root material in
//! an immutable-at-runtime record. Runtime clients use the adjacent PEM bundle
//! directly; neither the operating-system CA store nor a Proxenos service is
//! required on the remote machine.

use base64::Engine as _;
use proxenos::site_root_public_consumer_v1::{
    verify_site_root_public_consumer_envelope_v1, SiteRootPublicConsumerActionV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const SCHEMA: &str = "dasobjectstore.remote_site_trust.v1";
const ENV_BUNDLE: &str = "DASOBJECTSTORE_REMOTE_SITE_TRUST_BUNDLE";
const DEFAULT_DIRECTORY: &str = "/etc/dasobjectstore-remote/site-trust.d";
const MAXIMUM_ENVELOPE_BYTES: u64 = 9_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionedSiteTrust {
    pub schema: String,
    pub site_uuid: String,
    pub endpoint_host: String,
    pub endpoint_port: u16,
    pub root_fingerprint_sha256: String,
    pub root_generation: u64,
    pub trust_state_revision: u64,
    pub publication_digest_sha256: String,
    pub provisioned_envelope_sha256: String,
    pub ca_bundle_file: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSiteTrust {
    pub record_path: PathBuf,
    pub ca_bundle_path: PathBuf,
    pub record: ProvisionedSiteTrust,
}

#[derive(Debug)]
pub enum SiteTrustError {
    NotProvisioned {
        host: String,
        port: u16,
        path: PathBuf,
    },
    Invalid(String),
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for SiteTrustError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotProvisioned { host, port, path } => write!(
                out,
                "site trust not provisioned for {host}:{port}; expected signed Site Trust record {}",
                path.display()
            ),
            Self::Invalid(message) => out.write_str(message),
            Self::Io(error) => write!(out, "site trust I/O failed: {error}"),
            Self::Json(error) => write!(out, "invalid provisioned Site Trust record: {error}"),
        }
    }
}

impl std::error::Error for SiteTrustError {}
impl From<io::Error> for SiteTrustError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for SiteTrustError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub struct ProvisionRequest<'a> {
    pub host: &'a str,
    pub port: u16,
    pub site_uuid_hex: &'a str,
    pub envelope_path: &'a Path,
    pub authenticated_envelope_sha256_hex: &'a str,
    pub output: Option<&'a Path>,
}

pub fn provision(request: ProvisionRequest<'_>) -> Result<LoadedSiteTrust, SiteTrustError> {
    let host = canonical_host(request.host)?;
    if request.port == 0 {
        return Err(SiteTrustError::Invalid(
            "Site Trust endpoint port must not be zero".to_string(),
        ));
    }
    let expected_site = decode_exact::<16>(request.site_uuid_hex, "Site UUID")?;
    let authenticated_sha = decode_exact::<32>(
        request.authenticated_envelope_sha256_hex,
        "authenticated envelope SHA-256",
    )?;
    let envelope = read_safe_file(request.envelope_path, MAXIMUM_ENVELOPE_BYTES)?;
    let envelope_digest: [u8; 32] = Sha256::digest(&envelope).into();
    if envelope_digest != authenticated_sha {
        return Err(SiteTrustError::Invalid(
            "authenticated Site Trust envelope SHA-256 mismatch".to_string(),
        ));
    }
    let verified = verify_site_root_public_consumer_envelope_v1(
        &envelope,
        now_millis()?,
        expected_site,
        authenticated_sha,
    )
    .map_err(|_| {
        SiteTrustError::Invalid(
            "signed Site Trust envelope was rejected (wrong site, stale, substituted, or malformed)"
                .to_string(),
        )
    })?;
    if !matches!(
        verified.action,
        SiteRootPublicConsumerActionV1::Install | SiteRootPublicConsumerActionV1::Replace
    ) {
        return Err(SiteTrustError::Invalid(
            "signed Site Trust action does not provision an active Site root".to_string(),
        ));
    }
    let record_path = request
        .output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_record_path(&host, request.port));
    let ca_bundle_path = record_path.with_extension("pem");
    let ca_bundle_file = ca_bundle_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            SiteTrustError::Invalid("Site Trust output filename is invalid".to_string())
        })?
        .to_string();
    let record = ProvisionedSiteTrust {
        schema: SCHEMA.to_string(),
        site_uuid: hex::encode(verified.site_uuid),
        endpoint_host: host,
        endpoint_port: request.port,
        root_fingerprint_sha256: hex::encode(verified.root_fingerprint),
        root_generation: verified.root_generation,
        trust_state_revision: verified.trust_state_revision,
        publication_digest_sha256: hex::encode(verified.publication_digest),
        provisioned_envelope_sha256: hex::encode(authenticated_sha),
        ca_bundle_file,
    };
    persist_new(
        &record_path,
        &ca_bundle_path,
        &record,
        &pem(&verified.root_der),
    )?;
    load_record(&record_path, request.host, request.port)
}

pub fn load_for_endpoint(
    host: &str,
    port: u16,
    explicit_path: Option<&Path>,
) -> Result<LoadedSiteTrust, SiteTrustError> {
    let path = explicit_path
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os(ENV_BUNDLE).map(PathBuf::from))
        .unwrap_or_else(|| default_record_path(host, port));
    load_record(&path, host, port)
}

pub fn load_record(path: &Path, host: &str, port: u16) -> Result<LoadedSiteTrust, SiteTrustError> {
    if !path.exists() {
        return Err(SiteTrustError::NotProvisioned {
            host: host.to_string(),
            port,
            path: path.to_path_buf(),
        });
    }
    let bytes = read_safe_file(path, 64 * 1024)?;
    let record: ProvisionedSiteTrust = serde_json::from_slice(&bytes)?;
    validate_record(&record, host, port)?;
    let parent = path.parent().ok_or_else(|| {
        SiteTrustError::Invalid("Site Trust record has no parent directory".to_string())
    })?;
    let ca_bundle_path = parent.join(&record.ca_bundle_file);
    let bundle = read_safe_file(&ca_bundle_path, 32 * 1024)?;
    let root_der = pem_to_der(&bundle)?;
    reqwest::Certificate::from_pem(&bundle).map_err(|_| {
        SiteTrustError::Invalid(
            "Site Trust CA bundle is not a usable X.509 certificate".to_string(),
        )
    })?;
    let root_digest: [u8; 32] = Sha256::digest(&root_der).into();
    if root_digest
        != decode_exact::<32>(
            &record.root_fingerprint_sha256,
            "Site Trust root fingerprint",
        )?
    {
        return Err(SiteTrustError::Invalid(
            "site trust root fingerprint mismatch; reprovision the signed Site Trust bundle"
                .to_string(),
        ));
    }
    Ok(LoadedSiteTrust {
        record_path: path.to_path_buf(),
        ca_bundle_path,
        record,
    })
}

fn validate_record(
    record: &ProvisionedSiteTrust,
    host: &str,
    port: u16,
) -> Result<(), SiteTrustError> {
    if record.schema != SCHEMA {
        return Err(SiteTrustError::Invalid(
            "site trust record schema is unsupported".to_string(),
        ));
    }
    if record.endpoint_host != canonical_host(host)? || record.endpoint_port != port {
        return Err(SiteTrustError::Invalid(
            "site trust is provisioned for a different HTTPS endpoint".to_string(),
        ));
    }
    if record.root_generation == 0 || record.trust_state_revision == 0 {
        return Err(SiteTrustError::Invalid(
            "site trust record has an invalid authority generation".to_string(),
        ));
    }
    decode_exact::<16>(&record.site_uuid, "Site Trust site UUID")?;
    decode_exact::<32>(
        &record.root_fingerprint_sha256,
        "Site Trust root fingerprint",
    )?;
    decode_exact::<32>(
        &record.publication_digest_sha256,
        "Site Trust publication digest",
    )?;
    decode_exact::<32>(
        &record.provisioned_envelope_sha256,
        "Site Trust envelope digest",
    )?;
    if record.ca_bundle_file.contains('/') || record.ca_bundle_file.contains('\\') {
        return Err(SiteTrustError::Invalid(
            "site trust CA bundle filename is unsafe".to_string(),
        ));
    }
    Ok(())
}

fn persist_new(
    record_path: &Path,
    ca_bundle_path: &Path,
    record: &ProvisionedSiteTrust,
    ca_bundle: &[u8],
) -> Result<(), SiteTrustError> {
    let parent = record_path.parent().ok_or_else(|| {
        SiteTrustError::Invalid("Site Trust output has no parent directory".to_string())
    })?;
    fs::create_dir_all(parent)?;
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.permissions().readonly()
    {
        return Err(SiteTrustError::Invalid(
            "Site Trust output directory is unsafe or read-only".to_string(),
        ));
    }
    if record_path.exists() || ca_bundle_path.exists() {
        return Err(SiteTrustError::Invalid(
            "site trust already provisioned; root replacement requires a new explicit output path"
                .to_string(),
        ));
    }
    write_new(record_path, &serde_json::to_vec_pretty(record)?)?;
    if let Err(error) = write_new(ca_bundle_path, ca_bundle) {
        let _ = fs::remove_file(record_path);
        return Err(error);
    }
    sync_parent(parent)?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), SiteTrustError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o644);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_safe_file(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>, SiteTrustError> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(SiteTrustError::Invalid(format!(
                "site trust input {} is group- or world-writable",
                path.display()
            )));
        }
    }
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > maximum_bytes
    {
        return Err(SiteTrustError::Invalid(format!(
            "site trust input {} is not a safe bounded regular file",
            path.display()
        )));
    }
    Ok(fs::read(path)?)
}

fn default_record_path(host: &str, port: u16) -> PathBuf {
    let name = host
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect::<String>();
    Path::new(DEFAULT_DIRECTORY).join(format!("{name}-{port}.json"))
}

fn canonical_host(host: &str) -> Result<String, SiteTrustError> {
    let value = host.trim();
    if value.is_empty()
        || value.len() > 253
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(SiteTrustError::Invalid(
            "Site Trust host must be a single hostname or IP address".to_string(),
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn pem(der: &[u8]) -> Vec<u8> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut output = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        output.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        output.push('\n');
    }
    output.push_str("-----END CERTIFICATE-----\n");
    output.into_bytes()
}

fn pem_to_der(pem: &[u8]) -> Result<Vec<u8>, SiteTrustError> {
    let mut reader = std::io::BufReader::new(pem);
    let certificates = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            SiteTrustError::Invalid("Site Trust CA bundle is not valid PEM".to_string())
        })?;
    match certificates.as_slice() {
        [certificate] => Ok(certificate.as_ref().to_vec()),
        _ => Err(SiteTrustError::Invalid(
            "Site Trust CA bundle must contain exactly one certificate".to_string(),
        )),
    }
}

fn decode_exact<const N: usize>(value: &str, label: &str) -> Result<[u8; N], SiteTrustError> {
    let bytes = hex::decode(value)
        .map_err(|_| SiteTrustError::Invalid(format!("{label} is not hexadecimal")))?;
    bytes
        .try_into()
        .map_err(|_| SiteTrustError::Invalid(format!("{label} must be exactly {N} bytes")))
}

fn now_millis() -> Result<u64, SiteTrustError> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| SiteTrustError::Invalid(format!("system time is invalid: {error}")))?
            .as_millis(),
    )
    .map_err(|_| SiteTrustError::Invalid("system time is outside supported range".to_string()))
}

fn sync_parent(path: &Path) -> Result<(), SiteTrustError> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use coset::{iana, CborSerializable as _, CoseSign1Builder, HeaderBuilder};
    use ed25519_dalek::{Signer as _, SigningKey};
    use proxenos::site_root_public_consumer_v1::{
        assemble_site_root_public_consumer_v1, encode_site_root_public_consumer_envelope_v1,
        encode_site_root_public_consumer_unsigned_v1, SiteRootPublicConsumerSourceV1,
        SiteRootPublicConsumerVerificationKeyV1,
    };

    const ROOT_DER_B64: &str = "MIIDNTCCAh2gAwIBAgIUCbGF8AiTtrMu+YlhoOkyX76TLsowDQYJKoZIhvcNAQELBQAwKjEoMCYGA1UEAwwfREFTT2JqZWN0U3RvcmUgUmVtb3RlIFRlc3QgUm9vdDAeFw0yNjA4MzExMDAzMjZaFw0zNjA4MjgxMDAzMjZaMCoxKDAmBgNVBAMMH0RBU09iamVjdFN0b3JlIFJlbW90ZSBUZXN0IFJvb3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDAFA+NToQRSppFo9Lu7c2QTsbe9Hf3hNWWtSlfmMhRwIi++bFsoepg355rcfCDC0Ojqz2Ky1tge8AoyUDoZ1fMhqWgbf+h0i5QU1pb3NMwfZElAdfpc6hfAN4/NQP1Fypg9fG8cwS3HUwsp81QSNYbtxFSK0P3DethDt3EByyZEvQ5x+Xxxgl+O8Tlyq05a7mlA9wo1cU3+Htgo7WqKn8k6cMxA4pW4OWYQ75yg6rL70cr1TDK87ZWQkwlDmge1gmsk+DNr0AeadoBwcYjXjlGEL59rLG3/nRDYdNTMdoTpGpl4rXbevvKXJ4YChXcXFYYr2p3AaRiO90PBOSplfrpAgMBAAGjUzBRMB0GA1UdDgQWBBQwLxYqe7OQfVQB7uQkt0KmpNARpzAfBgNVHSMEGDAWgBQwLxYqe7OQfVQB7uQkt0KmpNARpzAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQCchbkBgmmk/o+DXuwoHQlsAEVYgiWOIW1E4UVauFSXx3lpLQnsdaKe1lOu15aogk3QVRK/EUxHBvY6EBGrXIonPbOZojYcwf5Q9+3VlGKthItEAGt4xyrvXSvzaodH16oO+mSfYO/V5qZOhKMABysoVLNYabrwj7tCnrL/6s3AaVpF/sKWPv/8Umy9XF59UD2MH4wmC0Jyee0V/EIW9T5XH70jdPgW8JIJMGdFGNLWk5keTwEzzUnfd4AqztkLwyVRqatpaEX2HouuLGKNiQgHou0z5bdq1c8LlpRz0KP20UOvvIZnauscUnb4rCHFlSrrSMyVYfgKP2XxmrvFhok5";

    #[test]
    fn absent_trust_has_the_precise_preflight_error() {
        let path = unique_path("missing.json");
        let error = load_record(&path, "192.168.0.193", 8443).unwrap_err();
        assert!(error
            .to_string()
            .starts_with("site trust not provisioned for 192.168.0.193:8443"));
    }

    #[test]
    fn signed_pxce_provisions_a_process_local_ca_without_os_mutation() {
        let root = base64::engine::general_purpose::STANDARD
            .decode(ROOT_DER_B64)
            .unwrap();
        let signer = SigningKey::from_bytes(&[7; 32]);
        let key = SiteRootPublicConsumerVerificationKeyV1 {
            generation: 4,
            public_key: signer.verifying_key().to_bytes(),
            registration_digest: [5; 32],
            current: true,
        };
        let source = SiteRootPublicConsumerSourceV1 {
            site_uuid: [1; 16],
            action: SiteRootPublicConsumerActionV1::Install,
            root_generation: 2,
            trust_state_revision: 3,
            root_fingerprint: Sha256::digest(&root).into(),
            previous_root_fingerprint: None,
            root_der: root,
            receipt_generation: key.generation,
            receipt_public_key: key.public_key,
            receipt_registration_digest: key.registration_digest,
        };
        let now = now_millis().unwrap();
        let unsigned =
            encode_site_root_public_consumer_unsigned_v1(&source, now, now + 300_000).unwrap();
        let proof = CoseSign1Builder::new()
            .protected(
                HeaderBuilder::new()
                    .algorithm(iana::Algorithm::EdDSA)
                    .content_type("application/vnd.mnemosyne.pxpc.v1".to_owned())
                    .key_id(key.generation.to_be_bytes().to_vec())
                    .build(),
            )
            .create_detached_signature(&unsigned, &[], |input| {
                signer.sign(input).to_bytes().to_vec()
            })
            .build()
            .to_vec()
            .unwrap();
        let artifact = assemble_site_root_public_consumer_v1(&unsigned, &proof, key).unwrap();
        let envelope =
            encode_site_root_public_consumer_envelope_v1(&artifact, source.site_uuid, key, now)
                .unwrap();
        let envelope_digest: [u8; 32] = Sha256::digest(&envelope).into();
        let directory = unique_path("site-trust");
        fs::create_dir_all(&directory).unwrap();
        let envelope_path = directory.join("trust.pxce");
        fs::write(&envelope_path, envelope).unwrap();
        let record_path = directory.join("trust.json");
        let trust = provision(ProvisionRequest {
            host: "192.168.0.193",
            port: 8443,
            site_uuid_hex: &hex::encode(source.site_uuid),
            envelope_path: &envelope_path,
            authenticated_envelope_sha256_hex: &hex::encode(envelope_digest),
            output: Some(&record_path),
        })
        .unwrap();
        assert_eq!(trust.record.root_generation, 2);
        assert_eq!(trust.record.trust_state_revision, 3);
        assert!(trust.ca_bundle_path.exists());
        assert_eq!(
            load_record(&record_path, "192.168.0.193", 8443)
                .unwrap()
                .record
                .root_fingerprint_sha256,
            hex::encode(source.root_fingerprint)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    fn unique_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-site-trust-{}-{nonce}-{name}",
            std::process::id()
        ))
    }
}
