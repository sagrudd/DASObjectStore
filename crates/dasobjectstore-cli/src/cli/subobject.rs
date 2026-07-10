use clap::{Args, Subcommand};
use dasobjectstore_core::ids::StoreId;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectArgs {
    #[command(subcommand)]
    command: SubobjectCommand,
}

impl SubobjectArgs {
    pub(crate) fn command(&self) -> &SubobjectCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum SubobjectCommand {
    Create(SubobjectCreateArgs),
    List(SubobjectListArgs),
    Search(SubobjectSearchArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectCreateArgs {
    name: String,
    #[arg(long, conflicts_with = "parent")]
    store: Option<StoreId>,
    #[arg(long)]
    parent: Option<String>,
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
    #[arg(long, hide = true)]
    stores_registry_path: Option<PathBuf>,
}

impl SubobjectCreateArgs {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn store(&self) -> Option<&StoreId> {
        self.store.as_ref()
    }
    pub(crate) fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }
    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }
    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
    pub(crate) fn stores_registry_path(&self) -> Option<&Path> {
        self.stores_registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectListArgs {
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl SubobjectListArgs {
    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectSearchArgs {
    query: String,
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl SubobjectSearchArgs {
    pub(crate) fn query(&self) -> &str {
        &self.query
    }
    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::SubobjectCommand;
    use crate::cli::{Cli, Command};
    use clap::Parser;

    #[test]
    fn parses_create_under_store() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "subobject",
            "create",
            "Xenognostikon",
            "--store",
            "ENA",
        ])
        .expect("subobject create parses");
        let Some(Command::Subobject(args)) = cli.command() else {
            panic!("expected subobject command")
        };
        let SubobjectCommand::Create(create) = args.command() else {
            panic!("expected create command")
        };
        assert_eq!(create.name(), "Xenognostikon");
        assert_eq!(create.store().expect("store").as_str(), "ENA");
        assert_eq!(create.parent(), None);
    }
}
