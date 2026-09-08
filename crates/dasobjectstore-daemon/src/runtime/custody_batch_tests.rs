// Real controller/ledger/one-use files with synthetic Garage command execution.
// No systemd service, real credential or Garage endpoint is used.
#[derive(Default)]
struct BatchRunner {
    admission: CustodyRetainRunner,
    objects: Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    operations: Mutex<Vec<String>>,
    fail: Mutex<Option<(String, usize)>>,
    ledger_failure: Mutex<Option<PathBuf>>,
    deny_head_at: Mutex<Option<usize>>,
}
impl ServiceCommandRunner for BatchRunner {
    fn run(
        &self,
        p: &str,
        a: &[String],
    ) -> Result<super::ServiceCommandOutput, super::DaemonServiceRuntimeError> {
        self.admission.run(p, a)
    }
    fn run_with_display_args_and_env(
        &self,
        _: &str,
        a: &[String],
        _: &[String],
        environment: &[(String, String)],
    ) -> Result<super::ServiceCommandOutput, super::DaemonServiceRuntimeError> {
        use sha2::{Digest, Sha256};
        let op = a
            .iter()
            .find(|v| ["head-object", "put-object", "get-object"].contains(&v.as_str()))
            .unwrap()
            .clone();
        let required = if op == "put-object" {
            "writer-access"
        } else {
            "reader-access"
        };
        if !environment
            .iter()
            .any(|(name, value)| name == "AWS_ACCESS_KEY_ID" && value == required)
        {
            return Err(super::DaemonServiceRuntimeError::CommandFailed {
                program: "aws".into(),
                args: Vec::new(),
                status: "254".into(),
                stderr: "AccessDenied (403)".into(),
            });
        }
        let value = |flag: &str| a[a.iter().position(|v| v == flag).unwrap() + 1].clone();
        let key = value("--key");
        let error = || super::DaemonServiceRuntimeError::CommandFailed {
            program: "fixture".into(),
            args: Vec::new(),
            status: "1".into(),
            stderr: "NotFound".into(),
        };
        let mut operations = self.operations.lock().unwrap();
        let occurrence = operations.iter().filter(|v| **v == op).count();
        operations.push(op.clone());
        if op == "head-object" && *self.deny_head_at.lock().unwrap() == Some(occurrence) {
            return Err(super::DaemonServiceRuntimeError::CommandFailed {
                program: "aws".into(),
                args: Vec::new(),
                status: "254".into(),
                stderr: "AccessDenied (403)".into(),
            });
        }
        if self.fail.lock().unwrap().as_ref() == Some(&(op.clone(), occurrence)) {
            return Err(error());
        }
        let mut objects = self.objects.lock().unwrap();
        let stdout = match op.as_str() {
            "head-object" => {
                let bytes = objects.get(&key).ok_or_else(error)?;
                serde_json::json!({"ContentLength":bytes.len(),"Metadata":{
                    "dasobjectstore-sha256":format!("{:x}",Sha256::digest(bytes)),
                    "dasobjectstore-object-lock-policy":"local_trusted_administrator_non_shortenable",
                    "dasobjectstore-object-lock-shortening-forbidden":"true",
                    "dasobjectstore-object-lock-delete-forbidden":"true",
                    "dasobjectstore-object-lock-hold-authority":"dasobjectstore-custody-ledger-permanent-legal-hold",
                    "dasobjectstore-object-lock-retention-until-utc":"2036-09-05T12:00:00Z"}}).to_string()
            }
            "put-object" => {
                assert!(a.windows(2).any(|w| w == ["--if-none-match", "*"]));
                if objects.contains_key(&key) {
                    return Err(error());
                }
                objects.insert(key, fs::read(value("--body")).unwrap());
                String::new()
            }
            "get-object" => {
                fs::write(a.last().unwrap(), objects.get(&key).ok_or_else(error)?).unwrap();
                if occurrence == 1 {
                    if let Some(path) = self.ledger_failure.lock().unwrap().take() {
                        rusqlite::Connection::open(path).unwrap().execute_batch("CREATE TRIGGER fixture_deny_append BEFORE INSERT ON custody_events BEGIN SELECT RAISE(ABORT, 'fixture capacity failure'); END;").unwrap();
                    }
                }
                String::new()
            }
            _ => unreachable!(),
        };
        Ok(super::ServiceCommandOutput { stdout })
    }
}

struct BatchFixture {
    root: PathBuf,
    catalog: dasobjectstore_object_service::CustodyCatalogBinding,
    normal: GarageServiceRuntimeConfig,
    custody: GarageServiceRuntimeConfig,
    state: crate::runtime::CustodyServiceState,
    runner: BatchRunner,
    credentials: Arc<dyn CustodyRuntimeCredentialResolver>,
    provisioner: Arc<dyn CustodyAdmissionProvisioningAuthority>,
}
impl BatchFixture {
    fn new() -> Self {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let root = temp_root().join(format!("batch-{}", SEQUENCE.fetch_add(1, Ordering::SeqCst)));
        Self::new_at(root)
    }
    fn new_at(root: PathBuf) -> Self {
        let credential_dir = root.join("credentials");
        fs::create_dir_all(&credential_dir).unwrap();
        let mut definition = custody_definition();
        definition.profile.writer_credential_reference = "systemd-credential://batch-writer".into();
        definition.profile.reader_credential_reference = "systemd-credential://batch-reader".into();
        let digest = custody_store_definition_sha256(&definition).unwrap();
        for role in ["writer", "reader"] {
            let path = credential_dir.join(format!("batch-{role}"));
            fs::write(&path, format!("version=1\nrole={role}\nstore_id={}\nconfiguration_sha256={digest}\nidentity=custody-{role}\naws_access_key_id={role}-access\naws_secret_access_key=synthetic-not-a-key\n", definition.store_id)).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        let credentials = Arc::new(crate::runtime::SystemdServiceCredentialHandoffResolver::from_test_credential_directory(credential_dir, root.join("consumed")).unwrap());
        let provisioner = Arc::new(
            TestOnlyCustodyAdmissionProvisioningAuthority::new([(
                "attended://provisioner".into(),
                custody_provisioning_request(&definition),
            )])
            .unwrap(),
        );
        let fixture = Self {
            catalog: dasobjectstore_object_service::CustodyCatalogBinding::new(
                root.join("sealed/catalog.jsonl"),
            )
            .unwrap(),
            root,
            normal: config(),
            custody: custody_config(),
            state: Default::default(),
            runner: Default::default(),
            credentials,
            provisioner,
        };
        fixture
            .controller()
            .admit_custody_store(
                CustodyAdmissionRequest {
                    definition,
                    provisioner_handoff_reference: "attended://provisioner".into(),
                    dry_run: false,
                    verified_subject: None,
                    confirmation_marker: CUSTODY_ADMISSION_CONFIRMATION.into(),
                },
                "2026-09-05T12:00:00Z",
            )
            .unwrap();
        fixture
    }
    fn controller(&self) -> crate::runtime::CustodyServiceController<'_, BatchRunner> {
        crate::runtime::CustodyServiceController::new(
            crate::runtime::CustodyServiceBindings {
                custody_plane: &self.custody,
                excluded_ordinary_plane: &self.normal,
                catalog: &self.catalog,
                credentials: Some(&self.credentials),
                provisioner: Some(&self.provisioner),
            },
            &self.runner,
            &self.state,
        )
        .unwrap()
    }
    fn reconstruct_resolver(&mut self) {
        self.credentials = Arc::new(crate::runtime::SystemdServiceCredentialHandoffResolver::from_test_credential_directory(self.root.join("credentials"), self.root.join("consumed")).unwrap());
        self.state = Default::default();
    }
    fn ledger(&self) -> PathBuf {
        dasobjectstore_object_service::read_custody_catalog(self.catalog.path()).unwrap()[0]
            .ledger_path
            .clone()
    }
}
impl Drop for BatchFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn batch_inputs() -> Vec<CustodyObjectInputV1> {
    [
        b"first".as_slice(),
        b"second".as_slice(),
        b"third".as_slice(),
    ]
    .into_iter()
    .map(|bytes| CustodyObjectInputV1 {
        bytes: bytes.to_vec(),
        object_type: "application/test".into(),
        retained_at_utc: "2026-09-05T12:01:00Z".into(),
    })
    .collect()
}
fn batch_inventory() -> crate::runtime::CustodyFiniteInventory {
    use sha2::{Digest, Sha256};
    crate::runtime::CustodyFiniteInventory {
        store_id: custody_definition().store_id,
        objects: batch_inputs()
            .into_iter()
            .map(|i| crate::runtime::CustodyInventoryObject {
                content_sha256: format!("{:x}", Sha256::digest(&i.bytes)),
                size_bytes: i.bytes.len() as u64,
            })
            .collect(),
    }
}

#[test]
fn finite_batch_reader_head403_never_records_receipt_or_adopts_written_object() {
    for head in [0, 1] {
        let f = BatchFixture::new();
        *f.runner.deny_head_at.lock().unwrap() = Some(head);
        let before = fs::read(f.ledger()).unwrap();
        let result = f
            .controller()
            .retain_custody_inventory(&batch_inventory(), batch_inputs());
        let failure = result.unwrap_err();
        assert_eq!(
            failure.phase,
            crate::runtime::CustodyBatchPhase::ObjectRetention
        );
        assert_eq!(failure.failed_index, Some(0));
        assert!(failure.completed.is_empty());
        assert_eq!(
            dasobjectstore_object_service::inspect_custody_ledger(f.ledger())
                .unwrap()
                .committed_receipts,
            0
        );
        assert_eq!(fs::read(f.ledger()).unwrap(), before);
        assert_eq!(f.runner.objects.lock().unwrap().len(), head);
        assert_eq!(fs::read_dir(f.root.join("consumed")).unwrap().count(), 2);
    }
}

#[test]
fn finite_batch_foreign_reader_identity_denies_before_backend() {
    let f = BatchFixture::new();
    let path = f.root.join("credentials/batch-reader");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        path,
        original.replace("identity=custody-reader\n", "identity=foreign-reader\n"),
    )
    .unwrap();
    let error = f
        .controller()
        .retain_custody_inventory(&batch_inventory(), batch_inputs())
        .unwrap_err();
    assert_eq!(
        error.phase,
        crate::runtime::CustodyBatchPhase::AdapterConstruction
    );
    assert!(f.runner.operations.lock().unwrap().is_empty());
    assert_eq!(
        dasobjectstore_object_service::inspect_custody_ledger(f.ledger())
            .unwrap()
            .committed_receipts,
        0
    );
}

#[test]
fn finite_batch_real_ledger_three_objects_one_handoff_pair_restart_denied() {
    let mut f = BatchFixture::new();
    let mut inputs = batch_inputs();
    inputs.reverse();
    let receipts = f
        .controller()
        .retain_custody_inventory(&batch_inventory(), inputs)
        .unwrap();
    assert_eq!(
        receipts
            .iter()
            .map(|r| &r.content_sha256)
            .collect::<Vec<_>>(),
        batch_inventory()
            .objects
            .iter()
            .map(|o| &o.content_sha256)
            .collect::<Vec<_>>()
    );
    assert_eq!(fs::read_dir(f.root.join("consumed")).unwrap().count(), 2);
    assert_eq!(
        dasobjectstore_object_service::inspect_custody_ledger(f.ledger())
            .unwrap()
            .committed_receipts,
        3
    );
    let count = f.runner.operations.lock().unwrap().len();
    f.reconstruct_resolver();
    let error = f
        .controller()
        .retain_custody_inventory(&batch_inventory(), batch_inputs())
        .unwrap_err();
    assert_eq!(
        error.phase,
        crate::runtime::CustodyBatchPhase::WriterHandoff
    );
    assert_eq!(f.runner.operations.lock().unwrap().len(), count);
}

#[test]
fn finite_batch_all_input_prevalidation_precedes_handoffs() {
    for mutation in 0..8 {
        let f = BatchFixture::new();
        let mut inputs = batch_inputs();
        let mut expected = batch_inventory();
        match mutation {
            0 => {
                inputs.pop();
            }
            1 => inputs.push(inputs[0].clone()),
            2 => inputs[2].bytes.push(0),
            3 => inputs[2].object_type.clear(),
            4 => inputs[2].retained_at_utc = "invalid".into(),
            5 => inputs[2].retained_at_utc = "2037-09-05T12:00:00Z".into(),
            6 => expected.objects[2].size_bytes += 1,
            _ => expected.store_id = StoreId::new("another").unwrap(),
        }
        assert_eq!(
            f.controller()
                .retain_custody_inventory(&expected, inputs)
                .unwrap_err()
                .phase,
            crate::runtime::CustodyBatchPhase::Prevalidation
        );
        assert!(!f.root.join("consumed").exists());
        assert!(f.runner.operations.lock().unwrap().is_empty());
    }
}

#[test]
fn finite_batch_partial_backend_failure_keeps_prefix_and_blocks_reconstruction() {
    for (op, occurrence, retained, objects) in (0..3).flat_map(|index| {
        [
            ("put-object", index, index, index),
            ("get-object", index, index, index + 1),
            ("head-object", 2 * index + 1, index, index + 1),
        ]
    }) {
        let mut f = BatchFixture::new();
        *f.runner.fail.lock().unwrap() = Some((op.into(), occurrence));
        let error = f
            .controller()
            .retain_custody_inventory(&batch_inventory(), batch_inputs())
            .unwrap_err();
        assert_eq!(
            error.phase,
            crate::runtime::CustodyBatchPhase::ObjectRetention
        );
        assert_eq!(error.failed_index, Some(retained));
        assert_eq!(error.completed.len(), retained);
        assert_eq!(f.runner.objects.lock().unwrap().len(), objects);
        assert_eq!(
            dasobjectstore_object_service::inspect_custody_ledger(f.ledger())
                .unwrap()
                .committed_receipts,
            retained as u64
        );
        let count = f.runner.operations.lock().unwrap().len();
        f.reconstruct_resolver();
        assert!(f
            .controller()
            .retain_custody_inventory(&batch_inventory(), batch_inputs())
            .is_err());
        assert_eq!(f.runner.operations.lock().unwrap().len(), count);
    }
}

#[test]
fn finite_batch_reader_failure_keeps_writer_consumed_without_objects() {
    let mut f = BatchFixture::new();
    fs::write(f.root.join("credentials/batch-reader"), "invalid").unwrap();
    assert_eq!(
        f.controller()
            .retain_custody_inventory(&batch_inventory(), batch_inputs())
            .unwrap_err()
            .phase,
        crate::runtime::CustodyBatchPhase::ReaderHandoff
    );
    assert_eq!(fs::read_dir(f.root.join("consumed")).unwrap().count(), 2);
    assert!(f.runner.operations.lock().unwrap().is_empty());
    f.reconstruct_resolver();
    assert_eq!(
        f.controller()
            .retain_custody_inventory(&batch_inventory(), batch_inputs())
            .unwrap_err()
            .phase,
        crate::runtime::CustodyBatchPhase::WriterHandoff
    );
}

#[test]
fn finite_batch_writer_failure_is_terminal_before_reader_or_backend() {
    let mut f = BatchFixture::new();
    fs::write(f.root.join("credentials/batch-writer"), "invalid").unwrap();
    assert_eq!(
        f.controller()
            .retain_custody_inventory(&batch_inventory(), batch_inputs())
            .unwrap_err()
            .phase,
        crate::runtime::CustodyBatchPhase::WriterHandoff
    );
    assert_eq!(fs::read_dir(f.root.join("consumed")).unwrap().count(), 1);
    assert!(f.runner.operations.lock().unwrap().is_empty());
    f.reconstruct_resolver();
    assert!(f
        .controller()
        .retain_custody_inventory(&batch_inventory(), batch_inputs())
        .is_err());
    assert_eq!(fs::read_dir(f.root.join("consumed")).unwrap().count(), 1);
}

#[test]
fn finite_batch_competing_calls_have_only_one_complete_winner() {
    let f = BatchFixture::new();
    let winners = std::thread::scope(|scope| {
        let jobs = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    f.controller()
                        .retain_custody_inventory(&batch_inventory(), batch_inputs())
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        jobs.into_iter()
            .map(|j| usize::from(j.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(winners, 1);
    assert_eq!(f.runner.objects.lock().unwrap().len(), 3);
    assert_eq!(fs::read_dir(f.root.join("consumed")).unwrap().count(), 2);
}

#[test]
fn finite_batch_late_sql_failure_preserves_orphan_and_first_receipt() {
    let mut f = BatchFixture::new();
    *f.runner.ledger_failure.lock().unwrap() = Some(f.ledger());
    let error = f
        .controller()
        .retain_custody_inventory(&batch_inventory(), batch_inputs())
        .unwrap_err();
    assert_eq!(
        error.phase,
        crate::runtime::CustodyBatchPhase::ObjectRetention
    );
    assert_eq!(error.completed.len(), 1);
    assert_eq!(error.failed_index, Some(1));
    assert_eq!(f.runner.objects.lock().unwrap().len(), 2);
    let db = rusqlite::Connection::open(f.ledger()).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM custody_readback_receipts",
            [],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    let count = f.runner.operations.lock().unwrap().len();
    f.reconstruct_resolver();
    assert!(f
        .controller()
        .retain_custody_inventory(&batch_inventory(), batch_inputs())
        .is_err());
    assert_eq!(f.runner.operations.lock().unwrap().len(), count);
}
