use clap::Parser;
use dasobjectstore_remote::cli::RemoteCli;
use dasobjectstore_remote::run::run;
use std::io;

fn main() {
    let cli = RemoteCli::parse();
    let mut stdout = io::stdout();
    if let Err(error) = run(&cli, &mut stdout) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
