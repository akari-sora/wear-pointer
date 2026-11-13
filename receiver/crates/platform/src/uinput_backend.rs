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
use wear_pointer_core::{Button, Screen};

pub struct UinputBackend {
    device: VirtualDevice,
    screen: Screen,
    last: Option<(f64, f64)>,
}

impl UinputBackend {
    pub fn open(screen: Screen) -> Result<Self> {
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
        eprintln!("wear-pointer: uinput relative mouse. The first aim sample sets the origin.");
        Ok(Self {
            device,
            screen,
            last: None,
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
        let dx = (x - prev_x).round() as i32;
        let dy = (y - prev_y).round() as i32;
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
        if delta_px == 0 {
            return Ok(());
        }
        // REL_WHEEL is in detent steps. Map about 30 px to one step.
        let steps = delta_px / 30;
        if steps == 0 {
            return Ok(());
        }
        self.emit(&[
            InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_WHEEL.0, steps),
            InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
        ])
    }

    fn screen(&self) -> Screen {
        self.screen
    }
}
