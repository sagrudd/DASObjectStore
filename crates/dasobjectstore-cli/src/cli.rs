use clap::{Parser, Subcommand};

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
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum Command {
    /// Inspect candidate DAS disks and enclosures.
    Probe,
    /// Report pool, disk, and service health.
    Health,
    /// Manage portable storage pools.
    Pool,
    /// Manage DAS member disks.
    Disk,
    /// Manage object stores and policy.
    Store,
    /// Inspect SSD ingest and destage work.
    Ingest,
    /// Export Mnemosyne/Synoptikon integration metadata.
    Mnemosyne,
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn parses_without_subcommand() {
        let cli = Cli::try_parse_from(["dasobjectstore"]).expect("root command parses");

        assert_eq!(cli.command(), None);
    }

    #[test]
    fn parses_top_level_command_skeletons() {
        let cases = [
            ("probe", Command::Probe),
            ("health", Command::Health),
            ("pool", Command::Pool),
            ("disk", Command::Disk),
            ("store", Command::Store),
            ("ingest", Command::Ingest),
            ("mnemosyne", Command::Mnemosyne),
        ];

        for (name, expected) in cases {
            let cli =
                Cli::try_parse_from(["dasobjectstore", name]).expect("subcommand should parse");

            assert_eq!(cli.command(), Some(&expected));
        }
    }
}
