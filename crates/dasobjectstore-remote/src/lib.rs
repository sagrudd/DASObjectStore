//! Remote DASObjectStore upload client.

pub mod auth;
pub mod authenticate;
pub mod cli;
pub mod config;
pub mod easyconnect;
pub mod run;
pub mod s3;

/// Returns the remote client crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
