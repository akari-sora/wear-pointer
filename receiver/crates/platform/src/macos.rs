//! macOS cursor via CGEventPost with full multi-monitor support.
//!
//! Enumerates all active CoreGraphics displays (`CGDisplay::active_displays`)
//! in global desktop point coordinates (including negative origins for monitors
//! arranged to the left of or above the main display) and refreshes topology
//! periodically so hotplug / arrangement changes take effect automatically.

use crate::PointerBackend;
use anyhow::{anyhow, Result};
use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::display::{kCGNullDirectDisplayID, CGDisplay};
use core_graphics::event::{
    CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use std::time::{Duration, Instant};
use wear_pointer_core::{Button, DesktopLayout, Display, Rect, Screen};

const LAYOUT_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGGetOnlineDisplayList(
        max_displays: u32,
        online_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
}

fn active_display_ids() -> Vec<u32> {
    let mut ids = [0u32; 32];
    let mut count = 0u32;
    let rc = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if rc == 0 && count > 0 {
        return ids[..(count as usize).min(ids.len())].to_vec();
    }
    // When displays are dimmed/asleep at login or headless wake, `CGGetActiveDisplayList`
    // reports 0; `CGGetOnlineDisplayList` still enumerates every connected non-mirrored screen.
    count = 0;
    let rc_online =
        unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if rc_online == 0 && count > 0 {
        ids[..(count as usize).min(ids.len())].to_vec()
    } else {
        Vec::new()
    }
}

pub struct MacosBackend {
    layout: DesktopLayout,
    fixed_override: bool,
    last_refresh: Instant,
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
        match screen_override {
            Some(screen) => Self::new(screen),
            None => Self::from_layout(Self::detect_layout(), false),
        }
    }

    /// Queries all active non-mirrored macOS displays in global point coordinates.
    pub fn detect_layout() -> DesktopLayout {
        struct RawDisplay {
            id: u32,
            bounds: Rect,
            primary: bool,
            builtin: bool,
        }

        let mut raw = Vec::new();
        for id in active_display_ids() {
            let cg = CGDisplay::new(id);
            if cg.mirrors_display() != kCGNullDirectDisplayID {
                continue;
            }
            let b = cg.bounds();
            if b.size.width <= 1.0 || b.size.height <= 1.0 {
                continue;
            }
            let bounds = Rect::new(b.origin.x, b.origin.y, b.size.width, b.size.height);
            if raw
                .iter()
                .any(|existing: &RawDisplay| existing.bounds == bounds)
            {
                continue;
            }
            raw.push(RawDisplay {
                id,
                bounds,
                primary: cg.is_main(),
                builtin: cg.is_builtin(),
            });
        }

        if raw.is_empty() {
            let main = CGDisplay::main();
            let b = main.bounds();
            if b.size.width > 1.0 && b.size.height > 1.0 {
                raw.push(RawDisplay {
                    id: main.id,
                    bounds: Rect::new(b.origin.x, b.origin.y, b.size.width, b.size.height),
                    primary: true,
                    builtin: main.is_builtin(),
                });
            }
        }

        // Deterministic order: Primary first, then left-to-right, top-to-bottom.
        raw.sort_by(|a, b| {
            b.primary
                .cmp(&a.primary)
                .then_with(|| a.bounds.x.total_cmp(&b.bounds.x))
                .then_with(|| a.bounds.y.total_cmp(&b.bounds.y))
                .then_with(|| a.id.cmp(&b.id))
        });

        let non_primary_externals = raw.iter().filter(|d| !d.builtin && !d.primary).count();
        let mut ext_idx = 0usize;
        let displays = raw
            .into_iter()
            .map(|d| {
                let name = if d.builtin {
                    "Built-in".to_string()
                } else if d.primary {
                    "Main".to_string()
                } else if non_primary_externals > 1 {
                    ext_idx += 1;
                    format!("External {ext_idx}")
                } else {
                    "External".to_string()
                };
                Display::new(d.id, name, d.bounds, d.primary)
            })
            .collect();

        DesktopLayout::new(displays)
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
            left_down: false,
            right_down: false,
        }
    }

    fn refresh_if_due(&mut self) {
        if !self.fixed_override && self.last_refresh.elapsed() >= LAYOUT_REFRESH_INTERVAL {
            self.last_refresh = Instant::now();
            self.layout = Self::detect_layout();
        }
    }

    fn clamp_point(&self, x: f64, y: f64) -> (f64, f64) {
        self.layout.clamp_pixel(x, y)
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

    fn cursor_position(&self) -> Option<(f64, f64)> {
        let source = Self::source().ok()?;
        let event = CGEvent::new(source).ok()?;
        let point = event.location();
        Some((point.x, point.y))
    }

    fn screen(&self) -> Screen {
        self.layout.primary_display().bounds.to_screen()
    }

    fn layout(&mut self) -> DesktopLayout {
        self.refresh_if_due();
        self.layout.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_detects_active_displays_and_respects_override() {
        let mut backend = MacosBackend::open_unprompted(None);
        let detected = backend.screen();
        assert!(detected.width > 1.0 && detected.height > 1.0);
        let layout = backend.layout();
        assert!(!layout.displays().is_empty());
        eprintln!("macos detected {}", layout.summary());

        let custom = MacosBackend::open_unprompted(Some(Screen::new(1440.0, 900.0))).screen();
        assert_eq!(custom, Screen::new(1440.0, 900.0));
    }

    #[test]
    fn clamp_point_supports_single_and_negative_multi_monitor_layouts() {
        let backend = MacosBackend::new(Screen::new(1920.0, 1080.0));
        assert_eq!(backend.clamp_point(-50.0, -10.0), (0.0, 0.0));
        assert_eq!(backend.clamp_point(2500.0, 1500.0), (1919.0, 1079.0));
        assert_eq!(backend.clamp_point(960.0, 540.0), (960.0, 540.0));

        let multi = MacosBackend::from_layout(
            DesktopLayout::new(vec![
                Display::new(1, "Main", Rect::new(0.0, 0.0, 2560.0, 1440.0), true),
                Display::new(
                    2,
                    "Built-in",
                    Rect::new(-1512.0, 200.0, 1512.0, 982.0),
                    false,
                ),
            ]),
            true,
        );
        assert_eq!(multi.clamp_point(-1200.0, 600.0), (-1200.0, 600.0));
        assert_eq!(multi.clamp_point(-2000.0, 600.0), (-1512.0, 600.0));
    }
}
