mod server_cli;
mod server_run;

use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = server_cli::ServerCli::parse();
    let mut stdout = std::io::stdout();

    match server_run::run(&cli, &mut stdout).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
