//! Hyprland / wlroots virtual pointer with full multi-monitor support.
//!
//! Uses `zwlr_virtual_pointer_v1` from wlr-protocols (Copyright © 2019 Josef
//! Gajdusek, MIT) via wayland-protocols-wlr. Queries Hyprland's IPC socket
//! (`monitors` / `cursorpos`) for exact per-monitor logical coordinates
//! (including HiDPI scaling, rotation transforms, and negative offsets) and
//! falls back to aggregating advertised `wl_output` globals on other wlroots
//! compositors.

use crate::PointerBackend;
use anyhow::{anyhow, Context, Result};
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_output::{self, Mode, WlOutput};
use wayland_client::protocol::wl_pointer::{Axis, AxisSource, ButtonState};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;
use wear_pointer_core::{Button, DesktopLayout, Display, Rect, Screen};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const LAYOUT_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
struct WlOutputEntry {
    global_name: u32,
    name: String,
    width: f64,
    height: f64,
    scale: i32,
    saw_current: bool,
}

struct App {
    pointer: Option<ZwlrVirtualPointerV1>,
    outputs: Vec<WlOutputEntry>,
}

impl App {
    fn fallback_layout(&self) -> Option<DesktopLayout> {
        if self.outputs.is_empty() {
            return None;
        }
        let mut sorted = self.outputs.clone();
        sorted.sort_by_key(|o| o.global_name);
        let mut cursor_x = 0.0;
        let mut displays = Vec::with_capacity(sorted.len());
        for (i, out) in sorted.into_iter().enumerate() {
            let scale = f64::from(out.scale.max(1));
            let lw = (out.width / scale).round().max(1.0);
            let lh = (out.height / scale).round().max(1.0);
            let label = if out.name.is_empty() {
                format!("Output-{}", i + 1)
            } else {
                out.name
            };
            displays.push(Display::new(
                out.global_name,
                label,
                Rect::new(cursor_x, 0.0, lw, lh),
                i == 0,
            ));
            cursor_x += lw;
        }
        Some(DesktopLayout::new(displays))
    }
}

pub struct HyprlandBackend {
    conn: Connection,
    queue: EventQueue<App>,
    app: App,
    layout: DesktopLayout,
    fixed_override: bool,
    last_refresh: Instant,
    pointer: ZwlrVirtualPointerV1,
}

impl HyprlandBackend {
    pub fn connect(screen_override: Option<Screen>) -> Result<Self> {
        let conn = Connection::connect_to_env().context(
            "connecting to Wayland. Run this inside the Hyprland session, or pass --backend log",
        )?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        let _registry = conn.display().get_registry(&qh, ());
        let mut app = App {
            pointer: None,
            outputs: Vec::new(),
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

        let (layout, fixed_override) = match screen_override {
            Some(screen) => (DesktopLayout::single(screen), true),
            None => {
                if let Some(hypr) = hypr_monitors_layout() {
                    (hypr, false)
                } else if let Some(wl) = app.fallback_layout() {
                    (wl, false)
                } else {
                    eprintln!("wear-pointer: no wl_output mode yet, assuming 1920x1080");
                    (DesktopLayout::single(Screen::new(1920.0, 1080.0)), false)
                }
            }
        };

        eprintln!("wear-pointer: hyprland pointer, {}", layout.summary());

        Ok(Self {
            conn,
            queue,
            app,
            layout,
            fixed_override,
            last_refresh: Instant::now(),
            pointer,
        })
    }

    fn refresh_if_due(&mut self) {
        if self.fixed_override || self.last_refresh.elapsed() < LAYOUT_REFRESH_INTERVAL {
            return;
        }
        self.last_refresh = Instant::now();
        let _ = self.queue.dispatch_pending(&mut self.app);
        if let Some(hypr) = hypr_monitors_layout() {
            self.layout = hypr;
        } else if let Some(wl) = self.app.fallback_layout() {
            self.layout = wl;
        }
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
        let (cx, cy) = self.layout.clamp_pixel(x, y);
        let vb = self.layout.virtual_bounds();
        let extent_x = (vb.width.max(1.0) * 16.0).round() as u32;
        let extent_y = (vb.height.max(1.0) * 16.0).round() as u32;
        let max_px = f64::from(extent_x.saturating_sub(1));
        let max_py = f64::from(extent_y.saturating_sub(1));
        let px = ((cx - vb.x) * 16.0).round().clamp(0.0, max_px) as u32;
        let py = ((cy - vb.y) * 16.0).round().clamp(0.0, max_py) as u32;
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
        self.layout.primary_display().bounds.to_screen()
    }

    fn layout(&mut self) -> DesktopLayout {
        self.refresh_if_due();
        self.layout.clone()
    }

    /// Asks Hyprland's IPC socket (`cursorpos`), which reports global layout
    /// coordinates matching [`DesktopLayout`] 1:1.
    fn cursor_position(&self) -> Option<(f64, f64)> {
        hypr_cursorpos()
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
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
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
                    state.outputs.push(WlOutputEntry {
                        global_name: name,
                        name: String::new(),
                        width: 1920.0,
                        height: 1080.0,
                        scale: 1,
                        saw_current: false,
                    });
                    registry.bind::<WlOutput, _, _>(name, version.min(4), qh, name);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.retain(|o| o.global_name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, u32> for App {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        global_name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(entry) = state
            .outputs
            .iter_mut()
            .find(|o| o.global_name == *global_name)
        else {
            return;
        };
        match event {
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                let current = match flags {
                    WEnum::Value(mode) => mode.contains(Mode::Current),
                    WEnum::Unknown(raw) => raw & Mode::Current.bits() != 0,
                };
                if current || !entry.saw_current {
                    entry.width = f64::from(width.max(1));
                    entry.height = f64::from(height.max(1));
                    entry.saw_current = entry.saw_current || current;
                }
            }
            wl_output::Event::Scale { factor } => {
                entry.scale = factor.max(1);
            }
            wl_output::Event::Name { name } => {
                entry.name = name;
            }
            _ => {}
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

pub(crate) fn hypr_request(cmd: &[u8]) -> Option<String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
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

pub(crate) fn hypr_monitors_layout() -> Option<DesktopLayout> {
    let reply = hypr_request(b"monitors")?;
    parse_hypr_monitors(&reply)
}

fn parse_monitor_header(first_line: &str, fallback_id: u32) -> (u32, String) {
    let rest = first_line
        .trim()
        .strip_prefix("Monitor ")
        .unwrap_or(first_line.trim())
        .trim_end_matches(':');
    if let Some((name_part, id_part)) = rest.rsplit_once("(ID ") {
        let id = id_part
            .trim_end_matches(')')
            .trim()
            .parse::<u32>()
            .unwrap_or(fallback_id);
        return (id, name_part.trim().to_string());
    }
    (fallback_id, rest.trim().to_string())
}

/// Parses `hyprctl monitors` plain-text blocks into a full [`DesktopLayout`]
/// preserving each monitor's ID, connector name (`DP-1`, `eDP-1`, …), HiDPI
/// logical size, 90°/270° rotation transform, and signed `(x, y)` coordinates.
pub(crate) fn parse_hypr_monitors(text: &str) -> Option<DesktopLayout> {
    let mut displays: Vec<Display> = Vec::new();
    let mut block_idx = 0u32;

    for raw_block in text.split("\nMonitor ") {
        let trimmed = raw_block.trim();
        if trimmed.is_empty() {
            continue;
        }
        let header_line = trimmed.lines().next().unwrap_or("");
        let (id, name) = parse_monitor_header(header_line, block_idx);
        block_idx += 1;

        let mut mode_wh: Option<(f64, f64)> = None;
        let mut pos_xy: Option<(f64, f64)> = None;
        let mut scale = 1.0_f64;
        let mut transform = 0_u32;
        let mut disabled = false;

        for line in trimmed.lines().skip(1).map(str::trim) {
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
            let at_origin = x.abs() < 0.5 && y.abs() < 0.5;
            displays.push(Display::new(id, name, Rect::new(x, y, lw, lh), at_origin));
        }
    }

    if displays.is_empty() {
        return None;
    }

    // Ensure stable ordering across focus shifts: origin `(0, 0)` monitor first
    // (or lowest monitor ID if none sits at `(0, 0)`), then ascending ID.
    if !displays.iter().any(|d| d.primary) {
        if let Some(min_id) = displays.iter().map(|d| d.id).min() {
            if let Some(first) = displays.iter_mut().find(|d| d.id == min_id) {
                first.primary = true;
            }
        }
    }
    displays.sort_by(|a, b| {
        b.primary
            .cmp(&a.primary)
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.bounds.x.total_cmp(&b.bounds.x))
    });
    Some(DesktopLayout::new(displays))
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
        let layout = parse_hypr_monitors(sample).expect("layout");
        assert_eq!(layout.len(), 1);
        assert_eq!(layout.displays()[0].name, "eDP-1");
        assert_eq!(
            layout.displays()[0].bounds,
            Rect::new(0.0, 0.0, 1280.0, 800.0)
        );
    }

    #[test]
    fn parses_rotated_and_negative_offset_multi_monitor_layout() {
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
        let layout = parse_hypr_monitors(sample).expect("layout");
        assert_eq!(layout.len(), 2);
        assert_eq!(layout.displays()[0].name, "DP-1");
        assert!(layout.displays()[0].primary);
        assert_eq!(layout.displays()[1].name, "HDMI-A-1");
        assert_eq!(
            layout.displays()[1].bounds,
            Rect::new(-1080.0, 0.0, 1080.0, 1920.0)
        );
        assert_eq!(
            layout.virtual_bounds(),
            Rect::new(-1080.0, 0.0, 3640.0, 1920.0)
        );
    }
}
