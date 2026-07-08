//! Core domain types for DASObjectStore.

pub mod config;
pub mod file_export;
pub mod health;
pub mod ids;
pub mod lifecycle;
pub mod object_type;
pub mod placement;
pub mod protection;
pub mod repair;
pub mod risk;
pub mod store;

pub use config::{
    DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_CONFIG_PATH,
    DEFAULT_STANDALONE_HTTPS_PORT,
};

/// Current core crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_package_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
