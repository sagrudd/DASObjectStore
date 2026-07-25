//! Explicit, endpoint-bound TLS enrollment for DAS appliances.
//!
//! The certificate probe deliberately completes only a TLS handshake. It never
//! writes HTTP application data and therefore cannot disclose credentials.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, Error as TlsError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::{self, BufReader, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use x509_parser::extensions::GeneralName;
use x509_parser::parse_x509_certificate;

const TRUST_SCHEMA_VERSION: &str = "v1";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTrustRecord {
    pub schema_version: String,
    pub endpoint_host: String,
    pub endpoint_port: u16,
    pub appliance_id: String,
    pub enrolled_at_utc: String,
    pub subject: String,
    pub issuer: String,
    pub subject_alt_names: Vec<String>,
    pub not_before: String,
    pub not_after: String,
    pub fingerprint_sha256: String,
    pub spki_sha256: String,
    pub address_matches_certificate: bool,
    pub legacy_fingerprint_pinned: bool,
    pub tls_server_name: String,
    pub certificate_pem: String,
}

impl ApplianceTrustRecord {
    pub fn certificate_pem(&self) -> &[u8] {
        self.certificate_pem.as_bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentedCertificate {
    pub leaf_der: Vec<u8>,
    pub certificate_pem: String,
    pub subject: String,
    pub issuer: String,
    pub subject_alt_names: Vec<String>,
    pub not_before: String,
    pub not_after: String,
    pub fingerprint_sha256: String,
    pub spki_sha256: String,
    pub address_matches_certificate: bool,
    pub tls_server_name: Option<String>,
}

#[derive(Debug)]
pub enum TrustError {
    Invalid(String),
    Io(io::Error),
    Tls(String),
    Json(serde_json::Error),
}

impl fmt::Display for TrustError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::Tls(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "invalid appliance trust record: {error}"),
        }
    }
}

impl std::error::Error for TrustError {}

impl From<io::Error> for TrustError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TrustError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug)]
struct CaptureOnlyVerifier {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for CaptureOnlyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn probe_certificate(host: &str, port: u16) -> Result<PresentedCertificate, TrustError> {
    let endpoint = socket_endpoint(host, port)?;
    let socket = endpoint
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| TrustError::Tls("appliance address resolved to no socket".to_string()))?;
    let mut stream = TcpStream::connect_timeout(&socket, PROBE_TIMEOUT)?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(CaptureOnlyVerifier {
        provider: Arc::clone(&provider),
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| TrustError::Tls(format!("configure TLS probe: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| TrustError::Invalid("host is not a valid TLS server name".to_string()))?;
    let mut connection = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|error| TrustError::Tls(format!("start TLS probe: {error}")))?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut stream)
            .map_err(|error| TrustError::Tls(format!("TLS certificate probe failed: {error}")))?;
    }
    let chain = connection.peer_certificates().ok_or_else(|| {
        TrustError::Tls("appliance TLS listener presented no certificate".to_string())
    })?;
    let leaf = chain
        .first()
        .ok_or_else(|| TrustError::Tls("appliance certificate chain is empty".to_string()))?;
    inspect_leaf_certificate(host, leaf.as_ref())
}

pub fn inspect_leaf_certificate(
    host: &str,
    leaf_der: &[u8],
) -> Result<PresentedCertificate, TrustError> {
    let (_, certificate) = parse_x509_certificate(leaf_der)
        .map_err(|error| TrustError::Invalid(format!("invalid X.509 certificate: {error}")))?;
    if !certificate.validity().is_valid() {
        return Err(TrustError::Invalid(format!(
            "appliance certificate is not currently valid (valid {} through {})",
            certificate.validity().not_before,
            certificate.validity().not_after
        )));
    }
    let subject_alt_names = certificate
        .subject_alternative_name()
        .map_err(|error| {
            TrustError::Invalid(format!("invalid certificate SAN extension: {error}"))
        })?
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(display_general_name)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let address_matches_certificate = san_matches_host(&subject_alt_names, host);
    let tls_server_name = if address_matches_certificate {
        Some(host.to_string())
    } else if subject_alt_names.iter().any(|name| name == "DNS:localhost") {
        Some("localhost".to_string())
    } else {
        None
    };
    let spki = certificate.public_key().raw;
    Ok(PresentedCertificate {
        leaf_der: leaf_der.to_vec(),
        certificate_pem: pem_certificate(leaf_der),
        subject: certificate.subject().to_string(),
        issuer: certificate.issuer().to_string(),
        subject_alt_names,
        not_before: certificate.validity().not_before.to_string(),
        not_after: certificate.validity().not_after.to_string(),
        fingerprint_sha256: formatted_sha256(leaf_der),
        spki_sha256: formatted_sha256(spki),
        address_matches_certificate,
        tls_server_name,
    })
}

pub fn load_trust(host: &str, port: u16) -> Result<Option<ApplianceTrustRecord>, TrustError> {
    let path = trust_record_path(host, port)?;
    reject_unsafe_file(&path)?;
    match fs::read(&path) {
        Ok(raw) => {
            let record: ApplianceTrustRecord = serde_json::from_slice(&raw)?;
            validate_record_binding(&record, host, port)?;
            Ok(Some(record))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn verify_presented_pin(
    record: &ApplianceTrustRecord,
    presented: &PresentedCertificate,
) -> Result<(), TrustError> {
    let old = parse_fingerprint(&record.fingerprint_sha256)?;
    let new = parse_fingerprint(&presented.fingerprint_sha256)?;
    if !bool::from(old.ct_eq(&new)) {
        return Err(TrustError::Invalid(format!(
            "appliance certificate changed; refusing connection\nold SHA-256: {}\nnew SHA-256: {}\nuse `dasobjectstore-remote trust rotate {} --trust-fingerprint SHA256` after independent verification",
            record.fingerprint_sha256, presented.fingerprint_sha256, record.appliance_id
        )));
    }
    Ok(())
}

pub fn expected_fingerprint_matches(
    expected: &str,
    presented: &PresentedCertificate,
) -> Result<(), TrustError> {
    let expected = parse_fingerprint(expected)?;
    let actual = parse_fingerprint(&presented.fingerprint_sha256)?;
    if !bool::from(expected.ct_eq(&actual)) {
        return Err(TrustError::Invalid(format!(
            "appliance certificate fingerprint mismatch; expected {}, presented {}",
            canonical_fingerprint(&expected),
            presented.fingerprint_sha256
        )));
    }
    Ok(())
}

pub fn new_trust_record(
    host: &str,
    port: u16,
    appliance_id: Option<&str>,
    presented: &PresentedCertificate,
) -> Result<ApplianceTrustRecord, TrustError> {
    let tls_server_name = presented.tls_server_name.clone().ok_or_else(|| {
        TrustError::Invalid(
            "certificate does not match the appliance address and exposes no explicitly supported legacy DNS identity"
                .to_string(),
        )
    })?;
    Ok(ApplianceTrustRecord {
        schema_version: TRUST_SCHEMA_VERSION.to_string(),
        endpoint_host: host.to_string(),
        endpoint_port: port,
        appliance_id: appliance_id
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("{value}@{}", endpoint_identity_suffix(host, port)))
            .unwrap_or_else(|| endpoint_identity(host, port)),
        enrolled_at_utc: dasobjectstore_core::utc::format_utc_timestamp_seconds(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| TrustError::Invalid("system clock is before Unix epoch".to_string()))?
                .as_secs() as i64,
        ),
        subject: presented.subject.clone(),
        issuer: presented.issuer.clone(),
        subject_alt_names: presented.subject_alt_names.clone(),
        not_before: presented.not_before.clone(),
        not_after: presented.not_after.clone(),
        fingerprint_sha256: presented.fingerprint_sha256.clone(),
        spki_sha256: presented.spki_sha256.clone(),
        address_matches_certificate: presented.address_matches_certificate,
        legacy_fingerprint_pinned: !presented.address_matches_certificate,
        tls_server_name,
        certificate_pem: presented.certificate_pem.clone(),
    })
}

pub fn persist_trust(record: &ApplianceTrustRecord) -> Result<PathBuf, TrustError> {
    let path = trust_record_path(&record.endpoint_host, record.endpoint_port)?;
    let parent = path
        .parent()
        .ok_or_else(|| TrustError::Invalid("trust record has no parent directory".to_string()))?;
    create_private_directory(parent)?;
    reject_unsafe_file(&path)?;
    let lock_path = parent.join(".enrollment.lock");
    reject_unsafe_file(&lock_path)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    restrict_private_file(&lock_path)?;
    lock.lock()?;
    if path.exists() {
        let existing: ApplianceTrustRecord = serde_json::from_slice(&fs::read(&path)?)?;
        if existing.fingerprint_sha256 != record.fingerprint_sha256 {
            return Err(TrustError::Invalid(
                "an appliance trust record already exists and cannot be replaced by authenticate; use trust rotate"
                    .to_string(),
            ));
        }
        return Ok(path);
    }
    atomic_write_private(&path, &serde_json::to_vec_pretty(record)?)?;
    Ok(path)
}

pub fn trust_record_path(host: &str, port: u16) -> Result<PathBuf, TrustError> {
    let root = trust_root()?;
    Ok(root
        .join(endpoint_directory_name(host, port))
        .join("trust.json"))
}

pub fn trust_root() -> Result<PathBuf, TrustError> {
    let base = if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(path)
    } else {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            TrustError::Invalid(
                "HOME is not set; pass --ca-cert for administrator-controlled trust".to_string(),
            )
        })?;
        PathBuf::from(home).join(".config")
    };
    Ok(base.join("dasobjectstore").join("trusted-appliances"))
}

pub fn list_trust() -> Result<Vec<(PathBuf, ApplianceTrustRecord)>, TrustError> {
    let root = trust_root()?;
    let mut records = Vec::new();
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(records),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_symlink() || !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join("trust.json");
        reject_unsafe_file(&path)?;
        if path.is_file() {
            records.push((path.clone(), serde_json::from_slice(&fs::read(path)?)?));
        }
    }
    records.sort_by(|left, right| left.1.appliance_id.cmp(&right.1.appliance_id));
    Ok(records)
}

pub fn remove_trust(appliance_id: &str) -> Result<PathBuf, TrustError> {
    let matching = list_trust()?
        .into_iter()
        .filter(|(_, record)| record.appliance_id == appliance_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(TrustError::Invalid(format!(
            "expected exactly one trust record for appliance ID {appliance_id}, found {}",
            matching.len()
        )));
    }
    let (path, _) = &matching[0];
    reject_unsafe_file(path)?;
    fs::remove_file(path)?;
    Ok(path.clone())
}

pub fn rotate_trust(
    appliance_id: &str,
    expected_fingerprint: &str,
) -> Result<ApplianceTrustRecord, TrustError> {
    let matching = list_trust()?
        .into_iter()
        .filter(|(_, record)| record.appliance_id == appliance_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(TrustError::Invalid(format!(
            "expected exactly one trust record for appliance ID {appliance_id}, found {}",
            matching.len()
        )));
    }
    let (path, existing) = &matching[0];
    let presented = probe_certificate(&existing.endpoint_host, existing.endpoint_port)?;
    expected_fingerprint_matches(expected_fingerprint, &presented)?;
    let mut replacement = new_trust_record(
        &existing.endpoint_host,
        existing.endpoint_port,
        None,
        &presented,
    )?;
    replacement.appliance_id = existing.appliance_id.clone();
    replacement.tls_server_name = presented.tls_server_name.clone().ok_or_else(|| {
        TrustError::Invalid(
            "replacement certificate exposes no usable endpoint or legacy TLS name".to_string(),
        )
    })?;
    let parent = path
        .parent()
        .ok_or_else(|| TrustError::Invalid("trust record has no parent directory".to_string()))?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(parent.join(".enrollment.lock"))?;
    restrict_private_file(&parent.join(".enrollment.lock"))?;
    lock.lock()?;
    reject_unsafe_file(path)?;
    let current: ApplianceTrustRecord = serde_json::from_slice(&fs::read(path)?)?;
    if current.fingerprint_sha256 != existing.fingerprint_sha256 {
        return Err(TrustError::Invalid(
            "trust changed concurrently; refusing certificate rotation".to_string(),
        ));
    }
    atomic_replace_private(path, &serde_json::to_vec_pretty(&replacement)?)?;
    Ok(replacement)
}

pub fn format_certificate_details(
    host: &str,
    port: u16,
    certificate: &PresentedCertificate,
    appliance_id: Option<&str>,
) -> String {
    format!(
        "DASObjectStore appliance certificate\nAppliance: {host}:{port}\nSubject: {}\nSANs: {}\nIssuer: {}\nValidity: {} through {}\nSHA-256 fingerprint: {}\nAddress match: {}\nAppliance identity: {}{}",
        certificate.subject,
        if certificate.subject_alt_names.is_empty() {
            "<none>".to_string()
        } else {
            certificate.subject_alt_names.join(", ")
        },
        certificate.issuer,
        certificate.not_before,
        certificate.not_after,
        certificate.fingerprint_sha256,
        if certificate.address_matches_certificate { "yes" } else { "no" },
        appliance_id.unwrap_or("<not yet available>"),
        if certificate.address_matches_certificate {
            ""
        } else {
            "\nConnection policy: legacy fingerprint-pinned certificate name mismatch"
        }
    )
}

pub fn pem_leaf_der(pem: &[u8]) -> Result<Vec<u8>, TrustError> {
    let mut reader = BufReader::new(pem);
    let certificate = rustls_pemfile::certs(&mut reader)
        .next()
        .transpose()
        .map_err(|error| TrustError::Invalid(format!("invalid PEM certificate: {error}")))?
        .map(|certificate| certificate.as_ref().to_vec())
        .ok_or_else(|| TrustError::Invalid("PEM contains no certificate".to_string()))?;
    Ok(certificate)
}

fn display_general_name(name: &GeneralName<'_>) -> Option<String> {
    match name {
        GeneralName::DNSName(value) => Some(format!("DNS:{value}")),
        GeneralName::IPAddress(bytes) if bytes.len() == 4 => Some(format!(
            "IP:{}.{}.{}.{}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )),
        GeneralName::IPAddress(bytes) if bytes.len() == 16 => {
            let octets: [u8; 16] = (*bytes).try_into().ok()?;
            Some(format!("IP:{}", std::net::Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

fn san_matches_host(sans: &[String], host: &str) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        sans.iter().any(|name| name == &format!("IP:{ip}"))
    } else {
        sans.iter()
            .filter_map(|name| name.strip_prefix("DNS:"))
            .any(|name| name.eq_ignore_ascii_case(host))
    }
}

fn parse_fingerprint(value: &str) -> Result<Vec<u8>, TrustError> {
    let compact = value
        .trim()
        .strip_prefix("SHA256:")
        .unwrap_or(value.trim())
        .chars()
        .filter(|character| *character != ':')
        .collect::<String>();
    if compact.len() != 64 || !compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TrustError::Invalid(
            "SHA-256 fingerprint must contain exactly 32 hexadecimal bytes".to_string(),
        ));
    }
    (0..compact.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&compact[offset..offset + 2], 16)
                .map_err(|_| TrustError::Invalid("invalid SHA-256 fingerprint".to_string()))
        })
        .collect()
}

fn formatted_sha256(bytes: &[u8]) -> String {
    canonical_fingerprint(Sha256::digest(bytes).as_slice())
}

fn canonical_fingerprint(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn pem_certificate(der: &[u8]) -> String {
    let encoded = STANDARD.encode(der);
    let body = encoded
        .as_bytes()
        .chunks(64)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n")
}

fn endpoint_identity(host: &str, port: u16) -> String {
    format!("endpoint:{}", endpoint_identity_suffix(host, port))
}

fn endpoint_identity_suffix(host: &str, port: u16) -> String {
    let digest = format!("{:x}", Sha256::digest(format!("{host}:{port}").as_bytes()));
    digest[..24].to_string()
}

fn endpoint_directory_name(host: &str, port: u16) -> String {
    let digest = format!("{:x}", Sha256::digest(format!("{host}:{port}").as_bytes()));
    format!("endpoint-{}", &digest[..24])
}

fn socket_endpoint(host: &str, port: u16) -> Result<String, TrustError> {
    if port == 0 || host.trim().is_empty() || host.contains('/') || host.contains('@') {
        return Err(TrustError::Invalid(
            "appliance host and TLS port are invalid".to_string(),
        ));
    }
    Ok(if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    })
}

fn validate_record_binding(
    record: &ApplianceTrustRecord,
    host: &str,
    port: u16,
) -> Result<(), TrustError> {
    if record.schema_version != TRUST_SCHEMA_VERSION
        || record.endpoint_host != host
        || record.endpoint_port != port
        || record.appliance_id.trim().is_empty()
        || record.certificate_pem.contains("PRIVATE KEY")
    {
        return Err(TrustError::Invalid(
            "appliance trust record is invalid or bound to another endpoint".to_string(),
        ));
    }
    let leaf = pem_leaf_der(record.certificate_pem())?;
    let fingerprint = formatted_sha256(&leaf);
    if fingerprint != record.fingerprint_sha256 {
        return Err(TrustError::Invalid(
            "appliance trust record certificate does not match its fingerprint".to_string(),
        ));
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), TrustError> {
    fs::create_dir_all(path)?;
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(TrustError::Invalid(
            "refusing to store appliance trust through a symlinked directory".to_string(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn restrict_private_file(path: &Path) -> Result<(), TrustError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn reject_unsafe_file(path: &Path) -> Result<(), TrustError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(TrustError::Invalid(
            "refusing to use a symlink as appliance trust state".to_string(),
        )),
        #[cfg(unix)]
        Ok(metadata)
            if {
                use std::os::unix::fs::MetadataExt;
                metadata.is_file() && metadata.nlink() > 1
            } =>
        {
            Err(TrustError::Invalid(
                "refusing to use hard-linked appliance trust state".to_string(),
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<(), TrustError> {
    let parent = path
        .parent()
        .ok_or_else(|| TrustError::Invalid("trust path has no parent".to_string()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".trust-{}-{nonce}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    if let Ok(directory) = fs::File::open(parent) {
        directory.sync_all()?;
    }
    Ok(())
}

fn atomic_replace_private(path: &Path, bytes: &[u8]) -> Result<(), TrustError> {
    let parent = path
        .parent()
        .ok_or_else(|| TrustError::Invalid("trust path has no parent".to_string()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".rotate-{}-{nonce}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    if let Ok(directory) = fs::File::open(parent) {
        directory.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_fingerprint, expected_fingerprint_matches, inspect_leaf_certificate,
        parse_fingerprint,
    };

    #[test]
    fn parses_colon_and_compact_fingerprints() {
        let bytes = vec![0xAB; 32];
        let canonical = canonical_fingerprint(&bytes);
        assert_eq!(parse_fingerprint(&canonical).unwrap(), bytes);
        assert_eq!(
            parse_fingerprint(&canonical.replace(':', "")).unwrap(),
            bytes
        );
        assert!(parse_fingerprint("AA:BB").is_err());
    }

    #[test]
    fn fingerprint_mismatch_is_rejected() {
        let presented = super::PresentedCertificate {
            leaf_der: vec![],
            certificate_pem: String::new(),
            subject: String::new(),
            issuer: String::new(),
            subject_alt_names: vec![],
            not_before: String::new(),
            not_after: String::new(),
            fingerprint_sha256: canonical_fingerprint(&[0x11; 32]),
            spki_sha256: String::new(),
            address_matches_certificate: false,
            tls_server_name: None,
        };
        assert!(
            expected_fingerprint_matches(&canonical_fingerprint(&[0x22; 32]), &presented).is_err()
        );
    }

    #[test]
    fn recognizes_ip_san_without_legacy_name_rewrite() {
        let generated =
            rcgen::generate_simple_self_signed(vec!["192.168.1.192".to_string()]).unwrap();
        let presented =
            inspect_leaf_certificate("192.168.1.192", generated.cert.der().as_ref()).unwrap();
        assert!(presented.address_matches_certificate);
        assert_eq!(presented.tls_server_name.as_deref(), Some("192.168.1.192"));
        assert!(presented
            .subject_alt_names
            .contains(&"IP:192.168.1.192".to_string()));
    }

    #[test]
    fn localhost_only_certificate_is_explicitly_legacy_for_remote_ip() {
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let presented =
            inspect_leaf_certificate("192.168.1.192", generated.cert.der().as_ref()).unwrap();
        assert!(!presented.address_matches_certificate);
        assert_eq!(presented.tls_server_name.as_deref(), Some("localhost"));
    }
}
