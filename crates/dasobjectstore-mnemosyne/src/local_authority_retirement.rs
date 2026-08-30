//! Package-owned verification and atomic retirement of legacy local authority.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    net::Shutdown,
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use coset::{iana, Algorithm, CborSerializable as _, CoseSign1};
use p256::ecdsa::{signature::Verifier as _, Signature, VerifyingKey};
use pistis_canonical::{from_slice, to_vec, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

const PURPOSE: &str = "monas.das-local-authority-replacement-receipt.v1";
const SIGNING_PURPOSE: &str = "das_local_authority_replacement_receipt";
const AUDIENCE: &str = "dasobjectstore-local-authority-retirement";
const PEER: &str = "org.mnemosyne.dasobjectstore.package.local-authority-retirement.v1";
/// Sole Monas receipt endpoint accepted by the package consumer.
pub const MONAS_DAS_REPLACEMENT_RECEIPT_SOCKET_V1: &str =
    "/run/mnemosyne-monas/das-replacement-receipt.v1.sock";
const REQUEST_MAGIC: &[u8; 5] = b"MDRQ\x01";
const RESPONSE_MAGIC: &[u8; 5] = b"MDRR\x01";
const MAX_RECEIPT: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(300);
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Root-only signer discovery projected through the accepted Site Trust path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DasReplacementVerifierRecordV1 {
    pub site_trust_domain_id: String,
    pub site_trust_state_revision: u64,
    pub authority_id: Uuid,
    pub custody_generation: String,
    pub key_generation: u64,
    pub key_id: [u8; 32],
    pub public_key_sec1: [u8; 33],
    pub descriptor_sha256: [u8; 32],
    pub site_trust_anchor_sha256: [u8; 32],
    pub active: bool,
}

/// Locally reserved challenge and exact producer pins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DasReplacementReceiptExpectationV1 {
    pub challenge: [u8; 32],
    pub now: u64,
    pub verifier: DasReplacementVerifierRecordV1,
    pub monas_version: String,
    pub monas_source_revision: [u8; 20],
    pub monas_package_sha256: [u8; 32],
    pub prosopikon_version: String,
    pub prosopikon_source_revision: [u8; 20],
    pub prosopikon_artifact_sha256: [u8; 32],
}

/// Verified redacted authority for one exact retirement transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedDasReplacementReceiptV1 {
    pub receipt_sha256: [u8; 32],
    pub challenge_digest: [u8; 32],
    pub authority_revision: u64,
}

/// Complete closed legacy-surface observation supplied by package probes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyAuthoritySurfaceObservationV1 {
    pub standalone_service_disabled_inactive: bool,
    pub legacy_listeners_absent: bool,
    pub monas_authority_selected_only: bool,
    pub legacy_routes_absent: bool,
    pub legacy_helpers_and_pam_absent: bool,
    pub live_sessions: u64,
    pub live_registration_tokens: u64,
}

impl LegacyAuthoritySurfaceObservationV1 {
    fn is_inactive(self) -> bool {
        self.standalone_service_disabled_inactive
            && self.legacy_listeners_absent
            && self.monas_authority_selected_only
            && self.legacy_routes_absent
            && self.legacy_helpers_and_pam_absent
            && self.live_sessions == 0
            && self.live_registration_tokens == 0
    }
}

/// Fixed filesystem boundary; production callers cannot select individual paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DasLocalAuthorityRetirementPathsV1 {
    source: PathBuf,
    archive_directory: PathBuf,
    transaction_directory: PathBuf,
    source_uid: u32,
    source_gid: u32,
    state_uid: u32,
}

impl DasLocalAuthorityRetirementPathsV1 {
    /// Exact package production paths.
    #[must_use]
    pub fn production(source_uid: u32, source_gid: u32) -> Option<Self> {
        if source_uid == 0 || source_gid == 0 {
            return None;
        }
        Some(Self {
            source: PathBuf::from("/var/lib/dasobjectstore/auth/users.json"),
            archive_directory: PathBuf::from("/var/lib/dasobjectstore/auth-retired"),
            transaction_directory: PathBuf::from("/var/lib/dasobjectstore/authority-retirement"),
            source_uid,
            source_gid,
            state_uid: 0,
        })
    }

    #[cfg(test)]
    fn fixture(root: &Path, uid: u32, gid: u32) -> Self {
        Self {
            source: root.join("auth/users.json"),
            archive_directory: root.join("auth-retired"),
            transaction_directory: root.join("authority-retirement"),
            source_uid: uid,
            source_gid: gid,
            state_uid: uid,
        }
    }
}

/// Coarse fail-closed verifier/retirement error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DasLocalAuthorityRetirementErrorV1 {
    ReceiptDenied,
    LegacySurfaceActive,
    UnsafeState,
    Conflict,
    IoUnavailable,
}

/// Requests the one-use receipt through the fixed package socket.
///
/// # Errors
///
/// Returns a coarse unavailable result for any connection, framing, timeout,
/// truncation, or bounds failure. The returned bytes still require full
/// [`verify_das_replacement_receipt_v1`] verification before use.
pub fn request_das_replacement_receipt_v1(
    challenge: [u8; 32],
) -> Result<Vec<u8>, DasLocalAuthorityRetirementErrorV1> {
    request_das_replacement_receipt_from_v1(
        Path::new(MONAS_DAS_REPLACEMENT_RECEIPT_SOCKET_V1),
        challenge,
    )
}

fn request_das_replacement_receipt_from_v1(
    socket: &Path,
    challenge: [u8; 32],
) -> Result<Vec<u8>, DasLocalAuthorityRetirementErrorV1> {
    request_das_replacement_receipt_within_v1(socket, challenge, IO_TIMEOUT)
}

fn request_das_replacement_receipt_within_v1(
    socket: &Path,
    challenge: [u8; 32],
    total_timeout: Duration,
) -> Result<Vec<u8>, DasLocalAuthorityRetirementErrorV1> {
    if challenge == [0; 32] {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    let deadline = Instant::now()
        .checked_add(total_timeout)
        .ok_or(DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(DasLocalAuthorityRetirementErrorV1::IoUnavailable);
        }
        if let Ok(receipt) = request_das_replacement_receipt_once_v1(socket, challenge, remaining) {
            return Ok(receipt);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(DasLocalAuthorityRetirementErrorV1::IoUnavailable);
        }
        thread::sleep(RETRY_DELAY.min(remaining));
    }
}

fn request_das_replacement_receipt_once_v1(
    socket: &Path,
    challenge: [u8; 32],
    timeout: Duration,
) -> Result<Vec<u8>, DasLocalAuthorityRetirementErrorV1> {
    let mut stream = UnixStream::connect(socket)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    stream
        .write_all(REQUEST_MAGIC)
        .and_then(|()| stream.write_all(&challenge))
        .and_then(|()| stream.flush())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    let mut prefix = [0_u8; 9];
    stream
        .read_exact(&mut prefix)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    let length = usize::try_from(u32::from_be_bytes(
        prefix[5..]
            .try_into()
            .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?,
    ))
    .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    if &prefix[..5] != RESPONSE_MAGIC || length == 0 || length > MAX_RECEIPT {
        return Err(DasLocalAuthorityRetirementErrorV1::IoUnavailable);
    }
    let mut receipt = vec![0_u8; length];
    stream
        .read_exact(&mut receipt)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    let mut trailing = [0_u8; 1];
    if stream
        .read(&mut trailing)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?
        != 0
    {
        return Err(DasLocalAuthorityRetirementErrorV1::IoUnavailable);
    }
    Ok(receipt)
}

/// Completion acknowledgement published only after durable archive rename.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DasLocalAuthorityRetirementCompletionV1 {
    pub schema: &'static str,
    pub transaction_id: String,
    pub source_sha256: String,
    pub receipt_sha256: String,
    pub authority_revision: u64,
    pub archive_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DasLocalAuthorityRetirementIntentV1 {
    schema: String,
    transaction_id: String,
    source_sha256: String,
    receipt_sha256: String,
    authority_revision: u64,
    archive_path: String,
}

/// Verifies the byte-exact attached receipt against trusted local discovery.
pub fn verify_das_replacement_receipt_v1(
    receipt: &[u8],
    expected: &DasReplacementReceiptExpectationV1,
) -> Result<VerifiedDasReplacementReceiptV1, DasLocalAuthorityRetirementErrorV1> {
    let cose = CoseSign1::from_slice(receipt)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    if cose.clone().to_vec().ok().as_deref() != Some(receipt)
        || !cose.unprotected.is_empty()
        || cose.protected.header.alg != Some(Algorithm::Assigned(iana::Algorithm::ES256))
        || cose.protected.header.key_id != expected.verifier.key_id
        || cose.signature.len() != 64
        || !expected.verifier.active
        || expected.verifier.key_generation == 0
        || expected.verifier.site_trust_state_revision == 0
        || expected.verifier.site_trust_anchor_sha256 == [0; 32]
    {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    let payload = cose
        .payload
        .as_deref()
        .ok_or(DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    let decoded =
        from_slice(payload).map_err(|_| DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    if to_vec(&decoded).ok().as_deref() != Some(payload) {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    let Value::Map(map) = decoded else {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    };
    let challenge_digest = challenge_digest(&expected.challenge);
    let evidence_digests = [10, 11, 12, 13, 14]
        .map(|key| bytes(&map, key).and_then(|value| <[u8; 32]>::try_from(value).ok()));
    let Some(evidence_digests) = evidence_digests.into_iter().collect::<Option<Vec<_>>>() else {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    };
    let evidence_set: [u8; 32] = Sha256::digest(
        [
            b"mnemosyne.monas.das-replacement.evidence-set.v1\0".as_slice(),
            evidence_digests[0].as_slice(),
            evidence_digests[1].as_slice(),
            evidence_digests[2].as_slice(),
            evidence_digests[3].as_slice(),
            evidence_digests[4].as_slice(),
        ]
        .concat(),
    )
    .into();
    if map.len() != 31
        || unsigned(&map, 0) != Some(1)
        || text(&map, 1) != Some(PURPOSE)
        || text(&map, 2) != Some(AUDIENCE)
        || text(&map, 3) != Some(PEER)
        || bytes(&map, 4) != Some(challenge_digest.as_slice())
        || bytes(&map, 5) != Some(expected.verifier.authority_id.as_bytes())
        || !matches!(unsigned(&map, 7), Some(1 | 2))
        || text(&map, 8) != Some("dasobjectstore")
        || text(&map, 9) != Some("Administer")
        || evidence_digests.contains(&[0; 32])
        || bytes(&map, 15) != Some(evidence_set.as_slice())
        || text(&map, 16) != Some(expected.verifier.site_trust_domain_id.as_str())
        || unsigned(&map, 17) != Some(expected.verifier.site_trust_state_revision)
        || text(&map, 18) != Some(expected.verifier.custody_generation.as_str())
        || text(&map, 19) != Some(SIGNING_PURPOSE)
        || bytes(&map, 20) != Some(expected.verifier.key_id.as_slice())
        || unsigned(&map, 21) != Some(expected.verifier.key_generation)
        || bytes(&map, 22) != Some(expected.verifier.descriptor_sha256.as_slice())
        || text(&map, 25) != Some(expected.monas_version.as_str())
        || bytes(&map, 26) != Some(expected.monas_source_revision.as_slice())
        || bytes(&map, 27) != Some(expected.monas_package_sha256.as_slice())
        || text(&map, 28) != Some(expected.prosopikon_version.as_str())
        || bytes(&map, 29) != Some(expected.prosopikon_source_revision.as_slice())
        || bytes(&map, 30) != Some(expected.prosopikon_artifact_sha256.as_slice())
    {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    let issued = unsigned(&map, 23).ok_or(DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    let expires = unsigned(&map, 24).ok_or(DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    if issued == 0
        || expires <= issued
        || expires - issued > 60
        || expected.now < issued
        || expected.now >= expires
    {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    let authority_revision =
        unsigned(&map, 6).ok_or(DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    if authority_revision == 0 {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    let verifying = VerifyingKey::from_sec1_bytes(&expected.verifier.public_key_sec1)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    let signature = Signature::from_slice(&cose.signature)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    if signature.normalize_s().is_some() {
        return Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied);
    }
    cose.verify_signature(&[], |sig, tbs| {
        let parsed = Signature::from_slice(sig).map_err(|_| ())?;
        verifying.verify(tbs, &parsed).map_err(|_| ())
    })
    .map_err(|_| DasLocalAuthorityRetirementErrorV1::ReceiptDenied)?;
    Ok(VerifiedDasReplacementReceiptV1 {
        receipt_sha256: Sha256::digest(receipt).into(),
        challenge_digest,
        authority_revision,
    })
}

/// Resumes the exact pre-archive crash window recorded by a durable intent.
///
/// The intent is root-owned, write-once evidence produced only after receipt
/// verification. Recovery binds it back to the locally reserved challenge and
/// current legacy bytes; it never requests or accepts a second approval.
pub fn resume_local_authority_retirement_v1(
    paths: &DasLocalAuthorityRetirementPathsV1,
    challenge: [u8; 32],
    observation: LegacyAuthoritySurfaceObservationV1,
) -> Result<Option<DasLocalAuthorityRetirementCompletionV1>, DasLocalAuthorityRetirementErrorV1> {
    if !observation.is_inactive() {
        return Err(DasLocalAuthorityRetirementErrorV1::LegacySurfaceActive);
    }
    let transaction_id = hex(&challenge_digest(&challenge));
    let archive = paths
        .archive_directory
        .join(format!("{transaction_id}.users.json"));
    let intent = paths
        .transaction_directory
        .join(format!("{transaction_id}.intent.json"));
    let manifest = paths
        .transaction_directory
        .join(format!("{transaction_id}.complete.json"));
    let temporary = paths
        .transaction_directory
        .join(format!(".{transaction_id}.complete.tmp"));
    let intent_meta = match fs::symlink_metadata(&intent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DasLocalAuthorityRetirementErrorV1::IoUnavailable),
    };
    if !intent_meta.is_file()
        || intent_meta.uid() != paths.state_uid
        || intent_meta.permissions().mode() & 0o777 != 0o600
        || intent_meta.nlink() != 1
        || !(1..=4096).contains(&intent_meta.len())
        || archive.exists()
        || manifest.exists()
        || temporary.exists()
    {
        return Err(DasLocalAuthorityRetirementErrorV1::Conflict);
    }
    let source_meta = safe_file(&paths.source, paths.source_uid, paths.source_gid, 0o640)?;
    let archive_meta = safe_directory(&paths.archive_directory, paths.state_uid, 0o700)?;
    let transaction_meta = safe_directory(&paths.transaction_directory, paths.state_uid, 0o700)?;
    if source_meta.dev() != archive_meta.dev() || source_meta.dev() != transaction_meta.dev() {
        return Err(DasLocalAuthorityRetirementErrorV1::UnsafeState);
    }
    let source_sha256: [u8; 32] = Sha256::digest(
        fs::read(&paths.source).map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?,
    )
    .into();
    let retained: DasLocalAuthorityRetirementIntentV1 = serde_json::from_slice(
        &fs::read(&intent).map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?,
    )
    .map_err(|_| DasLocalAuthorityRetirementErrorV1::UnsafeState)?;
    let expected_archive = archive.to_string_lossy().into_owned();
    if retained.schema != "dasobjectstore.local-authority-retirement-completion.v1"
        || retained.transaction_id != transaction_id
        || retained.source_sha256 != hex(&source_sha256)
        || !is_lower_hex_32(&retained.receipt_sha256)
        || retained.authority_revision == 0
        || retained.archive_path != expected_archive
    {
        return Err(DasLocalAuthorityRetirementErrorV1::UnsafeState);
    }
    let completion = DasLocalAuthorityRetirementCompletionV1 {
        schema: "dasobjectstore.local-authority-retirement-completion.v1",
        transaction_id: retained.transaction_id,
        source_sha256: retained.source_sha256,
        receipt_sha256: retained.receipt_sha256,
        authority_revision: retained.authority_revision,
        archive_path: retained.archive_path,
    };
    finish_retirement_v1(
        paths,
        &source_meta,
        source_sha256,
        &completion,
        &archive,
        &manifest,
        &temporary,
    )?;
    Ok(Some(completion))
}

/// Atomically preserves the inactive registry and publishes completion.
pub fn retire_local_authority_v1(
    paths: &DasLocalAuthorityRetirementPathsV1,
    receipt: &VerifiedDasReplacementReceiptV1,
    observation: LegacyAuthoritySurfaceObservationV1,
) -> Result<DasLocalAuthorityRetirementCompletionV1, DasLocalAuthorityRetirementErrorV1> {
    if !observation.is_inactive() {
        return Err(DasLocalAuthorityRetirementErrorV1::LegacySurfaceActive);
    }
    let source_meta = safe_file(&paths.source, paths.source_uid, paths.source_gid, 0o640)?;
    let archive_meta = safe_directory(&paths.archive_directory, paths.state_uid, 0o700)?;
    let transaction_meta = safe_directory(&paths.transaction_directory, paths.state_uid, 0o700)?;
    if source_meta.dev() != archive_meta.dev() || source_meta.dev() != transaction_meta.dev() {
        return Err(DasLocalAuthorityRetirementErrorV1::UnsafeState);
    }
    let source_bytes =
        fs::read(&paths.source).map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    let source_sha256: [u8; 32] = Sha256::digest(&source_bytes).into();
    let transaction_id = hex(&receipt.challenge_digest);
    let archive = paths
        .archive_directory
        .join(format!("{transaction_id}.users.json"));
    let intent = paths
        .transaction_directory
        .join(format!("{transaction_id}.intent.json"));
    let manifest = paths
        .transaction_directory
        .join(format!("{transaction_id}.complete.json"));
    let temporary = paths
        .transaction_directory
        .join(format!(".{transaction_id}.complete.tmp"));
    if archive.exists() || intent.exists() || manifest.exists() || temporary.exists() {
        return Err(DasLocalAuthorityRetirementErrorV1::Conflict);
    }
    let completion = DasLocalAuthorityRetirementCompletionV1 {
        schema: "dasobjectstore.local-authority-retirement-completion.v1",
        transaction_id,
        source_sha256: hex(&source_sha256),
        receipt_sha256: hex(&receipt.receipt_sha256),
        authority_revision: receipt.authority_revision,
        archive_path: archive.to_string_lossy().into_owned(),
    };
    write_new_fsync(
        &intent,
        &serde_json::to_vec(&completion)
            .map_err(|_| DasLocalAuthorityRetirementErrorV1::UnsafeState)?,
    )?;
    sync_dir(&paths.transaction_directory)?;
    finish_retirement_v1(
        paths,
        &source_meta,
        source_sha256,
        &completion,
        &archive,
        &manifest,
        &temporary,
    )?;
    Ok(completion)
}

fn finish_retirement_v1(
    paths: &DasLocalAuthorityRetirementPathsV1,
    source_meta: &fs::Metadata,
    source_sha256: [u8; 32],
    completion: &DasLocalAuthorityRetirementCompletionV1,
    archive: &Path,
    manifest: &Path,
    temporary: &Path,
) -> Result<(), DasLocalAuthorityRetirementErrorV1> {
    let current_source_sha256: [u8; 32] = Sha256::digest(
        fs::read(&paths.source).map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?,
    )
    .into();
    if fs::symlink_metadata(&paths.source)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::UnsafeState)?
        .ino()
        != source_meta.ino()
        || current_source_sha256 != source_sha256
    {
        return Err(DasLocalAuthorityRetirementErrorV1::UnsafeState);
    }
    fs::rename(&paths.source, archive)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    sync_dir(
        paths
            .source
            .parent()
            .ok_or(DasLocalAuthorityRetirementErrorV1::UnsafeState)?,
    )?;
    sync_dir(&paths.archive_directory)?;
    write_new_fsync(
        temporary,
        &serde_json::to_vec(&completion)
            .map_err(|_| DasLocalAuthorityRetirementErrorV1::UnsafeState)?,
    )?;
    fs::rename(temporary, manifest)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)?;
    sync_dir(&paths.transaction_directory)?;
    Ok(())
}

fn safe_file(
    path: &Path,
    uid: u32,
    gid: u32,
    mode: u32,
) -> Result<fs::Metadata, DasLocalAuthorityRetirementErrorV1> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| DasLocalAuthorityRetirementErrorV1::UnsafeState)?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.permissions().mode() & 0o777 != mode
    {
        return Err(DasLocalAuthorityRetirementErrorV1::UnsafeState);
    }
    Ok(metadata)
}

fn safe_directory(
    path: &Path,
    uid: u32,
    mode: u32,
) -> Result<fs::Metadata, DasLocalAuthorityRetirementErrorV1> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| DasLocalAuthorityRetirementErrorV1::UnsafeState)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.permissions().mode() & 0o777 != mode
    {
        return Err(DasLocalAuthorityRetirementErrorV1::UnsafeState);
    }
    Ok(metadata)
}

fn write_new_fsync(path: &Path, bytes: &[u8]) -> Result<(), DasLocalAuthorityRetirementErrorV1> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::Conflict)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)
}

fn sync_dir(path: &Path) -> Result<(), DasLocalAuthorityRetirementErrorV1> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| DasLocalAuthorityRetirementErrorV1::IoUnavailable)
}

fn challenge_digest(challenge: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"mnemosyne.monas.das-replacement.challenge.v1\0");
    digest.update(32_u32.to_be_bytes());
    digest.update(challenge);
    digest.update((PEER.len() as u32).to_be_bytes());
    digest.update(PEER.as_bytes());
    digest.update((AUDIENCE.len() as u32).to_be_bytes());
    digest.update(AUDIENCE.as_bytes());
    digest.finalize().into()
}

fn text(map: &std::collections::BTreeMap<u64, Value>, key: u64) -> Option<&str> {
    match map.get(&key) {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}
fn bytes(map: &std::collections::BTreeMap<u64, Value>, key: u64) -> Option<&[u8]> {
    match map.get(&key) {
        Some(Value::Bytes(value)) => Some(value),
        _ => None,
    }
}
fn unsigned(map: &std::collections::BTreeMap<u64, Value>, key: u64) -> Option<u64> {
    match map.get(&key) {
        Some(Value::Unsigned(value)) => Some(*value),
        _ => None,
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_lower_hex_32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::{fs::PermissionsExt as _, net::UnixListener},
        thread,
    };

    use coset::{CoseSign1Builder, HeaderBuilder};
    use p256::ecdsa::{signature::Signer as _, Signature, SigningKey};

    use super::*;

    fn expectation(signing: &SigningKey) -> DasReplacementReceiptExpectationV1 {
        let public_key_sec1 = signing
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .try_into()
            .unwrap();
        DasReplacementReceiptExpectationV1 {
            challenge: [7; 32],
            now: 101,
            verifier: DasReplacementVerifierRecordV1 {
                site_trust_domain_id: "site-1".to_owned(),
                site_trust_state_revision: 4,
                authority_id: Uuid::from_u128(1),
                custody_generation: "custody-1".to_owned(),
                key_generation: 2,
                key_id: [8; 32],
                public_key_sec1,
                descriptor_sha256: [9; 32],
                site_trust_anchor_sha256: [10; 32],
                active: true,
            },
            monas_version: "0.86.0".to_owned(),
            monas_source_revision: [11; 20],
            monas_package_sha256: [12; 32],
            prosopikon_version: "0.25.0".to_owned(),
            prosopikon_source_revision: [13; 20],
            prosopikon_artifact_sha256: [14; 32],
        }
    }

    fn receipt(signing: &SigningKey, expected: &DasReplacementReceiptExpectationV1) -> Vec<u8> {
        let evidence = [[21; 32], [22; 32], [23; 32], [24; 32], [25; 32]];
        let evidence_set: [u8; 32] = Sha256::digest(
            [
                b"mnemosyne.monas.das-replacement.evidence-set.v1\0".as_slice(),
                &evidence[0],
                &evidence[1],
                &evidence[2],
                &evidence[3],
                &evidence[4],
            ]
            .concat(),
        )
        .into();
        let payload = to_vec(&Value::Map(
            vec![
                (0, Value::Unsigned(1)),
                (1, Value::Text(PURPOSE.to_owned())),
                (2, Value::Text(AUDIENCE.to_owned())),
                (3, Value::Text(PEER.to_owned())),
                (
                    4,
                    Value::Bytes(challenge_digest(&expected.challenge).to_vec()),
                ),
                (
                    5,
                    Value::Bytes(expected.verifier.authority_id.as_bytes().to_vec()),
                ),
                (6, Value::Unsigned(3)),
                (7, Value::Unsigned(1)),
                (8, Value::Text("dasobjectstore".to_owned())),
                (9, Value::Text("Administer".to_owned())),
                (10, Value::Bytes(evidence[0].to_vec())),
                (11, Value::Bytes(evidence[1].to_vec())),
                (12, Value::Bytes(evidence[2].to_vec())),
                (13, Value::Bytes(evidence[3].to_vec())),
                (14, Value::Bytes(evidence[4].to_vec())),
                (15, Value::Bytes(evidence_set.to_vec())),
                (
                    16,
                    Value::Text(expected.verifier.site_trust_domain_id.clone()),
                ),
                (
                    17,
                    Value::Unsigned(expected.verifier.site_trust_state_revision),
                ),
                (
                    18,
                    Value::Text(expected.verifier.custody_generation.clone()),
                ),
                (19, Value::Text(SIGNING_PURPOSE.to_owned())),
                (20, Value::Bytes(expected.verifier.key_id.to_vec())),
                (21, Value::Unsigned(expected.verifier.key_generation)),
                (
                    22,
                    Value::Bytes(expected.verifier.descriptor_sha256.to_vec()),
                ),
                (23, Value::Unsigned(100)),
                (24, Value::Unsigned(160)),
                (25, Value::Text(expected.monas_version.clone())),
                (26, Value::Bytes(expected.monas_source_revision.to_vec())),
                (27, Value::Bytes(expected.monas_package_sha256.to_vec())),
                (28, Value::Text(expected.prosopikon_version.clone())),
                (
                    29,
                    Value::Bytes(expected.prosopikon_source_revision.to_vec()),
                ),
                (
                    30,
                    Value::Bytes(expected.prosopikon_artifact_sha256.to_vec()),
                ),
            ]
            .into_iter()
            .collect(),
        ))
        .unwrap();
        CoseSign1Builder::new()
            .protected(
                HeaderBuilder::new()
                    .algorithm(iana::Algorithm::ES256)
                    .key_id(expected.verifier.key_id.to_vec())
                    .build(),
            )
            .payload(payload)
            .create_signature(&[], |tbs| {
                let signature: Signature = signing.sign(tbs);
                signature
                    .normalize_s()
                    .unwrap_or(signature)
                    .to_bytes()
                    .to_vec()
            })
            .build()
            .to_vec()
            .unwrap()
    }

    #[test]
    fn verifies_exact_receipt_and_denies_changed_expectation() {
        let signing = SigningKey::from_bytes((&[3; 32]).into()).unwrap();
        let expected = expectation(&signing);
        let bytes = receipt(&signing, &expected);
        let verified = verify_das_replacement_receipt_v1(&bytes, &expected).unwrap();
        assert_eq!(verified.authority_revision, 3);

        let mut changed = expected;
        changed.monas_package_sha256 = [99; 32];
        assert_eq!(
            verify_das_replacement_receipt_v1(&bytes, &changed),
            Err(DasLocalAuthorityRetirementErrorV1::ReceiptDenied)
        );
    }

    #[test]
    fn fixed_client_accepts_only_the_exact_bounded_response() {
        let root = PathBuf::from("/tmp").join(format!("das-receipt-client-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let socket = root.join("monas.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 37];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request[..5], REQUEST_MAGIC);
            assert_eq!(&request[5..], &[7; 32]);
            stream.write_all(RESPONSE_MAGIC).unwrap();
            stream.write_all(&7_u32.to_be_bytes()).unwrap();
            stream.write_all(b"receipt").unwrap();
        });
        assert_eq!(
            request_das_replacement_receipt_from_v1(&socket, [7; 32]).unwrap(),
            b"receipt"
        );
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixed_client_retries_the_exact_challenge_after_lost_response() {
        let root = PathBuf::from("/tmp").join(format!("das-receipt-retry-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let socket = root.join("monas.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 37];
                stream.read_exact(&mut request).unwrap();
                assert_eq!(&request[..5], REQUEST_MAGIC);
                assert_eq!(&request[5..], &[7; 32]);
                if attempt == 1 {
                    stream.write_all(RESPONSE_MAGIC).unwrap();
                    stream.write_all(&7_u32.to_be_bytes()).unwrap();
                    stream.write_all(b"receipt").unwrap();
                }
            }
        });
        assert_eq!(
            request_das_replacement_receipt_within_v1(&socket, [7; 32], Duration::from_secs(3),)
                .unwrap(),
            b"receipt"
        );
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retires_only_a_fully_inactive_surface_and_retains_bytes() {
        let root = std::env::temp_dir().join(format!("das-retirement-{}", Uuid::new_v4()));
        let auth = root.join("auth");
        fs::create_dir_all(&auth).unwrap();
        fs::create_dir(root.join("auth-retired")).unwrap();
        fs::create_dir(root.join("authority-retirement")).unwrap();
        for directory in [
            &root,
            &auth,
            &root.join("auth-retired"),
            &root.join("authority-retirement"),
        ] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let source = auth.join("users.json");
        fs::write(&source, b"legacy-authority").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
        let metadata = fs::metadata(&source).unwrap();
        let paths =
            DasLocalAuthorityRetirementPathsV1::fixture(&root, metadata.uid(), metadata.gid());
        let verified = VerifiedDasReplacementReceiptV1 {
            receipt_sha256: [31; 32],
            challenge_digest: [32; 32],
            authority_revision: 5,
        };
        let inactive = LegacyAuthoritySurfaceObservationV1 {
            standalone_service_disabled_inactive: true,
            legacy_listeners_absent: true,
            monas_authority_selected_only: true,
            legacy_routes_absent: true,
            legacy_helpers_and_pam_absent: true,
            live_sessions: 0,
            live_registration_tokens: 0,
        };
        let completion = retire_local_authority_v1(&paths, &verified, inactive).unwrap();
        assert!(!source.exists());
        assert_eq!(
            fs::read(completion.archive_path).unwrap(),
            b"legacy-authority"
        );
        assert!(root
            .join("authority-retirement")
            .join(format!("{}.complete.json", completion.transaction_id))
            .exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resumes_root_owned_intent_without_requesting_a_second_receipt() {
        let root = std::env::temp_dir().join(format!("das-retirement-resume-{}", Uuid::new_v4()));
        let auth = root.join("auth");
        fs::create_dir_all(&auth).unwrap();
        fs::create_dir(root.join("auth-retired")).unwrap();
        fs::create_dir(root.join("authority-retirement")).unwrap();
        for directory in [
            &root,
            &auth,
            &root.join("auth-retired"),
            &root.join("authority-retirement"),
        ] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let source = auth.join("users.json");
        fs::write(&source, b"legacy-authority").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
        let metadata = fs::metadata(&source).unwrap();
        let paths =
            DasLocalAuthorityRetirementPathsV1::fixture(&root, metadata.uid(), metadata.gid());
        let challenge = [41; 32];
        let transaction_id = hex(&challenge_digest(&challenge));
        let archive = root
            .join("auth-retired")
            .join(format!("{transaction_id}.users.json"));
        let completion = DasLocalAuthorityRetirementCompletionV1 {
            schema: "dasobjectstore.local-authority-retirement-completion.v1",
            transaction_id: transaction_id.clone(),
            source_sha256: hex(&Sha256::digest(b"legacy-authority")),
            receipt_sha256: hex(&[31; 32]),
            authority_revision: 5,
            archive_path: archive.to_string_lossy().into_owned(),
        };
        write_new_fsync(
            &root
                .join("authority-retirement")
                .join(format!("{transaction_id}.intent.json")),
            &serde_json::to_vec(&completion).unwrap(),
        )
        .unwrap();
        let inactive = LegacyAuthoritySurfaceObservationV1 {
            standalone_service_disabled_inactive: true,
            legacy_listeners_absent: true,
            monas_authority_selected_only: true,
            legacy_routes_absent: true,
            legacy_helpers_and_pam_absent: true,
            live_sessions: 0,
            live_registration_tokens: 0,
        };

        let resumed = resume_local_authority_retirement_v1(&paths, challenge, inactive)
            .unwrap()
            .unwrap();

        assert_eq!(resumed, completion);
        assert!(!source.exists());
        assert_eq!(fs::read(archive).unwrap(), b"legacy-authority");
        assert!(root
            .join("authority-retirement")
            .join(format!("{transaction_id}.complete.json"))
            .exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn production_source_identity_is_explicit_and_wrong_metadata_denies() {
        assert!(DasLocalAuthorityRetirementPathsV1::production(0, 100).is_none());
        assert!(DasLocalAuthorityRetirementPathsV1::production(100, 0).is_none());

        let root = std::env::temp_dir().join(format!("das-retirement-meta-{}", Uuid::new_v4()));
        let auth = root.join("auth");
        fs::create_dir_all(&auth).unwrap();
        fs::create_dir(root.join("auth-retired")).unwrap();
        fs::create_dir(root.join("authority-retirement")).unwrap();
        for directory in [
            &root,
            &auth,
            &root.join("auth-retired"),
            &root.join("authority-retirement"),
        ] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let source = auth.join("users.json");
        fs::write(&source, b"legacy-authority").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let metadata = fs::metadata(&source).unwrap();
        let paths =
            DasLocalAuthorityRetirementPathsV1::fixture(&root, metadata.uid(), metadata.gid());
        let verified = VerifiedDasReplacementReceiptV1 {
            receipt_sha256: [31; 32],
            challenge_digest: [33; 32],
            authority_revision: 5,
        };
        let inactive = LegacyAuthoritySurfaceObservationV1 {
            standalone_service_disabled_inactive: true,
            legacy_listeners_absent: true,
            monas_authority_selected_only: true,
            legacy_routes_absent: true,
            legacy_helpers_and_pam_absent: true,
            live_sessions: 0,
            live_registration_tokens: 0,
        };
        assert_eq!(
            retire_local_authority_v1(&paths, &verified, inactive),
            Err(DasLocalAuthorityRetirementErrorV1::UnsafeState)
        );
        assert_eq!(fs::read(source).unwrap(), b"legacy-authority");
        fs::remove_dir_all(root).unwrap();
    }
}
