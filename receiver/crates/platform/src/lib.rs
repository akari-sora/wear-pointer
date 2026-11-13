//! Pointer injection behind one trait.
//!
//! Stage 1 uses [`HyprlandBackend`] (`zwlr_virtual_pointer_v1`). [`LogBackend`]
//! prints actions and is the Stage 0 path that does not move the cursor.
//! `uinput` is optional (`--features uinput`) and relative. macOS and Windows
//! compile only on those targets and were not built in the Linux checkout.

mod log_backend;

#[cfg(target_os = "linux")]
mod hyprland;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(feature = "uinput", target_os = "linux"))]
mod uinput_backend;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use hyprland::HyprlandBackend;
pub use log_backend::LogBackend;
#[cfg(target_os = "macos")]
pub use macos::MacosBackend;
#[cfg(all(feature = "uinput", target_os = "linux"))]
pub use uinput_backend::UinputBackend;
#[cfg(target_os = "windows")]
pub use windows::WindowsBackend;

use anyhow::Result;
use wear_pointer_core::{Button, Screen};

pub trait PointerBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()>;
    fn button(&mut self, button: Button, down: bool) -> Result<()>;
    fn scroll(&mut self, delta_px: i32) -> Result<()>;
    fn screen(&self) -> Screen;
}
