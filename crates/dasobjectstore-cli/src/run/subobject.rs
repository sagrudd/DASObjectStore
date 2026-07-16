//! SubObject registry command handlers.

use super::*;
use crate::cli::{SubobjectCommand, SubobjectListArgs, SubobjectSearchArgs};
use dasobjectstore_core::store::CapacityPolicy;
use dasobjectstore_object_service::{
    create_subobject_definition_with_capacity, SubObjectParent, SubObjectRegistryUpdateReport,
};

pub(super) fn run_subobject(args: &SubobjectArgs, writer: &mut impl Write) -> Result<(), CliError> {
    match args.command() {
        SubobjectCommand::Create(args) => run_subobject_create(args, writer),
        SubobjectCommand::List(args) => run_subobject_list(args, writer),
        SubobjectCommand::Search(args) => run_subobject_search(args, writer),
    }
}

fn run_subobject_create(
    args: &SubobjectCreateArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let parent = subobject_parent_from_args(args)?;
    let capacity = args
        .capacity_limit_bytes()
        .map(|limit| CapacityPolicy::bounded(limit, 0));
    let report =
        create_subobject_definition_with_capacity(&registry_path, args.name(), parent, capacity)?;
    let allow_default_ssd = args.registry_path().is_none() || args.ssd_root().is_some();
    let portable_report = mirror_portable_subobject_definition(
        args.ssd_root(),
        allow_default_ssd,
        &report.definition,
    )?;
    super::registry_access::grant_subobject_writer_group_registry_access(
        args,
        &report.definition,
        &registry_path,
    )?;

    write_subobject_create_report(&report, portable_report.as_ref(), writer)
}

fn subobject_parent_from_args(args: &SubobjectCreateArgs) -> Result<SubObjectParent, CliError> {
    match (args.store(), args.parent()) {
        (Some(store_id), None) => {
            let stores_registry_path = args
                .stores_registry_path()
                .map(Path::to_path_buf)
                .unwrap_or_else(default_store_registry_path);
            let store_exists = read_store_registry(&stores_registry_path)?
                .iter()
                .any(|definition| definition.store_id == *store_id);
            if !store_exists {
                return Err(CliError::CommandFailed(format!(
                    "store {} was not found in {}",
                    store_id,
                    stores_registry_path.display()
                )));
            }
            Ok(SubObjectParent::Store {
                store_id: store_id.clone(),
            })
        }
        (None, Some(name)) => Ok(SubObjectParent::SubObject {
            name: name.to_string(),
        }),
        _ => Err(CliError::CommandFailed(
            "subobject create requires exactly one of --store or --parent".to_string(),
        )),
    }
}

fn run_subobject_list(args: &SubobjectListArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let definitions = read_subobject_registry(&registry_path)?;

    writeln!(writer, "SubObjects: {}", definitions.len())?;
    for definition in definitions {
        write_subobject_definition_line(&definition, writer)?;
    }

    Ok(())
}

fn run_subobject_search(
    args: &SubobjectSearchArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let definitions = read_subobject_registry(&registry_path)?;
    let matches = search_subobjects(&definitions, args.query());

    writeln!(writer, "SubObjects matched: {}", matches.len())?;
    for definition in matches {
        write_subobject_definition_line(definition, writer)?;
    }

    Ok(())
}

fn write_subobject_create_report(
    report: &SubObjectRegistryUpdateReport,
    portable_report: Option<&SubObjectRegistryUpdateReport>,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "SubObject {}", report.action.as_str())?;
    writeln!(writer, "Name: {}", report.definition.name)?;
    writeln!(writer, "Store: {}", report.definition.store_id)?;
    writeln!(
        writer,
        "Parent: {}",
        subobject_parent_label(&report.definition.parent)
    )?;
    writeln!(
        writer,
        "Object prefix: {}",
        report.definition.object_prefix()
    )?;
    match &report.definition.capacity {
        Some(policy) => writeln!(
            writer,
            "Logical capacity limit: {} bytes",
            policy
                .logical_limit_bytes
                .expect("bounded SubObject policy")
        )?,
        None => writeln!(writer, "Logical capacity limit: inherited")?,
    }
    writeln!(
        writer,
        "Registry: {}",
        report.registry_path.to_string_lossy()
    )?;
    match portable_report {
        Some(report) => writeln!(
            writer,
            "Portable registry: {}",
            report.registry_path.to_string_lossy()
        )?,
        None => writeln!(writer, "Portable registry: not detected")?,
    }

    Ok(())
}

fn mirror_portable_subobject_definition(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    definition: &SubObjectDefinition,
) -> Result<Option<SubObjectRegistryUpdateReport>, CliError> {
    let Some(ssd_root) =
        super::registry_access::known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)?
    else {
        return Ok(None);
    };
    let registry_path = portable_subobject_registry_path(&ssd_root);
    let report = mirror_subobject_definition(&registry_path, definition.clone())?;

    Ok(Some(report))
}

fn write_subobject_definition_line(
    definition: &SubObjectDefinition,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(
        writer,
        "- {} store={} parent={} prefix={} capacity={}",
        definition.name,
        definition.store_id,
        subobject_parent_label(&definition.parent),
        definition.object_prefix(),
        definition
            .capacity
            .as_ref()
            .and_then(|policy| policy.logical_limit_bytes)
            .map(|bytes| format!("{bytes}_bytes"))
            .unwrap_or_else(|| "inherited".to_string())
    )?;

    Ok(())
}

fn subobject_parent_label(parent: &SubObjectParent) -> String {
    match parent {
        SubObjectParent::Store { store_id } => format!("store:{store_id}"),
        SubObjectParent::SubObject { name } => format!("subobject:{name}"),
    }
}
