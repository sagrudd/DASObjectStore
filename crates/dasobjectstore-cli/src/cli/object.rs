use clap::{Args, Subcommand};
use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_core::object_type::ObjectType;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectArgs {
    #[command(subcommand)]
    command: ObjectCommand,
}

impl ObjectArgs {
    pub(crate) fn command(&self) -> &ObjectCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum ObjectCommand {
    /// Export one settled object from a verified HDD placement.
    Export(ObjectExportArgs),
    /// Inspect one object from live metadata.
    Inspect(ObjectInspectArgs),
    /// Stage one object on SSD and settle verified HDD copies.
    Put(ObjectPutArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectExportArgs {
    /// Object identifier to export.
    object_id: ObjectId,
    /// Path to live.sqlite for the attached read-only pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Destination file to write.
    #[arg(long)]
    destination: PathBuf,
    /// Disk root mapping in the form disk-id=/mounted/disk/root.
    #[arg(long = "disk-root")]
    disk_roots: Vec<String>,
    /// Emit export report as JSON.
    #[arg(long)]
    json: bool,
}

impl ObjectExportArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }
    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }
    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }
    pub(crate) fn disk_roots(&self) -> &[String] {
        &self.disk_roots
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectPutArgs {
    /// Object identifier to store.
    object_id: ObjectId,
    /// Source file to import.
    #[arg(long)]
    source: PathBuf,
    /// Logical object type assigned to this file.
    #[arg(long, default_value_t = ObjectType::Naive)]
    object_type: ObjectType,
    /// SSD ingest root used for the mandatory fast landing copy.
    #[arg(long)]
    ssd_root: PathBuf,
    /// Disk root mapping in the form disk-id=/mounted/disk/root.
    #[arg(long = "disk-root")]
    disk_roots: Vec<String>,
    /// Number of verified HDD copies to settle.
    #[arg(long, default_value_t = 1)]
    copies: u8,
    /// Emit put report as JSON.
    #[arg(long)]
    json: bool,
}

impl ObjectPutArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }
    pub(crate) fn source(&self) -> &Path {
        &self.source
    }
    pub(crate) fn object_type(&self) -> ObjectType {
        self.object_type
    }
    pub(crate) fn ssd_root(&self) -> &Path {
        &self.ssd_root
    }
    pub(crate) fn disk_roots(&self) -> &[String] {
        &self.disk_roots
    }
    pub(crate) fn copies(&self) -> u8 {
        self.copies
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectInspectArgs {
    /// Object identifier to inspect.
    object_id: ObjectId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Emit object metadata as JSON.
    #[arg(long)]
    json: bool,
}

impl ObjectInspectArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }
    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectCommand;
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use dasobjectstore_core::object_type::ObjectType;
    use std::path::Path;

    #[test]
    fn parses_object_commands() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "inspect",
            "object-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--json",
        ])
        .expect("object inspect parses");
        let Some(Command::Object(args)) = cli.command() else {
            panic!("expected object command")
        };
        let ObjectCommand::Inspect(inspect) = args.command() else {
            panic!("expected inspect")
        };
        assert_eq!(inspect.object_id().as_str(), "object-a");
        assert_eq!(inspect.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
        assert!(inspect.json());

        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "export",
            "object-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--destination",
            "/tmp/export/object-a",
            "--disk-root",
            "disk-a=/Volumes/disk-a",
            "--json",
        ])
        .expect("object export parses");
        let Some(Command::Object(args)) = cli.command() else {
            panic!("expected object command")
        };
        let ObjectCommand::Export(export) = args.command() else {
            panic!("expected export")
        };
        assert_eq!(export.object_id().as_str(), "object-a");
        assert_eq!(export.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
        assert_eq!(export.destination(), Path::new("/tmp/export/object-a"));
        assert_eq!(export.disk_roots(), &["disk-a=/Volumes/disk-a".to_string()]);
        assert!(export.json());

        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "put",
            "object-a",
            "--source",
            "/tmp/input/object-a",
            "--object-type",
            "bam",
            "--ssd-root",
            "/tmp/ssd",
            "--disk-root",
            "disk-a=/Volumes/disk-a",
            "--disk-root",
            "disk-b=/Volumes/disk-b",
            "--copies",
            "2",
            "--json",
        ])
        .expect("object put parses");
        let Some(Command::Object(args)) = cli.command() else {
            panic!("expected object command")
        };
        let ObjectCommand::Put(put) = args.command() else {
            panic!("expected put")
        };
        assert_eq!(put.object_id().as_str(), "object-a");
        assert_eq!(put.source(), Path::new("/tmp/input/object-a"));
        assert_eq!(put.object_type(), ObjectType::Bam);
        assert_eq!(put.ssd_root(), Path::new("/tmp/ssd"));
        assert_eq!(
            put.disk_roots(),
            &[
                "disk-a=/Volumes/disk-a".to_string(),
                "disk-b=/Volumes/disk-b".to_string()
            ]
        );
        assert_eq!(put.copies(), 2);
        assert!(put.json());
    }
}
