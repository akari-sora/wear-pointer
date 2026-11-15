//! macOS cursor via CGEventPost.
//!
//! Accessibility permission is required at runtime when moving/clicking the
//! cursor (`System Settings -> Privacy & Security -> Accessibility`).

use crate::PointerBackend;
use anyhow::{anyhow, Result};
use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use wear_pointer_core::{Button, Screen};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
}

pub struct MacosBackend {
    screen: Screen,
    x: f64,
    y: f64,
    left_down: bool,
    right_down: bool,
}

impl MacosBackend {
    pub fn open(screen_override: Option<Screen>) -> Self {
        Self::check_accessibility(true);
        Self::open_unprompted(screen_override)
    }

    pub fn open_unprompted(screen_override: Option<Screen>) -> Self {
        let screen = screen_override.unwrap_or_else(|| {
            let bounds = CGDisplay::main().bounds();
            if bounds.size.width > 1.0 && bounds.size.height > 1.0 {
                Screen::new(bounds.size.width, bounds.size.height)
            } else {
                Screen::new(1920.0, 1080.0)
            }
        });
        Self::new(screen)
    }

    /// Queries macOS TCC Accessibility trust and optionally triggers the
    /// standard system prompt (`kAXTrustedCheckOptionPrompt`) when untrusted.
    pub fn check_accessibility(prompt: bool) -> bool {
        let trusted = unsafe {
            if prompt {
                let key = CFString::new("AXTrustedCheckOptionPrompt");
                let dict = CFDictionary::from_CFType_pairs(&[(
                    key.as_CFType(),
                    CFBoolean::true_value().as_CFType(),
                )]);
                AXIsProcessTrustedWithOptions(dict.as_CFTypeRef())
            } else {
                AXIsProcessTrusted()
            }
        };
        if !trusted {
            eprintln!(
                "wear-pointer: warning: macOS Accessibility permission is not yet granted; \
                 CGEventPost cursor/click events will be ignored until allowed in \
                 System Settings -> Privacy & Security -> Accessibility."
            );
        }
        trusted
    }

    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            x: screen.width / 2.0,
            y: screen.height / 2.0,
            left_down: false,
            right_down: false,
        }
    }

    fn clamp_point(&self, x: f64, y: f64) -> (f64, f64) {
        let max_x = (self.screen.width - 1.0).max(0.0);
        let max_y = (self.screen.height - 1.0).max(0.0);
        (x.clamp(0.0, max_x), y.clamp(0.0, max_y))
    }

    fn source() -> Result<CGEventSource> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow!("CGEventSource::new failed. Grant Accessibility to the terminal."))
    }
}

impl PointerBackend for MacosBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        let (x, y) = self.clamp_point(x, y);
        self.x = x;
        self.y = y;
        let source = Self::source()?;
        let (kind, cg_button) = if self.left_down {
            (CGEventType::LeftMouseDragged, CGMouseButton::Left)
        } else if self.right_down {
            (CGEventType::RightMouseDragged, CGMouseButton::Right)
        } else {
            (CGEventType::MouseMoved, CGMouseButton::Left)
        };
        let event = CGEvent::new_mouse_event(source, kind, CGPoint::new(x, y), cg_button)
            .map_err(|_| anyhow!("CGEvent::new_mouse_event failed"))?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        match button {
            Button::Left => self.left_down = down,
            Button::Right => self.right_down = down,
        }
        let source = Self::source()?;
        let (kind, cg_button) = match (button, down) {
            (Button::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            (Button::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            (Button::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
            (Button::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
        };
        let event = CGEvent::new_mouse_event(source, kind, CGPoint::new(self.x, self.y), cg_button)
            .map_err(|_| anyhow!("CGEvent button failed"))?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn scroll(&mut self, delta_px: i32) -> Result<()> {
        let source = Self::source()?;
        let event = CGEvent::new_scroll_event(source, ScrollEventUnit::PIXEL, 1, delta_px, 0, 0)
            .map_err(|_| anyhow!("CGEvent scroll failed"))?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn screen(&self) -> Screen {
        self.screen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_detects_positive_display_bounds_and_respects_override() {
        let detected = MacosBackend::open_unprompted(None).screen();
        assert!(detected.width > 1.0 && detected.height > 1.0);

        let custom = MacosBackend::open_unprompted(Some(Screen::new(1440.0, 900.0))).screen();
        assert_eq!(custom, Screen::new(1440.0, 900.0));
    }

    #[test]
    fn clamp_point_stays_within_display_bounds() {
        let backend = MacosBackend::new(Screen::new(1920.0, 1080.0));
        assert_eq!(backend.clamp_point(-50.0, -10.0), (0.0, 0.0));
        assert_eq!(backend.clamp_point(2500.0, 1500.0), (1919.0, 1079.0));
        assert_eq!(backend.clamp_point(960.0, 540.0), (960.0, 540.0));
    }
}
