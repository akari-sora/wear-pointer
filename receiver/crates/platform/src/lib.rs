//! Pointer injection behind one trait.
//!
//! Stage 1 uses [`HyprlandBackend`] (`zwlr_virtual_pointer_v1`). [`LogBackend`]
//! prints actions and is the Stage 0 path that does not move the cursor.
//! `uinput` is optional (`--features uinput`) and relative. macOS and Windows
//! compile only on those targets and were not built in the Linux checkout.

mod log_backend;
#[cfg(any(test, feature = "uinput"))]
mod residual;

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
use wear_pointer_core::{Button, DesktopLayout, Screen};

pub trait PointerBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()>;
    fn button(&mut self, button: Button, down: bool) -> Result<()>;
    fn scroll(&mut self, delta_px: i32) -> Result<()>;
    fn screen(&self) -> Screen;
    /// Full multi-monitor desktop layout in global coordinates. Backends that
    /// support display hotplug refresh this lazily when polled.
    fn layout(&mut self) -> DesktopLayout {
        DesktopLayout::single(self.screen())
    }
    /// Where the OS cursor is now, in the same global coordinates as
    /// `motion_absolute`, if the platform can say.
    fn cursor_position(&self) -> Option<(f64, f64)> {
        None
    }
}

/// Read-only probe of the host OS's active multi-monitor layout without
/// opening cursor injection handles or triggering permission prompts.
pub fn detect_host_layout() -> DesktopLayout {
    #[cfg(target_os = "macos")]
    {
        MacosBackend::detect_layout()
    }
    #[cfg(target_os = "windows")]
    {
        WindowsBackend::detect_layout()
    }
    #[cfg(target_os = "linux")]
    {
        hyprland::hypr_monitors_layout().unwrap_or_default()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        DesktopLayout::default()
    }
}
