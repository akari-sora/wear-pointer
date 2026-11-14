//! Windows cursor via SendInput.
//!
//! Skeleton. This file is compiled only on Windows and was not built in the
//! Linux checkout that produced the Stage 1 merge request. Absolute motion
//! uses the 0..65535 normalized coordinate space.

use crate::PointerBackend;
use anyhow::{anyhow, Result};
use wear_pointer_core::{Button, Screen};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT,
};

pub struct WindowsBackend {
    screen: Screen,
    x: f64,
    y: f64,
}

impl WindowsBackend {
    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            x: screen.width / 2.0,
            y: screen.height / 2.0,
        }
    }

    fn send(&self, flags: u32, dx: i32, dy: i32, data: u32) -> Result<()> {
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
            (x / self.screen.width * 65535.0).round() as i32
        };
        let ny = if self.screen.height <= 1.0 {
            0
        } else {
            (y / self.screen.height * 65535.0).round() as i32
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
        // WHEEL_DELTA is 120 per notch. Treat ~30 px as one notch.
        let data = (delta_px / 30) * 120;
        self.send(MOUSEEVENTF_WHEEL, 0, 0, data as u32)
    }

    fn screen(&self) -> Screen {
        self.screen
    }
}
