//! Keep the fractional part of a relative axis so sub-pixel motion and
//! small scroll steps add up instead of rounding away.

pub fn take_steps(pending: &mut f64, delta: f64) -> i32 {
    *pending += delta;
    let steps = pending.round() as i32;
    *pending -= f64::from(steps);
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_pixel_motion_accumulates_past_one_pixel() {
        let mut pending = 0.0;
        assert_eq!(take_steps(&mut pending, 0.4), 0);
        assert_eq!(take_steps(&mut pending, 0.4), 1);
        assert!(pending.abs() < 0.3);
    }

    #[test]
    fn thirds_of_a_wheel_step_add_up() {
        let mut pending = 0.0;
        assert_eq!(take_steps(&mut pending, 10.0 / 30.0), 0);
        assert_eq!(take_steps(&mut pending, 10.0 / 30.0), 1);
    }
}
