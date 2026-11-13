//! Packets in, cursor actions out.
//!
//! Aim samples update a filtered target. A separate 240 Hz tick eases the
//! shown cursor toward that target. Clicks rewind to where the cursor was
//! when the finger went down. Adapted from WatchPointer (Copyright (c) 2026
//! Umar Afzal, MIT).

use crate::filter::OneEuroFilter;
use crate::packet::{decode, Packet};
use crate::project::{project_aim, reach_degrees, Screen};
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
    pub cursor_x: Option<f64>,
    pub cursor_y: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub speed: f64,
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
            speed: 1.0,
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
            history: Vec::new(),
        }
    }

    pub fn set_screen(&mut self, screen: Screen) {
        self.config.screen = screen;
    }

    pub fn screen(&self) -> Screen {
        self.config.screen
    }

    pub fn push_packet(&mut self, bytes: &[u8], local_s: f64) -> Vec<CursorAction> {
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
                ..
            } => {
                self.stats.note_remote(samples, notifies_ok, notifies_drop);
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
        let raw = project_aim(
            left,
            up,
            self.config.screen,
            reach_degrees(self.config.speed),
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
        if paused && self.dragging {
            self.dragging = false;
            vec![CursorAction::Button {
                button: Button::Left,
                down: false,
            }]
        } else {
            Vec::new()
        }
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
        cursor_x: session.follower.shown.map(|p| p.0),
        cursor_y: session.follower.shown.map(|p| p.1),
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
    use crate::packet::{
        encode, header, Packet, KIND_AIM, KIND_LEFT_CLICK, KIND_PAUSE, KIND_SCROLL,
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
}
