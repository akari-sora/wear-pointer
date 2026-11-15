//! Packets in, cursor actions out.
//!
//! Aim samples update a filtered target. A separate 240 Hz tick eases the
//! shown cursor toward that target. Clicks rewind to where the cursor was
//! when the finger went down. Adapted from WatchPointer (Copyright (c) 2026
//! Umar Afzal, MIT).

use crate::filter::OneEuroFilter;
use crate::packet::{decode, Packet, DEFAULT_DPI, MAX_DPI, MIN_DPI};
use crate::project::{dpi_to_speed, project_aim, reach_degrees, speed_to_dpi, Screen};
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
            link_open: true,
            history: Vec::new(),
        }
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
            matches!(packet, Packet::Aim { .. }),
        );
        if timing.duplicate {
            return Vec::new();
        }
        match packet {
            Packet::Aim {
                left_cdeg, up_cdeg, ..
            } => self.push_aim(left_cdeg, up_cdeg, local_s, timing.fresh, timing.watch_dt_s),
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

    fn center(&mut self) -> Vec<CursorAction> {
        self.filter_x.reset();
        self.filter_y.reset();
        self.last_aim_deg = None;
        let center = self.center_point();
        self.target = Some(center);
        self.follower.snap(center);
        vec![CursorAction::Move {
            x: center.0,
            y: center.1,
        }]
    }

    fn set_paused(&mut self, paused: bool) -> Vec<CursorAction> {
        self.paused = paused;
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
        cursor_x: session.follower.shown.map(|p| p.0),
        cursor_y: session.follower.shown.map(|p| p.1),
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
        encode, header, Packet, KIND_AIM, KIND_DRAG, KIND_LEFT_CLICK, KIND_PAUSE, KIND_SCROLL,
        KIND_SENSITIVITY,
    };

    fn session() -> PointerSession {
        let mut config = SessionConfig::default();
        config.screen = Screen::new(2000.0, 1000.0);
        config.steady_hz = 0.8;
        config.quickness = 0.0;
        PointerSession::new(config)
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
        let mut config = SessionConfig::default();
        config.screen = Screen::new(2000.0, 1000.0);
        config.dead_zone_deg = 1.0;
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
        let mut cfg = SessionConfig::default();
        cfg.speed = 2.0;
        cfg.cli_speed_override = Some(2.0);
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
}
