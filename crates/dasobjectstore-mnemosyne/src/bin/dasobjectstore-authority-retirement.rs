use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use dasobjectstore_mnemosyne::local_authority_retirement::{
    request_das_replacement_receipt_v1, retire_local_authority_v1,
    verify_das_replacement_receipt_v1, DasLocalAuthorityRetirementPathsV1,
    DasReplacementReceiptExpectationV1, DasReplacementVerifierRecordV1,
    LegacyAuthoritySurfaceObservationV1,
};
use serde::Deserialize;
use uuid::Uuid;

const PROJECTION: &str = "/var/lib/dasobjectstore/authority-retirement/accepted-projection.v1.json";
const RESERVATION: &str = "/var/lib/dasobjectstore/authority-retirement/challenge-reservation.v1";
const RANDOM: &str = "/dev/urandom";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedProjectionV1 {
    schema: String,
    verifier: VerifierProjectionV1,
    monas_version: String,
    monas_source_revision: [u8; 20],
    monas_package_sha256: [u8; 32],
    prosopikon_version: String,
    prosopikon_source_revision: [u8; 20],
    prosopikon_artifact_sha256: [u8; 32],
    observation: LegacyAuthoritySurfaceObservationV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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

fn main() {
    if std::env::args_os().len() != 1 || run().is_err() {
        eprintln!("DAS authority retirement unavailable");
        std::process::exit(1);
    }
}

fn run() -> Result<(), ()> {
    let (service_uid, service_gid) = service_account_ids()?;
    let projection = load_root_projection()?;
    let challenge = reserve_challenge()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ())?
        .as_secs();
    let expected = DasReplacementReceiptExpectationV1 {
        challenge,
        now,
        verifier: DasReplacementVerifierRecordV1 {
            site_trust_domain_id: projection.verifier.site_trust_domain_id,
            site_trust_state_revision: projection.verifier.site_trust_state_revision,
            authority_id: projection.verifier.authority_id,
            custody_generation: projection.verifier.custody_generation,
            key_generation: projection.verifier.key_generation,
            key_id: projection.verifier.key_id,
            public_key_sec1: projection
                .verifier
                .public_key_sec1
                .try_into()
                .map_err(|_| ())?,
            descriptor_sha256: projection.verifier.descriptor_sha256,
            site_trust_anchor_sha256: projection.verifier.site_trust_anchor_sha256,
            active: projection.verifier.active,
        },
        monas_version: projection.monas_version,
        monas_source_revision: projection.monas_source_revision,
        monas_package_sha256: projection.monas_package_sha256,
        prosopikon_version: projection.prosopikon_version,
        prosopikon_source_revision: projection.prosopikon_source_revision,
        prosopikon_artifact_sha256: projection.prosopikon_artifact_sha256,
    };
    let receipt = request_das_replacement_receipt_v1(challenge).map_err(|_| ())?;
    let verified = verify_das_replacement_receipt_v1(&receipt, &expected).map_err(|_| ())?;
    let completion = retire_local_authority_v1(
        &DasLocalAuthorityRetirementPathsV1::production(service_uid, service_gid).ok_or(())?,
        &verified,
        projection.observation,
    )
    .map_err(|_| ())?;
    println!("{}", serde_json::to_string(&completion).map_err(|_| ())?);
    Ok(())
}

fn service_account_ids() -> Result<(u32, u32), ()> {
    let account = CString::new("dasobjectstore").map_err(|_| ())?;
    // SAFETY: `account` is a live NUL-terminated string. We copy the numeric
    // fields immediately and never retain the libc-owned passwd pointer.
    let record = unsafe { libc::getpwnam(account.as_ptr()) };
    if record.is_null() {
        return Err(());
    }
    // SAFETY: non-null was checked above and the fields are copied immediately.
    let record = unsafe { &*record };
    if record.pw_uid == 0 || record.pw_gid == 0 {
        return Err(());
    }
    Ok((record.pw_uid, record.pw_gid))
}

fn load_root_projection() -> Result<AcceptedProjectionV1, ()> {
    let path = Path::new(PROJECTION);
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > 16 * 1024
    {
        return Err(());
    }
    let bytes = fs::read(path).map_err(|_| ())?;
    let projection: AcceptedProjectionV1 = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if projection.schema != "dasobjectstore.accepted-authority-retirement-projection.v1" {
        return Err(());
    }
    Ok(projection)
}

fn reserve_challenge() -> Result<[u8; 32], ()> {
    reserve_challenge_from(Path::new(RESERVATION), Path::new(RANDOM), 0, 0)
}

fn reserve_challenge_from(
    reservation_path: &Path,
    random_path: &Path,
    state_uid: u32,
    state_gid: u32,
) -> Result<[u8; 32], ()> {
    if let Ok(metadata) = fs::symlink_metadata(reservation_path) {
        if !metadata.is_file()
            || metadata.uid() != state_uid
            || metadata.gid() != state_gid
            || metadata.permissions().mode() & 0o777 != 0o600
            || metadata.len() != 65
        {
            return Err(());
        }
        let encoded = fs::read(reservation_path).map_err(|_| ())?;
        if encoded.len() != 65
            || encoded[64] != b'\n'
            || !encoded[..64]
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(());
        }
        let mut challenge = [0_u8; 32];
        for (index, byte) in challenge.iter_mut().enumerate() {
            *byte = u8::from_str_radix(
                std::str::from_utf8(&encoded[index * 2..index * 2 + 2]).map_err(|_| ())?,
                16,
            )
            .map_err(|_| ())?;
        }
        return (challenge != [0; 32]).then_some(challenge).ok_or(());
    }
    let mut challenge = [0_u8; 32];
    File::open(random_path)
        .and_then(|mut file| file.read_exact(&mut challenge))
        .map_err(|_| ())?;
    if challenge == [0; 32] {
        return Err(());
    }
    let mut reservation = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(reservation_path)
        .map_err(|_| ())?;
    for byte in challenge {
        write!(reservation, "{byte:02x}").map_err(|_| ())?;
    }
    writeln!(reservation).map_err(|_| ())?;
    reservation.sync_all().map_err(|_| ())?;
    File::open(reservation_path.parent().ok_or(())?)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ())?;
    Ok(challenge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_reservation_reuses_exact_bytes_and_denies_conflict() {
        let root = std::env::temp_dir().join(format!("das-challenge-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let random = root.join("random");
        let reservation = root.join("reservation");
        fs::write(&random, [7_u8; 32]).unwrap();
        let metadata = fs::metadata(&root).unwrap();
        let first =
            reserve_challenge_from(&reservation, &random, metadata.uid(), metadata.gid()).unwrap();
        assert_eq!(first, [7; 32]);
        fs::write(&random, [8_u8; 32]).unwrap();
        assert_eq!(
            reserve_challenge_from(&reservation, &random, metadata.uid(), metadata.gid(),).unwrap(),
            first
        );
        fs::write(&reservation, b"conflicting-reservation").unwrap();
        fs::set_permissions(&reservation, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            reserve_challenge_from(&reservation, &random, metadata.uid(), metadata.gid(),).is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
