//! Performance-test command-line contracts.

use clap::{Args, ValueEnum};
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PerformanceTestArgs {
    /// Size of each generated test file, for example 100MiB, 1GiB, or 1.1TiB.
    #[arg(long = "file_size", alias = "file-size")]
    file_size: Option<String>,
    /// Number of generated files to test.
    #[arg(long = "file_count", alias = "file-count")]
    file_count: Option<u32>,
    /// Existing folder of files to benchmark instead of generated random data.
    #[arg(long)]
    source: Option<PathBuf>,
    /// Cap an existing source-folder benchmark to a prefix such as 750GiB or 1TiB.
    #[arg(long)]
    cap: Option<String>,
    /// Source-folder file selection policy used with --cap.
    #[arg(
        long = "file_select",
        alias = "file-select",
        value_enum,
        default_value_t = PerformanceFileSelection::Random
    )]
    file_select: PerformanceFileSelection,
    /// File upload order for generated or selected source workloads.
    #[arg(
        long = "file_order",
        alias = "file-order",
        value_enum,
        value_delimiter = ',',
        num_args = 1..
    )]
    file_order: Vec<PerformanceFileOrder>,
    /// Maximum concurrent HDD writes to model.
    #[arg(long, default_value_t = 3)]
    max_hdd_concurrency: usize,
    /// Scenario class to include; repeat to run a selected matrix instead of the default full sweep.
    #[arg(long = "scenario", value_enum)]
    scenarios: Vec<PerformanceScenarioSelection>,
    /// HDD concurrency values to run for HDD-writing scenarios, for example 1,3,5.
    #[arg(long = "hdd-concurrency", value_delimiter = ',', num_args = 1..)]
    hdd_concurrency: Vec<usize>,
    /// Number of HDD copies to land for each logical file; accepted values are 1, 2, or 3.
    #[arg(long, default_value_t = 1)]
    redundancy: usize,
    /// SSD root to stress; defaults to DASOBJECTSTORE_SSD_ROOT or the packaged root.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Managed HDD root containing per-disk roots.
    #[arg(long, hide = true)]
    hdd_root: Option<PathBuf>,
    /// Directory for generated source files; defaults to /tmp.
    #[arg(long, default_value = "/tmp")]
    tmp_dir: PathBuf,
    /// Final PDF report path; defaults to a timestamped PDF file in /tmp.
    #[arg(long)]
    report: Option<PathBuf>,
    /// JSON artifact path; defaults beside the PDF report.
    #[arg(long = "json-artifact")]
    json_artifact: Option<PathBuf>,
    /// Render an embedded terminal benchmark view while the run executes.
    #[arg(long)]
    tui: bool,
    /// Store the benchmark recommendation as the daemon's authoritative ingest policy.
    #[arg(long)]
    authoritative: bool,
    /// Keep temporary benchmark files for inspection instead of deleting them.
    #[arg(long)]
    keep_temp: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
pub(crate) enum PerformanceScenarioSelection {
    /// Write all selected files to SSD, then read them back from SSD.
    SsdOnly,
    /// Stage selected files to SSD first, then drain SSD to HDD.
    SsdStageThenDrain,
    /// Overlap SSD ingest with FIFO HDD drain.
    SsdOverlapDrain,
    /// Write source files directly to HDD without SSD staging.
    DirectHdd,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
pub(crate) enum PerformanceFileSelection {
    /// Select a random whole-file subset under --cap.
    Random,
    /// Select smaller files first until the --cap budget is exhausted.
    Smaller,
    /// Select larger files first until the --cap budget is exhausted.
    Larger,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, ValueEnum)]
pub(crate) enum PerformanceFileOrder {
    /// Preserve FIFO source order by relative path.
    Fifo,
    /// Upload smaller files first.
    #[value(alias = "size_asc")]
    SizeAsc,
    /// Upload larger files first.
    #[value(alias = "size_desc")]
    SizeDesc,
    /// Upload older files first by source modification time.
    #[value(alias = "time_asc")]
    TimeAsc,
    /// Upload newer files first by source modification time.
    #[value(alias = "time_desc")]
    TimeDesc,
}

impl PerformanceFileOrder {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Fifo => "fifo",
            Self::SizeAsc => "size_asc",
            Self::SizeDesc => "size_desc",
            Self::TimeAsc => "time_asc",
            Self::TimeDesc => "time_desc",
        }
    }
}

impl PerformanceFileSelection {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Smaller => "smaller",
            Self::Larger => "larger",
        }
    }
}

impl PerformanceTestArgs {
    pub(crate) fn file_size(&self) -> Option<&str> {
        self.file_size.as_deref()
    }
    pub(crate) fn file_count(&self) -> Option<u32> {
        self.file_count
    }
    pub(crate) fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
    pub(crate) fn cap(&self) -> Option<&str> {
        self.cap.as_deref()
    }
    pub(crate) fn file_select(&self) -> PerformanceFileSelection {
        self.file_select
    }
    pub(crate) fn file_orders(&self) -> Vec<PerformanceFileOrder> {
        if self.file_order.is_empty() {
            vec![PerformanceFileOrder::SizeDesc]
        } else {
            self.file_order.clone()
        }
    }
    pub(crate) fn max_hdd_concurrency(&self) -> usize {
        self.max_hdd_concurrency
    }
    pub(crate) fn scenarios(&self) -> &[PerformanceScenarioSelection] {
        &self.scenarios
    }
    pub(crate) fn hdd_concurrency(&self) -> &[usize] {
        &self.hdd_concurrency
    }
    pub(crate) fn redundancy(&self) -> usize {
        self.redundancy
    }
    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }
    pub(crate) fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }
    pub(crate) fn tmp_dir(&self) -> &Path {
        &self.tmp_dir
    }
    pub(crate) fn report(&self) -> Option<&Path> {
        self.report.as_deref()
    }
    pub(crate) fn json_artifact(&self) -> Option<&Path> {
        self.json_artifact.as_deref()
    }
    pub(crate) fn tui(&self) -> bool {
        self.tui
    }
    pub(crate) fn authoritative(&self) -> bool {
        self.authoritative
    }
    pub(crate) fn keep_temp(&self) -> bool {
        self.keep_temp
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PerformanceReportArgs {
    /// Existing performance-test JSON artifact to rebuild from.
    #[arg(long = "json-artifact")]
    json_artifact: PathBuf,
    /// Final PDF report path. Defaults to the PDF path recorded in the JSON artifact.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Directory for temporary Markdown during report rendering; defaults to /tmp.
    #[arg(long, default_value = "/tmp")]
    tmp_dir: PathBuf,
    /// Keep the temporary Markdown report source for inspection.
    #[arg(long)]
    keep_markdown: bool,
}

impl PerformanceReportArgs {
    pub(crate) fn json_artifact(&self) -> &Path {
        &self.json_artifact
    }
    pub(crate) fn report(&self) -> Option<&Path> {
        self.report.as_deref()
    }
    pub(crate) fn tmp_dir(&self) -> &Path {
        &self.tmp_dir
    }
    pub(crate) fn keep_markdown(&self) -> bool {
        self.keep_markdown
    }
}
