//! Packets in, cursor actions out.
//!
//! Aim samples update a filtered target. A separate 240 Hz tick eases the
//! shown cursor toward that target. Clicks rewind to where the cursor was
//! when the finger went down. Adapted from WatchPointer (Copyright (c) 2026
//! Umar Afzal, MIT).

use crate::filter::OneEuroFilter;
use crate::packet::{decode, Packet, DEFAULT_DPI, MAX_DPI, MIN_DPI};
use crate::project::{dpi_to_speed, project_aim, reach_degrees, speed_to_dpi, Screen};
use crate::relative::{gain_scale, AirMouse, ControlMode, TouchPad};
use crate::stats::{LinkStats, WindowStats};
use crate::{FRESH_GAP_S, MAX_FILTER_DT_S};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CursorAction {
    Move { x: f64, y: f64 },
    Button { button: Button, down: bool },
    Scroll { delta_px: i32 },
}

#[derive(Clone, Debug)]
pub struct DebugSnapshot {
    pub rx_hz: f64,
    pub lost_window: u32,
    pub lost_total: u64,
    pub packets_window: u32,
    pub packets_total: u64,
    pub loss_pct_window: f64,
    pub latency_avg_ms: f64,
    pub latency_min_ms: f64,
    pub latency_max_ms: f64,
    pub remote_samples: Option<u16>,
    pub remote_notify_ok: Option<u16>,
    pub remote_notify_drop: Option<u16>,
    /// Watch gyroscope rate from the stats packet, samples / window, not the
    /// desktop's all-kind receive rate.
    pub remote_aim_hz: Option<f64>,
    pub effective_dpi: u16,
    pub remote_dpi: Option<u16>,
    pub mode: ControlMode,
    pub cursor_x: Option<f64>,
    pub cursor_y: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub speed: f64,
    /// When explicitly set via `--dpi` or `--speed`, scales watch DPI adjustments
    /// relative to `DEFAULT_DPI` so both CLI preference and live wrist tuning work.
    pub cli_speed_override: Option<f64>,
    /// 1€ cutoff while the hand is still, in Hz.
    pub steady_hz: f64,
    /// 1€ speed coefficient. Higher means less lag on a fast move.
    pub quickness: f64,
    /// Ignore aim changes smaller than this, in degrees. 0 matches
    /// WatchPointer, which removed its dead zone because it walked the cursor
    /// out of line.
    pub dead_zone_deg: f64,
    pub screen: Screen,
    pub follow_tau_s: f64,
    pub min_move_px: f64,
    /// `--mode`: forces a pointing model and ignores the watch's choice.
    pub mode_override: Option<ControlMode>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            speed: dpi_to_speed(DEFAULT_DPI),
            cli_speed_override: None,
            steady_hz: 0.8,
            quickness: 0.01,
            dead_zone_deg: 0.0,
            screen: Screen::new(1920.0, 1080.0),
            follow_tau_s: crate::FOLLOW_TAU_S,
            min_move_px: crate::MIN_MOVE_PX,
            mode_override: None,
        }
    }
}

#[derive(Clone, Debug)]
struct Follower {
    shown: Option<(f64, f64)>,
    tau_s: f64,
    min_move_px: f64,
}

impl Follower {
    fn new(tau_s: f64, min_move_px: f64) -> Self {
        Self {
            shown: None,
            tau_s,
            min_move_px,
        }
    }

    fn snap(&mut self, point: (f64, f64)) {
        self.shown = Some(point);
    }

    fn reset(&mut self) {
        self.shown = None;
    }

    /// Ease toward `target`. Returns the new position when the step is large
    /// enough to send. A step under `min_move_px` leaves `shown` alone. The
    /// next tick retries the same step, so this only settles once the cursor
    /// is already within a fraction of a pixel of the target.
    fn tick(&mut self, target: (f64, f64), dt: f64) -> Option<(f64, f64)> {
        let Some(shown) = self.shown else {
            self.shown = Some(target);
            return Some(target);
        };
        let dt = if dt <= 0.0 { 1.0 / 240.0 } else { dt };
        let follow = 1.0 - (-dt / self.tau_s).exp();
        let next = (
            shown.0 + (target.0 - shown.0) * follow,
            shown.1 + (target.1 - shown.1) * follow,
        );
        let step = (next.0 - shown.0).abs() + (next.1 - shown.1).abs();
        if step <= self.min_move_px {
            return None;
        }
        self.shown = Some(next);
        Some(next)
    }
}

pub struct PointerSession {
    config: SessionConfig,
    filter_x: OneEuroFilter,
    filter_y: OneEuroFilter,
    follower: Follower,
    stats: LinkStats,
    target: Option<(f64, f64)>,
    last_aim_deg: Option<(f64, f64)>,
    last_aim_local_s: Option<f64>,
    last_tick_s: Option<f64>,
    last_debug_s: Option<f64>,
    paused: bool,
    dragging: bool,
    remote_dpi: Option<u16>,
    remote_mode: Option<ControlMode>,
    air: AirMouse,
    touch: TouchPad,
    /// Virtual cursor for the relative modes (absolute mode derives the
    /// position from the arm pose every sample instead).
    pos: Option<(f64, f64)>,
    /// Local time of the last relative step that moved the cursor.
    last_relative_move_s: Option<f64>,
    /// False after `on_disconnected` until the next `on_connected`. Starts
    /// open so unit tests can feed packets without a fake link.
    link_open: bool,
    history: Vec<(f64, f64, f64)>,
}

impl PointerSession {
    pub fn new(config: SessionConfig) -> Self {
        let filter_x = OneEuroFilter::new(config.steady_hz, config.quickness);
        let filter_y = OneEuroFilter::new(config.steady_hz, config.quickness);
        let follower = Follower::new(config.follow_tau_s, config.min_move_px);
        Self {
            config,
            filter_x,
            filter_y,
            follower,
            stats: LinkStats::new(),
            target: None,
            last_aim_deg: None,
            last_aim_local_s: None,
            last_tick_s: None,
            last_debug_s: None,
            paused: false,
            dragging: false,
            remote_dpi: None,
            remote_mode: None,
            air: AirMouse::default(),
            touch: TouchPad::default(),
            pos: None,
            last_relative_move_s: None,
            link_open: true,
            history: Vec::new(),
        }
    }

    /// Pointing model in use: `--mode` if given, else the watch's choice, else
    /// absolute (what a watch that predates the Mode packet sends).
    pub fn effective_mode(&self) -> ControlMode {
        self.config
            .mode_override
            .or(self.remote_mode)
            .unwrap_or(ControlMode::Absolute)
    }

    pub fn remote_mode(&self) -> Option<ControlMode> {
        self.remote_mode
    }

    /// True when the receiver should report the OS cursor position through
    /// [`Self::sync_external_cursor`], so the relative modes continue from
    /// wherever a real mouse or trackpad left the cursor.
    pub fn wants_external_cursor(&self) -> bool {
        self.link_open && self.effective_mode().is_relative()
    }

    /// The OS cursor is at `point`. In a relative mode, while the watch is not
    /// moving the cursor and nothing is held, adopt that position so the next
    /// watch motion starts from it instead of yanking the cursor back.
    pub fn sync_external_cursor(&mut self, point: (f64, f64), local_s: f64) {
        if !self.effective_mode().is_relative() || self.dragging {
            return;
        }
        if let Some(t) = self.last_relative_move_s {
            if local_s - t < EXTERNAL_SYNC_IDLE_S {
                return;
            }
        }
        let point = self.clamp_to_screen(point);
        if let Some(shown) = self.follower.shown {
            if (shown.0 - point.0).abs() + (shown.1 - point.1).abs() <= EXTERNAL_SYNC_PX {
                return;
            }
        }
        self.pos = Some(point);
        self.target = Some(point);
        self.follower.snap(point);
        // A click rewinds through `history`; without this it would jump back
        // to where the watch last left the cursor.
        self.remember(local_s, point);
    }

    pub fn set_screen(&mut self, screen: Screen) {
        self.config.screen = screen;
    }

    pub fn screen(&self) -> Screen {
        self.config.screen
    }

    pub fn effective_speed(&self) -> f64 {
        match (self.config.cli_speed_override, self.remote_dpi) {
            (None, Some(watch_dpi)) => dpi_to_speed(watch_dpi.clamp(MIN_DPI, MAX_DPI)),
            (None, None) => self.config.speed,
            (Some(cli_speed), Some(watch_dpi)) => {
                let ratio = f64::from(watch_dpi.clamp(MIN_DPI, MAX_DPI)) / f64::from(DEFAULT_DPI);
                (cli_speed * ratio).clamp(0.1, 6.4)
            }
            (Some(cli_speed), None) => cli_speed,
        }
    }

    pub fn effective_dpi(&self) -> u16 {
        speed_to_dpi(self.effective_speed())
    }

    pub fn remote_dpi(&self) -> Option<u16> {
        self.remote_dpi
    }

    /// New BLE session. Drops sequence, clock, filters, cursor history, remote
    /// stats, and pause. Releases a stuck drag. The 300 ms silence release in
    /// `tick` still covers a link that goes quiet without a disconnect event.
    pub fn on_connected(&mut self) -> Vec<CursorAction> {
        let release = self.force_release_drag();
        self.stats.reset_link();
        self.filter_x.reset();
        self.filter_y.reset();
        self.follower.reset();
        self.target = None;
        self.last_aim_deg = None;
        self.last_aim_local_s = None;
        self.paused = false;
        self.remote_dpi = None;
        self.remote_mode = None;
        self.air.reset();
        self.touch.reset();
        self.pos = self.follower.shown;
        self.history.clear();
        self.link_open = true;
        release
    }

    /// Explicit disconnect. Releases drag immediately and ignores later
    /// packets until `on_connected`.
    pub fn on_disconnected(&mut self) -> Vec<CursorAction> {
        self.link_open = false;
        self.target = None;
        self.last_aim_local_s = None;
        self.force_release_drag()
    }

    #[cfg(test)]
    fn mapped_watch_us(&self, watch_us: u32) -> Option<f64> {
        self.stats.watch_to_local(watch_us)
    }

    #[cfg(test)]
    fn lost_total(&self) -> u64 {
        self.stats.lost
    }

    pub fn push_packet(&mut self, bytes: &[u8], local_s: f64) -> Vec<CursorAction> {
        if !self.link_open {
            return Vec::new();
        }
        let packet = match decode(bytes) {
            Ok(packet) => packet,
            Err(_) => return Vec::new(),
        };
        let header = packet.header().clone();
        let timing = self.stats.observe(
            header.sequence,
            header.timestamp_us,
            local_s,
            matches!(packet, Packet::Aim { .. } | Packet::Touch { .. }),
        );
        if timing.duplicate {
            return Vec::new();
        }
        match packet {
            Packet::Aim {
                left_cdeg, up_cdeg, ..
            } => match self.effective_mode() {
                ControlMode::Absolute => {
                    self.push_aim(left_cdeg, up_cdeg, local_s, timing.fresh, timing.watch_dt_s)
                }
                ControlMode::Air => {
                    self.push_air(left_cdeg, up_cdeg, local_s, timing.fresh, timing.watch_dt_s)
                }
                ControlMode::Touch => {
                    // Still a heartbeat for the silence / drag-release rules.
                    self.last_aim_local_s = Some(local_s);
                    Vec::new()
                }
            },
            Packet::Touch { x_acc, y_acc, .. } => {
                self.last_aim_local_s = Some(local_s);
                if self.effective_mode() == ControlMode::Touch {
                    let dt = usable_dt(timing.fresh, timing.watch_dt_s);
                    let scale = gain_scale(self.config.screen.width, self.effective_dpi());
                    let delta = self.touch.step(x_acc, y_acc, dt, scale);
                    self.push_relative(delta, local_s)
                } else {
                    Vec::new()
                }
            }
            Packet::Mode { mode, .. } => {
                let before = self.effective_mode();
                self.remote_mode = Some(ControlMode::from_wire(mode));
                if self.effective_mode() != before {
                    self.switch_mode();
                }
                Vec::new()
            }
            Packet::LeftClick { .. } => self.click(Button::Left, header.timestamp_us, local_s),
            Packet::RightClick { .. } => self.click(Button::Right, header.timestamp_us, local_s),
            Packet::Scroll { delta_px, .. } => vec![CursorAction::Scroll {
                delta_px: i32::from(delta_px),
            }],
            Packet::Center { .. } => self.center(),
            Packet::Pause { paused, .. } => self.set_paused(paused),
            Packet::Drag { down, header } => self.drag(down, header.timestamp_us, local_s),
            Packet::Stats {
                samples,
                notifies_ok,
                notifies_drop,
                window_ms,
                ..
            } => {
                self.stats
                    .note_remote(samples, notifies_ok, notifies_drop, window_ms);
                Vec::new()
            }
            Packet::Sensitivity { dpi, .. } => {
                self.remote_dpi = Some(dpi.clamp(MIN_DPI, MAX_DPI));
                Vec::new()
            }
            Packet::Info { .. } | Packet::Unknown { .. } => Vec::new(),
        }
    }

    pub fn tick(&mut self, local_s: f64) -> (Vec<CursorAction>, Option<DebugSnapshot>) {
        let mut actions = Vec::new();
        let dt = match self.last_tick_s {
            Some(prev) => local_s - prev,
            None => 1.0 / 240.0,
        };
        self.last_tick_s = Some(local_s);

        let silent = self
            .last_aim_local_s
            .map(|t| local_s - t >= FRESH_GAP_S)
            .unwrap_or(true);
        if self.dragging && silent {
            self.dragging = false;
            actions.push(CursorAction::Button {
                button: Button::Left,
                down: false,
            });
        }

        if !self.paused && !silent {
            if let Some(target) = self.target {
                if let Some(point) = self.follower.tick(target, dt) {
                    self.remember(local_s, point);
                    actions.push(CursorAction::Move {
                        x: point.0,
                        y: point.1,
                    });
                }
            }
        }

        let debug = self.debug_if_due(local_s);
        (actions, debug)
    }

    fn push_aim(
        &mut self,
        left_cdeg: i16,
        up_cdeg: i16,
        local_s: f64,
        fresh: bool,
        watch_dt_s: Option<f64>,
    ) -> Vec<CursorAction> {
        self.last_aim_local_s = Some(local_s);
        let left = f64::from(left_cdeg) / 100.0;
        let up = f64::from(up_cdeg) / 100.0;
        if let Some((prev_left, prev_up)) = self.last_aim_deg {
            if (left - prev_left).abs() < self.config.dead_zone_deg
                && (up - prev_up).abs() < self.config.dead_zone_deg
            {
                return Vec::new();
            }
        }
        self.last_aim_deg = Some((left, up));
        let projected = project_aim(
            left,
            up,
            self.config.screen,
            reach_degrees(self.effective_speed()),
        );
        let raw = (
            projected.0.clamp(0.0, self.config.screen.width.max(0.0)),
            projected.1.clamp(0.0, self.config.screen.height.max(0.0)),
        );
        let dt = match watch_dt_s {
            Some(dt) if dt > 0.0 && dt < MAX_FILTER_DT_S => dt,
            _ => 0.0,
        };
        let mut actions = Vec::new();
        if fresh {
            self.filter_x.reset();
            self.filter_y.reset();
            self.follower.snap(raw);
            self.remember(local_s, raw);
            actions.push(CursorAction::Move { x: raw.0, y: raw.1 });
        }
        let filtered = (
            self.filter_x.filter(raw.0, dt),
            self.filter_y.filter(raw.1, dt),
        );
        self.target = Some(filtered);
        actions
    }

    /// Air mouse: speed-dependent gain on the change in aim angles.
    fn push_air(
        &mut self,
        left_cdeg: i16,
        up_cdeg: i16,
        local_s: f64,
        fresh: bool,
        watch_dt_s: Option<f64>,
    ) -> Vec<CursorAction> {
        self.last_aim_local_s = Some(local_s);
        let dt = usable_dt(fresh, watch_dt_s);
        let scale = gain_scale(self.config.screen.width, self.effective_dpi());
        let delta = self.air.step(
            f64::from(left_cdeg) / 100.0,
            f64::from(up_cdeg) / 100.0,
            dt,
            scale,
        );
        self.push_relative(delta, local_s)
    }

    /// Moves the relative-mode virtual cursor; edge pinning re-aligns it.
    fn push_relative(&mut self, delta: (f64, f64), local_s: f64) -> Vec<CursorAction> {
        if delta.0 == 0.0 && delta.1 == 0.0 {
            return Vec::new();
        }
        let start = self
            .pos
            .or(self.follower.shown)
            .unwrap_or_else(|| self.center_point());
        let next = self.clamp_to_screen((start.0 + delta.0, start.1 + delta.1));
        self.pos = Some(next);
        self.target = Some(next);
        self.last_relative_move_s = Some(local_s);
        Vec::new()
    }

    /// The pointing model changed: start the new one from where the cursor is.
    fn switch_mode(&mut self) {
        self.filter_x.reset();
        self.filter_y.reset();
        self.last_aim_deg = None;
        self.air.reset();
        self.touch.reset();
        self.pos = self.follower.shown;
        self.target = self.follower.shown;
    }

    fn clamp_to_screen(&self, point: (f64, f64)) -> (f64, f64) {
        (
            point.0.clamp(0.0, self.config.screen.width.max(0.0)),
            point.1.clamp(0.0, self.config.screen.height.max(0.0)),
        )
    }

    fn center(&mut self) -> Vec<CursorAction> {
        self.filter_x.reset();
        self.filter_y.reset();
        self.last_aim_deg = None;
        self.air.reset();
        self.touch.reset();
        let center = self.center_point();
        self.pos = Some(center);
        self.target = Some(center);
        self.follower.snap(center);
        vec![CursorAction::Move {
            x: center.0,
            y: center.1,
        }]
    }

    fn set_paused(&mut self, paused: bool) -> Vec<CursorAction> {
        self.paused = paused;
        // Pause is the relative modes' clutch: resume continues from here.
        self.air.reset();
        self.touch.reset();
        if paused {
            self.force_release_drag()
        } else {
            Vec::new()
        }
    }

    fn force_release_drag(&mut self) -> Vec<CursorAction> {
        if !self.dragging {
            return Vec::new();
        }
        self.dragging = false;
        vec![CursorAction::Button {
            button: Button::Left,
            down: false,
        }]
    }

    fn click(&mut self, button: Button, watch_us: u32, local_s: f64) -> Vec<CursorAction> {
        let point = self.press_point(watch_us);
        self.follower.snap(point);
        self.pin_relative(point);
        self.remember(local_s, point);
        vec![
            CursorAction::Move {
                x: point.0,
                y: point.1,
            },
            CursorAction::Button { button, down: true },
            CursorAction::Button {
                button,
                down: false,
            },
        ]
    }

    fn drag(&mut self, down: bool, watch_us: u32, local_s: f64) -> Vec<CursorAction> {
        let point = self.press_point(watch_us);
        self.follower.snap(point);
        self.pin_relative(point);
        self.remember(local_s, point);
        if down {
            if self.dragging {
                return Vec::new();
            }
            self.dragging = true;
            vec![
                CursorAction::Move {
                    x: point.0,
                    y: point.1,
                },
                CursorAction::Button {
                    button: Button::Left,
                    down: true,
                },
            ]
        } else if self.dragging {
            self.dragging = false;
            vec![
                CursorAction::Move {
                    x: point.0,
                    y: point.1,
                },
                CursorAction::Button {
                    button: Button::Left,
                    down: false,
                },
            ]
        } else {
            Vec::new()
        }
    }

    /// In a relative mode the virtual cursor must also move to the rewound
    /// press point, or the follower would creep toward the jolted position.
    fn pin_relative(&mut self, point: (f64, f64)) {
        if self.effective_mode().is_relative() {
            self.pos = Some(point);
            self.target = Some(point);
        }
    }

    fn press_point(&self, watch_us: u32) -> (f64, f64) {
        let fallback = self.follower.shown.unwrap_or_else(|| self.center_point());
        match self.stats.watch_to_local(watch_us) {
            Some(time) => rewind_point(&self.history, time, fallback),
            None => fallback,
        }
    }

    fn center_point(&self) -> (f64, f64) {
        (
            self.config.screen.width / 2.0,
            self.config.screen.height / 2.0,
        )
    }

    fn remember(&mut self, time: f64, point: (f64, f64)) {
        self.history.push((time, point.0, point.1));
        let keep_after = time - 1.0;
        let drop = self.history.iter().take_while(|p| p.0 < keep_after).count();
        if drop > 0 {
            self.history.drain(0..drop);
        }
    }

    fn debug_if_due(&mut self, local_s: f64) -> Option<DebugSnapshot> {
        match self.last_debug_s {
            None => {
                self.last_debug_s = Some(local_s);
                None
            }
            Some(prev) if local_s - prev >= 1.0 => {
                let window = self.stats.take_window();
                let snapshot = snapshot_from(self, &window, local_s - prev);
                self.last_debug_s = Some(local_s);
                Some(snapshot)
            }
            Some(_) => None,
        }
    }
}

fn snapshot_from(session: &PointerSession, window: &WindowStats, elapsed: f64) -> DebugSnapshot {
    let elapsed = elapsed.max(1e-6);
    let denom = f64::from(window.packets + window.lost);
    let loss_pct = if denom == 0.0 {
        0.0
    } else {
        f64::from(window.lost) / denom * 100.0
    };
    DebugSnapshot {
        rx_hz: f64::from(window.packets) / elapsed,
        lost_window: window.lost,
        lost_total: session.stats.lost,
        packets_window: window.packets,
        packets_total: session.stats.packets,
        loss_pct_window: loss_pct,
        latency_avg_ms: window.latency_avg_ms,
        latency_min_ms: window.latency_min_ms,
        latency_max_ms: window.latency_max_ms,
        remote_samples: session.stats.remote_samples,
        remote_notify_ok: session.stats.remote_notify_ok,
        remote_notify_drop: session.stats.remote_notify_drop,
        remote_aim_hz: remote_aim_hz(session),
        effective_dpi: session.effective_dpi(),
        remote_dpi: session.remote_dpi(),
        mode: session.effective_mode(),
        cursor_x: session.follower.shown.map(|p| p.0),
        cursor_y: session.follower.shown.map(|p| p.1),
    }
}

/// Do not adopt the OS cursor until the watch has been still this long, so
/// the cursor's own lag behind our injected events is not mistaken for a
/// real mouse.
const EXTERNAL_SYNC_IDLE_S: f64 = 0.15;
/// OS cursor this far (L1 pixels) from where we put it means another device
/// moved it.
const EXTERNAL_SYNC_PX: f64 = 4.0;

fn usable_dt(fresh: bool, watch_dt_s: Option<f64>) -> Option<f64> {
    if fresh {
        return None;
    }
    match watch_dt_s {
        Some(dt) if dt > 0.0 && dt < MAX_FILTER_DT_S => Some(dt),
        _ => None,
    }
}

fn remote_aim_hz(session: &PointerSession) -> Option<f64> {
    let samples = session.stats.remote_samples?;
    let window_ms = session.stats.remote_window_ms?;
    if window_ms == 0 {
        None
    } else {
        Some(f64::from(samples) * 1000.0 / f64::from(window_ms))
    }
}

fn rewind_point(history: &[(f64, f64, f64)], time: f64, fallback: (f64, f64)) -> (f64, f64) {
    if history.is_empty() {
        return fallback;
    }
    history
        .iter()
        .rev()
        .find(|point| point.0 <= time)
        .map(|point| (point.1, point.2))
        .unwrap_or((history[0].1, history[0].2))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use crate::packet::{
        encode, header, Packet, KIND_AIM, KIND_CENTER, KIND_DRAG, KIND_LEFT_CLICK, KIND_MODE,
        KIND_PAUSE, KIND_SCROLL, KIND_SENSITIVITY, KIND_TOUCH, MODE_AIR, MODE_TOUCH,
    };

    fn session() -> PointerSession {
        PointerSession::new(SessionConfig {
            screen: Screen::new(2000.0, 1000.0),
            steady_hz: 0.8,
            quickness: 0.0,
            ..SessionConfig::default()
        })
    }

    fn aim(seq: u16, us: u32, left: i16, up: i16) -> Vec<u8> {
        encode(&Packet::Aim {
            header: header(KIND_AIM, seq, us),
            left_cdeg: left,
            up_cdeg: up,
        })
    }

    #[test]
    fn follower_covers_most_of_a_step_at_240_hz() {
        let mut follower = Follower::new(0.005, 0.05);
        follower.snap((0.0, 0.0));
        let moved = follower.tick((100.0, 0.0), 1.0 / 240.0).unwrap();
        let follow = 1.0 - (-(1.0_f64 / 240.0) / 0.005).exp();
        assert!((moved.0 - 100.0 * follow).abs() < 1e-6);
    }

    #[test]
    fn sub_pixel_residue_is_not_emitted() {
        let mut follower = Follower::new(0.005, 0.05);
        follower.snap((0.0, 0.0));
        assert!(follower.tick((0.02, 0.0), 1.0 / 240.0).is_none());
        assert!(follower.shown.unwrap().0.abs() < 1e-9);
        assert!(follower.tick((10.0, 0.0), 1.0 / 240.0).is_some());
    }

    #[test]
    fn rewind_uses_the_last_point_at_or_before_the_touch() {
        let history = [(1.0, 10.0, 20.0), (1.1, 30.0, 40.0), (1.2, 50.0, 60.0)];
        assert_eq!(rewind_point(&history, 1.15, (0.0, 0.0)), (30.0, 40.0));
        assert_eq!(rewind_point(&history, 0.5, (0.0, 0.0)), (10.0, 20.0));
        assert_eq!(rewind_point(&[], 1.0, (7.0, 8.0)), (7.0, 8.0));
    }

    #[test]
    fn left_turn_moves_the_cursor_left_of_center() {
        let mut session = session();
        let first = session.push_packet(&aim(1, 0, 0, 0), 0.0);
        assert!(matches!(first[0], CursorAction::Move { x, .. } if (x - 1000.0).abs() < 1.0));
        session.push_packet(&aim(2, 20_000, 1000, 0), 0.02);
        let (actions, _) = session.tick(0.02 + 1.0 / 240.0);
        match actions[0] {
            CursorAction::Move { x, .. } => assert!(x < 1000.0, "got {x}"),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn dead_zone_holds_a_tiny_aim_change() {
        let config = SessionConfig {
            screen: Screen::new(2000.0, 1000.0),
            dead_zone_deg: 1.0,
            ..SessionConfig::default()
        };
        let mut session = PointerSession::new(config);
        session.push_packet(&aim(1, 0, 0, 0), 0.0);
        let held = session.push_packet(&aim(2, 10_000, 10, 0), 0.01);
        assert!(held.is_empty());
        assert!((session.target.unwrap().0 - 1000.0).abs() < 1.0);
    }

    #[test]
    fn pause_suppresses_motion_and_click_emits_a_press() {
        let mut session = session();
        session.push_packet(&aim(1, 0, 0, 0), 0.0);
        session.push_packet(&aim(2, 20_000, 2000, 0), 0.02);
        let pause = encode(&Packet::Pause {
            header: header(KIND_PAUSE, 3, 30_000),
            paused: true,
        });
        session.push_packet(&pause, 0.03);
        let (actions, _) = session.tick(0.04);
        assert!(actions.is_empty());

        let click = encode(&Packet::LeftClick {
            header: header(KIND_LEFT_CLICK, 4, 10_000),
        });
        let actions = session.push_packet(&click, 0.05);
        assert!(matches!(
            actions[1],
            CursorAction::Button {
                button: Button::Left,
                down: true
            }
        ));
        assert!(matches!(
            actions[2],
            CursorAction::Button {
                button: Button::Left,
                down: false
            }
        ));
    }

    #[test]
    fn scroll_passes_the_pixel_delta() {
        let mut session = session();
        let packet = encode(&Packet::Scroll {
            header: header(KIND_SCROLL, 1, 0),
            delta_px: -8,
        });
        let actions = session.push_packet(&packet, 1.0);
        assert_eq!(actions, vec![CursorAction::Scroll { delta_px: -8 }]);
    }

    #[test]
    fn historical_click_through_push_packet_keeps_the_touch_time() {
        // reproduce_stats.py: Aim 1.000s/5.000, Aim 1.100s/5.100, Click 1.020s received 5.120.
        // The touch must map to 5.020, not the receive time.
        let mut session = session();
        session.push_packet(&aim(100, 1_000_000, 0, 0), 5.0);
        session.push_packet(&aim(101, 1_100_000, 0, 0), 5.1);
        let click = encode(&Packet::LeftClick {
            header: header(KIND_LEFT_CLICK, 102, 1_020_000),
        });
        session.push_packet(&click, 5.120);
        let mapped = session.mapped_watch_us(1_020_000).expect("clock anchor");
        assert!(
            (mapped - 5.020).abs() < 1e-6,
            "historical touch maps to receive time {mapped:.6}s; expected 5.020000s"
        );
    }

    #[test]
    fn drag_up_with_an_older_timestamp_does_not_move_the_anchor() {
        let mut session = session();
        session.push_packet(&aim(1, 2_000_000, 0, 0), 8.0);
        let down = encode(&Packet::Drag {
            header: header(KIND_DRAG, 2, 1_650_000),
            down: true,
        });
        session.push_packet(&down, 8.040);
        let mapped = session.mapped_watch_us(1_650_000).unwrap();
        assert!(
            (mapped - 7.650).abs() < 1e-6,
            "350 ms hold maps to {mapped:.6}, expected 7.650"
        );
        session.push_packet(&aim(3, 2_050_000, 0, 0), 8.050);
        let later = session.mapped_watch_us(2_050_000).unwrap();
        assert!(
            (later - 8.050).abs() < 1e-3,
            "aim after a historical drag lost the anchor: {later:.6}"
        );
    }

    #[test]
    fn new_connection_sequence_zero_is_not_packet_loss() {
        // Last packet of the old link is sequence 1000. The watch resets to 0.
        // Without a connection boundary this is reported as 64,535 losses.
        let mut session = session();
        session.push_packet(&aim(1000, 1_000_000, 0, 0), 5.0);
        let _ = session.on_connected();
        session.push_packet(&aim(0, 11_000_000, 0, 0), 15.0);
        assert_eq!(
            session.lost_total(),
            0,
            "reconnection is counted as {} missing packets",
            session.lost_total()
        );
    }

    #[test]
    fn real_sequence_and_timestamp_wrap_stay_continuous() {
        let mut session = session();
        let near = u32::MAX - 4_999;
        session.push_packet(&aim(65535, near, 0, 0), 10.0);
        session.push_packet(&aim(0, 5_000, 0, 0), 10.01);
        assert_eq!(session.lost_total(), 0);
        let mapped = session.mapped_watch_us(5_000).unwrap();
        assert!(
            (mapped - 10.01).abs() < 1e-3,
            "wrapped timestamp mapped to {mapped}"
        );
    }

    #[test]
    fn duplicate_click_does_not_fire_twice() {
        let mut session = session();
        let click = encode(&Packet::LeftClick {
            header: header(KIND_LEFT_CLICK, 7, 1_000),
        });
        let first = session.push_packet(&click, 1.0);
        let second = session.push_packet(&click, 1.01);
        assert_eq!(first.len(), 3);
        assert!(second.is_empty());
    }

    #[test]
    fn disconnect_releases_drag_and_drops_a_late_click() {
        let mut session = session();
        session.push_packet(&aim(1, 1_000_000, 0, 0), 3.0);
        let down = encode(&Packet::Drag {
            header: header(KIND_DRAG, 2, 1_000_000),
            down: true,
        });
        session.push_packet(&down, 3.01);
        let released = session.on_disconnected();
        assert!(matches!(
            released.last(),
            Some(CursorAction::Button {
                button: Button::Left,
                down: false
            })
        ));
        let click = encode(&Packet::LeftClick {
            header: header(KIND_LEFT_CLICK, 3, 1_000_000),
        });
        assert!(session.push_packet(&click, 3.02).is_empty());
    }

    #[test]
    fn debug_snapshot_reports_a_dropped_sequence() {
        let mut session = session();
        session.push_packet(&aim(1, 0, 0, 0), 0.0);
        session.tick(0.0);
        session.push_packet(&aim(4, 1_000_000, 0, 0), 1.0);
        let (_, debug) = session.tick(1.01);
        let debug = debug.expect("snapshot after one second");
        assert_eq!(debug.lost_window, 2);
        assert!(debug.rx_hz > 0.0);
        assert!(debug.loss_pct_window > 0.0);
    }

    #[test]
    fn extreme_tilt_clamps_to_screen_bounds_without_filter_windup() {
        let mut session = session();
        let actions = session.push_packet(&aim(1, 0, 7_500, 7_500), 0.0);
        assert_eq!(actions, vec![CursorAction::Move { x: 0.0, y: 0.0 }]);
        assert_eq!(session.target, Some((0.0, 0.0)));

        let actions_far = session.push_packet(&aim(2, 500_000, -7_500, -7_500), 0.5);
        assert_eq!(
            actions_far,
            vec![CursorAction::Move {
                x: 2000.0,
                y: 1000.0
            }]
        );
        assert_eq!(session.target, Some((2000.0, 1000.0)));
    }

    #[test]
    fn default_config_is_1600_dpi_ergonomic_reach() {
        let s = PointerSession::new(SessionConfig::default());
        assert_eq!(s.effective_dpi(), 1600);
        assert!((s.effective_speed() - 1.6).abs() < 1e-9);
        assert!((reach_degrees(s.effective_speed()) - 15.625).abs() < 1e-6);
    }

    #[test]
    fn watch_sensitivity_packet_updates_live_dpi_and_projection() {
        let mut s = session();
        let low = encode(&Packet::Sensitivity {
            header: header(KIND_SENSITIVITY, 1, 0),
            dpi: 800,
        });
        s.push_packet(&low, 0.0);
        assert_eq!(s.effective_dpi(), 800);
        let low_move = s.push_packet(&aim(2, 10_000, 500, 0), 0.01);
        let low_x = match low_move[0] {
            CursorAction::Move { x, .. } => x,
            other => panic!("expected move, got {other:?}"),
        };

        let mut fast = session();
        let high = encode(&Packet::Sensitivity {
            header: header(KIND_SENSITIVITY, 1, 0),
            dpi: 2400,
        });
        fast.push_packet(&high, 0.0);
        assert_eq!(fast.effective_dpi(), 2400);
        let high_move = fast.push_packet(&aim(2, 10_000, 500, 0), 0.01);
        let high_x = match high_move[0] {
            CursorAction::Move { x, .. } => x,
            other => panic!("expected move, got {other:?}"),
        };
        // Turning 5 deg left from center (1000) moves significantly farther left at 2400 DPI than 800 DPI.
        assert!(high_x < low_x, "expected {high_x} < {low_x}");
    }

    #[test]
    fn cli_dpi_override_preserves_initial_value_and_scales_with_watch_changes() {
        let cfg = SessionConfig {
            speed: 2.0,
            cli_speed_override: Some(2.0),
            ..SessionConfig::default()
        };
        let mut s = PointerSession::new(cfg);
        assert_eq!(s.effective_dpi(), 2000);

        // Watch connects at default 1600 DPI -> CLI 2000 DPI is preserved!
        let init = encode(&Packet::Sensitivity {
            header: header(KIND_SENSITIVITY, 1, 0),
            dpi: 1600,
        });
        s.push_packet(&init, 0.0);
        assert_eq!(s.effective_dpi(), 2000);

        // User steps watch up to 2400 DPI (+50%) -> desktop scales to 3000 DPI!
        let boosted = encode(&Packet::Sensitivity {
            header: header(KIND_SENSITIVITY, 2, 10_000),
            dpi: 2400,
        });
        s.push_packet(&boosted, 0.01);
        assert_eq!(s.effective_dpi(), 3000);
    }

    fn mode(seq: u16, value: u8) -> Vec<u8> {
        encode(&Packet::Mode {
            header: header(KIND_MODE, seq, 0),
            mode: value,
        })
    }

    fn touch(seq: u16, us: u32, x: i16, y: i16) -> Vec<u8> {
        encode(&Packet::Touch {
            header: header(KIND_TOUCH, seq, us),
            x_acc: x,
            y_acc: y,
            down: true,
        })
    }

    /// Runs the 240 Hz tick for `secs` starting at `t0`; returns the last shown point.
    fn run_ticks(s: &mut PointerSession, t0: f64, secs: f64) -> Option<(f64, f64)> {
        let mut last = None;
        let mut t = t0;
        while t < t0 + secs {
            let (actions, _) = s.tick(t);
            for a in actions {
                if let CursorAction::Move { x, y } = a {
                    last = Some((x, y));
                }
            }
            t += 1.0 / 240.0;
        }
        last
    }

    #[test]
    fn watch_mode_packet_switches_to_air_and_turns_move_relatively() {
        let mut s = session();
        assert_eq!(s.effective_mode(), ControlMode::Absolute);
        s.push_packet(&mode(0, MODE_AIR), 0.0);
        assert_eq!(s.effective_mode(), ControlMode::Air);
        s.push_packet(
            &encode(&Packet::Center {
                header: header(KIND_CENTER, 1, 0),
            }),
            0.0,
        );
        // A steady right turn at 60 °/s for 0.5 s (100 Hz).
        let mut left: i16 = 0;
        for i in 0..50u16 {
            left -= 60;
            let us = 10_000 * u32::from(i + 1);
            s.push_packet(&aim(i + 2, us, left, 0), f64::from(us) / 1e6);
        }
        let shown = run_ticks(&mut s, 0.0, 0.55).expect("cursor moved");
        assert!(
            shown.0 > 1300.0,
            "right turn should move right of center, got {shown:?}"
        );
        assert!(
            (shown.1 - 500.0).abs() < 5.0,
            "no vertical motion, got {shown:?}"
        );
        // Holding still afterwards: the cursor stays (no laser snap back to the arm pose).
        for i in 50..100u16 {
            let us = 10_000 * u32::from(i + 1);
            s.push_packet(&aim(i + 2, us, left, 0), f64::from(us) / 1e6);
        }
        let held = run_ticks(&mut s, 0.55, 0.5).unwrap_or(shown);
        assert!(
            (held.0 - shown.0).abs() < 2.0,
            "still watch moved {shown:?} -> {held:?}"
        );
    }

    #[test]
    fn cli_mode_override_ignores_the_watch() {
        let mut s = PointerSession::new(SessionConfig {
            mode_override: Some(ControlMode::Absolute),
            ..SessionConfig::default()
        });
        s.push_packet(&mode(0, MODE_AIR), 0.0);
        assert_eq!(s.remote_mode(), Some(ControlMode::Air));
        assert_eq!(s.effective_mode(), ControlMode::Absolute);
    }

    #[test]
    fn touch_mode_follows_the_finger_and_clicks_where_it_stopped() {
        let mut s = session();
        s.push_packet(&mode(0, MODE_TOUCH), 0.0);
        s.push_packet(
            &encode(&Packet::Center {
                header: header(KIND_CENTER, 1, 0),
            }),
            0.0,
        );
        // Aims are ignored in touch mode.
        s.push_packet(&aim(2, 5_000, 2000, 2000), 0.005);
        let mut x: i16 = 0;
        for i in 0..30u16 {
            x = x.wrapping_add(100); // 1 mm per 10 ms = 100 mm/s, to the right
            let us = 10_000 * u32::from(i + 1);
            s.push_packet(&touch(i + 3, us, x, 0), f64::from(us) / 1e6);
        }
        let shown = run_ticks(&mut s, 0.0, 0.35).expect("cursor moved");
        assert!(
            shown.0 > 1200.0,
            "finger right moves the cursor right, got {shown:?}"
        );
        assert!(
            (shown.1 - 500.0).abs() < 1.0,
            "aims must not move it, got {shown:?}"
        );
        let click = encode(&Packet::LeftClick {
            header: header(KIND_LEFT_CLICK, 40, 300_000),
        });
        let actions = s.push_packet(&click, 0.36);
        match actions.first() {
            Some(CursorAction::Move { x, .. }) => assert!(*x > 1200.0, "click at {x}"),
            other => panic!("expected a move first, got {other:?}"),
        }
    }

    #[test]
    fn relative_mode_adopts_a_real_mouse_position_when_idle() {
        let mut s = session();
        s.push_packet(&mode(0, MODE_AIR), 0.0);
        s.push_packet(
            &encode(&Packet::Center {
                header: header(KIND_CENTER, 1, 0),
            }),
            0.0,
        );
        run_ticks(&mut s, 0.0, 0.1);
        assert!(s.wants_external_cursor());
        s.sync_external_cursor((300.0, 200.0), 1.0);
        let mut left: i16 = 0;
        for i in 0..20u16 {
            left -= 60;
            let us = 1_000_000 + 10_000 * u32::from(i + 1);
            s.push_packet(&aim(i + 2, us, left, 0), f64::from(us) / 1e6);
        }
        let shown = run_ticks(&mut s, 1.0, 0.25).expect("moved");
        assert!(
            shown.0 > 300.0 && shown.0 < 1000.0,
            "continued from the mouse, got {shown:?}"
        );
        assert!(
            (shown.1 - 200.0).abs() < 5.0,
            "kept the mouse's y, got {shown:?}"
        );
        // A watch click right after a real-mouse move lands where the mouse left it.
        let mut s2 = session();
        s2.push_packet(&mode(0, MODE_AIR), 0.0);
        s2.push_packet(
            &encode(&Packet::Center {
                header: header(KIND_CENTER, 1, 0),
            }),
            0.0,
        );
        s2.push_packet(&aim(2, 10_000, 0, 0), 0.01);
        run_ticks(&mut s2, 0.0, 0.1);
        s2.sync_external_cursor((1055.0, 465.0), 2.0);
        let click = encode(&Packet::LeftClick {
            header: header(KIND_LEFT_CLICK, 3, 2_100_000),
        });
        s2.push_packet(&aim(4, 2_090_000, 0, 0), 2.09);
        match s2.push_packet(&click, 2.1).first() {
            Some(CursorAction::Move { x, y }) => {
                assert!(
                    (x - 1055.0).abs() < 1.0 && (y - 465.0).abs() < 1.0,
                    "click at {x},{y}"
                )
            }
            other => panic!("expected a move, got {other:?}"),
        }
        // While the watch is moving, the OS cursor's lag is not adopted.
        let before = s.target;
        s.sync_external_cursor((10.0, 10.0), 1.21);
        assert_eq!(s.target, before);
    }

    #[test]
    fn pause_is_a_clutch_in_air_mode() {
        let mut s = session();
        s.push_packet(&mode(0, MODE_AIR), 0.0);
        s.push_packet(
            &encode(&Packet::Center {
                header: header(KIND_CENTER, 1, 0),
            }),
            0.0,
        );
        let pause = |seq, p| {
            encode(&Packet::Pause {
                header: header(KIND_PAUSE, seq, 0),
                paused: p,
            })
        };
        s.push_packet(&aim(2, 10_000, 0, 0), 0.01);
        s.push_packet(&pause(3, true), 0.02);
        // Arm swings 40° while paused; the watch sends no aims.
        s.push_packet(&pause(4, false), 0.5);
        s.push_packet(&aim(5, 510_000, -4000, 0), 0.51);
        s.push_packet(&aim(6, 520_000, -4000, 0), 0.52);
        let shown = run_ticks(&mut s, 0.5, 0.1).unwrap_or((1000.0, 500.0));
        assert!(
            (shown.0 - 1000.0).abs() < 2.0,
            "re-clutch must not jump, got {shown:?}"
        );
    }
}
