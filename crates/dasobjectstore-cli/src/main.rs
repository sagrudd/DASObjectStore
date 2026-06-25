use clap::Parser;

/// Portable mixed-disk DAS object store.
#[derive(Debug, Parser)]
#[command(name = "dasobjectstore", version = dasobjectstore_core::VERSION)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
