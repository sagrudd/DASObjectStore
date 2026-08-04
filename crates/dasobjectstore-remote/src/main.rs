use clap::Parser;
use dasobjectstore_remote::cli::RemoteCli;
use dasobjectstore_remote::run::run;
use std::io;

fn main() {
    if let Err(error) = install_tls_crypto_provider() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    let cli = RemoteCli::parse();
    let mut stdout = io::stdout();
    if let Err(error) = run(&cli, &mut stdout) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn install_tls_crypto_provider() -> Result<(), &'static str> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Err("DASObjectStore TLS crypto provider is already installed");
    }
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| "failed to install the DASObjectStore AWS-LC TLS crypto provider")
}
