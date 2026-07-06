mod server_cli;
mod server_run;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = server_cli::ServerCli::parse();
    let mut stdout = std::io::stdout();

    match server_run::run(&cli, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
