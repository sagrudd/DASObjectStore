//! Local authorization primitives for daemon-owned storage mutation.

mod actor;
#[cfg(target_os = "linux")]
mod linux;
mod policy;

pub use actor::DaemonLocalActor;
#[cfg(target_os = "linux")]
pub use linux::{read_linux_peer_credentials, LinuxPeerCredentials};
pub use policy::{authorize_store_write, DaemonAuthorizationError, DaemonStoreAccessPolicy};
