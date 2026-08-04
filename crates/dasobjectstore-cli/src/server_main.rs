mod server_cli;
mod server_run;
mod tls_provider;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = tls_provider::install() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not create DASObjectStore server runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = server_cli::ServerCli::parse();
    let mut stdout = std::io::stdout();

    server_run::run(&cli, &mut stdout).await.map_err(Into::into)
}
