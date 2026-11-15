//! Per-OS log path, file launcher, and "Start at Login" management.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Default log file used when running as a GUI Menu Bar / System Tray app.
pub fn default_log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let dir = PathBuf::from(home).join("Library").join("Logs");
            let _ = fs::create_dir_all(&dir);
            return dir.join("WearPointer.log");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            let dir = PathBuf::from(base).join("WearPointer");
            let _ = fs::create_dir_all(&dir);
            return dir.join("wear-pointer.log");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))
            });
        if let Some(base) = base {
            let dir = base.join("wear-pointer");
            let _ = fs::create_dir_all(&dir);
            return dir.join("wear-pointer.log");
        }
    }
    std::env::temp_dir().join("wear-pointer.log")
}

/// Opens a file or URL using the OS desktop shell without blocking.
pub fn open_in_desktop(target: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(target).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer").arg(target).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(target).spawn();
    }
}

/// Opens macOS Privacy & Security -> Accessibility directly.
#[cfg(target_os = "macos")]
pub fn open_macos_accessibility_settings() {
    open_in_desktop(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    );
}

pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_plist_path().map(|p| p.is_file()).unwrap_or(false)
    }
    #[cfg(target_os = "linux")]
    {
        linux_autostart_path().map(|p| p.is_file()).unwrap_or(false)
            || linux_systemd_service_enabled()
    }
    #[cfg(target_os = "windows")]
    {
        windows_autostart_enabled()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

pub fn set_autostart_enabled(enable: bool) -> Result<()> {
    let exe = std::env::current_exe().context("resolve current executable path")?;
    #[cfg(target_os = "macos")]
    {
        set_macos_autostart(enable, &exe)
    }
    #[cfg(target_os = "linux")]
    {
        set_linux_autostart(enable, &exe)
    }
    #[cfg(target_os = "windows")]
    {
        set_windows_autostart(enable, &exe)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (enable, exe);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library")
            .join("LaunchAgents")
            .join("dev.wearpointer.receiver.plist")
    })
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn render_macos_launch_agent_plist(exe: &Path) -> String {
    let exe_str = exe.to_string_lossy();
    let args_xml = if let Some((prefix, _)) = exe_str.split_once(".app/Contents/MacOS/") {
        let app_bundle = xml_escape(&format!("{prefix}.app"));
        format!(
            "        <string>/usr/bin/open</string>\n        <string>-g</string>\n        <string>{app_bundle}</string>"
        )
    } else {
        let escaped = xml_escape(&exe_str);
        format!("        <string>{escaped}</string>\n        <string>--tray</string>")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.wearpointer.receiver</string>
    <key>ProgramArguments</key>
    <array>
{args_xml}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#
    )
}

#[cfg(target_os = "macos")]
fn set_macos_autostart(enable: bool, exe: &Path) -> Result<()> {
    let plist = macos_plist_path().context("HOME is not set")?;
    if enable {
        if let Some(parent) = plist.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&plist, render_macos_launch_agent_plist(exe))?;
    } else if plist.exists() {
        fs::remove_file(&plist)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_autostart_path() -> Option<PathBuf> {
    let cfg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(cfg.join("autostart").join("wear-pointer.desktop"))
}

#[cfg(target_os = "linux")]
fn linux_systemd_service_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "--quiet", "wear-pointer.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn set_linux_autostart(enable: bool, exe: &Path) -> Result<()> {
    // If the user installed the systemd user unit, toggle it directly as well.
    let unit_exists = std::env::var_os("HOME")
        .map(|h| {
            PathBuf::from(h)
                .join(".config/systemd/user/wear-pointer.service")
                .is_file()
        })
        .unwrap_or(false);
    if unit_exists {
        let _ = Command::new("systemctl")
            .args([
                "--user",
                if enable { "enable" } else { "disable" },
                "wear-pointer.service",
            ])
            .status();
        return Ok(());
    }
    let path = linux_autostart_path().context("HOME is not set")?;
    if enable {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=Wear Pointer\nComment=Bluetooth LE spatial pointer receiver\nExec=\"{}\" --tray\nIcon=wear-pointer\nTerminal=false\nCategories=Utility;\n",
            exe.to_string_lossy()
        );
        fs::write(&path, body)?;
    } else if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_autostart_enabled() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }
        let ok = RegQueryValueExW(hkey, w!("WearPointer"), None, None, None, None).is_ok();
        let _ = RegCloseKey(hkey);
        ok
    }
}

#[cfg(target_os = "windows")]
fn set_windows_autostart(enable: bool, exe: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
        .ok()
        .context("open HKCU Run registry key")?;
        if enable {
            let cmd = format!("\"{}\" --tray", exe.to_string_lossy());
            let wide: Vec<u16> = std::ffi::OsStr::new(&cmd)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            let res = RegSetValueExW(hkey, w!("WearPointer"), 0, REG_SZ, Some(bytes));
            let _ = RegCloseKey(hkey);
            res.ok().context("write HKCU Run WearPointer")?;
        } else {
            let _ = RegDeleteValueW(hkey, w!("WearPointer"));
            let _ = RegCloseKey(hkey);
        }
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_valid_macos_launch_agent_plist() {
        let app_plist = render_macos_launch_agent_plist(Path::new(
            "/Applications/WearPointer.app/Contents/MacOS/wear-pointer",
        ));
        assert!(app_plist.contains("<string>dev.wearpointer.receiver</string>"));
        assert!(app_plist.contains("<string>/usr/bin/open</string>"));
        assert!(app_plist.contains("<string>/Applications/WearPointer.app</string>"));

        let bin_plist = render_macos_launch_agent_plist(Path::new("/usr/local/bin/wear-pointer"));
        assert!(bin_plist.contains("<string>--tray</string>"));
    }
}
