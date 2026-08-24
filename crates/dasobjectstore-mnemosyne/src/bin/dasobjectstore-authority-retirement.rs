use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
    os::unix::process::CommandExt as _,
    path::Path,
    process::{Command, Stdio},
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
const RECEIPT_HELPER_ARGUMENT: &str = "--request-replacement-receipt-v1";
const RECEIPT_SOCKET_GROUP: &str = "mnemosyne-pistis-das";
const MAXIMUM_RECEIPT_BYTES: usize = 16 * 1024;

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
    let mut arguments = std::env::args_os();
    let _executable = arguments.next();
    let operation = arguments.next();
    if arguments.next().is_some() {
        eprintln!("DAS authority retirement unavailable");
        std::process::exit(1);
    }
    let result = match operation.as_deref() {
        None => run(),
        Some(value) if value == RECEIPT_HELPER_ARGUMENT => run_receipt_helper(),
        Some(_) => Err(()),
    };
    if result.is_err() {
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
    let receipt = request_receipt_as_service_identity(challenge, service_uid)?;
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

/// Request only the kernel-authenticated receipt as the unprivileged DAS
/// service identity. The root parent retains projection verification and the
/// atomic root-owned archive transaction; the helper receives no filesystem
/// path, authority material, or mutation capability.
fn request_receipt_as_service_identity(
    challenge: [u8; 32],
    service_uid: u32,
) -> Result<Vec<u8>, ()> {
    if service_uid == 0 {
        return Err(());
    }
    let receipt_gid = named_group_id(RECEIPT_SOCKET_GROUP)?;
    let executable = std::env::current_exe().map_err(|_| ())?;
    let metadata = fs::symlink_metadata(&executable).map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(());
    }
    let mut child = Command::new(executable);
    child
        .arg(RECEIPT_HELPER_ARGUMENT)
        .env_clear()
        .current_dir("/")
        .uid(service_uid)
        .gid(receipt_gid)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child.spawn().map_err(|_| ())?;
    let mut input = child.stdin.take().ok_or(())?;
    if input.write_all(&challenge).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(());
    }
    drop(input);
    let output = child.wait_with_output().map_err(|_| ())?;
    if !output.status.success()
        || output.stdout.is_empty()
        || output.stdout.len() > MAXIMUM_RECEIPT_BYTES
    {
        return Err(());
    }
    Ok(output.stdout)
}

fn run_receipt_helper() -> Result<(), ()> {
    if unsafe { libc::geteuid() } == 0 || unsafe { libc::getegid() } == 0 {
        return Err(());
    }
    request_receipt_helper_io(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        request_das_replacement_receipt_v1,
    )
}

fn request_receipt_helper_io(
    input: &mut impl Read,
    output: &mut impl Write,
    request: impl FnOnce(
        [u8; 32],
    ) -> Result<
        Vec<u8>,
        dasobjectstore_mnemosyne::local_authority_retirement::DasLocalAuthorityRetirementErrorV1,
    >,
) -> Result<(), ()> {
    let mut challenge = [0_u8; 32];
    input.read_exact(&mut challenge).map_err(|_| ())?;
    let mut trailing = [0_u8; 1];
    if challenge == [0; 32] || input.read(&mut trailing).map_err(|_| ())? != 0 {
        return Err(());
    }
    let receipt = request(challenge).map_err(|_| ())?;
    if receipt.is_empty() || receipt.len() > MAXIMUM_RECEIPT_BYTES {
        return Err(());
    }
    output.write_all(&receipt).map_err(|_| ())?;
    output.flush().map_err(|_| ())
}

fn named_group_id(name: &str) -> Result<u32, ()> {
    let name = CString::new(name).map_err(|_| ())?;
    let record = unsafe { libc::getgrnam(name.as_ptr()) };
    if record.is_null() {
        return Err(());
    }
    let gid = unsafe { (*record).gr_gid };
    (gid != 0).then_some(gid).ok_or(())
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
    fn receipt_helper_is_exactly_one_request_without_retirement_inputs() {
        let challenge = [7_u8; 32];
        let mut input = challenge.as_slice();
        let mut output = Vec::new();
        request_receipt_helper_io(&mut input, &mut output, |observed| {
            assert_eq!(observed, challenge);
            Ok(vec![9_u8; 48])
        })
        .unwrap();
        assert_eq!(output, vec![9_u8; 48]);

        let extended_bytes = [challenge.as_slice(), &[1]].concat();
        let mut extended = extended_bytes.as_slice();
        assert!(
            request_receipt_helper_io(&mut extended, &mut Vec::new(), |_| Ok(vec![1])).is_err()
        );
    }

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
