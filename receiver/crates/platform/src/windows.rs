//! Windows cursor via SendInput.
//!
//! Absolute motion uses the 0..=65535 normalized primary-monitor coordinate
//! space; primary screen dimensions are queried with `GetSystemMetrics` unless
//! overridden via `--screen`.

use crate::PointerBackend;
use anyhow::{anyhow, Result};
use wear_pointer_core::{Button, Screen};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

pub struct WindowsBackend {
    screen: Screen,
    x: f64,
    y: f64,
}

impl WindowsBackend {
    pub fn open(screen_override: Option<Screen>) -> Self {
        let screen = screen_override.unwrap_or_else(|| {
            let width = f64::from(unsafe { GetSystemMetrics(SM_CXSCREEN) });
            let height = f64::from(unsafe { GetSystemMetrics(SM_CYSCREEN) });
            if width > 1.0 && height > 1.0 {
                Screen::new(width, height)
            } else {
                Screen::new(1920.0, 1080.0)
            }
        });
        Self::new(screen)
    }

    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            x: screen.width / 2.0,
            y: screen.height / 2.0,
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

    fn absolute(&self, x: f64, y: f64) -> (i32, i32) {
        let nx = if self.screen.width <= 1.0 {
            0
        } else {
            ((x / (self.screen.width - 1.0)) * 65535.0)
                .clamp(0.0, 65535.0)
                .round() as i32
        };
        let ny = if self.screen.height <= 1.0 {
            0
        } else {
            ((y / (self.screen.height - 1.0)) * 65535.0)
                .clamp(0.0, 65535.0)
                .round() as i32
        };
        (nx, ny)
    }
}

impl PointerBackend for WindowsBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        self.x = x;
        self.y = y;
        let (dx, dy) = self.absolute(x, y);
        self.send(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, dx, dy, 0)
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        let (dx, dy) = self.absolute(self.x, self.y);
        let flags = match (button, down) {
            (Button::Left, true) => MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_ABSOLUTE,
            (Button::Left, false) => MOUSEEVENTF_LEFTUP | MOUSEEVENTF_ABSOLUTE,
            (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN | MOUSEEVENTF_ABSOLUTE,
            (Button::Right, false) => MOUSEEVENTF_RIGHTUP | MOUSEEVENTF_ABSOLUTE,
        };
        self.send(flags | MOUSEEVENTF_MOVE, dx, dy, 0)
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
        self.screen
    }
}
