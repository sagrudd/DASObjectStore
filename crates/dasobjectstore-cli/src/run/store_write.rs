//! Store creation and portable-registry write handlers.

use super::*;

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
