mod server_cli;
mod server_run;

use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        eprintln!("unable to install the DASObjectStore TLS crypto provider");
        return ExitCode::FAILURE;
    }
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
