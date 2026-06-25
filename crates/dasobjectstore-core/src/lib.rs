//! Core domain types for DASObjectStore.

pub mod ids;
pub mod lifecycle;

/// Current core crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_package_version() {
        assert_eq!(VERSION, "0.0.0");
    }
}
