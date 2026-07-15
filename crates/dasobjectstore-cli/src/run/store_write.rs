//! Store creation and portable-registry write handlers.

use super::*;
use dasobjectstore_core::store::{CapacityBehavior, CapacityPolicy, ExportPolicy, RetentionPolicy};

pub(super) fn run_store_profile_binding(
    args: &StoreProfileBindingArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let manifest_json = fs::read_to_string(args.manifest()).map_err(CliError::Io)?;
    let manifest = ObjectStoreManifest::decode_json(&manifest_json)
        .map_err(|error| CliError::CommandFailed(error.to_string()))?;
    let operation = match args.operation() {
        StoreProfileBindingOperation::Create => ProfileBindingOperation::Create,
        StoreProfileBindingOperation::Provision => ProfileBindingOperation::Provision,
        StoreProfileBindingOperation::Adopt => ProfileBindingOperation::Adopt,
    };
    let config = DaemonRuntimeConfig::default_packaged();
    let response = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path))
        .register_profile_binding(ProfileBindingRequest {
            operation,
            manifest,
            capacity: args
                .capacity_limit_bytes()
                .map(|limit| CapacityPolicy::bounded(limit, args.backend_reserve_bytes()))
                .unwrap_or_default(),
            store_definition: None,
            backend_root: args.backend_root().to_path_buf(),
            ssd_staging_root: args.ssd_staging_root().map(Path::to_path_buf),
            dry_run: args.dry_run(),
            client_request_id: None,
            administrator_actor: std::env::var("USER").ok(),
            confirmation_marker: args.confirm().to_string(),
        })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(
            writer,
            "Profile binding {}",
            if args.dry_run() {
                "validated"
            } else {
                "registered"
            }
        )?;
        writeln!(writer, "Store: {}", response.store_id)?;
        writeln!(writer, "Profile: {}", response.deployment_profile.name())?;
        // Backend paths are daemon-owned implementation details and must not
        // cross the profile-binding transport boundary.
        writeln!(writer, "Backend root: daemon-managed")?;
        writeln!(writer, "Adopted objects: {}", response.adopted_object_count)?;
        writeln!(writer, "Adopted bytes: {}", response.adopted_bytes)?;
        if response.operation == ProfileBindingOperation::Provision {
            writeln!(
                writer,
                "Provisioning: {}",
                if response.reused {
                    "reused existing binding"
                } else {
                    "created binding"
                }
            )?;
        }
        writeln!(writer, "Job: {}", response.accepted.job_id)?;
    }
    Ok(())
}

pub(super) fn run_store_profile_migration(
    args: &StoreProfileMigrationArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let response = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path))
        .profile_migration(dasobjectstore_daemon::api::ProfileMigrationRequest {
            migration_id: args.migration_id().to_string(),
            source_store_id: args.source_store_id().to_string(),
            destination_store_id: args.destination_store_id().to_string(),
            client_request_id: None,
            administrator_actor: std::env::var("USER").ok(),
            confirmation_marker: args.confirm().to_string(),
        })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Migration: {}", response.migration_id)?;
        writeln!(writer, "Source store: {}", response.source_store_id)?;
        writeln!(
            writer,
            "Destination store: {}",
            response.destination_store_id
        )?;
        writeln!(
            writer,
            "Verified objects: {}",
            response.verified_object_count
        )?;
        writeln!(
            writer,
            "Destination logical bytes: {}",
            response.destination_used_bytes
        )?;
        writeln!(writer, "State: {:?}", response.state)?;
        writeln!(writer, "Source retained: {}", response.source_retained)?;
        writeln!(writer, "Job: {}", response.accepted.job_id)?;
    }
    Ok(())
}

pub(super) fn run_store_drain(
    args: &StoreDrainArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_admin_for_destructive_store_action(args.dry_run())?;
    if !args.dry_run() {
        RiskGate::new(RiskPolicy {
            allow_store_drain: args.allow_store_drain(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::StoreDrain,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.store_drain(DaemonStoreDrainRequest {
        store_id: args.store_id().to_string(),
        dry_run: args.dry_run(),
        allow_store_drain: args.allow_store_drain(),
        confirmation_marker: args.confirm().to_string(),
    })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response.report)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_drain_report(&response.report, writer)?;
    }
    Ok(())
}

pub(super) fn run_store_delete(
    args: &StoreDeleteArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_admin_for_destructive_store_action(args.dry_run())?;
    if !args.dry_run() {
        RiskGate::new(RiskPolicy {
            allow_store_delete: args.allow_store_delete(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::StoreDelete,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.store_delete(DaemonStoreDeleteRequest {
        store_id: args.store_id().to_string(),
        dry_run: args.dry_run(),
        allow_store_delete: args.allow_store_delete(),
        confirmation_marker: args.confirm().to_string(),
    })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response.report)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_delete_report(&response.report, writer)?;
    }
    Ok(())
}

pub(super) fn require_admin_for_destructive_store_action(dry_run: bool) -> Result<(), CliError> {
    if dry_run || current_user_is_root()? {
        return Ok(());
    }
    Err(CliError::CommandFailed(
        "destructive storage cleanup requires an administrative user; rerun with sudo".to_string(),
    ))
}

pub(super) fn run_store_create(
    args: &StoreCreateArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let mut policy = StorePolicy::defaults_for(args.class());
    if let Some(copies) = args.copies() {
        policy.copies = copies;
    }
    policy.validate()?;
    super::enforce_supported_das_for_store_create(args)?;

    let definition = StoreServiceDefinition {
        store_id: args.store_id().clone(),
        policy,
        bucket_name: args.bucket().map(ToOwned::to_owned),
        reader_group: args.reader_group().map(ToOwned::to_owned),
        writer_group: args.writer_group().map(ToOwned::to_owned),
        public: args.public(),
    };
    let daemon_config = DaemonRuntimeConfig::default_packaged();
    let use_daemon = args.registry_path().is_none()
        && definition.writer_group.is_some()
        && daemon_config.socket_path.exists();
    let daemon_response = if use_daemon {
        let request = create_object_store_request(args, &definition);
        Some(
            DaemonClient::new(UnixSocketDaemonTransport::new(daemon_config.socket_path))
                .create_object_store(request)?,
        )
    } else {
        None
    };
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let report = if daemon_response.is_none() {
        Some(upsert_store_definition(&registry_path, definition.clone())?)
    } else {
        None
    };
    let allow_default_ssd = args.registry_path().is_none() || args.ssd_root().is_some();
    let portable_report = super::registry_access::upsert_portable_store_definition(
        args.ssd_root(),
        allow_default_ssd,
        &definition,
    )?;
    if let Some(writer_group) = &definition.writer_group {
        super::registry_access::grant_store_writer_group_access(
            args.ssd_root(),
            allow_default_ssd,
            writer_group,
        )?;
        super::registry_access::grant_writer_group_registry_access(&registry_path, writer_group)?;
        super::registry_access::grant_writer_group_registry_access(
            &default_subobject_registry_path(),
            writer_group,
        )?;
    }
    if let Some(reader_group) = &definition.reader_group {
        super::registry_access::grant_writer_group_registry_access(&registry_path, reader_group)?;
        super::registry_access::grant_writer_group_registry_access(
            &default_subobject_registry_path(),
            reader_group,
        )?;
    }

    if let Some(response) = daemon_response {
        if args.json() {
            serde_json::to_writer_pretty(
                &mut *writer,
                &serde_json::json!({
                    "daemon": response,
                    "portable": portable_report,
                }),
            )?;
            writer.write_all(b"\n")?;
        } else {
            writeln!(writer, "Store creation accepted by daemon")?;
            writeln!(writer, "Store: {}", response.store_id)?;
            writeln!(writer, "Job: {}", response.accepted.job_id)?;
            writeln!(writer, "Class: {}", response.store_class)?;
            writeln!(writer, "Copies: {}", response.required_copies)?;
            if let Some(bucket) = response.bucket {
                writeln!(writer, "Bucket: {bucket}")?;
            }
            if let Some(portable) = &portable_report {
                writeln!(
                    writer,
                    "Portable registry: {}",
                    portable.registry_path.display()
                )?;
            } else {
                writeln!(writer, "Portable registry: not detected")?;
            }
        }
    } else if args.json() {
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({
                "host": report.expect("direct store create report"),
                "portable": portable_report,
            }),
        )?;
        writer.write_all(b"\n")?;
    } else {
        write_store_create_report(&report.expect("direct store create report"), writer)?;
        match &portable_report {
            Some(report) => writeln!(
                writer,
                "Portable registry: {}",
                report.registry_path.to_string_lossy()
            )?,
            None => writeln!(writer, "Portable registry: not detected")?,
        }
    }

    Ok(())
}

pub(super) fn create_object_store_request(
    args: &StoreCreateArgs,
    definition: &StoreServiceDefinition,
) -> CreateObjectStoreRequest {
    let policy = &definition.policy;
    CreateObjectStoreRequest {
        store_id: definition.store_id.to_string(),
        store_class: policy.class.name().to_string(),
        required_copies: policy.copies,
        bucket: definition.bucket_name.clone(),
        reader_group: definition.reader_group.clone(),
        writer_group: definition
            .writer_group
            .clone()
            .expect("daemon create requires a writer group"),
        ssd_root: args
            .ssd_root()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_ssd_root),
        object_type: "naive".to_string(),
        enclosure_id: None,
        public: definition.public,
        writeable: true,
        capacity_behavior: match policy.capacity_behavior {
            CapacityBehavior::RejectWrites => "reject_writes",
            CapacityBehavior::BackpressureByPriority => "backpressure_by_priority",
            CapacityBehavior::MarkRedownloadRequired => "mark_redownload_required",
        }
        .to_string(),
        retention: match policy.retention_policy {
            RetentionPolicy::ImmediateDelete => "immediate_delete",
            RetentionPolicy::TombstoneThenGc => "tombstone_then_gc",
        }
        .to_string(),
        endpoint_export_mode: match policy.export_policy {
            ExportPolicy::S3 => "s3_bucket",
            ExportPolicy::ReadOnlyFileExport => "read_only_file_export",
            ExportPolicy::Disabled => "disabled",
        }
        .to_string(),
        dry_run: false,
        client_request_id: Some(format!("cli-store-create-{}", definition.store_id)),
        administrator_actor: None,
        confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn daemon_create_request_maps_store_policy_contract() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "create",
            "generated-data",
            "--class",
            "generated_data",
            "--copies",
            "2",
            "--bucket",
            "generated-data",
            "--reader-group",
            "bioinformatics-readers",
            "--writer-group",
            "bioinformatics-writers",
            "--public",
        ])
        .expect("store create parses");
        let Some(Command::Store(store_args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::Create(args)) = store_args.command() else {
            panic!("expected store create command")
        };
        let definition = StoreServiceDefinition {
            store_id: args.store_id().clone(),
            policy: StorePolicy::defaults_for(args.class()),
            bucket_name: args.bucket().map(ToOwned::to_owned),
            reader_group: args.reader_group().map(ToOwned::to_owned),
            writer_group: args.writer_group().map(ToOwned::to_owned),
            public: args.public(),
        };
        let request = create_object_store_request(args, &definition);

        assert_eq!(request.store_id, "generated-data");
        assert_eq!(request.store_class, "generated_data");
        assert_eq!(request.required_copies, 2);
        assert_eq!(request.bucket.as_deref(), Some("generated-data"));
        assert_eq!(
            request.reader_group.as_deref(),
            Some("bioinformatics-readers")
        );
        assert_eq!(request.writer_group, "bioinformatics-writers");
        assert_eq!(request.object_type, "naive");
        assert!(request.public);
        assert!(request.writeable);
        assert_eq!(request.capacity_behavior, "backpressure_by_priority");
        assert_eq!(request.retention, "tombstone_then_gc");
        assert_eq!(request.endpoint_export_mode, "s3_bucket");
        assert!(!request.dry_run);
        assert_eq!(
            request.client_request_id.as_deref(),
            Some("cli-store-create-generated-data")
        );
        assert_eq!(
            request.confirmation_marker,
            OBJECT_STORE_CREATE_CONFIRMATION
        );
        request.validate().expect("daemon request validates");
    }
}

pub(super) fn run_store_adopt(
    args: &StoreAdoptArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let ssd_root = super::registry_access::known_ssd_root_for_adopt(args.ssd_root())?;
    let portable_registry_path = portable_store_registry_path(&ssd_root);
    let definitions = read_store_registry(&portable_registry_path)?;
    if definitions.is_empty() {
        return Err(CliError::PortableRegistry(format!(
            "portable store registry is empty at {}",
            portable_registry_path.display()
        )));
    }

    let host_registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let mut reports = Vec::new();
    for definition in definitions {
        reports.push(upsert_store_definition(
            &host_registry_path,
            definition.clone(),
        )?);
    }

    if args.json() {
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({
                "ssd_root": ssd_root,
                "portable_registry_path": portable_registry_path,
                "host_registry_path": host_registry_path,
                "adopted": reports,
            }),
        )?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Portable store registry adopted")?;
        writeln!(writer, "SSD root: {}", ssd_root.to_string_lossy())?;
        writeln!(
            writer,
            "Portable registry: {}",
            portable_registry_path.to_string_lossy()
        )?;
        writeln!(
            writer,
            "Host registry: {}",
            host_registry_path.to_string_lossy()
        )?;
        writeln!(writer, "Stores adopted: {}", reports.len())?;
        for report in &reports {
            writeln!(
                writer,
                "- {} action={} class={} copies={}",
                report.definition.store_id,
                report.action.as_str(),
                report.definition.policy.class.name(),
                report.definition.policy.copies
            )?;
        }
    }

    Ok(())
}

pub(super) fn run_store_ingest_policy(
    args: &StoreIngestPolicyArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path.clone()));

    if let Some(mode) = args.ingest_mode() {
        let response =
            client.update_object_store_ingest_policy(UpdateObjectStoreIngestPolicyRequest {
                store_id: args.store_id().to_string(),
                ingest_mode: mode.as_api_value().to_string(),
                dry_run: args.dry_run(),
                client_request_id: Some(format!("cli-store-policy-{}", args.store_id())),
                administrator_actor: None,
                confirmation_marker: args.confirm().to_string(),
            })?;
        if args.json() {
            serde_json::to_writer_pretty(&mut *writer, &response)?;
            writer.write_all(b"\n")?;
        } else {
            writeln!(
                writer,
                "ObjectStore ingest policy {}",
                if response.changed {
                    "updated"
                } else {
                    "unchanged"
                }
            )?;
            writeln!(writer, "Store: {}", response.store_id)?;
            writeln!(writer, "Previous mode: {:?}", response.previous_ingest_mode)?;
            writeln!(writer, "Requested mode: {:?}", response.ingest_mode)?;
            writeln!(writer, "Dry run: {}", response.accepted.dry_run)?;
            writeln!(
                writer,
                "Administrator: {}",
                response.administrator_actor.as_deref().unwrap_or("unknown")
            )?;
        }
    } else {
        let response = client.store_inventory(StoreInventoryRequest {
            include_policy: true,
            ..StoreInventoryRequest::default()
        })?;
        let store = response
            .stores
            .into_iter()
            .find(|store| store.store_id == *args.store_id())
            .ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "object store not found or not visible: {}",
                    args.store_id()
                ))
            })?;
        if args.json() {
            serde_json::to_writer_pretty(&mut *writer, &store)?;
            writer.write_all(b"\n")?;
        } else {
            writeln!(writer, "ObjectStore ingest policy")?;
            writeln!(writer, "Store: {}", store.store_id)?;
            writeln!(writer, "Mode: {:?}", store.policy.ingest_mode)?;
            writeln!(writer, "Copies: {}", store.policy.copies)?;
        }
    }
    if !args.json() {
        writeln!(writer, "Daemon socket: {}", config.socket_path.display())?;
    }
    Ok(())
}

pub(super) fn run_store_repair(
    args: &StoreRepairArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path.clone()));
    let started_at = std::time::Instant::now();
    let response = client.store_repair_with_progress(
        DaemonStoreRepairRequest {
            store_id: args.store_id().cloned(),
            dry_run: !args.apply(),
            confirmation: args.confirm().to_string(),
            reconcile_s3: args.reconcile_s3(),
            s3_prefix: args.s3_prefix().map(ToOwned::to_owned),
        },
        |event| {
            super::write_daemon_ingest_progress(writer, &event, started_at)
                .map_err(|error| DaemonClientError::Transport(error.to_string()))
        },
    )?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        let dasobjectstore_daemon::StoreRepairResponse {
            report,
            s3_reconciliation,
        } = response;
        writeln!(writer, "ObjectStore metadata repair")?;
        writeln!(writer, "Metadata: {}", report.metadata_path)?;
        writeln!(writer, "Dry run: {}", report.dry_run)?;
        writeln!(writer, "Stores scanned: {}", report.stores_scanned)?;
        writeln!(writer, "Payload files: {}", report.payload_files)?;
        writeln!(writer, "Objects recovered: {}", report.objects_recovered)?;
        writeln!(
            writer,
            "Placements recovered: {}",
            report.placements_recovered
        )?;
        writeln!(writer, "Payload bytes: {}", report.payload_bytes)?;
        writeln!(
            writer,
            "Partial duplicates omitted: {}",
            report.partial_duplicates_omitted
        )?;
        writeln!(writer, "Hashes verified: {}", report.hashes_verified)?;
        if let Some(backup_path) = report.backup_path {
            writeln!(writer, "Previous metadata backup: {backup_path}")?;
        }
        writeln!(writer, "Warning: {}", report.warning)?;
        if let Some(reconciliation) = s3_reconciliation {
            writeln!(writer, "Garage S3 reconciliation")?;
            writeln!(writer, "Bucket: {}", reconciliation.bucket_name)?;
            if let Some(prefix) = reconciliation.prefix {
                writeln!(writer, "Prefix: {prefix}")?;
            }
            writeln!(writer, "SSD staging: {}", reconciliation.staging_path)?;
            if let Some(manifest_path) = reconciliation.manifest_path {
                writeln!(writer, "Reconciliation manifest: {manifest_path}")?;
            }
            if let Some(job_id) = reconciliation.ingest_job_id {
                writeln!(writer, "Ingest job: {job_id}")?;
            }
        }
    }
    Ok(())
}

pub(super) fn run_store_verify(
    args: &StoreVerifyArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path.clone()));
    let response = client.store_verify(DaemonStoreVerifyRequest {
        store_id: args.store_id().cloned(),
        hash_payloads: args.hash(),
    })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        let report = response.report;
        writeln!(writer, "ObjectStore verification")?;
        writeln!(writer, "Metadata: {}", report.metadata_path)?;
        writeln!(writer, "Healthy: {}", report.healthy)?;
        writeln!(writer, "Objects: {}", report.objects_scanned)?;
        writeln!(writer, "Placements: {}", report.placements_scanned)?;
        writeln!(writer, "Payloads checked: {}", report.payloads_checked)?;
        writeln!(writer, "Missing payloads: {}", report.missing_payloads)?;
        writeln!(writer, "Orphan payloads: {}", report.orphan_payloads)?;
        writeln!(writer, "Size mismatches: {}", report.size_mismatches)?;
        writeln!(writer, "Hash mismatches: {}", report.hash_mismatches)?;
        writeln!(
            writer,
            "Unverified placements: {}",
            report.unverified_placements
        )?;
        writeln!(
            writer,
            "Duplicate content groups: {}",
            report.duplicate_content_groups
        )?;
        writeln!(
            writer,
            "Duplicate placement rows: {}",
            report.duplicate_placement_rows
        )?;
        writeln!(writer, "I/O errors: {}", report.io_errors)?;
        for finding in report.findings {
            writeln!(writer, "- {finding}")?;
        }
    }
    Ok(())
}

pub(super) fn run_store_deduplicate(
    args: &StoreDeduplicateArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path.clone()));
    let response = client.store_deduplicate(DaemonStoreDeduplicateRequest {
        store_id: args.store_id().cloned(),
        dry_run: !args.apply(),
        confirmation: args.confirm().to_string(),
    })?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
    } else {
        let report = response.report;
        writeln!(writer, "ObjectStore deduplication")?;
        writeln!(writer, "Metadata: {}", report.metadata_path)?;
        writeln!(writer, "Dry run: {}", report.dry_run)?;
        writeln!(writer, "Payloads hashed: {}", report.payloads_hashed)?;
        writeln!(writer, "Hash errors: {}", report.hash_errors)?;
        writeln!(
            writer,
            "Duplicate content groups: {}",
            report.duplicate_content_groups
        )?;
        writeln!(
            writer,
            "Duplicate placement rows: {}",
            report.duplicate_placement_rows
        )?;
        writeln!(
            writer,
            "Metadata rows removed: {}",
            report.metadata_rows_removed
        )?;
        writeln!(writer, "Hashes recorded: {}", report.hashes_recorded)?;
        writeln!(writer, "Warning: {}", report.warning)?;
    }
    Ok(())
}
