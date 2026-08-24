#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod stub;

#[cfg(target_os = "macos")]
pub use macos::{PermissionsReport, PlatformCapture};
#[cfg(not(target_os = "macos"))]
pub use stub::{PermissionsReport, PlatformCapture};
