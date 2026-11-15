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
    origin: (f64, f64),
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
        let mut origin = (0.0, 0.0);
        if let Some((ox, oy, logical)) = hypr_layout() {
            origin = (ox, oy);
            app.screen = logical;
            app.saw_mode = true;
        }
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
            origin,
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
        let extent_x = (screen.width.max(1.0) * 16.0).round() as u32;
        let extent_y = (screen.height.max(1.0) * 16.0).round() as u32;
        let px = (x * 16.0).round().clamp(0.0, f64::from(extent_x)) as u32;
        let py = (y * 16.0).round().clamp(0.0, f64::from(extent_y)) as u32;
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

    /// Asks Hyprland's IPC socket (`cursorpos`) and converts layout coordinates
    /// into this backend's logical output bounding box.
    fn cursor_position(&self) -> Option<(f64, f64)> {
        let (x, y) = hypr_cursorpos()?;
        Some((x - self.origin.0, y - self.origin.1))
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

fn hypr_request(cmd: &[u8]) -> Option<String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let path = format!("{runtime}/hypr/{signature}/.socket.sock");
    let mut stream = UnixStream::connect(path).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(50)))
        .ok()?;
    stream.write_all(cmd).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    Some(reply)
}

fn hypr_cursorpos() -> Option<(f64, f64)> {
    let reply = hypr_request(b"cursorpos")?;
    let (x, y) = reply.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn hypr_layout() -> Option<(f64, f64, Screen)> {
    let reply = hypr_request(b"monitors")?;
    parse_hypr_layout(&reply)
}

/// Parses `hyprctl monitors` plain-text blocks into `(origin_x, origin_y, logical_screen)`
/// so HiDPI fractional scaling (e.g. `1920x1200` at `scale: 1.5` -> `1280x800`)
/// matches `cursorpos` and `zwlr_virtual_pointer_v1` layout coordinates 1:1.
fn parse_hypr_layout(text: &str) -> Option<(f64, f64, Screen)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut found = false;

    for block in text
        .split("\nMonitor ")
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        let mut mode_wh: Option<(f64, f64)> = None;
        let mut pos_xy: Option<(f64, f64)> = None;
        let mut scale = 1.0_f64;
        let mut transform = 0_u32;
        let mut disabled = false;

        for line in block.lines().map(str::trim) {
            if mode_wh.is_none() {
                if let Some((res_hz, at_xy)) = line.split_once(" at ") {
                    if let Some((res, _)) = res_hz.split_once('@') {
                        if let (Some((w, h)), Some((x, y))) =
                            (res.split_once('x'), at_xy.split_once('x'))
                        {
                            if let (Ok(w), Ok(h), Ok(x), Ok(y)) = (
                                w.parse::<f64>(),
                                h.parse::<f64>(),
                                x.parse::<f64>(),
                                y.parse::<f64>(),
                            ) {
                                mode_wh = Some((w, h));
                                pos_xy = Some((x, y));
                            }
                        }
                    }
                }
            } else if let Some(rest) = line.strip_prefix("scale:") {
                if let Ok(s) = rest.trim().parse::<f64>() {
                    if s > 0.0 {
                        scale = s;
                    }
                }
            } else if let Some(rest) = line.strip_prefix("transform:") {
                if let Ok(t) = rest.trim().parse::<u32>() {
                    transform = t;
                }
            } else if let Some(rest) = line.strip_prefix("disabled:") {
                disabled = rest.trim() == "true";
            }
        }

        if disabled {
            continue;
        }
        if let (Some((mut w, mut h)), Some((x, y))) = (mode_wh, pos_xy) {
            if transform % 2 == 1 {
                std::mem::swap(&mut w, &mut h);
            }
            let lw = (w / scale).round().max(1.0);
            let lh = (h / scale).round().max(1.0);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + lw);
            max_y = max_y.max(y + lh);
            found = true;
        }
    }

    if !found || max_x <= min_x || max_y <= min_y {
        return None;
    }
    Some((min_x, min_y, Screen::new(max_x - min_x, max_y - min_y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fractional_scaled_hyprland_monitor() {
        let sample = "\
Monitor eDP-1 (ID 0):
\t1920x1200@60.00300 at 0x0
\tdescription: Lenovo Group Limited NV133WUM-N6D
\tscale: 1.5
\ttransform: 0
\tdisabled: false
";
        let (ox, oy, screen) = parse_hypr_layout(sample).expect("layout");
        assert_eq!((ox, oy), (0.0, 0.0));
        assert_eq!((screen.width, screen.height), (1280.0, 800.0));
    }

    #[test]
    fn parses_rotated_and_offset_multi_monitor_layout() {
        let sample = "\
Monitor DP-1 (ID 0):
\t2560x1440@144.00 at 0x0
\tscale: 1.0
\ttransform: 0
\tdisabled: false

Monitor HDMI-A-1 (ID 1):
\t1920x1080@60.00 at -1080x0
\tscale: 1.0
\ttransform: 1
\tdisabled: false
";
        let (ox, oy, screen) = parse_hypr_layout(sample).expect("layout");
        assert_eq!((ox, oy), (-1080.0, 0.0));
        assert_eq!((screen.width, screen.height), (3640.0, 1920.0));
    }
}
