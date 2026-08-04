//! Executable-owned Rustls provider installation.
//!
//! This module is compiled into DASObjectStore CLI binaries only.  Libraries
//! remain provider-neutral so an embedding executable (notably Monas) keeps
//! ownership of its process-wide provider.

pub fn install() -> Result<(), &'static str> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Err("DASObjectStore TLS crypto provider is already installed");
    }
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| "failed to install the DASObjectStore AWS-LC TLS crypto provider")
}
