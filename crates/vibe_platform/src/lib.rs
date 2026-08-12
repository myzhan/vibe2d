mod common;
pub use common::{PlatformCallbacks, PlatformConfig};

/// gilrs-backed gamepad polling, shared by the desktop and web backends.
#[cfg(feature = "gamepad")]
mod gamepad;

#[cfg(not(target_arch = "wasm32"))]
mod desktop;
#[cfg(not(target_arch = "wasm32"))]
pub use desktop::{read_file, run_desktop};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{fetch_file, run_web};
