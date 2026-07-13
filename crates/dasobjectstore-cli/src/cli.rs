use clap::{Args, CommandFactory, Parser, Subcommand};
use dasobjectstore_object_service::ObjectServiceProviderId;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod disk;
mod ingest;
mod object;
mod performance;
mod pool;
mod service;
mod store;
mod subobject;

pub(crate) use disk::{
    DiskArgs, DiskCommand, DiskDrainArgs, DiskForceRetireArgs, DiskLockdownDasArgs,
    DiskPrepareDasArgs, DiskPrepareFilesystem, DiskReplaceArgs, DiskRetireArgs,
};
pub(crate) use ingest::{
    IngestArgs, IngestCommand, IngestControlArgs, IngestDirectImportArgs, IngestDrainQueueArgs,
    IngestFilesArgs, IngestQueueArgs, IngestStatusArgs,
};
pub(crate) use object::{
    ObjectArgs, ObjectCommand, ObjectExportArgs, ObjectInspectArgs, ObjectPutArgs,
};
#[cfg(feature = "debug-commands")]
pub(crate) use pool::PoolMarkerArgs;
pub(crate) use pool::{PoolArgs, PoolCommand, PoolImportArgs, PoolInspectArgs, PoolRepairArgs};
pub(crate) use service::{
    ServiceArgs, ServiceCommand, ServiceComposeArgs, ServiceProvisionArgs,
    ServiceRenderComposeArgs, ServiceStatusArgs,
};
pub(crate) use store::{
    StoreAdoptArgs, StoreArgs, StoreCapabilitiesArgs, StoreCapacityArgs, StoreCommand,
    StoreContentsArgs, StoreCreateArgs, StoreDeduplicateArgs, StoreDefaultsArgs, StoreDeleteArgs,
    StoreDrainArgs, StoreIngestPolicyArgs, StoreListArgs, StoreProfileBindingArgs,
    StoreProfileBindingOperation, StoreProfileInspectionArgs, StoreRepairArgs, StoreS3UploadArgs,
    StoreUserServicePlanArgs, StoreValidateArgs, StoreVerifyArgs,
};
pub(crate) use subobject::{
    SubobjectArgs, SubobjectCommand, SubobjectCreateArgs, SubobjectListArgs, SubobjectSearchArgs,
};

pub(crate) use performance::{
    PerformanceFileOrder, PerformanceFileSelection, PerformanceReportArgs,
    PerformanceScenarioSelection, PerformanceTestArgs,
};

/// Portable mixed-disk DAS object store.
#[derive(Debug, Parser)]
#[command(name = "dasobjectstore", version = dasobjectstore_core::VERSION)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

impl Cli {
    pub(crate) fn command(&self) -> Option<&Command> {
        self.command.as_ref()
    }

    pub(crate) fn write_help(writer: &mut impl Write) -> io::Result<()> {
        let mut command = <Self as CommandFactory>::command();
        command.write_help(writer)?;
        writeln!(writer)
    }

    pub(crate) fn write_subcommand_help(name: &str, writer: &mut impl Write) -> io::Result<()> {
        let mut command = <Self as CommandFactory>::command();
        let help_result = command.try_get_matches_from_mut(["dasobjectstore", name, "--help"]);

        match help_result {
            Ok(_) => Ok(()),
            Err(err) => write!(writer, "{err}"),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum Command {
    /// Inspect candidate DAS disks and enclosures.
    Probe(ProbeArgs),
    /// Report pool, disk, and service health.
    Health(HealthArgs),
    /// Report daemon, Web UI, and object-service listener status.
    Status(StatusArgs),
    /// Manage portable storage pools.
    Pool(PoolArgs),
    /// Manage DAS member disks.
    Disk(DiskArgs),
    /// Manage object stores and policy.
    Store(StoreArgs),
    /// Inspect SSD ingest and destage work.
    Ingest(IngestArgs),
    /// Manage named SubObject endpoints.
    Subobject(SubobjectArgs),
    /// Inspect object metadata.
    Object(ObjectArgs),
    /// Render and manage the S3-compatible object service.
    Service(ServiceArgs),
    /// Export Mnemosyne/Synoptikon integration metadata.
    Mnemosyne(MnemosyneArgs),
    /// Benchmark SSD and HDD ingest settlement performance.
    PerformanceTest(PerformanceTestArgs),
    /// Rebuild a formal performance PDF report from an existing JSON artifact.
    PerformanceReport(PerformanceReportArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StatusArgs {
    /// Emit status as JSON.
    #[arg(long)]
    json: bool,
}

impl StatusArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct HealthArgs {
    /// Emit one-line pool and disk health summary.
    #[arg(long)]
    summary: bool,
    /// Emit per-disk health details.
    #[arg(long)]
    verbose: bool,
    /// Emit host connection and DAS transport warnings.
    #[arg(long)]
    connections: bool,
    /// Emit health report as JSON.
    #[arg(long)]
    json: bool,
}

impl HealthArgs {
    pub(crate) fn summary(&self) -> bool {
        self.summary
    }

    pub(crate) fn verbose(&self) -> bool {
        self.verbose
    }

    pub(crate) fn connections(&self) -> bool {
        self.connections
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ProbeArgs {
    /// Emit probe results as JSON.
    #[arg(long)]
    json: bool,
    /// Emit probe results as human-readable text.
    #[arg(long)]
    pretty: bool,
}

impl ProbeArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn pretty(&self) -> bool {
        self.pretty
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct MnemosyneArgs {
    #[command(subcommand)]
    command: MnemosyneCommand,
}

impl MnemosyneArgs {
    pub(crate) fn command(&self) -> &MnemosyneCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum MnemosyneCommand {
    /// Export Mneion-compatible storage definition and binding JSON.
    Export(MnemosyneExportArgs),
    /// Validate a DASObjectStore-managed NAS/NFS endpoint definition.
    ValidateNasNfsEndpoint(MnemosyneValidateNasNfsEndpointArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct MnemosyneExportArgs {
    /// Mneion object-store UUID to create or update.
    #[arg(long)]
    object_store_id: String,
    /// Mneion object-store display name.
    #[arg(long)]
    display_name: String,
    /// DASObjectStore object-service provider backing the endpoint.
    #[arg(long)]
    provider: ObjectServiceProviderId,
    /// S3-compatible HTTP endpoint exposed to Mneion/Limen.
    #[arg(long)]
    endpoint: String,
    /// Mneion governance-domain UUID to bind to the object store.
    #[arg(long)]
    governance_domain_id: String,
    /// Optional operator note to include in the Mneion link request.
    #[arg(long)]
    note: Option<String>,
}

impl MnemosyneExportArgs {
    pub(crate) fn object_store_id(&self) -> &str {
        &self.object_store_id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) fn provider(&self) -> ObjectServiceProviderId {
        self.provider
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn governance_domain_id(&self) -> &str {
        &self.governance_domain_id
    }

    pub(crate) fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct MnemosyneValidateNasNfsEndpointArgs {
    /// Path to a JSON NAS/NFS endpoint definition file.
    #[arg(long)]
    definition_file: PathBuf,
    /// Emit validated endpoint details as JSON.
    #[arg(long)]
    json: bool,
}

impl MnemosyneValidateNasNfsEndpointArgs {
    pub(crate) fn definition_file(&self) -> &Path {
        &self.definition_file
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, MnemosyneCommand, PerformanceFileOrder, PerformanceFileSelection,
        PerformanceScenarioSelection, ProbeArgs, StatusArgs,
    };
    use clap::Parser;
    use std::path::Path;

    #[test]
    fn parses_without_subcommand() {
        let cli = Cli::try_parse_from(["dasobjectstore"]).expect("root command parses");

        assert_eq!(cli.command(), None);
    }

    #[test]
    fn parses_top_level_command_skeletons() {
        let store = Cli::try_parse_from(["dasobjectstore", "store"])
            .expect("store subcommand should parse");
        let Some(Command::Store(args)) = store.command() else {
            panic!("expected store command");
        };
        assert!(args.command().is_none());

        let ingest = Cli::try_parse_from(["dasobjectstore", "ingest"])
            .expect("ingest subcommand should parse");
        let Some(Command::Ingest(args)) = ingest.command() else {
            panic!("expected ingest command");
        };
        assert!(args.command().is_none());
    }

    #[test]
    fn parses_top_level_status_json() {
        let cli = Cli::try_parse_from(["dasobjectstore", "status", "--json"])
            .expect("status command parses");

        assert_eq!(
            cli.command(),
            Some(&Command::Status(StatusArgs { json: true }))
        );
    }

    #[test]
    fn parses_performance_test_options() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--file-size",
            "1GiB",
            "--file-count",
            "2",
            "--max-hdd-concurrency",
            "5",
            "--scenario",
            "ssd-overlap-drain",
            "--scenario",
            "direct-hdd",
            "--hdd-concurrency",
            "1,3,5",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--hdd-root",
            "/srv/dasobjectstore/hdd",
            "--tmp-dir",
            "/tmp/dos-perf",
            "--report",
            "/tmp/dos-perf/report.pdf",
            "--json-artifact",
            "/tmp/dos-perf/report.json",
            "--tui",
            "--authoritative",
            "--keep-temp",
        ])
        .expect("performance-test parses");

        let Some(Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };
        assert_eq!(args.file_size(), Some("1GiB"));
        assert_eq!(args.file_count(), Some(2));
        assert_eq!(args.source(), None);
        assert_eq!(args.cap(), None);
        assert_eq!(args.file_orders(), vec![PerformanceFileOrder::SizeDesc]);
        assert_eq!(args.max_hdd_concurrency(), 5);
        assert_eq!(
            args.scenarios(),
            &[
                PerformanceScenarioSelection::SsdOverlapDrain,
                PerformanceScenarioSelection::DirectHdd
            ]
        );
        assert_eq!(args.hdd_concurrency(), &[1, 3, 5]);
        assert_eq!(args.ssd_root(), Some(Path::new("/srv/dasobjectstore/ssd")));
        assert_eq!(args.hdd_root(), Some(Path::new("/srv/dasobjectstore/hdd")));
        assert_eq!(args.tmp_dir(), Path::new("/tmp/dos-perf"));
        assert_eq!(args.report(), Some(Path::new("/tmp/dos-perf/report.pdf")));
        assert_eq!(
            args.json_artifact(),
            Some(Path::new("/tmp/dos-perf/report.json"))
        );
        assert!(args.tui());
        assert!(args.authoritative());
        assert!(args.keep_temp());
    }

    #[test]
    fn parses_performance_test_source_folder_options() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--source",
            "/data/source-folder",
            "--cap",
            "750GiB",
            "--file_select",
            "larger",
            "--file_order",
            "fifo,size_desc",
            "--max-hdd-concurrency",
            "5",
        ])
        .expect("performance-test source parses");

        let Some(Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };
        assert_eq!(args.source(), Some(Path::new("/data/source-folder")));
        assert_eq!(args.cap(), Some("750GiB"));
        assert_eq!(args.file_select(), PerformanceFileSelection::Larger);
        assert_eq!(
            args.file_orders(),
            vec![PerformanceFileOrder::Fifo, PerformanceFileOrder::SizeDesc]
        );
        assert_eq!(args.file_size(), None);
        assert_eq!(args.file_count(), None);
        assert_eq!(args.max_hdd_concurrency(), 5);
    }

    #[test]
    fn parses_performance_report_rebuild_options() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-report",
            "--json-artifact",
            "/var/lib/dasobjectstore/reports/performance.json",
            "--report",
            "/var/lib/dasobjectstore/reports/performance.pdf",
            "--tmp-dir",
            "/var/tmp",
            "--keep-markdown",
        ])
        .expect("performance-report parses");

        let Some(Command::PerformanceReport(args)) = cli.command() else {
            panic!("expected performance-report command");
        };
        assert_eq!(
            args.json_artifact(),
            Path::new("/var/lib/dasobjectstore/reports/performance.json")
        );
        assert_eq!(
            args.report(),
            Some(Path::new("/var/lib/dasobjectstore/reports/performance.pdf"))
        );
        assert_eq!(args.tmp_dir(), Path::new("/var/tmp"));
        assert!(args.keep_markdown());
    }

    #[test]
    fn parses_mnemosyne_export() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "export",
            "--object-store-id",
            "4f0a1ba7-9f00-422b-bf18-87567b076daa",
            "--display-name",
            "DASObjectStore Development",
            "--provider",
            "garage",
            "--endpoint",
            "http://127.0.0.1:3900",
            "--governance-domain-id",
            "22222222-2222-2222-2222-222222222222",
            "--note",
            "DASObjectStore development store",
        ])
        .expect("mnemosyne export parses");

        let Some(Command::Mnemosyne(args)) = cli.command() else {
            panic!("expected mnemosyne command");
        };
        match args.command() {
            MnemosyneCommand::Export(export) => {
                assert_eq!(
                    export.object_store_id(),
                    "4f0a1ba7-9f00-422b-bf18-87567b076daa"
                );
                assert_eq!(export.display_name(), "DASObjectStore Development");
                assert_eq!(export.provider().name(), "garage");
                assert_eq!(export.endpoint(), "http://127.0.0.1:3900");
                assert_eq!(
                    export.governance_domain_id(),
                    "22222222-2222-2222-2222-222222222222"
                );
                assert_eq!(export.note(), Some("DASObjectStore development store"));
            }
            MnemosyneCommand::ValidateNasNfsEndpoint(_) => panic!("expected export command"),
        }
    }

    #[test]
    fn parses_mnemosyne_validate_nas_nfs_endpoint() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "validate-nas-nfs-endpoint",
            "--definition-file",
            "/tmp/nas-endpoint.json",
            "--json",
        ])
        .expect("mnemosyne NAS/NFS endpoint validation parses");

        let Some(Command::Mnemosyne(args)) = cli.command() else {
            panic!("expected mnemosyne command");
        };
        match args.command() {
            MnemosyneCommand::ValidateNasNfsEndpoint(validate) => {
                assert_eq!(
                    validate.definition_file(),
                    Path::new("/tmp/nas-endpoint.json")
                );
                assert!(validate.json());
            }
            MnemosyneCommand::Export(_) => panic!("expected NAS/NFS endpoint validation command"),
        }
    }

    #[test]
    fn parses_health_summary_default() {
        let cli = Cli::try_parse_from(["dasobjectstore", "health"]).expect("health parses");

        let Some(Command::Health(args)) = cli.command() else {
            panic!("expected health command");
        };
        assert!(!args.summary());
        assert!(!args.verbose());
        assert!(!args.connections());
        assert!(!args.json());
    }

    #[test]
    fn parses_health_output_flags() {
        let cases = [
            ("--summary", true, false, false, false),
            ("--verbose", false, true, false, false),
            ("--connections", false, false, true, false),
            ("--json", false, false, false, true),
        ];

        for (flag, summary, verbose, connections, json) in cases {
            let cli =
                Cli::try_parse_from(["dasobjectstore", "health", flag]).expect("health parses");

            let Some(Command::Health(args)) = cli.command() else {
                panic!("expected health command");
            };
            assert_eq!(args.summary(), summary);
            assert_eq!(args.verbose(), verbose);
            assert_eq!(args.connections(), connections);
            assert_eq!(args.json(), json);
        }
    }

    #[test]
    fn parses_probe_json_flag() {
        let cli = Cli::try_parse_from(["dasobjectstore", "probe", "--json"]).expect("probe parses");

        assert_eq!(
            cli.command(),
            Some(&Command::Probe(ProbeArgs {
                json: true,
                pretty: false
            }))
        );
    }

    #[test]
    fn parses_probe_pretty_flag() {
        let cli =
            Cli::try_parse_from(["dasobjectstore", "probe", "--pretty"]).expect("probe parses");

        assert_eq!(
            cli.command(),
            Some(&Command::Probe(ProbeArgs {
                json: false,
                pretty: true
            }))
        );
    }
}
