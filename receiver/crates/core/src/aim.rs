//! Gravity elevation + yaw about vertical.
//!
//! Adapted from WatchPointer (Copyright (c) 2026 Umar Afzal, MIT).
//!
//! Android's gravity vector points the opposite way from Apple's
//! `CMDeviceMotion.gravity`. WatchPointer uses `up = -gravity`. On Android,
//! when an axis points at the sky the gravity sensor reads about +9.81 on
//! that axis, so `up = normalize(gravity)` is the same physical direction.
//!
//! Elevation is `asin(up.y)`. Device +Y is the 12 o'clock edge, so this is
//! how far that edge sits above the horizon, and it does not drift.
//! Heading integrates `dot(gyro, up) * dt`. Positive rotation about the
//! upward axis is a turn to the left (right-hand rule). There is no absolute
//! yaw reference.
//!
//! `dead_zone_rad_s` drops yaw rates slower than the threshold. WatchPointer
//! ships this at 0: a dead zone made the cursor walk out of line. Keep the
//! default at 0 unless a Stage 0 log shows a reason to raise it.

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

#[derive(Clone, Debug)]
pub struct AimTracker {
    heading: f64,
    elevation: f64,
    elevation_zero: f64,
    last_t: Option<f64>,
    dead_zone_rad_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimSample {
    /// Radians turned left since Center.
    pub heading: f64,
    /// Radians the 12 o'clock edge is above the Center elevation.
    pub elevation_rel: f64,
}

impl AimTracker {
    pub fn new(dead_zone_rad_s: f64) -> Self {
        Self {
            heading: 0.0,
            elevation: 0.0,
            elevation_zero: 0.0,
            last_t: None,
            dead_zone_rad_s,
        }
    }

    /// The pose right now becomes the middle of the screen.
    pub fn center(&mut self) {
        self.heading = 0.0;
        self.elevation_zero = self.elevation;
    }

    /// `t` is seconds on a monotonic clock. `gravity` is m/s² in the device
    /// frame (Android sign). `gyro` is rad/s.
    pub fn push(&mut self, t: f64, gravity: [f64; 3], gyro: [f64; 3]) -> AimSample {
        let dt = match self.last_t {
            Some(prev) => (t - prev).clamp(0.0, 0.05),
            None => 0.0,
        };
        self.last_t = Some(t);

        let norm = (gravity[0] * gravity[0] + gravity[1] * gravity[1] + gravity[2] * gravity[2])
            .sqrt()
            .max(1e-6);
        let up = [gravity[0] / norm, gravity[1] / norm, gravity[2] / norm];
        self.elevation = up[1].clamp(-1.0, 1.0).asin();

        let mut yaw = gyro[0] * up[0] + gyro[1] * up[1] + gyro[2] * up[2];
        if yaw.abs() < self.dead_zone_rad_s {
            yaw = 0.0;
        }
        self.heading += yaw * dt;

        AimSample {
            heading: self.heading,
            elevation_rel: self.elevation - self.elevation_zero,
        }
    }

    pub fn left_cdeg(&self) -> i16 {
        cdeg(signed_remainder(self.heading, TWO_PI))
    }

    pub fn up_cdeg(&self) -> i16 {
        cdeg(self.elevation - self.elevation_zero)
    }
}

fn cdeg(radians: f64) -> i16 {
    let hundredths = (radians * 180.0 / std::f64::consts::PI * 100.0).round();
    hundredths.clamp(i16::MIN as f64, i16::MAX as f64) as i16
}

fn signed_remainder(value: f64, modulus: f64) -> f64 {
    let mut r = value % modulus;
    if r > modulus / 2.0 {
        r -= modulus;
    }
    if r < -modulus / 2.0 {
        r += modulus;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_about_vertical_accumulates_when_face_up() {
        let mut tracker = AimTracker::new(0.0);
        let g = [0.0, 0.0, 9.81];
        tracker.push(0.0, g, [0.0, 0.0, 0.0]);
        let sample = tracker.push(0.01, g, [0.0, 0.0, 0.2]);
        assert!((sample.heading - 0.002).abs() < 1e-9);
        assert!(sample.elevation_rel.abs() < 1e-9);
        // 0.002 rad = 11.459 hundredths of a degree.
        assert_eq!(tracker.left_cdeg(), 11);
        assert_eq!(tracker.up_cdeg(), 0);
    }

    #[test]
    fn twelve_oclock_pointing_up_is_ninety_degrees() {
        let mut tracker = AimTracker::new(0.0);
        tracker.push(0.0, [0.0, 9.81, 0.0], [0.0, 0.0, 0.0]);
        assert_eq!(tracker.up_cdeg(), 9000);
    }

    #[test]
    fn dead_zone_drops_slow_yaw() {
        let mut tracker = AimTracker::new(1.0);
        let g = [0.0, 0.0, 9.81];
        tracker.push(0.0, g, [0.0, 0.0, 0.0]);
        let sample = tracker.push(0.01, g, [0.0, 0.0, 0.2]);
        assert_eq!(sample.heading, 0.0);
    }

    #[test]
    fn center_zeroes_heading_and_elevation() {
        let mut tracker = AimTracker::new(0.0);
        tracker.push(0.0, [0.0, 9.81, 0.0], [0.0, 0.0, 0.0]);
        tracker.center();
        assert_eq!(tracker.left_cdeg(), 0);
        assert_eq!(tracker.up_cdeg(), 0);
    }
}
