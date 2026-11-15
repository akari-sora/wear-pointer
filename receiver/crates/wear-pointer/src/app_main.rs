//! Consumer GUI entry point (`wear-pointer-app` / `WearPointer.exe`).
//!
//! Runs with no console window on Windows and starts the System Tray / Menu Bar
//! icon plus user-folder logging by default.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    if let Err(err) = wear_pointer::run_app_entry() {
        let msg = format!("Wear Pointer could not start:\n\n{err:#}");
        eprintln!("{msg}");
        #[cfg(target_os = "windows")]
        show_windows_error(&msg);
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
fn show_windows_error(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let wide: Vec<u16> = OsStr::new(message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(wide.as_ptr()),
            w!("Wear Pointer"),
            MB_OK | MB_ICONERROR,
        );
    }
}
