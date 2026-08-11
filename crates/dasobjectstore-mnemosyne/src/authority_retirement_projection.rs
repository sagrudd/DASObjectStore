//! Root-only composition of the accepted DAS authority-retirement projection.

use std::{
    collections::HashSet,
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Write as _,
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
};

use chrono::DateTime;
use prosopikon_core::{AssignmentStatus, DasReplacementSignerDiscoveryV1, ProductGrant};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::local_authority_retirement::LegacyAuthoritySurfaceObservationV1;

pub const GOVERNANCE_PATH: &str = "/var/lib/prosopikon/das-replacement/accepted-governance.v1.json";
pub const PROVENANCE_PATH: &str =
    "/etc/dasobjectstore/authority-retirement-projection-finalize-v1.json";
pub const OUTPUT_PATH: &str =
    "/var/lib/dasobjectstore/authority-retirement/accepted-projection.v1.json";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceProjectionV1 {
    pub schema: String,
    pub authority_id: Uuid,
    pub authority_revision: u64,
    pub reconciliation_outcome: u8,
    pub assignment_evidence_sha256: [u8; 32],
    pub mutation_evidence_sha256: [u8; 32],
    pub initial_receipt_sha256: [u8; 32],
    pub provider_verification_sha256: [u8; 32],
    pub invitation_evidence_sha256: [u8; 32],
    pub site_trust_domain_id: String,
    pub site_trust_state_revision: u64,
    pub custody_generation: String,
    pub principal_id: Uuid,
    pub assignment_id: Uuid,
    pub product_id: String,
    pub grant: ProductGrant,
    pub status: AssignmentStatus,
    pub signer_discovery: DasReplacementSignerDiscoveryV1,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceV1 {
    pub schema: String,
    pub monas_version: String,
    pub monas_source_revision_hex: String,
    pub monas_package_sha256_hex: String,
    pub prosopikon_version: String,
    pub prosopikon_source_revision_hex: String,
    pub prosopikon_artifact_sha256_hex: String,
}

#[derive(Serialize)]
struct AcceptedProjectionV1 {
    schema: &'static str,
    verifier: VerifierProjectionV1,
    monas_version: String,
    monas_source_revision: [u8; 20],
    monas_package_sha256: [u8; 32],
    prosopikon_version: String,
    prosopikon_source_revision: [u8; 20],
    prosopikon_artifact_sha256: [u8; 32],
    observation: LegacyAuthoritySurfaceObservationV1,
}

#[derive(Serialize)]
struct VerifierProjectionV1 {
    site_trust_domain_id: String,
    site_trust_state_revision: u64,
    authority_id: Uuid,
    custody_generation: String,
    key_generation: u64,
    key_id: [u8; 32],
    public_key_sec1: Vec<u8>,
    descriptor_sha256: [u8; 32],
    site_trust_anchor_sha256: [u8; 32],
    active: bool,
}

#[derive(Clone, Debug)]
pub struct ProjectionPathsV1 {
    pub governance: PathBuf,
    pub provenance: PathBuf,
    pub output: PathBuf,
    pub local_registry: PathBuf,
    pub server_config: PathBuf,
    pub registry_uid: u32,
    pub registry_gid: u32,
}

impl ProjectionPathsV1 {
    #[must_use]
    pub fn production() -> Option<Self> {
        let (registry_uid, registry_gid) = account_ids("dasobjectstore")?;
        Some(Self {
            governance: GOVERNANCE_PATH.into(),
            provenance: PROVENANCE_PATH.into(),
            output: OUTPUT_PATH.into(),
            local_registry: "/var/lib/dasobjectstore/auth/users.json".into(),
            server_config: "/opt/dasobjectstore/config.json".into(),
            registry_uid,
            registry_gid,
        })
    }
}

pub trait LegacySurfaceProbeV1 {
    fn service_disabled_inactive(&self) -> bool;
    fn legacy_listeners_absent(&self) -> bool;
    fn retired_process_absent(&self) -> bool;
    fn helpers_and_pam_absent(&self) -> bool;
}

/// Validates fixed governed inputs, observes local state, and atomically emits
/// the sole projection accepted by the retirement consumer.
pub fn finalize_authority_retirement_projection_v1(
    paths: &ProjectionPathsV1,
    probe: &impl LegacySurfaceProbeV1,
    now_unix: i64,
) -> Result<(), ()> {
    let governance_bytes = read_root_bytes(&paths.governance, 64 * 1024)?;
    let governance: GovernanceProjectionV1 =
        serde_json::from_slice(&governance_bytes).map_err(|_| ())?;
    if serde_jcs::to_vec(&governance).map_err(|_| ())? != governance_bytes {
        return Err(());
    }
    let provenance: ProvenanceV1 = read_root_json(&paths.provenance, 16 * 1024)?;
    validate_governance(&governance)?;
    validate_provenance(&provenance)?;
    let (live_sessions, live_registration_tokens) = observe_registry(
        &paths.local_registry,
        paths.registry_uid,
        paths.registry_gid,
        now_unix,
    )?;
    let monas_only = observe_monas_config(&paths.server_config)?;
    let disabled = probe.service_disabled_inactive();
    let listeners_absent = probe.legacy_listeners_absent();
    let process_absent = probe.retired_process_absent();
    let helpers_absent = probe.helpers_and_pam_absent();
    let observation = LegacyAuthoritySurfaceObservationV1 {
        standalone_service_disabled_inactive: disabled && process_absent,
        legacy_listeners_absent: listeners_absent && process_absent,
        monas_authority_selected_only: monas_only,
        legacy_routes_absent: disabled && listeners_absent && process_absent,
        legacy_helpers_and_pam_absent: helpers_absent,
        live_sessions,
        live_registration_tokens,
    };
    if !(observation.standalone_service_disabled_inactive
        && observation.legacy_listeners_absent
        && observation.monas_authority_selected_only
        && observation.legacy_routes_absent
        && observation.legacy_helpers_and_pam_absent
        && live_sessions == 0
        && live_registration_tokens == 0)
    {
        return Err(());
    }
    let descriptor = &governance.signer_discovery.descriptor;
    let accepted = AcceptedProjectionV1 {
        schema: "dasobjectstore.accepted-authority-retirement-projection.v1",
        verifier: VerifierProjectionV1 {
            site_trust_domain_id: governance.site_trust_domain_id,
            site_trust_state_revision: governance.site_trust_state_revision,
            authority_id: governance.authority_id,
            custody_generation: governance.custody_generation,
            key_generation: governance.signer_discovery.key_generation,
            key_id: descriptor.key_id,
            public_key_sec1: descriptor.public_key_sec1.clone(),
            descriptor_sha256: descriptor.descriptor_sha256,
            site_trust_anchor_sha256: governance.signer_discovery.site_trust_anchor_sha256,
            active: true,
        },
        monas_version: provenance.monas_version,
        monas_source_revision: decode_hex(&provenance.monas_source_revision_hex)?,
        monas_package_sha256: decode_hex(&provenance.monas_package_sha256_hex)?,
        prosopikon_version: provenance.prosopikon_version,
        prosopikon_source_revision: decode_hex(&provenance.prosopikon_source_revision_hex)?,
        prosopikon_artifact_sha256: decode_hex(&provenance.prosopikon_artifact_sha256_hex)?,
        observation,
    };
    let bytes = serde_json::to_vec(&accepted).map_err(|_| ())?;
    write_once_root(&paths.output, &bytes)
}

fn validate_governance(value: &GovernanceProjectionV1) -> Result<(), ()> {
    value.signer_discovery.validate().map_err(|_| ())?;
    if value.schema != "prosopikon.das-replacement-governance-projection.v1"
        || value.authority_id.is_nil()
        || value.authority_revision == 0
        || !matches!(value.reconciliation_outcome, 1 | 2)
        || [
            value.assignment_evidence_sha256,
            value.mutation_evidence_sha256,
            value.initial_receipt_sha256,
            value.provider_verification_sha256,
            value.invitation_evidence_sha256,
        ]
        .contains(&[0; 32])
        || value.site_trust_state_revision == 0
        || value.principal_id.is_nil()
        || value.assignment_id.is_nil()
        || value.product_id != "dasobjectstore"
        || value.grant != ProductGrant::Administer
        || value.status != AssignmentStatus::Active
        || value.signer_discovery.authority_id != value.authority_id
        || value.signer_discovery.site_trust_domain_id != value.site_trust_domain_id
        || value.signer_discovery.site_trust_state_revision != value.site_trust_state_revision
        || value.signer_discovery.custody_generation != value.custody_generation
        || value.signer_discovery.descriptor.public_key_sec1.len() != 33
    {
        return Err(());
    }
    Ok(())
}

fn validate_provenance(value: &ProvenanceV1) -> Result<(), ()> {
    if value.schema != "dasobjectstore.authority-retirement-projection-finalize.v1"
        || !semver(&value.monas_version)
        || !semver(&value.prosopikon_version)
    {
        return Err(());
    }
    let _: [u8; 20] = decode_hex(&value.monas_source_revision_hex)?;
    let _: [u8; 32] = decode_hex(&value.monas_package_sha256_hex)?;
    let _: [u8; 20] = decode_hex(&value.prosopikon_source_revision_hex)?;
    let _: [u8; 32] = decode_hex(&value.prosopikon_artifact_sha256_hex)?;
    Ok(())
}

fn semver(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && !value.contains(['+', '-'])
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part == &"0" || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn observe_monas_config(path: &Path) -> Result<bool, ()> {
    let (_, service_gid) = account_ids("dasobjectstore").ok_or(())?;
    observe_monas_config_for_identity(path, 0, service_gid)
}

fn observe_monas_config_for_identity(
    path: &Path,
    root_uid: u32,
    service_gid: u32,
) -> Result<bool, ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != root_uid
        || metadata.gid() != service_gid
        || metadata.permissions().mode() & 0o777 != 0o640
    {
        return Err(());
    }
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(|_| ())?).map_err(|_| ())?;
    Ok(value
        .pointer("/authentication/authority")
        .and_then(Value::as_str)
        == Some("monas"))
}

fn observe_registry(path: &Path, uid: u32, gid: u32, now: i64) -> Result<(u64, u64), ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o777 != 0o640
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.len() == 0
        || metadata.len() > 4 * 1024 * 1024
    {
        return Err(());
    }
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(|_| ())?).map_err(|_| ())?;
    exact_keys(
        &value,
        &[
            "schema_version",
            "users",
            "groups",
            "group_memberships",
            "rights",
            "device_tokens",
        ],
    )?;
    if value.get("schema_version").and_then(Value::as_u64) != Some(2) {
        return Err(());
    }
    if !value
        .get("device_tokens")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        return Err(());
    }
    let users = value.get("users").and_then(Value::as_array).ok_or(())?;
    let mut sessions = 0_u64;
    let mut registrations = 0_u64;
    let mut usernames = HashSet::new();
    let mut credential_hashes = HashSet::new();
    for user in users {
        exact_keys(
            user,
            &[
                "username",
                "created_at_utc",
                "password_hash",
                "registered_at_utc",
                "sudo_administrator",
                "registration_tokens",
                "sessions",
            ],
        )?;
        if !user
            .get("username")
            .and_then(Value::as_str)
            .is_some_and(|value| usernames.insert(value))
        {
            return Err(());
        }
        for session in user.get("sessions").and_then(Value::as_array).ok_or(())? {
            exact_keys(
                session,
                &[
                    "token_hash",
                    "issued_at_utc",
                    "expires_at_utc",
                    "revoked_at_utc",
                ],
            )?;
            if !session
                .get("token_hash")
                .and_then(Value::as_str)
                .is_some_and(|value| credential_hashes.insert(value))
            {
                return Err(());
            }
            let expires = timestamp(session.get("expires_at_utc").ok_or(())?)?;
            if session.get("revoked_at_utc").map_or(true, Value::is_null) && expires > now {
                sessions = sessions.checked_add(1).ok_or(())?;
            }
        }
        for token in user
            .get("registration_tokens")
            .and_then(Value::as_array)
            .ok_or(())?
        {
            exact_keys(
                token,
                &[
                    "token_hash",
                    "issued_at_utc",
                    "expires_at_utc",
                    "used_at_utc",
                ],
            )?;
            if !token
                .get("token_hash")
                .and_then(Value::as_str)
                .is_some_and(|value| credential_hashes.insert(value))
            {
                return Err(());
            }
            let expires = timestamp(token.get("expires_at_utc").ok_or(())?)?;
            if token.get("used_at_utc").map_or(true, Value::is_null) && expires > now {
                registrations = registrations.checked_add(1).ok_or(())?;
            }
        }
    }
    Ok((sessions, registrations))
}

fn exact_keys(value: &Value, expected: &[&str]) -> Result<(), ()> {
    let object = value.as_object().ok_or(())?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(());
    }
    Ok(())
}

fn timestamp(value: &Value) -> Result<i64, ()> {
    let text = value.as_str().ok_or(())?;
    DateTime::parse_from_rfc3339(text)
        .map(|value| value.timestamp())
        .map_err(|_| ())
}

fn account_ids(name: &str) -> Option<(u32, u32)> {
    let name = CString::new(name).ok()?;
    // SAFETY: the CString is NUL terminated; numeric fields are copied now.
    let record = unsafe { libc::getpwnam(name.as_ptr()) };
    if record.is_null() {
        return None;
    }
    // SAFETY: the pointer was checked and is not retained.
    let record = unsafe { &*record };
    (record.pw_uid != 0 && record.pw_gid != 0).then_some((record.pw_uid, record.pw_gid))
}

fn read_root_json<T: for<'de> Deserialize<'de>>(path: &Path, max: u64) -> Result<T, ()> {
    serde_json::from_slice(&read_root_bytes(path, max)?).map_err(|_| ())
}

fn read_root_bytes(path: &Path, max: u64) -> Result<Vec<u8>, ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > max
    {
        return Err(());
    }
    fs::read(path).map_err(|_| ())
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], ()> {
    if value.len() != N * 2 {
        return Err(());
    }
    let mut out = [0_u8; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| ())?;
    }
    if out.iter().all(|byte| *byte == 0) {
        return Err(());
    }
    Ok(out)
}

fn write_once_root(path: &Path, bytes: &[u8]) -> Result<(), ()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.is_file()
            && metadata.uid() == 0
            && metadata.gid() == 0
            && metadata.permissions().mode() & 0o777 == 0o600
            && fs::read(path).ok().as_deref() == Some(bytes)
        {
            Ok(())
        } else {
            Err(())
        };
    }
    let parent = path.parent().ok_or(())?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| ())?;
    if !parent_metadata.is_dir()
        || parent_metadata.uid() != 0
        || parent_metadata.gid() != 0
        || parent_metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(());
    }
    let temporary = parent.join(format!(".accepted-projection.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| ())?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| ())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| ())?;
        fs::rename(&temporary, path).map_err(|_| ())?;
        File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|_| ())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEBIAN_POSTINST: &str = include_str!("../../../packaging/debian/postinst");

    #[test]
    fn semver_and_hex_are_closed() {
        assert!(semver("1.2.3"));
        assert!(!semver("1.2.3-dev"));
        assert!(!semver("01.2.3"));
        assert!(decode_hex::<20>(&"ab".repeat(20)).is_ok());
        assert!(decode_hex::<20>(&"00".repeat(20)).is_err());
    }

    #[test]
    fn registry_counts_only_live_local_credentials() {
        let root = std::env::temp_dir().join(format!("das-observation-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("users.json");
        fs::write(&path, br#"{"schema_version":2,"users":[{"username":"owner","created_at_utc":"2020-01-01T00:00:00Z","password_hash":null,"registered_at_utc":null,"sudo_administrator":false,"sessions":[{"token_hash":"session-live","issued_at_utc":"2020-01-01T00:00:00Z","expires_at_utc":"2030-01-01T00:00:00Z","revoked_at_utc":null},{"token_hash":"session-expired","issued_at_utc":"2019-01-01T00:00:00Z","expires_at_utc":"2020-01-01T00:00:00Z","revoked_at_utc":null}],"registration_tokens":[{"token_hash":"registration-live","issued_at_utc":"2020-01-01T00:00:00Z","expires_at_utc":"2030-01-01T00:00:00Z","used_at_utc":null}]}],"groups":[],"group_memberships":[],"rights":[],"device_tokens":[]}"#).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(
            observe_registry(&path, metadata.uid(), metadata.gid(), 1_700_000_000).unwrap(),
            (1, 1)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_and_finalizer_share_server_config_metadata_contract() {
        assert!(
            DEBIAN_POSTINST.contains("chown root:\"$service_group\" \"$product_root/config.json\"")
        );
        assert!(DEBIAN_POSTINST.contains("chmod 0640 \"$product_root/config.json\""));

        let root = std::env::temp_dir().join(format!("das-monas-config-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("config.json");
        fs::write(&path, br#"{"authentication":{"authority":"monas"}}"#).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(
            observe_monas_config_for_identity(&path, metadata.uid(), metadata.gid()),
            Ok(true)
        );

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(observe_monas_config_for_identity(&path, metadata.uid(), metadata.gid()).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
