//! Actual loader in the isolated systemd VM; storage setup is synthetic only.
use super::*;
use std::os::unix::fs::MetadataExt;
use std::process::{Command, Stdio};

const CONTROL: &str = "/run/das-systemd-vm-fixture";
const EXECUTABLE: &str = "/opt/das-vm-adapter";
const HELPER: &str = "/opt/das-vm-aws";
// Public /usr/bin/aws from authenticated fixture image0c14e316, not executed.
const HELPER_SHA256: &str = "2a7695eaff8793186869a193da167a5e6d46826dac1a93d19d7484b27508a958";

pub(super) fn guest_guard(uid: u32) {
    assert_eq!(unsafe { libc::geteuid() }, uid);
    assert_eq!(
        fs::read_to_string("/proc/1/comm").unwrap().trim(),
        "systemd"
    );
    let permit = fs::symlink_metadata(format!("{CONTROL}/permit")).unwrap();
    assert!(permit.is_file() && !permit.file_type().is_symlink());
    assert_eq!(permit.uid(), 0);
    assert_eq!(permit.mode() & 0o022, 0);
}
fn share(path: &Path, mode: u32) {
    let path_c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::chown(path_c.as_ptr(), 0, 2000) }, 0);
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
fn allowance() -> CustodyReadLimits {
    // Two actual platform probes hash the 91MB test binary under TCG.
    // This fixture allowance is not a production default or latency claim.
    CustodyReadLimits {
        maximum_bytes: 4096,
        timeout: Duration::from_secs(120),
    }
}

#[test]
#[ignore = "root preparation in separately reviewed disposable systemd VM only"]
fn prepare_actual_loader_vm() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(location) = info.location() {
            eprintln!(
                "VM_LOADER_PREP_LOCATION {}:{}",
                location.file(),
                location.line()
            );
        }
    }));
    guest_guard(0);
    let mode = fs::read_to_string(format!("{CONTROL}/mode")).unwrap();
    assert!(matches!(
        mode.trim(),
        "positive" | "malformed" | "wrong-key" | "stale-current"
    ));
    prepare(&mode, |_| {});
}

// Fixture-only composition hook before encryption and real manager publication.
// The hook cannot replace the real credential loader or manager verification.
pub(super) fn prepare(mode: &str, configure: impl FnOnce(&mut Fixture)) {
    guest_guard(0);
    // The reused fixture creates/retains/seals through real public ledger APIs;
    // its in-memory object storage does not assert a Garage retention result.
    eprintln!("VM_LOADER_PREP_STAGE ledger");
    let mut f = fixture_at(Path::new("/var/lib"));
    eprintln!("VM_LOADER_PREP_STAGE ledger_complete");
    let root = f.root.clone();
    f.selection.encrypted_source = root.join("reader.enc");
    f.selection.aws_executable = HELPER.into();
    f.selection.aws_executable_sha256 = HELPER_SHA256.into();
    f.selection.binding.uid = 2000;
    f.selection.binding.service_identity = "das-vm-loader.service".into();
    f.selection.binding.credential_name = "reader".into();
    f.selection.binding.executable_sha256 = raw_sha256(&fs::read(EXECUTABLE).unwrap());
    f.selection.binding.bucket_name = "dos-formal-custody".into();
    configure(&mut f);
    eprintln!("VM_LOADER_PREP_STAGE helper");
    let helper = fs::symlink_metadata(&f.selection.aws_executable).unwrap();
    assert!(helper.is_file() && !helper.file_type().is_symlink());
    assert_eq!(helper.uid(), 0);
    assert_eq!(helper.mode() & 0o022, 0);
    assert_eq!(
        raw_sha256(&fs::read(&f.selection.aws_executable).unwrap()),
        f.selection.aws_executable_sha256
    );
    eprintln!("VM_LOADER_PREP_STAGE helper_complete");
    let key_id = if mode.trim() == "wrong-key" {
        "different-key"
    } else {
        &f.selection.binding.backend_key_id
    };
    let plaintext = zeroize::Zeroizing::new(if mode.trim() == "malformed" {
        "not-a-credential-record".to_owned()
    } else {
        format!("version=1\nrole=reader\nstore_id={}\nconfiguration_sha256={}\nidentity={}\naws_access_key_id={}\naws_secret_access_key={}\n",
            f.selection.binding.store_id, f.selection.binding.configuration_sha256,
            f.selection.binding.reader_identity, key_id, uuid::Uuid::new_v4())
    });
    let plain = root.join("plain");
    fs::write(&plain, plaintext.as_bytes()).unwrap();
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o600)).unwrap();
    // Genuine authenticated systemd ciphertext replaces, never adopts, the
    // ordinary source fixture's synthetic format-classification header.
    eprintln!("VM_LOADER_PREP_STAGE encrypt");
    assert!(Command::new("/usr/bin/systemd-creds")
        .args(["encrypt", "--with-key=host", "--name=reader"])
        .arg(&plain)
        .arg(&f.selection.encrypted_source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    fs::remove_file(plain).unwrap();
    fs::set_permissions(
        &f.selection.encrypted_source,
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    f.selection.binding.encrypted_source_sha256 =
        raw_sha256(&fs::read(&f.selection.encrypted_source).unwrap());
    f.current.binding_sha256 = raw_sha256(&f.selection.binding.encode().unwrap());
    eprintln!("VM_LOADER_PREP_STAGE publish");
    ReaderManager::open(f.selection.directory.clone())
        .unwrap()
        .publish_initial(&f.selection, &f.seal, &f.current, allowance())
        .unwrap();
    share(&root, 0o750);
    share(&f.selection.directory, 0o750);
    share(&f.selection.ledger, 0o640);
    for entry in fs::read_dir(&f.selection.directory).unwrap() {
        share(&entry.unwrap().path(), 0o640);
    }
    eprintln!("VM_LOADER_PREP_STAGE public_selection");
    let selected = serde_json::json!({
        "root": root, "binding": f.selection.binding,
        "inventory": f.selection.inventory,
        "inventory_sha256": f.selection.selected_inventory_sha256,
        "helper_sha256": f.selection.aws_executable_sha256,
        "helper": f.selection.aws_executable,
        "backend_endpoint": f.selection.backend_endpoint,
    });
    fs::write(
        format!("{CONTROL}/loader.json"),
        serde_json::to_vec(&selected).unwrap(),
    )
    .unwrap();
    if mode.trim() == "stale-current" {
        // Explicit privileged fixture corruption, not an admitted rotation.
        f.current.credential_generation += 1;
        fs::write(
            f.selection.directory.join("current.jcs"),
            f.current.encode().unwrap(),
        )
        .unwrap();
    }
    std::mem::forget(f); // guest lifecycle owns cleanup; no state leaves the VM
}

#[test]
#[ignore = "actual service invocation in separately reviewed disposable systemd VM only"]
fn actual_loader_vm_boundary() {
    guest_guard(2000);
    let selection = selected_fixture();
    let runner = super::super::super::service::SystemServiceCommandRunner;
    eprintln!("VM_LOADER_STAGE load");
    let result = ReaderContinuation::load(
        selection,
        &runner,
        Path::new("/run/das-vm-scratch"),
        allowance(),
    );
    if let Err(error) = &result {
        let label = match error {
            ReaderError::Format => "Format",
            ReaderError::Binding => "Binding",
            ReaderError::Boundary => "Boundary",
            ReaderError::Conflict => "Conflict",
            ReaderError::Read => "Read",
        };
        eprintln!("VM_LOADER_ERROR {label}");
    }
    let mode = fs::read_to_string(format!("{CONTROL}/mode")).unwrap();
    if matches!(mode.trim(), "positive" | "restart") {
        assert!(result.is_ok(), "actual full loader positive denied");
    } else {
        assert!(result.is_err(), "actual full loader negative accepted");
    }
    drop(result); // drops the actual acquired credential without any GET
    fs::write("/run/das-vm-results/passed", b"passed").unwrap();
}

pub(super) fn selected_fixture() -> ReaderSelection {
    let raw = fs::read(format!("{CONTROL}/loader.json")).unwrap();
    let selected: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let root = PathBuf::from(selected["root"].as_str().unwrap());
    assert!(
        root.starts_with("/var/lib")
            && root
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(".das-manager-review-")
    );
    let binding =
        ReaderBindingV1::decode(&serde_jcs::to_vec(&selected["binding"]).unwrap()).unwrap();
    ReaderSelection {
        binding,
        directory: root.join("records"),
        manager_uid: 0,
        ledger: root.join("ledger.sqlite3"),
        inventory: serde_json::from_value(selected["inventory"].clone()).unwrap(),
        selected_inventory_sha256: selected["inventory_sha256"].as_str().unwrap().into(),
        encrypted_source: root.join("reader.enc"),
        protection: CredentialProtection::Host,
        backend_endpoint: selected["backend_endpoint"].as_str().unwrap().into(),
        aws_executable: selected["helper"].as_str().unwrap().into(),
        aws_executable_sha256: selected["helper_sha256"].as_str().unwrap().into(),
    }
}
