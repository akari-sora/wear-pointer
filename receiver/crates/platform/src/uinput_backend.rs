//! Relative uinput mouse. Optional (`--features uinput`).
//!
//! Needs write access to `/dev/uinput`. This backend does not know the
//! compositor's cursor, so it emits deltas between projected positions.
//! The first sample sets the origin and does not jump. Absolute aim is a
//! worse fit here than on the Hyprland virtual pointer; use it when
//! `zwlr_virtual_pointer_v1` is missing.

use crate::PointerBackend;
use anyhow::{Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, Key, RelativeAxisType};
use wear_pointer_core::{Button, DesktopLayout, Screen};

pub struct UinputBackend {
    device: VirtualDevice,
    layout: DesktopLayout,
    last: Option<(f64, f64)>,
    pending_x: f64,
    pending_y: f64,
    pending_scroll: f64,
}

impl UinputBackend {
    pub fn open(screen: Screen, screen_explicit: bool) -> Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        keys.insert(Key::BTN_LEFT);
        keys.insert(Key::BTN_RIGHT);
        let mut axes = AttributeSet::<RelativeAxisType>::new();
        axes.insert(RelativeAxisType::REL_X);
        axes.insert(RelativeAxisType::REL_Y);
        axes.insert(RelativeAxisType::REL_WHEEL);
        let device = evdev::uinput::VirtualDeviceBuilder::new()
            .context("opening /dev/uinput")?
            .name("wear-pointer")
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()
            .context("creating the uinput device")?;
        let layout = if screen_explicit {
            DesktopLayout::single(screen)
        } else {
            crate::hyprland::hypr_monitors_layout().unwrap_or_else(|| DesktopLayout::single(screen))
        };
        eprintln!(
            "wear-pointer: uinput relative mouse ({}). The first aim sample sets the origin.",
            layout.summary()
        );
        Ok(Self {
            device,
            layout,
            last: None,
            pending_x: 0.0,
            pending_y: 0.0,
            pending_scroll: 0.0,
        })
    }

    fn emit(&mut self, events: &[InputEvent]) -> Result<()> {
        self.device.emit(events).context("uinput emit")
    }
}

impl PointerBackend for UinputBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        let Some((prev_x, prev_y)) = self.last else {
            self.last = Some((x, y));
            return Ok(());
        };
        let dx = crate::residual::take_steps(&mut self.pending_x, x - prev_x);
        let dy = crate::residual::take_steps(&mut self.pending_y, y - prev_y);
        self.last = Some((x, y));
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        let mut events = Vec::with_capacity(3);
        if dx != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE,
                RelativeAxisType::REL_X.0,
                dx,
            ));
        }
        if dy != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE,
                RelativeAxisType::REL_Y.0,
                dy,
            ));
        }
        events.push(InputEvent::new(EventType::SYNCHRONIZATION, 0, 0));
        self.emit(&events)
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        let key = match button {
            Button::Left => Key::BTN_LEFT,
            Button::Right => Key::BTN_RIGHT,
        };
        let value = if down { 1 } else { 0 };
        self.emit(&[
            InputEvent::new(EventType::KEY, key.code(), value),
            InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
        ])
    }

    fn scroll(&mut self, delta_px: i32) -> Result<()> {
        // REL_WHEEL is in detent steps. About 30 px is one step; the remainder
        // stays so a stream of smaller deltas still scrolls.
        let steps =
            crate::residual::take_steps(&mut self.pending_scroll, f64::from(delta_px) / 30.0);
        if steps == 0 {
            return Ok(());
        }
        self.emit(&[
            InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_WHEEL.0, steps),
            InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
        ])
    }

    fn screen(&self) -> Screen {
        self.layout.primary_display().bounds.to_screen()
    }

    fn layout(&mut self) -> DesktopLayout {
        self.layout.clone()
    }
}
