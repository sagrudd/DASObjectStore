//! Store creation and portable-registry write handlers.

use super::*;

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
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let report = upsert_store_definition(&registry_path, definition)?;
    let allow_default_ssd = args.registry_path().is_none() || args.ssd_root().is_some();
    let portable_report = super::upsert_portable_store_definition(
        args.ssd_root(),
        allow_default_ssd,
        &report.definition,
    )?;
    if let Some(writer_group) = &report.definition.writer_group {
        super::grant_store_writer_group_access(args.ssd_root(), allow_default_ssd, writer_group)?;
        super::grant_writer_group_registry_access(&registry_path, writer_group)?;
        super::grant_writer_group_registry_access(
            &default_subobject_registry_path(),
            writer_group,
        )?;
    }
    if let Some(reader_group) = &report.definition.reader_group {
        super::grant_writer_group_registry_access(&registry_path, reader_group)?;
        super::grant_writer_group_registry_access(
            &default_subobject_registry_path(),
            reader_group,
        )?;
    }

    if args.json() {
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({
                "host": report,
                "portable": portable_report,
            }),
        )?;
        writer.write_all(b"\n")?;
    } else {
        write_store_create_report(&report, writer)?;
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

pub(super) fn run_store_adopt(
    args: &StoreAdoptArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let ssd_root = super::known_ssd_root_for_adopt(args.ssd_root())?;
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
