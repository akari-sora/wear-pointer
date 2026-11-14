//! Hyprland / wlroots virtual pointer.
//!
//! Uses `zwlr_virtual_pointer_v1` from wlr-protocols (Copyright © 2019 Josef
//! Gajdusek, MIT) via wayland-protocols-wlr. Absolute motion is in the
//! coordinate space reported by `wl_output`. Hyprland advertises this global
//! by default.

use crate::PointerBackend;
use anyhow::{anyhow, Context, Result};
use wayland_client::protocol::wl_output::{self, Mode, WlOutput};
use wayland_client::protocol::wl_pointer::{Axis, AxisSource, ButtonState};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;
use wear_pointer_core::{Button, Screen};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

struct App {
    pointer: Option<ZwlrVirtualPointerV1>,
    screen: Screen,
    saw_mode: bool,
}

pub struct HyprlandBackend {
    conn: Connection,
    queue: EventQueue<App>,
    app: App,
    pointer: ZwlrVirtualPointerV1,
}

impl HyprlandBackend {
    pub fn connect() -> Result<Self> {
        let conn = Connection::connect_to_env().context(
            "connecting to Wayland. Run this inside the Hyprland session, or pass --backend log",
        )?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        let _registry = conn.display().get_registry(&qh, ());
        let mut app = App {
            pointer: None,
            screen: Screen::new(1920.0, 1080.0),
            saw_mode: false,
        };
        queue.roundtrip(&mut app).context("wayland roundtrip")?;
        queue
            .roundtrip(&mut app)
            .context("wayland output roundtrip")?;
        let pointer = app.pointer.take().ok_or_else(|| {
            anyhow!(
                "compositor is missing zwlr_virtual_pointer_manager_v1. \
                 Hyprland provides it. Use --backend log, or rebuild with --features uinput."
            )
        })?;
        if !app.saw_mode {
            eprintln!(
                "wear-pointer: no wl_output mode yet, assuming {:.0}x{:.0}",
                app.screen.width, app.screen.height
            );
        } else {
            eprintln!(
                "wear-pointer: hyprland pointer, screen {:.0}x{:.0}",
                app.screen.width, app.screen.height
            );
        }
        Ok(Self {
            conn,
            queue,
            app,
            pointer,
        })
    }

    fn stamp(&mut self) -> u32 {
        let _ = self.queue.dispatch_pending(&mut self.app);
        monotonic_ms()
    }

    fn flush(&self) -> Result<()> {
        self.conn.flush().context("wayland flush")
    }
}

impl PointerBackend for HyprlandBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        let screen = self.app.screen;
        let extent_x = screen.width.max(1.0).round() as u32;
        let extent_y = screen.height.max(1.0).round() as u32;
        let px = x.round().clamp(0.0, f64::from(extent_x)) as u32;
        let py = y.round().clamp(0.0, f64::from(extent_y)) as u32;
        let time = self.stamp();
        self.pointer
            .motion_absolute(time, px, py, extent_x, extent_y);
        self.pointer.frame();
        self.flush()
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        let code = match button {
            Button::Left => BTN_LEFT,
            Button::Right => BTN_RIGHT,
        };
        let state = if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        let time = self.stamp();
        self.pointer.button(time, code, state);
        self.pointer.frame();
        self.flush()
    }

    fn scroll(&mut self, delta_px: i32) -> Result<()> {
        let time = self.stamp();
        self.pointer.axis_source(AxisSource::Wheel);
        self.pointer
            .axis(time, Axis::VerticalScroll, f64::from(delta_px));
        self.pointer.frame();
        self.flush()
    }

    fn screen(&self) -> Screen {
        self.app.screen
    }
}

fn monotonic_ms() -> u32 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ((ts.tv_sec as u64).saturating_mul(1000) + (ts.tv_nsec as u64) / 1_000_000) as u32
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "zwlr_virtual_pointer_manager_v1" => {
                let manager = registry.bind::<ZwlrVirtualPointerManagerV1, _, _>(
                    name,
                    version.min(2),
                    qh,
                    (),
                );
                state.pointer = Some(manager.create_virtual_pointer(
                    None::<&wayland_client::protocol::wl_seat::WlSeat>,
                    qh,
                    (),
                ));
            }
            "wl_output" => {
                registry.bind::<WlOutput, _, _>(name, version.min(4), qh, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for App {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            flags,
            width,
            height,
            ..
        } = event
        {
            // Current mode bit is 1. Take the first current mode as the
            // absolute-motion extent. Multi-monitor layout is a follow-up.
            let current = match flags {
                WEnum::Value(mode) => mode.contains(Mode::Current),
                WEnum::Unknown(raw) => raw & Mode::Current.bits() != 0,
            };
            if current || !state.saw_mode {
                state.screen = Screen::new(width as f64, height as f64);
                state.saw_mode = state.saw_mode || current;
            }
        }
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
