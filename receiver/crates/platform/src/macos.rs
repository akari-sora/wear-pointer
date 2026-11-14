//! macOS cursor via CGEventPost.
//!
//! Skeleton. This file is compiled only on macOS and was not built in the
//! Linux checkout that produced the Stage 1 merge request. Accessibility
//! permission is required at runtime, the same as WatchPointer.

use crate::PointerBackend;
use anyhow::{anyhow, Result};
use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use wear_pointer_core::{Button, Screen};

pub struct MacosBackend {
    screen: Screen,
    x: f64,
    y: f64,
}

impl MacosBackend {
    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            x: screen.width / 2.0,
            y: screen.height / 2.0,
        }
    }

    fn source() -> Result<CGEventSource> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow!("CGEventSource::new failed. Grant Accessibility to the terminal."))
    }
}

impl PointerBackend for MacosBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        self.x = x;
        self.y = y;
        let source = Self::source()?;
        let event = CGEvent::new_mouse_event(
            source,
            CGEventType::MouseMoved,
            CGPoint::new(x, y),
            CGMouseButton::Left,
        )
        .ok_or_else(|| anyhow!("CGEvent::new_mouse_event failed"))?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        let source = Self::source()?;
        let (kind, cg_button) = match (button, down) {
            (Button::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            (Button::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            (Button::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
            (Button::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
        };
        let event = CGEvent::new_mouse_event(source, kind, CGPoint::new(self.x, self.y), cg_button)
            .ok_or_else(|| anyhow!("CGEvent button failed"))?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn scroll(&mut self, delta_px: i32) -> Result<()> {
        let source = Self::source()?;
        let event = CGEvent::new_scroll_event(
            source,
            core_graphics::event::ScrollEventUnit::Pixel,
            1,
            delta_px,
            0,
            0,
        )
        .ok_or_else(|| anyhow!("CGEvent scroll failed"))?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn screen(&self) -> Screen {
        self.screen
    }
}
