//! Read-only ObjectStore inspection and policy-validation handlers.

use super::*;

pub(super) fn run_store_contents(
    args: &StoreContentsArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if args.du() && args.tree() {
        return Err(CliError::UnsupportedStoreContentsFormat);
    }
    let live_sqlite_path =
        resolve_store_live_sqlite_path(args.store_id(), args.live_sqlite_path(), None)?;
    let mut request = StoreContentsRequest::new(live_sqlite_path, args.store_id().clone());
    if let Some(filter) = args.filter() {
        request = request.with_filter(filter);
    }
    let snapshot = read_store_contents(&request)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &snapshot)?;
        writer.write_all(b"\n")?;
    } else if args.tree() {
        write_store_contents_tree(&snapshot, args.depth(), writer)?;
    } else {
        write_store_contents_du(&snapshot, args.depth(), writer)?;
    }

    Ok(())
}

pub(super) fn run_store_list(
    args: &StoreListArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = if args.portable() {
        let ssd_root = known_ssd_root_for_adopt(args.ssd_root())?;
        portable_store_registry_path(ssd_root)
    } else {
        args.registry_path()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_store_registry_path)
    };
    let definitions = read_store_registry(&registry_path)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &definitions)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_list_report(&definitions, writer)?;
    }

    Ok(())
}

pub(super) fn run_store_defaults(
    args: &StoreDefaultsArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let policy = StorePolicy::defaults_for(args.class());

    serde_json::to_writer_pretty(&mut *writer, &policy)?;
    writer.write_all(b"\n")?;

    Ok(())
}

pub(super) fn run_store_validate(
    args: &StoreValidateArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let file = File::open(args.policy_file())?;
    let policy: StorePolicy = serde_json::from_reader(file)?;

    policy.validate()?;
    writeln!(writer, "Store policy is valid: {}", policy.class.name())?;

    Ok(())
}
