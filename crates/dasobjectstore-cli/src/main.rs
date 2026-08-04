mod cli;
mod run;
mod tls_provider;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = tls_provider::install() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    let cli = cli::Cli::parse();
    let mut stdout = std::io::stdout();

    match run::run(&cli, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
