//! Yew frontend scaffold for Monas and Synoptikon delivery surfaces.

pub mod disks;
pub mod mount;
pub mod overview;

#[cfg(target_arch = "wasm32")]
pub mod app;

#[cfg(target_arch = "wasm32")]
pub use app::App;
pub use disks::{disks_workspace_api_path, DISKS_WORKSPACE_ROUTE};
pub use mount::{FrontendHost, FrontendMount};
pub use overview::{overview_workspace_api_path, OVERVIEW_WORKSPACE_ROUTE};

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
