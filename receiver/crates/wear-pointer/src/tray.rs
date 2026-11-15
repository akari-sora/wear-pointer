//! Native OS Menu Bar / System Tray integration with zero GTK dependency.
//!
//! - macOS: AppKit `NSStatusBar` + `NSMenu` via `objc2`
//! - Windows: Win32 `Shell_NotifyIconW` + `CreatePopupMenu` via `windows`
//! - Linux: D-Bus `org.kde.StatusNotifierItem` + `com.canonical.dbusmenu` via `dbus`
//!   (gracefully degrades to background headless operation when no tray watcher exists)

use crate::autostart;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use wear_pointer_core::{ControlMode, DEFAULT_DPI};

pub const DPI_PRESETS: [u16; 6] = [800, 1200, 1600, 2000, 2400, 3200];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    SetPaused(bool),
    SetModeOverride(Option<ControlMode>),
    SetDpiOverride(Option<u16>),
    Recenter,
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraySnapshot {
    pub connected: bool,
    pub device_label: Option<String>,
    pub battery_pct: Option<u8>,
    pub charging: bool,
    pub paused: bool,
    pub effective_mode: ControlMode,
    pub mode_override: Option<ControlMode>,
    pub effective_dpi: u16,
    pub dpi_override: Option<u16>,
    pub rx_hz: f64,
    pub accessibility_ok: bool,
}

impl Default for TraySnapshot {
    fn default() -> Self {
        Self {
            connected: false,
            device_label: None,
            battery_pct: None,
            charging: false,
            paused: false,
            effective_mode: ControlMode::Absolute,
            mode_override: None,
            effective_dpi: DEFAULT_DPI,
            dpi_override: None,
            rx_hz: 0.0,
            accessibility_ok: true,
        }
    }
}

impl TraySnapshot {
    pub fn mode_title(mode: ControlMode) -> &'static str {
        match mode {
            ControlMode::Absolute => "Laser (Absolute)",
            ControlMode::Air => "Air Mouse (Gyro)",
            ControlMode::Touch => "Touchpad",
        }
    }

    pub fn status_line(&self) -> String {
        if self.connected {
            let name = self.device_label.as_deref().unwrap_or("Watch");
            let batt = match self.battery_pct {
                Some(pct) if self.charging => format!(" · {pct}% ⚡"),
                Some(pct) => format!(" · {pct}%"),
                None => String::new(),
            };
            let prefix = if self.paused { "Paused" } else { "Connected" };
            format!("{prefix}: {name}{batt}")
        } else {
            "Waiting for watch (Bluetooth LE)…".to_string()
        }
    }

    pub fn detail_line(&self) -> String {
        let mode = Self::mode_title(self.effective_mode);
        let source = if self.mode_override.is_some() || self.dpi_override.is_some() {
            " (Desktop override)"
        } else {
            ""
        };
        format!("{mode} · {} DPI{source}", self.effective_dpi)
    }

    pub fn tooltip_text(&self) -> String {
        if self.connected {
            let name = self.device_label.as_deref().unwrap_or("Watch");
            let state = if self.paused { "Paused" } else { "Active" };
            format!(
                "Wear Pointer — {state} ({name}) · {} {} DPI",
                self.effective_mode.label(),
                self.effective_dpi
            )
        } else {
            "Wear Pointer — Waiting for watch (BLE)".to_string()
        }
    }
}

pub struct TrayBridge {
    snapshot: Mutex<TraySnapshot>,
    cmd_tx: mpsc::Sender<TrayCommand>,
    stop: Arc<AtomicBool>,
}

impl TrayBridge {
    pub fn new(
        initial: TraySnapshot,
        stop: Arc<AtomicBool>,
    ) -> (Arc<Self>, mpsc::Receiver<TrayCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let bridge = Arc::new(Self {
            snapshot: Mutex::new(initial),
            cmd_tx,
            stop,
        });
        (bridge, cmd_rx)
    }

    pub fn update(&self, snap: TraySnapshot) {
        if let Ok(mut guard) = self.snapshot.lock() {
            *guard = snap;
        }
    }

    pub fn snapshot(&self) -> TraySnapshot {
        self.snapshot.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn send(&self, cmd: TrayCommand) {
        if matches!(cmd, TrayCommand::Quit) {
            self.stop.store(true, Ordering::Release);
        }
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

// Shared action IDs across macOS, Windows, and Linux menus.
const ACT_TOGGLE_PAUSE: i32 = 100;
const ACT_RECENTER: i32 = 101;
const ACT_OPEN_AX: i32 = 102;
const ACT_TOGGLE_AUTOSTART: i32 = 103;
const ACT_OPEN_LOG: i32 = 104;
const ACT_QUIT: i32 = 105;

const ACT_MODE_WATCH: i32 = 200;
const ACT_MODE_ABS: i32 = 201;
const ACT_MODE_AIR: i32 = 202;
const ACT_MODE_TOUCH: i32 = 203;

const ACT_DPI_WATCH: i32 = 300;
const ACT_DPI_BASE: i32 = 10_000;

fn handle_menu_action(action_id: i32, bridge: &TrayBridge) {
    match action_id {
        ACT_TOGGLE_PAUSE => {
            let currently_paused = bridge.snapshot().paused;
            bridge.send(TrayCommand::SetPaused(!currently_paused));
        }
        ACT_RECENTER => {
            bridge.send(TrayCommand::Recenter);
        }
        ACT_OPEN_AX => {
            #[cfg(target_os = "macos")]
            autostart::open_macos_accessibility_settings();
        }
        ACT_TOGGLE_AUTOSTART => {
            let next = !autostart::is_autostart_enabled();
            if let Err(err) = autostart::set_autostart_enabled(next) {
                eprintln!("wear-pointer: failed to update login item: {err:#}");
            }
        }
        ACT_OPEN_LOG => {
            let log = autostart::default_log_path();
            if !log.exists() {
                let _ = std::fs::write(&log, "");
            }
            autostart::open_in_desktop(&log.to_string_lossy());
        }
        ACT_QUIT => {
            bridge.send(TrayCommand::Quit);
        }
        ACT_MODE_WATCH => bridge.send(TrayCommand::SetModeOverride(None)),
        ACT_MODE_ABS => bridge.send(TrayCommand::SetModeOverride(Some(ControlMode::Absolute))),
        ACT_MODE_AIR => bridge.send(TrayCommand::SetModeOverride(Some(ControlMode::Air))),
        ACT_MODE_TOUCH => bridge.send(TrayCommand::SetModeOverride(Some(ControlMode::Touch))),
        ACT_DPI_WATCH => bridge.send(TrayCommand::SetDpiOverride(None)),
        id if id > ACT_DPI_BASE && id <= ACT_DPI_BASE + 6400 => {
            let dpi = (id - ACT_DPI_BASE) as u16;
            bridge.send(TrayCommand::SetDpiOverride(Some(dpi)));
        }
        _ => {}
    }
}

pub fn run_tray_loop(bridge: Arc<TrayBridge>) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos_imp::run(bridge)
    }
    #[cfg(target_os = "windows")]
    {
        windows_imp::run(bridge)
    }
    #[cfg(target_os = "linux")]
    {
        linux_imp::run(bridge)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        while !bridge.should_stop() {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// macOS Menu Bar (AppKit NSStatusBar + NSMenu)
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod macos_imp {
    use super::*;
    use objc2::encode::{Encode, Encoding};
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObject, Sel};
    use objc2::{msg_send, sel, ClassType};
    use std::ffi::{c_void, CString};
    use std::ptr;
    use std::sync::OnceLock;
    use wear_pointer_platform::MacosBackend;

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    #[link(name = "Foundation", kind = "framework")]
    extern "C" {}

    const TRAY_TEMPLATE_PNG: &[u8] = include_bytes!("../assets/tray-template-44.png");

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSSize {
        width: f64,
        height: f64,
    }

    unsafe impl Encode for NSSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }

    unsafe impl Encode for NSPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }

    struct MacUiState {
        bridge: Arc<TrayBridge>,
        app: usize,
        status_bar: usize,
        status_item: usize,
        menu: usize,
        last_snap: Option<TraySnapshot>,
        last_autostart: bool,
    }

    static MAC_STATE: OnceLock<Mutex<MacUiState>> = OnceLock::new();

    unsafe fn nsstring(text: &str) -> *mut AnyObject {
        let clean = text.replace('\0', "");
        let cstr = CString::new(clean).unwrap_or_default();
        let cls = AnyClass::get("NSString").expect("NSString");
        msg_send![cls, stringWithUTF8String: cstr.as_ptr()]
    }

    unsafe fn make_item(
        title: &str,
        action: Option<Sel>,
        key_eq: &str,
        target: *const AnyObject,
        tag: i32,
        enabled: bool,
        checked: bool,
    ) -> *mut AnyObject {
        let cls = AnyClass::get("NSMenuItem").expect("NSMenuItem");
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let title_ns = nsstring(title);
        let key_ns = nsstring(key_eq);
        let item: *mut AnyObject =
            msg_send![alloc, initWithTitle: title_ns, action: action, keyEquivalent: key_ns];
        if action.is_some() {
            let _: () = msg_send![item, setTarget: target];
        }
        let _: () = msg_send![item, setTag: tag as isize];
        let _: () = msg_send![item, setEnabled: if enabled { Bool::YES } else { Bool::NO }];
        let _: () = msg_send![item, setState: if checked { 1isize } else { 0isize }];
        let _: *mut AnyObject = msg_send![item, autorelease];
        item
    }

    unsafe fn separator_item() -> *mut AnyObject {
        let cls = AnyClass::get("NSMenuItem").expect("NSMenuItem");
        msg_send![cls, separatorItem]
    }

    unsafe fn rebuild_menu(
        menu: *mut AnyObject,
        status_item: *mut AnyObject,
        handler: *const AnyObject,
        snap: &TraySnapshot,
        autostart_on: bool,
    ) {
        let button: *mut AnyObject = msg_send![status_item, button];
        if !button.is_null() {
            let badge = if !snap.connected {
                ""
            } else if snap.paused {
                " ⏸"
            } else {
                " ●"
            };
            let badge_ns = nsstring(badge);
            let tip_ns = nsstring(&snap.tooltip_text());
            let _: () = msg_send![button, setTitle: badge_ns];
            let _: () = msg_send![button, setToolTip: tip_ns];
        }

        let _: () = msg_send![menu, removeAllItems];
        let act = Some(sel!(onMenuItem:));

        let header = make_item(&snap.status_line(), None, "", handler, 0, false, false);
        let detail = make_item(&snap.detail_line(), None, "", handler, 0, false, false);
        let _: () = msg_send![menu, addItem: header];
        let _: () = msg_send![menu, addItem: detail];

        if snap.accessibility_ok {
            let ax = make_item("Accessibility: Granted", None, "", handler, 0, false, false);
            let _: () = msg_send![menu, addItem: ax];
        } else {
            let ax = make_item(
                "Grant Accessibility Permission…",
                act,
                "",
                handler,
                ACT_OPEN_AX,
                true,
                false,
            );
            let _: () = msg_send![menu, addItem: ax];
        }

        let _: () = msg_send![menu, addItem: separator_item()];

        let pause_item = make_item(
            "Pause Pointer",
            act,
            "p",
            handler,
            ACT_TOGGLE_PAUSE,
            true,
            snap.paused,
        );
        let center_item = make_item(
            "Re-center Laser",
            act,
            "c",
            handler,
            ACT_RECENTER,
            true,
            false,
        );
        let _: () = msg_send![menu, addItem: pause_item];
        let _: () = msg_send![menu, addItem: center_item];

        let _: () = msg_send![menu, addItem: separator_item()];

        // Pointing Mode submenu
        let menu_cls = AnyClass::get("NSMenu").expect("NSMenu");
        let mode_menu_alloc: *mut AnyObject = msg_send![menu_cls, alloc];
        let mode_menu: *mut AnyObject =
            msg_send![mode_menu_alloc, initWithTitle: nsstring("Pointing Mode")];
        let _: () = msg_send![mode_menu, setAutoenablesItems: Bool::NO];
        let _: *mut AnyObject = msg_send![mode_menu, autorelease];

        let _: () = msg_send![
            mode_menu,
            addItem: make_item(
                "Follow Watch",
                act,
                "",
                handler,
                ACT_MODE_WATCH,
                true,
                snap.mode_override.is_none(),
            )
        ];
        let _: () = msg_send![mode_menu, addItem: separator_item()];
        for (mode, tag) in [
            (ControlMode::Absolute, ACT_MODE_ABS),
            (ControlMode::Air, ACT_MODE_AIR),
            (ControlMode::Touch, ACT_MODE_TOUCH),
        ] {
            let _: () = msg_send![
                mode_menu,
                addItem: make_item(
                    TraySnapshot::mode_title(mode),
                    act,
                    "",
                    handler,
                    tag,
                    true,
                    snap.mode_override == Some(mode),
                )
            ];
        }
        let mode_parent = make_item("Pointing Mode", None, "", handler, 0, true, false);
        let _: () = msg_send![mode_parent, setSubmenu: mode_menu];
        let _: () = msg_send![menu, addItem: mode_parent];

        // Sensitivity (DPI) submenu
        let dpi_menu_alloc: *mut AnyObject = msg_send![menu_cls, alloc];
        let dpi_menu: *mut AnyObject =
            msg_send![dpi_menu_alloc, initWithTitle: nsstring("Sensitivity (DPI)")];
        let _: () = msg_send![dpi_menu, setAutoenablesItems: Bool::NO];
        let _: *mut AnyObject = msg_send![dpi_menu, autorelease];

        let _: () = msg_send![
            dpi_menu,
            addItem: make_item(
                "Follow Watch",
                act,
                "",
                handler,
                ACT_DPI_WATCH,
                true,
                snap.dpi_override.is_none(),
            )
        ];
        let _: () = msg_send![dpi_menu, addItem: separator_item()];
        for preset in DPI_PRESETS {
            let label = format!("{preset} DPI");
            let _: () = msg_send![
                dpi_menu,
                addItem: make_item(
                    &label,
                    act,
                    "",
                    handler,
                    ACT_DPI_BASE + i32::from(preset),
                    true,
                    snap.dpi_override == Some(preset),
                )
            ];
        }
        let dpi_parent = make_item("Sensitivity (DPI)", None, "", handler, 0, true, false);
        let _: () = msg_send![dpi_parent, setSubmenu: dpi_menu];
        let _: () = msg_send![menu, addItem: dpi_parent];

        let _: () = msg_send![menu, addItem: separator_item()];

        let login_item = make_item(
            "Start at Login",
            act,
            "",
            handler,
            ACT_TOGGLE_AUTOSTART,
            true,
            autostart_on,
        );
        let log_item = make_item("Open Log File", act, "", handler, ACT_OPEN_LOG, true, false);
        let _: () = msg_send![menu, addItem: login_item];
        let _: () = msg_send![menu, addItem: log_item];

        let _: () = msg_send![menu, addItem: separator_item()];

        let quit_item = make_item(
            "Quit Wear Pointer",
            act,
            "q",
            handler,
            ACT_QUIT,
            true,
            false,
        );
        let _: () = msg_send![menu, addItem: quit_item];
    }

    extern "C" fn on_menu_item(_this: &AnyObject, _cmd: Sel, sender: *mut AnyObject) {
        autoreleasepool(|_| unsafe {
            if sender.is_null() {
                return;
            }
            let tag: isize = msg_send![sender, tag];
            let Some(lock) = MAC_STATE.get() else {
                return;
            };
            let bridge = match lock.lock() {
                Ok(guard) => Arc::clone(&guard.bridge),
                Err(_) => return,
            };
            handle_menu_action(tag as i32, &bridge);
            refresh_if_needed(_this as *const AnyObject);
        });
    }

    extern "C" fn on_tick(this: &AnyObject, _cmd: Sel, timer: *mut AnyObject) {
        autoreleasepool(|_| unsafe {
            let Some(lock) = MAC_STATE.get() else {
                return;
            };
            let should_stop = lock.lock().map(|g| g.bridge.should_stop()).unwrap_or(true);
            if should_stop {
                if !timer.is_null() {
                    let _: () = msg_send![timer, invalidate];
                }
                if let Ok(mut guard) = lock.lock() {
                    if guard.status_item != 0 && guard.status_bar != 0 {
                        let bar = guard.status_bar as *mut AnyObject;
                        let item = guard.status_item as *mut AnyObject;
                        let _: () = msg_send![bar, removeStatusItem: item];
                        guard.status_item = 0;
                    }
                    let app = guard.app as *mut AnyObject;
                    if !app.is_null() {
                        let nil_obj: *const AnyObject = ptr::null();
                        let _: () = msg_send![app, stop: nil_obj];
                        if let Some(ev_cls) = AnyClass::get("NSEvent") {
                            let dummy: *mut AnyObject = msg_send![
                                ev_cls,
                                otherEventWithType: 15usize,
                                location: NSPoint { x: 0.0, y: 0.0 },
                                modifierFlags: 0usize,
                                timestamp: 0.0f64,
                                windowNumber: 0isize,
                                context: nil_obj,
                                subtype: 0i16,
                                data1: 0isize,
                                data2: 0isize
                            ];
                            if !dummy.is_null() {
                                let _: () = msg_send![app, postEvent: dummy, atStart: Bool::YES];
                            }
                        }
                    }
                }
                return;
            }
            refresh_if_needed(this as *const AnyObject);
        });
    }

    unsafe fn refresh_if_needed(handler: *const AnyObject) {
        let Some(lock) = MAC_STATE.get() else {
            return;
        };
        let Ok(mut guard) = lock.lock() else {
            return;
        };
        if guard.status_item == 0 || guard.menu == 0 {
            return;
        }
        let mut snap = guard.bridge.snapshot();
        snap.accessibility_ok = MacosBackend::check_accessibility(false);
        let autostart_on = autostart::is_autostart_enabled();
        if guard.last_snap.as_ref() != Some(&snap) || guard.last_autostart != autostart_on {
            rebuild_menu(
                guard.menu as *mut AnyObject,
                guard.status_item as *mut AnyObject,
                handler,
                &snap,
                autostart_on,
            );
            guard.last_snap = Some(snap);
            guard.last_autostart = autostart_on;
        }
    }

    fn handler_class() -> &'static AnyClass {
        static CLS: OnceLock<&'static AnyClass> = OnceLock::new();
        CLS.get_or_init(|| {
            let mut builder = ClassBuilder::new("WearPointerMenuTarget", NSObject::class())
                .expect("register WearPointerMenuTarget");
            unsafe {
                builder.add_method(sel!(onMenuItem:), on_menu_item as extern "C" fn(_, _, _));
                builder.add_method(sel!(onTick:), on_tick as extern "C" fn(_, _, _));
            }
            builder.register()
        })
    }

    pub fn run(bridge: Arc<TrayBridge>) -> Result<()> {
        autoreleasepool(|_| unsafe {
            let app_cls = AnyClass::get("NSApplication").expect("NSApplication");
            let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
            // NSApplicationActivationPolicyAccessory = 1 (no Dock icon, no focus theft)
            let _: Bool = msg_send![app, setActivationPolicy: 1isize];

            let bar_cls = AnyClass::get("NSStatusBar").expect("NSStatusBar");
            let status_bar: *mut AnyObject = msg_send![bar_cls, systemStatusBar];
            // NSVariableStatusItemLength = -1.0
            let status_item: *mut AnyObject = msg_send![status_bar, statusItemWithLength: -1.0f64];
            let _: *mut AnyObject = msg_send![status_item, retain];

            // Configure crisp @2x monochrome template icon (22x22 pt)
            let data_cls = AnyClass::get("NSData").expect("NSData");
            let img_data: *mut AnyObject = msg_send![
                data_cls,
                dataWithBytes: TRAY_TEMPLATE_PNG.as_ptr().cast::<c_void>(),
                length: TRAY_TEMPLATE_PNG.len()
            ];
            let img_cls = AnyClass::get("NSImage").expect("NSImage");
            let img_alloc: *mut AnyObject = msg_send![img_cls, alloc];
            let img: *mut AnyObject = msg_send![img_alloc, initWithData: img_data];
            if !img.is_null() {
                let _: () = msg_send![img, setSize: NSSize { width: 22.0, height: 22.0 }];
                let _: () = msg_send![img, setTemplate: Bool::YES];
                let button: *mut AnyObject = msg_send![status_item, button];
                if !button.is_null() {
                    let _: () = msg_send![button, setImage: img];
                }
            }

            let menu_cls = AnyClass::get("NSMenu").expect("NSMenu");
            let menu_alloc: *mut AnyObject = msg_send![menu_cls, alloc];
            let menu: *mut AnyObject =
                msg_send![menu_alloc, initWithTitle: nsstring("Wear Pointer")];
            let _: () = msg_send![menu, setAutoenablesItems: Bool::NO];
            let _: () = msg_send![status_item, setMenu: menu];

            let target_cls = handler_class();
            let target_alloc: *mut AnyObject = msg_send![target_cls, alloc];
            let handler: *mut AnyObject = msg_send![target_alloc, init];

            let mut initial_snap = bridge.snapshot();
            initial_snap.accessibility_ok = MacosBackend::check_accessibility(false);
            let initial_autostart = autostart::is_autostart_enabled();
            rebuild_menu(menu, status_item, handler, &initial_snap, initial_autostart);

            let _ = MAC_STATE.set(Mutex::new(MacUiState {
                bridge: Arc::clone(&bridge),
                app: app as usize,
                status_bar: status_bar as usize,
                status_item: status_item as usize,
                menu: menu as usize,
                last_snap: Some(initial_snap),
                last_autostart: initial_autostart,
            }));

            let timer_cls = AnyClass::get("NSTimer").expect("NSTimer");
            let nil_obj: *const AnyObject = ptr::null();
            let _timer: *mut AnyObject = msg_send![
                timer_cls,
                scheduledTimerWithTimeInterval: 0.5f64,
                target: handler,
                selector: sel!(onTick:),
                userInfo: nil_obj,
                repeats: Bool::YES
            ];

            let _: () = msg_send![app, run];
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// Windows System Tray (Shell_NotifyIconW + Win32 Popup Menu)
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
mod windows_imp {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostMessageW, PostQuitMessage,
        RegisterClassW, SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, HMENU,
        IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
        MSG, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP,
        WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };

    const WM_TRAYICON: u32 = WM_APP + 1;
    const TRAY_UID: u32 = 1;
    const TIMER_ID: usize = 1;

    struct WinTrayState {
        bridge: Arc<TrayBridge>,
        nid: NOTIFYICONDATAW,
        last_tip: String,
    }

    thread_local! {
        static WIN_STATE: RefCell<Option<WinTrayState>> = const { RefCell::new(None) };
    }

    fn wide_null(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn write_tip(dst: &mut [u16; 128], text: &str) {
        dst.fill(0);
        let encoded: Vec<u16> = OsStr::new(text).encode_wide().collect();
        let len = encoded.len().min(dst.len() - 1);
        dst[..len].copy_from_slice(&encoded[..len]);
    }

    unsafe fn append_item(menu: HMENU, id: usize, label: &str, enabled: bool, checked: bool) {
        let mut flags = MF_STRING;
        if !enabled {
            flags |= MF_GRAYED;
        }
        flags |= if checked { MF_CHECKED } else { MF_UNCHECKED };
        let w = wide_null(label);
        let _ = AppendMenuW(menu, flags, id, PCWSTR(w.as_ptr()));
    }

    unsafe fn show_context_menu(hwnd: HWND, bridge: &TrayBridge) {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let snap = bridge.snapshot();
        let autostart_on = autostart::is_autostart_enabled();

        append_item(menu, 0, &snap.status_line(), false, false);
        append_item(menu, 0, &snap.detail_line(), false, false);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        append_item(
            menu,
            ACT_TOGGLE_PAUSE as usize,
            "Pause Pointer",
            true,
            snap.paused,
        );
        append_item(menu, ACT_RECENTER as usize, "Re-center Laser", true, false);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        if let Ok(mode_menu) = CreatePopupMenu() {
            append_item(
                mode_menu,
                ACT_MODE_WATCH as usize,
                "Follow Watch",
                true,
                snap.mode_override.is_none(),
            );
            let _ = AppendMenuW(mode_menu, MF_SEPARATOR, 0, PCWSTR::null());
            for (mode, id) in [
                (ControlMode::Absolute, ACT_MODE_ABS),
                (ControlMode::Air, ACT_MODE_AIR),
                (ControlMode::Touch, ACT_MODE_TOUCH),
            ] {
                append_item(
                    mode_menu,
                    id as usize,
                    TraySnapshot::mode_title(mode),
                    true,
                    snap.mode_override == Some(mode),
                );
            }
            let title = wide_null("Pointing Mode");
            let _ = AppendMenuW(menu, MF_POPUP, mode_menu.0 as usize, PCWSTR(title.as_ptr()));
        }

        if let Ok(dpi_menu) = CreatePopupMenu() {
            append_item(
                dpi_menu,
                ACT_DPI_WATCH as usize,
                "Follow Watch",
                true,
                snap.dpi_override.is_none(),
            );
            let _ = AppendMenuW(dpi_menu, MF_SEPARATOR, 0, PCWSTR::null());
            for preset in DPI_PRESETS {
                let label = format!("{preset} DPI");
                append_item(
                    dpi_menu,
                    (ACT_DPI_BASE + i32::from(preset)) as usize,
                    &label,
                    true,
                    snap.dpi_override == Some(preset),
                );
            }
            let title = wide_null("Sensitivity (DPI)");
            let _ = AppendMenuW(menu, MF_POPUP, dpi_menu.0 as usize, PCWSTR(title.as_ptr()));
        }

        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_item(
            menu,
            ACT_TOGGLE_AUTOSTART as usize,
            "Start with Windows",
            true,
            autostart_on,
        );
        append_item(menu, ACT_OPEN_LOG as usize, "Open Log File", true, false);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_item(menu, ACT_QUIT as usize, "Quit Wear Pointer", true, false);

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        if cmd.0 > 0 {
            handle_menu_action(cmd.0, bridge);
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TRAYICON => {
                let mouse_msg = (lparam.0 & 0xFFFF) as u32;
                if matches!(mouse_msg, WM_RBUTTONUP | WM_LBUTTONUP | WM_CONTEXTMENU) {
                    let bridge = WIN_STATE
                        .with(|cell| cell.borrow().as_ref().map(|s| Arc::clone(&s.bridge)));
                    if let Some(bridge) = bridge {
                        show_context_menu(hwnd, &bridge);
                    }
                }
                LRESULT(0)
            }
            WM_TIMER => {
                let should_destroy = WIN_STATE.with(|cell| {
                    let mut borrow = cell.borrow_mut();
                    let Some(state) = borrow.as_mut() else {
                        return false;
                    };
                    if state.bridge.should_stop() {
                        return true;
                    }
                    let tip = state.bridge.snapshot().tooltip_text();
                    if tip != state.last_tip {
                        write_tip(&mut state.nid.szTip, &tip);
                        let _ = Shell_NotifyIconW(NIM_MODIFY, &state.nid);
                        state.last_tip = tip;
                    }
                    false
                });
                if should_destroy {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                WIN_STATE.with(|cell| {
                    if let Some(state) = cell.borrow().as_ref() {
                        let _ = Shell_NotifyIconW(NIM_DELETE, &state.nid);
                    }
                });
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn run(bridge: Arc<TrayBridge>) -> Result<()> {
        unsafe {
            let hinstance = GetModuleHandleW(None)?;
            let class_name = w!("WearPointerTrayWindow");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("Wear Pointer"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                None,
                None,
                hinstance,
                None,
            )?;

            // Resource ID 1 embedded by build.rs via assets/app.rc
            let hicon = LoadIconW(hinstance, PCWSTR(1usize as *const u16))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;

            let initial_tip = bridge.snapshot().tooltip_text();
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_UID,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: hicon,
                ..Default::default()
            };
            write_tip(&mut nid.szTip, &initial_tip);
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);

            WIN_STATE.with(|cell| {
                *cell.borrow_mut() = Some(WinTrayState {
                    bridge: Arc::clone(&bridge),
                    nid,
                    last_tip: initial_tip,
                });
            });

            let _ = SetTimer(hwnd, TIMER_ID, 500, None);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Linux StatusNotifierItem + DbusMenu (Waybar / KDE / GNOME AppIndicator)
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
mod linux_imp {
    use super::*;
    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::blocking::{BlockingSender, Connection};
    use dbus::channel::{MatchingReceiver, Sender};
    use dbus::message::{MatchRule, Message};
    use std::time::Duration;

    const SNI_PATH: &str = "/StatusNotifierItem";
    const MENU_PATH: &str = "/MenuBar";
    const SNI_IFACE: &str = "org.kde.StatusNotifierItem";
    const MENU_IFACE: &str = "com.canonical.dbusmenu";

    /// Generates a crisp 24x24 ARGB32 (network byte order) crosshair tray icon.
    fn render_argb32_icon() -> (i32, i32, Vec<u8>) {
        let size: i32 = 24;
        let mut buf = vec![0u8; (size * size * 4) as usize];
        let c = 11.5f32;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - c;
                let dy = y as f32 - c;
                let r = (dx * dx + dy * dy).sqrt();
                let idx = ((y * size + x) * 4) as usize;
                let on_ring = (r - 8.2).abs() <= 1.25;
                let on_dot = r <= 2.1;
                let on_cross = (dx.abs() <= 0.9 && (3.6..=10.6).contains(&dy.abs()))
                    || (dy.abs() <= 0.9 && (3.6..=10.6).contains(&dx.abs()));
                if on_ring || on_dot || on_cross {
                    // #36E2EC cyan
                    buf[idx] = 0xFF;
                    buf[idx + 1] = 0x36;
                    buf[idx + 2] = 0xE2;
                    buf[idx + 3] = 0xEC;
                } else if r <= 10.8 {
                    // Subtle dark backing disc so cyan pops on light bars too
                    buf[idx] = 0xD8;
                    buf[idx + 1] = 0x0B;
                    buf[idx + 2] = 0x11;
                    buf[idx + 3] = 0x18;
                }
            }
        }
        (size, size, buf)
    }

    fn prop_str(val: &str) -> Variant<Box<dyn RefArg>> {
        Variant(Box::new(val.to_string()))
    }

    fn prop_bool(val: bool) -> Variant<Box<dyn RefArg>> {
        Variant(Box::new(val))
    }

    fn prop_i32(val: i32) -> Variant<Box<dyn RefArg>> {
        Variant(Box::new(val))
    }

    fn menu_item_props(
        label: &str,
        enabled: bool,
        toggle_type: Option<&str>,
        checked: bool,
    ) -> PropMap {
        let mut m = PropMap::new();
        m.insert("label".into(), prop_str(label));
        m.insert("enabled".into(), prop_bool(enabled));
        m.insert("visible".into(), prop_bool(true));
        if let Some(tt) = toggle_type {
            m.insert("toggle-type".into(), prop_str(tt));
            m.insert("toggle-state".into(), prop_i32(if checked { 1 } else { 0 }));
        }
        m
    }

    fn separator_props() -> PropMap {
        let mut m = PropMap::new();
        m.insert("type".into(), prop_str("separator"));
        m.insert("visible".into(), prop_bool(true));
        m
    }

    fn build_dbusmenu_layout(snap: &TraySnapshot) -> (i32, PropMap, Vec<Variant<Box<dyn RefArg>>>) {
        let autostart_on = autostart::is_autostart_enabled();
        let mut children: Vec<Variant<Box<dyn RefArg>>> = Vec::new();
        let leaf = |id: i32, props: PropMap| -> Variant<Box<dyn RefArg>> {
            let empty: Vec<Variant<Box<dyn RefArg>>> = Vec::new();
            Variant(Box::new((id, props, empty)))
        };

        children.push(leaf(
            1,
            menu_item_props(&snap.status_line(), false, None, false),
        ));
        children.push(leaf(
            2,
            menu_item_props(&snap.detail_line(), false, None, false),
        ));
        children.push(leaf(3, separator_props()));
        children.push(leaf(
            ACT_TOGGLE_PAUSE,
            menu_item_props("Pause Pointer", true, Some("checkmark"), snap.paused),
        ));
        children.push(leaf(
            ACT_RECENTER,
            menu_item_props("Re-center Laser", true, None, false),
        ));
        children.push(leaf(4, separator_props()));

        // Mode items directly with radio indicator for maximum Waybar/libdbusmenu compatibility
        children.push(leaf(
            ACT_MODE_WATCH,
            menu_item_props(
                "Mode: Follow Watch",
                true,
                Some("radio"),
                snap.mode_override.is_none(),
            ),
        ));
        for (mode, id) in [
            (ControlMode::Absolute, ACT_MODE_ABS),
            (ControlMode::Air, ACT_MODE_AIR),
            (ControlMode::Touch, ACT_MODE_TOUCH),
        ] {
            let label = format!("Mode: {}", TraySnapshot::mode_title(mode));
            children.push(leaf(
                id,
                menu_item_props(
                    &label,
                    true,
                    Some("radio"),
                    snap.mode_override == Some(mode),
                ),
            ));
        }

        children.push(leaf(5, separator_props()));
        children.push(leaf(
            ACT_DPI_WATCH,
            menu_item_props(
                "DPI: Follow Watch",
                true,
                Some("radio"),
                snap.dpi_override.is_none(),
            ),
        ));
        for preset in [800u16, 1600, 2400, 3200] {
            let label = format!("DPI: {preset}");
            children.push(leaf(
                ACT_DPI_BASE + i32::from(preset),
                menu_item_props(
                    &label,
                    true,
                    Some("radio"),
                    snap.dpi_override == Some(preset),
                ),
            ));
        }

        children.push(leaf(6, separator_props()));
        children.push(leaf(
            ACT_TOGGLE_AUTOSTART,
            menu_item_props("Start at Login", true, Some("checkmark"), autostart_on),
        ));
        children.push(leaf(
            ACT_OPEN_LOG,
            menu_item_props("Open Log File", true, None, false),
        ));
        children.push(leaf(7, separator_props()));
        children.push(leaf(
            ACT_QUIT,
            menu_item_props("Quit Wear Pointer", true, None, false),
        ));

        let mut root_props = PropMap::new();
        root_props.insert("children-display".into(), prop_str("submenu"));
        (0, root_props, children)
    }

    fn sni_properties(snap: &TraySnapshot) -> PropMap {
        let mut map = PropMap::new();
        map.insert("Category".into(), prop_str("Hardware"));
        map.insert("Id".into(), prop_str("wear-pointer"));
        map.insert("Title".into(), prop_str("Wear Pointer"));
        map.insert(
            "Status".into(),
            prop_str(if snap.connected { "Active" } else { "Passive" }),
        );
        map.insert("IconName".into(), prop_str("input-mouse"));
        map.insert(
            "IconPixmap".into(),
            Variant(Box::new(vec![render_argb32_icon()])),
        );
        map.insert("ItemIsMenu".into(), prop_bool(true));
        map.insert(
            "Menu".into(),
            Variant(Box::new(dbus::Path::new(MENU_PATH).unwrap())),
        );
        let empty_pix: Vec<(i32, i32, Vec<u8>)> = Vec::new();
        map.insert(
            "ToolTip".into(),
            Variant(Box::new((
                "input-mouse".to_string(),
                empty_pix,
                "Wear Pointer".to_string(),
                snap.tooltip_text(),
            ))),
        );
        map
    }

    fn handle_dbus_call(msg: &Message, bridge: &TrayBridge) -> Option<Message> {
        let path = msg.path()?.into_cstring();
        let path_str = path.to_str().ok()?;
        let iface = msg.interface().map(|i| i.to_string()).unwrap_or_default();
        let member = msg.member()?.to_string();

        if iface == "org.freedesktop.DBus.Introspectable" && member == "Introspect" {
            let xml = if path_str == MENU_PATH {
                "<node><interface name=\"com.canonical.dbusmenu\"/></node>"
            } else {
                "<node><interface name=\"org.kde.StatusNotifierItem\"/></node>"
            };
            return Some(msg.method_return().append1(xml));
        }

        if path_str == SNI_PATH {
            if iface == "org.freedesktop.DBus.Properties" {
                let props = sni_properties(&bridge.snapshot());
                if member == "GetAll" {
                    return Some(msg.method_return().append1(props));
                } else if member == "Get" {
                    let (_req_iface, prop_name): (String, String) = msg.read2().ok()?;
                    if let Some(val) = props.get(&prop_name) {
                        return Some(msg.method_return().append1(val));
                    }
                }
            } else if iface == SNI_IFACE {
                match member.as_str() {
                    "Activate" | "SecondaryActivate" => {
                        handle_menu_action(ACT_TOGGLE_PAUSE, bridge);
                        return Some(msg.method_return());
                    }
                    "ContextMenu" | "Scroll" => {
                        return Some(msg.method_return());
                    }
                    _ => {}
                }
            }
        } else if path_str == MENU_PATH {
            if iface == "org.freedesktop.DBus.Properties" {
                let mut m = PropMap::new();
                m.insert("Version".into(), Variant(Box::new(3u32)));
                m.insert("Status".into(), prop_str("normal"));
                if member == "GetAll" {
                    return Some(msg.method_return().append1(m));
                } else if member == "Get" {
                    let (_req_iface, prop_name): (String, String) = msg.read2().ok()?;
                    if let Some(val) = m.get(&prop_name) {
                        return Some(msg.method_return().append1(val));
                    }
                }
            } else if iface == MENU_IFACE {
                match member.as_str() {
                    "GetLayout" => {
                        let layout = build_dbusmenu_layout(&bridge.snapshot());
                        return Some(msg.method_return().append2(1u32, layout));
                    }
                    "AboutToShow" => {
                        return Some(msg.method_return().append1(true));
                    }
                    "Event" => {
                        if let Ok((id, event_id)) = msg.read2::<i32, String>() {
                            if event_id == "clicked" {
                                handle_menu_action(id, bridge);
                            }
                        }
                        return Some(msg.method_return());
                    }
                    "GetGroupProperties" => {
                        let empty: Vec<(i32, PropMap)> = Vec::new();
                        return Some(msg.method_return().append1(empty));
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn register_with_watcher(conn: &Connection, bus_name: &str) {
        for watcher in [
            "org.kde.StatusNotifierWatcher",
            "org.freedesktop.StatusNotifierWatcher",
        ] {
            if let Ok(call) = Message::new_method_call(
                watcher,
                "/StatusNotifierWatcher",
                watcher,
                "RegisterStatusNotifierItem",
            ) {
                let call = call.append1(bus_name);
                let _ = conn.send_with_reply_and_block(call, Duration::from_millis(400));
            }
        }
    }

    pub fn run(bridge: Arc<TrayBridge>) -> Result<()> {
        let Ok(conn) = Connection::new_session() else {
            // Headless or no session bus: stay alive silently until stopped.
            while !bridge.should_stop() {
                std::thread::sleep(Duration::from_millis(250));
            }
            return Ok(());
        };

        let bus_name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let _ = conn.request_name(&bus_name, false, true, false);

        let cb_bridge = Arc::clone(&bridge);
        let _token = conn.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |msg, c| {
                if let Some(reply) = handle_dbus_call(&msg, &cb_bridge) {
                    let _ = c.send(reply);
                    return false;
                }
                true
            }),
        );

        register_with_watcher(&conn, &bus_name);

        let mut last_tip = String::new();
        let mut revision: u32 = 1;
        while !bridge.should_stop() {
            let _ = conn.process(Duration::from_millis(250));
            let tip = bridge.snapshot().tooltip_text();
            if tip != last_tip {
                last_tip = tip;
                revision = revision.wrapping_add(1);
                let sig_tip = Message::new_signal(SNI_PATH, SNI_IFACE, "NewToolTip");
                if let Ok(sig) = sig_tip {
                    let _ = conn.send(sig);
                }
                let sig_status = Message::new_signal(SNI_PATH, SNI_IFACE, "NewStatus");
                if let Ok(sig) = sig_status {
                    let state = if bridge.snapshot().connected {
                        "Active"
                    } else {
                        "Passive"
                    };
                    let _ = conn.send(sig.append1(state));
                }
                let sig_menu = Message::new_signal(MENU_PATH, MENU_IFACE, "LayoutUpdated");
                if let Ok(sig) = sig_menu {
                    let _ = conn.send(sig.append2(revision, 0i32));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_status_and_tooltip_cleanly() {
        let mut snap = TraySnapshot::default();
        assert_eq!(snap.status_line(), "Waiting for watch (Bluetooth LE)…");
        assert!(snap.tooltip_text().contains("Waiting for watch"));

        snap.connected = true;
        snap.device_label = Some("Galaxy Watch7".into());
        snap.battery_pct = Some(84);
        snap.effective_mode = ControlMode::Air;
        snap.effective_dpi = 2000;
        assert_eq!(snap.status_line(), "Connected: Galaxy Watch7 · 84%");
        assert_eq!(snap.detail_line(), "Air Mouse (Gyro) · 2000 DPI");
        assert!(snap.tooltip_text().contains("Active (Galaxy Watch7)"));

        snap.paused = true;
        snap.charging = true;
        snap.mode_override = Some(ControlMode::Touch);
        assert_eq!(snap.status_line(), "Paused: Galaxy Watch7 · 84% ⚡");
        assert!(snap.detail_line().contains("(Desktop override)"));
    }

    #[test]
    fn menu_actions_send_expected_commands() {
        let stop = Arc::new(AtomicBool::new(false));
        let (bridge, rx) = TrayBridge::new(TraySnapshot::default(), Arc::clone(&stop));

        handle_menu_action(ACT_TOGGLE_PAUSE, &bridge);
        handle_menu_action(ACT_MODE_AIR, &bridge);
        handle_menu_action(ACT_DPI_BASE + 2400, &bridge);
        handle_menu_action(ACT_RECENTER, &bridge);
        handle_menu_action(ACT_QUIT, &bridge);

        let cmds: Vec<TrayCommand> = rx.try_iter().collect();
        assert_eq!(
            cmds,
            vec![
                TrayCommand::SetPaused(true),
                TrayCommand::SetModeOverride(Some(ControlMode::Air)),
                TrayCommand::SetDpiOverride(Some(2400)),
                TrayCommand::Recenter,
                TrayCommand::Quit,
            ]
        );
        assert!(stop.load(Ordering::Acquire));
    }
}
