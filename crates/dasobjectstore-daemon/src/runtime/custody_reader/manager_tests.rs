//! Independent public-operation manager tests; no systemd authentication claim.
#[path = "interruption_tests.rs"]
mod interruption_tests;
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use dasobjectstore_object_service::custody::*;
use dasobjectstore_object_service::ObjectServiceError;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    time::Duration,
};

struct Backend(RefCell<BTreeMap<String, Vec<u8>>>);
struct Writer<'a>(&'a Backend);
struct Reader<'a>(&'a Backend);
impl CustodyObjectWriter for Writer<'_> {
    fn identity(&self) -> &str {
        "custody-writer-v1"
    }
    fn object_state(&mut self, key: &str) -> Result<CustodyObjectState, ObjectServiceError> {
        Ok(match self.0 .0.borrow().get(key) {
            Some(bytes) => CustodyObjectState::Existing {
                content_sha256: raw_sha256(bytes),
                content_length: bytes.len() as u64,
            },
            None => CustodyObjectState::Missing,
        })
    }
    fn put_if_absent(&mut self, key: &str, bytes: &[u8]) -> Result<(), ObjectServiceError> {
        assert!(!self.0 .0.borrow().contains_key(key));
        self.0 .0.borrow_mut().insert(key.into(), bytes.to_vec());
        Ok(())
    }
}
impl CustodyObjectReader for Reader<'_> {
    fn identity(&self) -> &str {
        "custody-reader-v1"
    }
    fn read_exact(&mut self, key: &str) -> Result<Vec<u8>, ObjectServiceError> {
        Ok(self.0 .0.borrow()[key].clone())
    }
}
struct Fixture {
    root: PathBuf,
    selection: ReaderSelection,
    seal: ReaderSealV1,
    current: ReaderCurrentV1,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("owned synthetic manager cleanup");
    }
}
fn fixture() -> Fixture {
    let root = PathBuf::from(std::env::var_os("HOME").unwrap())
        .canonicalize()
        .unwrap()
        .join(format!(".das-manager-review-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let directory = root.join("records");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let ledger = root.join("ledger.sqlite3");
    let request = CustodyGarageProvisioningRequest {
        store_id: dasobjectstore_core::ids::StoreId::new("formal-custody").unwrap(),
        bucket_name: "dos-formal-custody".into(),
        profile: CustodyStoreProfileV1 {
            schema: CUSTODY_OVERLAY_SCHEMA_V1.into(),
            profile: CUSTODY_PROFILE_V1.into(),
            assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
            retention: CustodyRetentionPolicyV1::required(),
            target_id: "synthetic-manager-review".into(),
            retention_until_utc: "2027-09-05T10:00:00Z".into(),
            legal_hold: true,
            provisioner_credential_reference: "secret://custody/provisioner".into(),
            provisioner_identity: "custody-provisioner-v1".into(),
            writer_credential_reference: "secret://custody/writer".into(),
            writer_identity: "custody-writer-v1".into(),
            reader_credential_reference: "secret://custody/reader".into(),
            reader_identity: "custody-reader-v1".into(),
        },
        provisioner: CustodyGarageProvisionerIdentity {
            identity: "custody-provisioner-v1".into(),
            credential_reference: "secret://custody/provisioner".into(),
        },
        writer: CustodyGarageCredential::new(
            "secret://custody/writer",
            "synthetic-writer",
            "synthetic-writer-secret",
        )
        .unwrap(),
        reader: CustodyGarageCredential::new(
            "secret://custody/reader",
            "synthetic-reader",
            "synthetic-reader-secret",
        )
        .unwrap(),
    };
    let proof = CustodyFreshBucketProofV1 {
        schema: CUSTODY_FRESH_BUCKET_PROOF_SCHEMA_V1.into(),
        store_id: request.store_id.clone(),
        bucket_name: request.bucket_name.clone(),
        target_id: request.profile.target_id.clone(),
        provisioner_identity: request.provisioner.identity.clone(),
        provisioner_credential_reference: request.provisioner.credential_reference.clone(),
        provisioning_request_sha256: custody_provisioning_request_sha256(&request).unwrap(),
        absence_evidence_sha256: "a".repeat(64),
        creation_evidence_sha256: "b".repeat(64),
        creation_nonce: "synthetic-manager-review".into(),
        created_at_utc: "2026-09-05T10:00:00Z".into(),
    };
    create_custody_ledger(&ledger, &request, proof, "2026-09-05T10:00:00Z").unwrap();
    let backend = Backend(RefCell::new(BTreeMap::new()));
    let receipt = retain_custody_object_with_readback(
        &ledger,
        CustodyObjectInputV1 {
            object_type: "release_corpus".into(),
            bytes: b"actual synthetic manager receipt".to_vec(),
            retained_at_utc: "2026-09-05T11:00:00Z".into(),
        },
        &mut Writer(&backend),
        &mut Reader(&backend),
    )
    .unwrap();
    fs::set_permissions(&ledger, fs::Permissions::from_mode(0o600)).unwrap();
    let inspection = inspect_custody_ledger(&ledger).unwrap();
    let seal = ReaderSealV1 {
        schema: "das.custody.reader_seal.v1".into(),
        companion_sha256: "c".repeat(64),
        bootstrap_transaction_id: uuid::Uuid::new_v4().to_string(),
        store_id: receipt.store_id.to_string(),
        configuration_sha256: inspection.configuration_sha256,
        inventory_sha256: "d".repeat(64),
        ledger_head_sha256: inspection.ledger_head_sha256,
        receipt_jcs_sha256: vec![raw_sha256(&serde_jcs::to_vec(&receipt).unwrap())],
        completed_at_utc: "2026-09-05T12:00:00Z".into(),
    };
    // Valid format identifier, deliberately not authenticated ciphertext. This
    // tests public manager classification only, never continuation admission.
    let mut header = vec![0u8; 128];
    header[..16].copy_from_slice(&[
        0x5a, 0x1c, 0x6a, 0x86, 0xdf, 0x9d, 0x40, 0x96, 0xb1, 0xd5, 0xa6, 0x5e, 0x08, 0x62, 0xf1,
        0x9a,
    ]);
    for (offset, value) in [(16, 32u32), (20, 1), (24, 12), (28, 16)] {
        header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let encrypted = STANDARD.encode(header);
    let encrypted_source = root.join("synthetic.enc");
    fs::write(&encrypted_source, &encrypted).unwrap();
    fs::set_permissions(&encrypted_source, fs::Permissions::from_mode(0o600)).unwrap();
    let manager_uid = unsafe { libc::geteuid() };
    let mut binding = ReaderBindingV1::decode(include_bytes!(
        "../../../../../docs/adr/fixtures/0011-reader-wire/binding.jcs.json"
    ))
    .unwrap();
    binding.uid = if manager_uid == 1000 { 1001 } else { 1000 };
    binding.companion_sha256 = seal.companion_sha256.clone();
    binding.store_id = seal.store_id.clone();
    binding.configuration_sha256 = seal.configuration_sha256.clone();
    binding.inventory_sha256 = seal.inventory_sha256.clone();
    binding.seal_sha256 = raw_sha256(&seal.encode().unwrap());
    binding.reader_identity = "custody-reader-v1".into();
    binding.encrypted_source_sha256 = raw_sha256(encrypted.as_bytes());
    binding.not_before_utc = "2026-09-05T12:00:00Z".into();
    binding.expires_at_utc = "2030-09-05T12:00:00Z".into();
    let current = ReaderCurrentV1 {
        schema: "das.custody.reader_current.v1".into(),
        binding_sha256: raw_sha256(&binding.encode().unwrap()),
        credential_generation: binding.credential_generation,
        state: "active".into(),
        updated_at_utc: "2026-09-05T12:00:00Z".into(),
    };
    let selection = ReaderSelection {
        binding,
        directory,
        manager_uid,
        ledger,
        inventory: vec![(receipt.content_sha256, receipt.content_length)],
        selected_inventory_sha256: seal.inventory_sha256.clone(),
        encrypted_source,
        protection: CredentialProtection::Host,
        backend_endpoint: "https://fixture.invalid".into(),
        aws_executable: root.join("not-executed"),
        aws_executable_sha256: "e".repeat(64),
    };
    Fixture {
        root,
        selection,
        seal,
        current,
    }
}
fn limits() -> CustodyReadLimits {
    CustodyReadLimits {
        maximum_bytes: 1024,
        timeout: Duration::from_secs(5),
    }
}
#[test]
fn independent_manager_publishes_once_without_ledger_mutation() {
    let f = fixture();
    let before = fs::read(&f.selection.ledger).unwrap();
    let manager = ReaderManager::open(f.selection.directory.clone()).unwrap();
    manager
        .publish_initial(&f.selection, &f.seal, &f.current, limits())
        .unwrap();
    assert_eq!(fs::read_dir(&f.selection.directory).unwrap().count(), 4);
    assert!(!f.selection.directory.join("manager.claim").exists());
    assert!(manager
        .publish_initial(&f.selection, &f.seal, &f.current, limits())
        .is_err());
    assert_eq!(fs::read(&f.selection.ledger).unwrap(), before);
}
#[test]
fn independent_manager_bad_inventory_has_no_publication_effect() {
    let mut f = fixture();
    f.selection.inventory[0].1 += 1;
    let manager = ReaderManager::open(f.selection.directory.clone()).unwrap();
    assert!(manager
        .publish_initial(&f.selection, &f.seal, &f.current, limits())
        .is_err());
    assert_eq!(fs::read_dir(&f.selection.directory).unwrap().count(), 0);
}
#[test]
fn independent_manager_never_adopts_partial_claim() {
    let f = fixture();
    fs::write(f.selection.directory.join("manager.claim"), b"").unwrap();
    let manager = ReaderManager::open(f.selection.directory.clone()).unwrap();
    assert!(manager
        .publish_initial(&f.selection, &f.seal, &f.current, limits())
        .is_err());
    assert_eq!(fs::read_dir(&f.selection.directory).unwrap().count(), 1);
    assert!(!f.selection.directory.join("current.jcs").exists());
}
