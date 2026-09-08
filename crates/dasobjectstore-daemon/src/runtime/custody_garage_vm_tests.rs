// Disposable actual-Garage fixture only; no Compose/platform admission claim.
use super::*;

fn provider_category(stderr: &str, status: Option<i32>) -> &'static str {
    let text = stderr.to_ascii_lowercase();
    if status == Some(124) || status == Some(137) {
        "timeout"
    } else if text.contains("you must specify a region") {
        "no_region"
    } else if text.contains("unable to locate credentials") || text.contains("no credentials found")
    {
        "credentials"
    } else if text.contains("modulenotfounderror:") || text.contains("importerror:") {
        "import"
    } else if text.contains("signaturedoesnotmatch") || text.contains("requesttimetooskewed") {
        "signature"
    } else if text.contains("accessdenied") || text.contains("(403)") {
        "access_denied"
    } else if text.contains("notfound") || text.contains("(404)") {
        "not_found"
    } else if text.contains("(400)") || text.contains("bad request") {
        "bad_request"
    } else {
        "provider_failure"
    }
}

#[test]
fn actual_garage_dispatch_classifies_only_fixed_public_categories() {
    for (text, status, expected) in [
        ("You must specify a region.", Some(253), "no_region"),
        ("Unable to locate credentials", Some(253), "credentials"),
        ("ModuleNotFoundError: fixture_module", Some(1), "import"),
        ("SignatureDoesNotMatch", Some(254), "signature"),
        ("An error occurred (400)", Some(254), "bad_request"),
        ("An error occurred (403)", Some(254), "access_denied"),
        ("An error occurred (404)", Some(254), "not_found"),
        ("", Some(124), "timeout"),
        ("private-unrecognized-value", Some(1), "provider_failure"),
        ("", None, "provider_failure"),
    ] {
        assert_eq!(provider_category(text, status), expected);
    }
}

#[cfg(target_os = "linux")]
mod actual {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    const ROOT: &str = "/var/lib/das-garage-fixture";

    struct ActualRunner(GarageServiceRuntimeConfig);
    impl ServiceCommandRunner for ActualRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
        ) -> Result<
            super::super::super::ServiceCommandOutput,
            super::super::super::DaemonServiceRuntimeError,
        > {
            self.run_with_display_args_and_env(program, args, args, &[])
        }
        fn run_with_display_args(
            &self,
            program: &str,
            args: &[String],
            display: &[String],
        ) -> Result<
            super::super::super::ServiceCommandOutput,
            super::super::super::DaemonServiceRuntimeError,
        > {
            self.run_with_display_args_and_env(program, args, display, &[])
        }
        fn run_with_display_args_and_env(
            &self,
            program: &str,
            args: &[String],
            _display: &[String],
            environment: &[(String, String)],
        ) -> Result<
            super::super::super::ServiceCommandOutput,
            super::super::super::DaemonServiceRuntimeError,
        > {
            use std::io::Read;
            use std::process::{Command, Stdio};
            let deny = || super::super::super::DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "actual Garage fixture command denied".into(),
            };
            let (executable, argv) = if program == "docker" && environment.is_empty() {
                (
                    "/opt/das-vm-garage",
                    garage_argv(&self.0, program, args).ok_or_else(deny)?,
                )
            } else if program == "aws" && args.iter().any(|v| v == "s3api") {
                // Explicit fixture selection matches garage.toml's s3_region.
                // This is not an ambient production region or a grant change.
                let mut selected = vec!["--region".into(), "garage".into()];
                selected.extend_from_slice(args);
                ("/usr/bin/aws", selected)
            } else {
                return Err(deny());
            };
            // GNU timeout owns its command group; no shell or host process is used.
            // All fixture commands have a32s ceiling, and output collection is capped.
            let mut child = Command::new("/usr/bin/timeout")
                .args(["--kill-after=2s", "30s", executable])
                .args(argv)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("AWS_EC2_METADATA_DISABLED", "true")
                .env("AWS_MAX_ATTEMPTS", "1")
                .env("AWS_PAGER", "")
                .env("AWS_CONFIG_FILE", "/dev/null")
                .env("AWS_SHARED_CREDENTIALS_FILE", "/dev/null")
                .envs(environment.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| deny())?;
            let collect = |pipe: Box<dyn Read + Send>| {
                std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    pipe.take(1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map(|_| bytes)
                })
            };
            let out = collect(Box::new(child.stdout.take().unwrap()));
            let err = collect(Box::new(child.stderr.take().unwrap()));
            let status = child.wait().map_err(|_| deny())?;
            let stdout = out.join().map_err(|_| deny())?.map_err(|_| deny())?;
            let stderr = err.join().map_err(|_| deny())?.map_err(|_| deny())?;
            if stdout.len() > 1024 * 1024 || stderr.len() > 1024 * 1024 {
                return Err(deny());
            }
            if !status.success() {
                let operation = args
                    .iter()
                    .find(|v| ["head-object", "put-object", "get-object"].contains(&v.as_str()))
                    .map(String::as_str)
                    .unwrap_or("garage");
                let category = provider_category(&String::from_utf8_lossy(&stderr), status.code());
                let exit = status.code().unwrap_or(-1);
                eprintln!(
                    "VM_GARAGE_COMMAND operation={operation} category={category} exit={exit}"
                );
                // Preserve actual provider error text for existing absence classification.
                // The fixture panic hook never prints these values or secret argv.
                return Err(
                    super::super::super::DaemonServiceRuntimeError::CommandFailed {
                        program: "fixture-command".into(),
                        args: Vec::new(),
                        status: status.to_string(),
                        stderr: String::from_utf8(stderr).map_err(|_| deny())?,
                    },
                );
            }
            Ok(super::super::super::ServiceCommandOutput {
                stdout: String::from_utf8(stdout).map_err(|_| deny())?,
            })
        }
    }

    #[test]
    #[ignore = "actual Garage in reviewed disposable guest only; never a host invocation"]
    fn actual_garage_admission_and_finite_retention_vm() {
        use crate::runtime::{
            CustodyFiniteInventory, CustodyInventoryObject, CustodyServiceBindings,
            CustodyServiceController, CustodyServiceState,
        };
        use dasobjectstore_object_service::{read_custody_catalog, CustodyCatalogBinding};
        use sha2::{Digest, Sha256};
        std::panic::set_hook(Box::new(|info| {
            if let Some(l) = info.location() {
                eprintln!("VM_GARAGE_LOCATION {}:{}", l.file(), l.line());
            }
        }));
        assert_eq!(unsafe { libc::geteuid() }, 2002);
        assert_eq!(
            fs::read_to_string("/proc/1/comm").unwrap().trim(),
            "systemd"
        );
        let permit = fs::symlink_metadata("/run/das-systemd-vm-fixture/permit").unwrap();
        assert!(permit.is_file() && permit.uid() == 0 && permit.mode() & 0o022 == 0);
        let root = PathBuf::from(ROOT);
        assert_eq!(fs::symlink_metadata(&root).unwrap().uid(), 2002);
        let mut config = custody_config();
        config.config_path = root.join("garage.toml");
        config.metadata_path = root.join("meta");
        config.data_path = root.join("data");
        config.compose_file = root.join("unused.compose.yml");
        config.project_directory = Some(root.clone());
        config.endpoint = "http://127.0.0.1:3901".into();
        let runner = ActualRunner(config.clone());
        let mut definition = custody_definition();
        definition.profile.target_id = "disposable-garage-qualification".into();
        definition.profile.writer_credential_reference =
            "systemd-credential://garage-writer".into();
        definition.profile.reader_credential_reference =
            "systemd-credential://garage-reader".into();
        let digest = custody_store_definition_sha256(&definition).unwrap();
        let mut request = custody_provisioning_request(&definition);
        let credential_dir = root.join("handoffs");
        fs::create_dir(&credential_dir).unwrap();
        fs::set_permissions(&credential_dir, fs::Permissions::from_mode(0o700)).unwrap();
        for role in ["writer", "reader"] {
            let access = format!("GK{}", uuid::Uuid::new_v4().simple());
            let secret = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            let reference = format!("systemd-credential://garage-{role}");
            let credential = CustodyGarageCredential::new(reference, &access, &secret).unwrap();
            if role == "writer" {
                request.writer = credential;
            } else {
                request.reader = credential;
            }
            let path = credential_dir.join(format!("garage-{role}"));
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            write!(file,"version=1\nrole={role}\nstore_id={}\nconfiguration_sha256={digest}\nidentity=custody-{role}\naws_access_key_id={access}\naws_secret_access_key={secret}\n",definition.store_id).unwrap();
            file.sync_all().unwrap();
        }
        // Test-only attended issuer supplies generated credentials, not fake backend proof.
        let provisioner: Arc<dyn CustodyAdmissionProvisioningAuthority> = Arc::new(
            TestOnlyCustodyAdmissionProvisioningAuthority::new([(
                definition.profile.provisioner_credential_reference.clone(),
                request,
            )])
            .unwrap(),
        );
        let resolver = || -> Arc<dyn CustodyRuntimeCredentialResolver> {
            Arc::new(crate::runtime::SystemdServiceCredentialHandoffResolver::from_test_credential_directory(credential_dir.clone(),root.join("consumed")).unwrap())
        };
        let credentials = resolver();
        let catalog = CustodyCatalogBinding::new(root.join("sealed/catalog.jsonl")).unwrap();
        let normal = super::super::config();
        let state = CustodyServiceState::default();
        let controller = CustodyServiceController::new(
            CustodyServiceBindings {
                custody_plane: &config,
                excluded_ordinary_plane: &normal,
                catalog: &catalog,
                credentials: Some(&credentials),
                provisioner: Some(&provisioner),
            },
            &runner,
            &state,
        )
        .unwrap();
        let admission = || CustodyAdmissionRequest {
            definition: definition.clone(),
            provisioner_handoff_reference: definition
                .profile
                .provisioner_credential_reference
                .clone(),
            dry_run: false,
            verified_subject: None,
            confirmation_marker: CUSTODY_ADMISSION_CONFIRMATION.into(),
        };
        assert!(controller
            .admit_custody_store(admission(), "2026-09-08T16:00:00Z")
            .is_ok());
        assert!(controller
            .admit_custody_store(admission(), "2026-09-08T16:00:00Z")
            .is_err());
        let inputs = || {
            [
                b"actual Garage first".as_slice(),
                b"actual Garage second".as_slice(),
            ]
            .into_iter()
            .map(|bytes| CustodyObjectInputV1 {
                bytes: bytes.to_vec(),
                object_type: "application/test".into(),
                retained_at_utc: "2026-09-08T16:01:00Z".into(),
            })
            .collect::<Vec<_>>()
        };
        let expected = CustodyFiniteInventory {
            store_id: definition.store_id.clone(),
            objects: inputs()
                .into_iter()
                .map(|i| CustodyInventoryObject {
                    content_sha256: format!("{:x}", Sha256::digest(&i.bytes)),
                    size_bytes: i.bytes.len() as u64,
                })
                .collect(),
        };
        let mut wrong = inputs();
        wrong[0].bytes.push(0);
        assert!(matches!(
            controller
                .retain_custody_inventory(&expected, wrong)
                .unwrap_err()
                .phase,
            crate::runtime::CustodyBatchPhase::Prevalidation
        ));
        let receipts = controller.retain_custody_inventory(&expected, inputs());
        if let Err(error) = &receipts {
            eprintln!(
                "VM_GARAGE_BATCH phase={:?} index={} completed={}",
                error.phase,
                error
                    .failed_index
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".into()),
                error.completed.len()
            );
        }
        assert!(receipts.is_ok());
        let receipts = receipts.unwrap();
        assert_eq!(receipts.len(), 2);
        let ledger = read_custody_catalog(catalog.path()).unwrap()[0]
            .ledger_path
            .clone();
        let before = fs::read(&ledger).unwrap();
        let credentials = resolver();
        let reconstructed = CustodyServiceState::default();
        let controller = CustodyServiceController::new(
            CustodyServiceBindings {
                custody_plane: &config,
                excluded_ordinary_plane: &normal,
                catalog: &catalog,
                credentials: Some(&credentials),
                provisioner: Some(&provisioner),
            },
            &runner,
            &reconstructed,
        )
        .unwrap();
        assert!(matches!(
            controller
                .retain_custody_inventory(&expected, inputs())
                .unwrap_err()
                .phase,
            crate::runtime::CustodyBatchPhase::WriterHandoff
        ));
        assert_eq!(fs::read(&ledger).unwrap(), before);
        // Guest-only handoff for the subsequent protected publication fixture.
        // Existing closed records are retained verbatim; this envelope is test
        // plumbing, not a new receipt/inventory or companion protocol.
        let retained = serde_json::json!({
            "definition": definition,
            "ledger": ledger,
            "ledger_sha256": format!("{:x}", Sha256::digest(&before)),
            "receipts": receipts,
            "inventory": expected.objects.iter().map(|object|
                (&object.content_sha256, object.size_bytes)).collect::<Vec<_>>(),
        });
        let selected = root.join("retained.json");
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new().write(true).create_new(true)
            .mode(0o600).open(selected).unwrap();
        file.write_all(&serde_jcs::to_vec(&retained).unwrap()).unwrap();
        file.sync_all().unwrap();
        println!("VM_GARAGE_ADMISSION_BATCH_PASS objects=2 reconstructed_handoff=denied");
        // Retained state intentionally survives this test; guest teardown owns disposal.
    }
}

/// Accept only the wrapper generated for this exact fixture configuration.
/// The operation itself remains the existing provisioner's argv, never a shell.
fn garage_argv(
    config: &GarageServiceRuntimeConfig,
    program: &str,
    args: &[String],
) -> Option<Vec<String>> {
    let prefix = super::super::docker_compose_args(
        config,
        super::super::garage_exec_args(&config.service_name, Vec::new()),
    );
    if program != "docker" || !args.starts_with(&prefix) || args.len() == prefix.len() {
        return None;
    }
    let operation = &args[prefix.len()..];
    // Only operations emitted by the fresh-bucket provisioning planner.
    let words: Vec<_> = operation.iter().map(String::as_str).collect();
    if !matches!(
        words.as_slice(),
        ["bucket", "info" | "create", _]
            | ["bucket", "allow", "--write" | "--read", _, "--key", _]
            | ["key", "import", "--yes", "-n", _, _, _]
    ) {
        return None;
    }
    let mut result = vec!["-c".into(), config.config_path.to_str()?.into()];
    result.extend_from_slice(operation);
    Some(result)
}

#[test]
fn actual_garage_dispatch_preserves_exact_planner_arguments() {
    let config = custody_config();
    let operations = [
        vec!["bucket", "info", "fixture-bucket"],
        vec!["bucket", "create", "fixture-bucket"],
        vec![
            "key",
            "import",
            "--yes",
            "-n",
            "fixture",
            "fixture-id",
            "fixture-secret-not-real",
        ],
    ];
    for operation in operations {
        let operation: Vec<String> = operation.into_iter().map(String::from).collect();
        let args = super::super::docker_compose_args(
            &config,
            super::super::garage_exec_args(&config.service_name, operation.clone()),
        );
        let mapped = garage_argv(&config, "docker", &args).unwrap();
        assert_eq!(&mapped[..2], ["-c", config.config_path.to_str().unwrap()]);
        assert_eq!(&mapped[2..], operation);
    }
}

#[test]
fn actual_garage_dispatch_denies_foreign_wrapper_before_execution() {
    let config = custody_config();
    let args = super::super::docker_compose_args(
        &config,
        super::super::garage_exec_args(
            &config.service_name,
            vec!["bucket".into(), "info".into(), "fixture".into()],
        ),
    );
    assert!(garage_argv(&config, "sh", &args).is_none());
    for index in 0..args.len() - 3 {
        let mut changed = args.clone();
        changed[index].push_str("-foreign");
        assert!(garage_argv(&config, "docker", &changed).is_none());
    }
    let mut extra = args.clone();
    extra.insert(0, "--host=foreign".into());
    assert!(garage_argv(&config, "docker", &extra).is_none());
    let prefix = super::super::docker_compose_args(
        &config,
        super::super::garage_exec_args(&config.service_name, Vec::new()),
    );
    assert!(garage_argv(&config, "docker", &prefix).is_none());
    let mut shell = prefix;
    shell.extend(["sh".into(), "-c".into(), "false".into()]);
    assert!(garage_argv(&config, "docker", &shell).is_none());
}
