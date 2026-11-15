//! Windows cursor via SendInput with full multi-monitor virtual-desktop support.
//!
//! Enables Per-Monitor DPI Awareness v2, enumerates every active monitor via
//! `EnumDisplayMonitors` + `GetMonitorInfoW` (including negative-coordinate
//! left/top monitors), and injects absolute coordinates across the entire
//! virtual desktop with `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`.

use crate::PointerBackend;
use anyhow::{anyhow, Result};
use std::time::{Duration, Instant};
use wear_pointer_core::{Button, DesktopLayout, Display, Rect, Screen};
use windows::Win32::Foundation::{BOOL, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

const LAYOUT_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub struct WindowsBackend {
    layout: DesktopLayout,
    fixed_override: bool,
    last_refresh: Instant,
    x: f64,
    y: f64,
}

unsafe extern "system" fn enum_monitor_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<Display>);
    let mut info_ex = MONITORINFOEXW::default();
    info_ex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(
        hmonitor,
        &mut info_ex as *mut MONITORINFOEXW as *mut MONITORINFO,
    )
    .0 != 0
    {
        let rc = info_ex.monitorInfo.rcMonitor;
        let w = f64::from(rc.right - rc.left);
        let h = f64::from(rc.bottom - rc.top);
        if w > 1.0 && h > 1.0 {
            let primary = (info_ex.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;
            let nul_pos = info_ex
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info_ex.szDevice.len());
            let raw_name = String::from_utf16_lossy(&info_ex.szDevice[..nul_pos]);
            let clean_name = raw_name
                .strip_prefix(r"\\.\")
                .unwrap_or(&raw_name)
                .trim()
                .to_string();
            let parsed_id: Option<u32> = clean_name
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok();
            let id = parsed_id.unwrap_or((list.len() as u32) + 1);
            list.push(Display::new(
                id,
                clean_name,
                Rect::new(f64::from(rc.left), f64::from(rc.top), w, h),
                primary,
            ));
        }
    }
    BOOL(1)
}

impl WindowsBackend {
    pub fn open(screen_override: Option<Screen>) -> Self {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        match screen_override {
            Some(screen) => Self::new(screen),
            None => Self::from_layout(Self::detect_layout(), false),
        }
    }

    pub fn detect_layout() -> DesktopLayout {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let mut displays: Vec<Display> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_monitor_proc),
                LPARAM(&mut displays as *mut Vec<Display> as isize),
            );
        }
        if displays.is_empty() {
            let width = f64::from(unsafe { GetSystemMetrics(SM_CXSCREEN) });
            let height = f64::from(unsafe { GetSystemMetrics(SM_CYSCREEN) });
            let screen = if width > 1.0 && height > 1.0 {
                Screen::new(width, height)
            } else {
                Screen::new(1920.0, 1080.0)
            };
            return DesktopLayout::single(screen);
        }
        displays.sort_by(|a, b| {
            b.primary
                .cmp(&a.primary)
                .then_with(|| a.bounds.x.total_cmp(&b.bounds.x))
                .then_with(|| a.bounds.y.total_cmp(&b.bounds.y))
                .then_with(|| a.id.cmp(&b.id))
        });
        DesktopLayout::new(displays)
    }

    pub fn new(screen: Screen) -> Self {
        Self::from_layout(DesktopLayout::single(screen), true)
    }

    fn from_layout(layout: DesktopLayout, fixed_override: bool) -> Self {
        let (cx, cy) = layout.primary_display().bounds.center();
        Self {
            layout,
            fixed_override,
            last_refresh: Instant::now(),
            x: cx,
            y: cy,
        }
    }

    fn refresh_if_due(&mut self) {
        if !self.fixed_override && self.last_refresh.elapsed() >= LAYOUT_REFRESH_INTERVAL {
            self.last_refresh = Instant::now();
            self.layout = Self::detect_layout();
        }
    }

    fn send(&self, flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: u32) -> Result<()> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent == 0 {
            Err(anyhow!("SendInput failed"))
        } else {
            Ok(())
        }
    }

    fn absolute(&self, x: f64, y: f64) -> (i32, i32, MOUSE_EVENT_FLAGS) {
        let (cx, cy) = self.layout.clamp_pixel(x, y);
        if self.fixed_override {
            let b = self.layout.primary_display().bounds;
            let nx = if b.width <= 1.0 {
                0
            } else {
                (((cx - b.x + 0.5) * 65536.0) / b.width)
                    .floor()
                    .clamp(0.0, 65535.0) as i32
            };
            let ny = if b.height <= 1.0 {
                0
            } else {
                (((cy - b.y + 0.5) * 65536.0) / b.height)
                    .floor()
                    .clamp(0.0, 65535.0) as i32
            };
            return (nx, ny, MOUSEEVENTF_ABSOLUTE);
        }

        let sys_vw = f64::from(unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) });
        let sys_vh = f64::from(unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) });
        let (vx, vy, vw, vh) = if sys_vw > 1.0 && sys_vh > 1.0 {
            (
                f64::from(unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) }),
                f64::from(unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) }),
                sys_vw,
                sys_vh,
            )
        } else {
            let vb = self.layout.virtual_bounds();
            (vb.x, vb.y, vb.width, vb.height)
        };

        let nx = (((cx - vx + 0.5) * 65536.0) / vw.max(1.0))
            .floor()
            .clamp(0.0, 65535.0) as i32;
        let ny = (((cy - vy + 0.5) * 65536.0) / vh.max(1.0))
            .floor()
            .clamp(0.0, 65535.0) as i32;
        (nx, ny, MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)
    }
}

impl PointerBackend for WindowsBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        let (cx, cy) = self.layout.clamp_pixel(x, y);
        self.x = cx;
        self.y = cy;
        let (dx, dy, abs_flags) = self.absolute(cx, cy);
        self.send(MOUSEEVENTF_MOVE | abs_flags, dx, dy, 0)
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        let (dx, dy, abs_flags) = self.absolute(self.x, self.y);
        let btn_flag = match (button, down) {
            (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (Button::Left, false) => MOUSEEVENTF_LEFTUP,
            (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
        };
        self.send(btn_flag | MOUSEEVENTF_MOVE | abs_flags, dx, dy, 0)
    }

    fn scroll(&mut self, delta_px: i32) -> Result<()> {
        // WHEEL_DELTA is 120 per notch (~30 px). Preserve smooth sub-notch rotary deltas.
        let data = delta_px.saturating_mul(4);
        if data == 0 {
            return Ok(());
        }
        self.send(MOUSEEVENTF_WHEEL, 0, 0, data as u32)
    }

    fn cursor_position(&self) -> Option<(f64, f64)> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }.ok()?;
        Some((f64::from(point.x), f64::from(point.y)))
    }

    fn screen(&self) -> Screen {
        self.layout.primary_display().bounds.to_screen()
    }

    fn layout(&mut self) -> DesktopLayout {
        self.refresh_if_due();
        self.layout.clone()
    }
}
