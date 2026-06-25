//! Yew frontend scaffold for Monas and Synoptikon delivery surfaces.

pub mod mount;

#[cfg(target_arch = "wasm32")]
pub mod app;

#[cfg(target_arch = "wasm32")]
pub use app::App;
pub use mount::{FrontendHost, FrontendMount};

/// Returns the GUI web crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), "0.0.0");
    }
}
