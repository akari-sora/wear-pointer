//! Laser projection of aim angles onto a flat screen.
//!
//! Adapted from WatchPointer (Copyright (c) 2026 Umar Afzal, MIT).
//! `y` grows downward, which matches Wayland and the usual desktop pointer.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub width: f64,
    pub height: f64,
}

impl Screen {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// Degrees of turn from center to the left or right edge at `speed == 1`.
pub fn reach_degrees(speed: f64) -> f64 {
    let speed = if speed <= 0.0 { 1.0 } else { speed };
    25.0 / speed
}

/// `left_deg` positive means the watch has turned left. `up_deg` positive
/// means the 12 o'clock edge has tipped up. Both are clamped to ±80°.
pub fn project_aim(left_deg: f64, up_deg: f64, screen: Screen, reach_deg: f64) -> (f64, f64) {
    let reach = if reach_deg.abs() < 1e-3 {
        25.0
    } else {
        reach_deg
    };
    let h = (-left_deg).clamp(-80.0, 80.0).to_radians();
    let v = up_deg.clamp(-80.0, 80.0).to_radians();
    let pixels_per_unit = screen.width / 2.0 / reach.to_radians().tan();
    let x = screen.width / 2.0 + h.tan() * pixels_per_unit;
    let y = screen.height / 2.0 - v.tan() / h.cos() * pixels_per_unit;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Screen {
        Screen::new(1000.0, 500.0)
    }

    #[test]
    fn zero_aim_is_the_center() {
        let (x, y) = project_aim(0.0, 0.0, screen(), reach_degrees(1.0));
        assert!((x - 500.0).abs() < 1e-6);
        assert!((y - 250.0).abs() < 1e-6);
    }

    #[test]
    fn turning_left_and_tipping_up_move_toward_the_top_left() {
        let (x, y) = project_aim(10.0, 10.0, screen(), reach_degrees(1.0));
        assert!(x < 500.0, "left turn should decrease x, got {x}");
        assert!(y < 250.0, "tip up should decrease y, got {y}");
    }

    #[test]
    fn higher_speed_reaches_farther_for_the_same_angle() {
        let (slow_x, _) = project_aim(10.0, 0.0, screen(), reach_degrees(1.0));
        let (fast_x, _) = project_aim(10.0, 0.0, screen(), reach_degrees(2.0));
        assert!(fast_x < slow_x);
        assert!((reach_degrees(2.0) - 12.5).abs() < 1e-9);
    }
}
