//! Relative pointing: the air-mouse (gyro rate) and watch-touchpad transfer
//! functions.
//!
//! Absolute mode (`project.rs`) maps the arm pose to a screen position, like a
//! laser. The two relative modes here turn *motion* into cursor deltas with a
//! speed-dependent gain (pointer ballistics), the way Gyration / MX Air class
//! air mice and laptop touchpads do:
//!
//! - slow movement gets a low gain, so small corrections and wrist tremor
//!   barely move the cursor;
//! - fast movement gets a high gain, so a short flick still crosses the screen;
//! - nothing is integrated against a fixed origin, so yaw drift is irrelevant
//!   and the cursor never needs a Center to line up again (pinning against a
//!   screen edge re-aligns it, like lifting a mouse).
//!
//! Both work from *cumulative* values on the wire (aim angles, finger travel),
//! so a lost or coalesced packet only makes the next delta larger. No motion
//! is lost.
//!
//! The sigmoid gain follows Nancel et al., "Mid-air pointing on ultra-walls"
//! (TOCHI 2015); see also Casiez & Roussel, UIST 2011.

use crate::packet::{DEFAULT_DPI, MODE_AIR, MODE_TOUCH};

/// Pointing model. `Absolute` is WatchPointer's laser projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlMode {
    Absolute,
    Air,
    Touch,
}

impl ControlMode {
    pub fn from_wire(value: u8) -> Self {
        match value {
            MODE_AIR => ControlMode::Air,
            MODE_TOUCH => ControlMode::Touch,
            _ => ControlMode::Absolute,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "absolute" | "abs" | "laser" => Some(ControlMode::Absolute),
            "air" | "relative" | "gyro" => Some(ControlMode::Air),
            "touch" | "touchpad" | "trackpad" => Some(ControlMode::Touch),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ControlMode::Absolute => "absolute",
            ControlMode::Air => "air",
            ControlMode::Touch => "touch",
        }
    }

    pub fn is_relative(self) -> bool {
        !matches!(self, ControlMode::Absolute)
    }
}

/// Screen-size and DPI scale shared by both relative modes, so the watch DPI
/// knob keeps working and a 4K screen does not feel slower than 1080p.
pub fn gain_scale(screen_width: f64, dpi: u16) -> f64 {
    let width = if screen_width > 1.0 {
        screen_width
    } else {
        1920.0
    };
    (width / 1920.0) * (f64::from(dpi) / f64::from(DEFAULT_DPI))
}

fn smoothstep01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Air mouse: cursor delta from the change in wrist yaw / elevation.
#[derive(Clone, Debug, Default)]
pub struct AirMouse {
    prev: Option<(f64, f64)>,
    speed_ema: f64,
}

impl AirMouse {
    /// Gain while moving slowly, pixels per degree at 1080p / 1600 DPI.
    pub const GAIN_MIN: f64 = 10.0;
    /// Gain for a fast flick, pixels per degree.
    pub const GAIN_MAX: f64 = 70.0;
    /// Speed at the middle of the sigmoid, degrees per second.
    pub const SPEED_MID: f64 = 45.0;
    /// Sigmoid width, degrees per second.
    pub const SPEED_WIDTH: f64 = 12.0;
    /// Below this smoothed speed nothing moves (°/s). Above `DEAD_FULL` the
    /// motion passes in full. The ramp is radial, so direction is preserved.
    pub const DEAD_START: f64 = 0.8;
    pub const DEAD_FULL: f64 = 1.8;
    /// One packet turning more than this is a discontinuity (watch rebase,
    /// sensor gap), not a hand movement.
    pub const MAX_STEP_DEG: f64 = 30.0;
    pub const MAX_SPEED: f64 = 1000.0;
    const SPEED_TAU_S: f64 = 0.02;

    pub fn reset(&mut self) {
        self.prev = None;
        self.speed_ema = 0.0;
    }

    /// Speed-dependent gain in pixels per degree, before `gain_scale`.
    pub fn gain(speed_deg_s: f64) -> f64 {
        Self::GAIN_MIN
            + (Self::GAIN_MAX - Self::GAIN_MIN)
                / (1.0 + (-(speed_deg_s - Self::SPEED_MID) / Self::SPEED_WIDTH).exp())
    }

    /// `left_deg` / `up_deg` are the absolute aim angles from the packet.
    /// `dt` is the watch-clock gap to the previous aim, or `None` when unknown
    /// (first packet, after a silence). Returns the pixel delta.
    pub fn step(&mut self, left_deg: f64, up_deg: f64, dt: Option<f64>, scale: f64) -> (f64, f64) {
        let Some((prev_left, prev_up)) = self.prev.replace((left_deg, up_deg)) else {
            return (0.0, 0.0);
        };
        let Some(dt) = dt else {
            self.speed_ema = 0.0;
            return (0.0, 0.0);
        };
        let dt = dt.clamp(0.002, 0.05);
        let mut d_left = left_deg - prev_left;
        if d_left > 180.0 {
            d_left -= 360.0;
        } else if d_left < -180.0 {
            d_left += 360.0;
        }
        let d_up = up_deg - prev_up;
        let step = d_left.hypot(d_up);
        if step > Self::MAX_STEP_DEG || step / dt > Self::MAX_SPEED {
            self.speed_ema = 0.0;
            return (0.0, 0.0);
        }
        let alpha = dt / (Self::SPEED_TAU_S + dt);
        self.speed_ema += alpha * (step / dt - self.speed_ema);
        let s = self.speed_ema;
        let pass = smoothstep01((s - Self::DEAD_START) / (Self::DEAD_FULL - Self::DEAD_START));
        if pass <= 0.0 {
            return (0.0, 0.0);
        }
        let g = Self::gain(s) * scale * pass;
        // Turning left is +left; the cursor goes left (−x). Tipping up is +up;
        // screen y grows downward.
        (-d_left * g, -d_up * g)
    }
}

/// Watch touchpad: cursor delta from cumulative finger travel.
#[derive(Clone, Debug, Default)]
pub struct TouchPad {
    prev: Option<(i16, i16)>,
    speed_ema: f64,
}

impl TouchPad {
    /// Gain for slow strokes, pixels per millimetre of finger travel.
    pub const GAIN_SLOW: f64 = 6.0;
    /// Gain for fast flicks, pixels per millimetre.
    pub const GAIN_FAST: f64 = 45.0;
    /// Finger speeds (mm/s) where the gain starts / stops rising.
    pub const SPEED_SLOW: f64 = 15.0;
    pub const SPEED_FAST: f64 = 150.0;
    /// More than this in one packet is a discontinuity, not a stroke.
    pub const MAX_STEP_MM: f64 = 60.0;
    const SPEED_TAU_S: f64 = 0.03;

    pub fn reset(&mut self) {
        self.prev = None;
        self.speed_ema = 0.0;
    }

    pub fn gain(speed_mm_s: f64) -> f64 {
        let t = ((speed_mm_s - Self::SPEED_SLOW) / (Self::SPEED_FAST - Self::SPEED_SLOW))
            .clamp(0.0, 1.0);
        Self::GAIN_SLOW + (Self::GAIN_FAST - Self::GAIN_SLOW) * t
    }

    /// `x_acc` / `y_acc` are cumulative hundredths of a millimetre (wrapping).
    /// Finger right is +x, finger toward 6 o'clock is +y (screen down).
    pub fn step(&mut self, x_acc: i16, y_acc: i16, dt: Option<f64>, scale: f64) -> (f64, f64) {
        let Some((px, py)) = self.prev.replace((x_acc, y_acc)) else {
            return (0.0, 0.0);
        };
        let dx_mm = f64::from(x_acc.wrapping_sub(px)) / 100.0;
        let dy_mm = f64::from(y_acc.wrapping_sub(py)) / 100.0;
        let step = dx_mm.hypot(dy_mm);
        if step == 0.0 {
            // Heartbeat while the finger rests: let the speed decay.
            if let Some(dt) = dt {
                let dt = dt.clamp(0.002, 0.05);
                self.speed_ema *= 1.0 - dt / (Self::SPEED_TAU_S + dt);
            }
            return (0.0, 0.0);
        }
        if step > Self::MAX_STEP_MM {
            self.speed_ema = 0.0;
            return (0.0, 0.0);
        }
        let dt = dt.unwrap_or(0.01).clamp(0.002, 0.05);
        let alpha = dt / (Self::SPEED_TAU_S + dt);
        self.speed_ema += alpha * (step / dt - self.speed_ema);
        let g = Self::gain(self.speed_ema) * scale;
        (dx_mm * g, dy_mm * g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parse_and_wire_round_trip() {
        assert_eq!(ControlMode::parse("AIR"), Some(ControlMode::Air));
        assert_eq!(ControlMode::parse("touchpad"), Some(ControlMode::Touch));
        assert_eq!(ControlMode::parse("absolute"), Some(ControlMode::Absolute));
        assert_eq!(ControlMode::parse("joystick"), None);
        assert_eq!(ControlMode::from_wire(MODE_AIR), ControlMode::Air);
        assert_eq!(ControlMode::from_wire(MODE_TOUCH), ControlMode::Touch);
        assert_eq!(ControlMode::from_wire(42), ControlMode::Absolute);
    }

    #[test]
    fn air_gain_is_low_when_slow_and_high_when_fast() {
        assert!(AirMouse::gain(5.0) < 13.0);
        assert!((AirMouse::gain(45.0) - 40.0).abs() < 1e-9);
        assert!(AirMouse::gain(150.0) > 69.0);
    }

    /// Feed a steady turn at `deg_s` for one second at 100 Hz; return total px.
    fn sweep(deg_s: f64) -> f64 {
        let mut air = AirMouse::default();
        let mut left = 0.0;
        let mut total = 0.0;
        air.step(left, 0.0, None, 1.0);
        for _ in 0..100 {
            left -= deg_s * 0.01; // turning right
            total += air.step(left, 0.0, Some(0.01), 1.0).0;
        }
        total
    }

    #[test]
    fn air_turning_right_moves_right_with_acceleration() {
        let slow = sweep(10.0); // 10° of turn
        let fast = sweep(120.0); // 120° of turn
        assert!(slow > 0.0, "a right turn moves right, got {slow}");
        // Per degree, the fast sweep must travel several times farther.
        assert!(fast / 120.0 > 4.0 * slow / 10.0, "slow {slow} fast {fast}");
        // A 30° flick at speed covers about a 1080p screen width.
        assert!(sweep(120.0) / 4.0 > 1500.0);
    }

    #[test]
    fn air_ignores_sensor_noise_while_still() {
        let mut air = AirMouse::default();
        let mut moved = 0.0;
        air.step(0.0, 0.0, None, 1.0);
        // The aim is quantized to 0.01°. A watch lying still flips one
        // quantum now and then (measured on a Watch7: p90 ≈ 1 °/s per sample).
        for i in 0..500 {
            let jitter = if i % 3 == 0 { 0.01 } else { 0.0 };
            let (dx, dy) = air.step(jitter, 0.0, Some(0.01), 1.0);
            moved += dx.abs() + dy.abs();
        }
        assert!(moved < 1.0, "still watch walked {moved} px");
    }

    #[test]
    fn air_drops_a_rebase_jump_and_wraps_heading() {
        let mut air = AirMouse::default();
        air.step(10.0, 5.0, None, 1.0);
        assert_eq!(air.step(-40.0, 5.0, Some(0.01), 1.0), (0.0, 0.0));
        // Crossing ±180° is a small move, not a 359° jump.
        let mut air = AirMouse::default();
        air.step(179.9, 0.0, None, 1.0);
        let (dx, _) = air.step(-179.9, 0.0, Some(0.01), 1.0);
        assert!(dx.abs() < 20.0, "wrap produced {dx}");
    }

    #[test]
    fn touch_follows_the_finger_and_accelerates() {
        let mut pad = TouchPad::default();
        pad.step(0, 0, None, 1.0);
        // 10 mm slowly (10 mm/s): ~6 px/mm.
        let mut acc: i16 = 0;
        let mut slow = 0.0;
        for _ in 0..100 {
            acc = acc.wrapping_add(10);
            slow += pad.step(acc, 0, Some(0.01), 1.0).0;
        }
        assert!((50.0..80.0).contains(&slow), "slow stroke {slow}");
        // 20 mm fast (200 mm/s) goes much farther.
        let mut pad = TouchPad::default();
        pad.step(0, 0, None, 1.0);
        let mut acc: i16 = 0;
        let mut fast = 0.0;
        for _ in 0..10 {
            acc = acc.wrapping_add(200);
            fast += pad.step(acc, 0, Some(0.01), 1.0).0;
        }
        assert!(fast > 400.0, "fast flick {fast}");
    }

    #[test]
    fn touch_survives_wrap_and_coalescing() {
        let mut pad = TouchPad::default();
        pad.step(i16::MAX - 50, 0, None, 1.0);
        let (dx, _) = pad.step(i16::MIN + 49, 0, Some(0.01), 1.0); // +1.00 mm
        assert!(dx > 0.0 && dx < 50.0, "wrap gave {dx}");
    }

    #[test]
    fn gain_scale_tracks_screen_and_dpi() {
        assert!((gain_scale(1920.0, 1600) - 1.0).abs() < 1e-9);
        assert!((gain_scale(3840.0, 1600) - 2.0).abs() < 1e-9);
        assert!((gain_scale(1920.0, 3200) - 2.0).abs() < 1e-9);
    }
}
