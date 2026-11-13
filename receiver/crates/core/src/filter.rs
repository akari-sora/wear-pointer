//! 1€ filter (Casiez, Roussel, Vogel, CHI 2012).
//!
//! Heavy smoothing when the hand is nearly still, almost none when it moves
//! quickly. Parameter defaults and the decision to drive `dt` from watch
//! timestamps come from WatchPointer (Copyright (c) 2026 Umar Afzal, MIT).

/// `min_cutoff` is the cutoff in Hz while still. `beta` raises the cutoff in
/// proportion to speed. `d_cutoff` smooths the speed estimate.
#[derive(Clone, Debug)]
pub struct OneEuroFilter {
    pub min_cutoff: f64,
    pub beta: f64,
    pub d_cutoff: f64,
    value: Option<f64>,
    velocity: f64,
}

impl OneEuroFilter {
    pub fn new(min_cutoff: f64, beta: f64) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff: 1.0,
            value: None,
            velocity: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.value = None;
        self.velocity = 0.0;
    }

    /// `dt` is seconds since the previous sample. `dt <= 0` snaps to `x`.
    pub fn filter(&mut self, x: f64, dt: f64) -> f64 {
        let Some(previous) = self.value else {
            self.value = Some(x);
            return x;
        };
        if dt <= 0.0 {
            self.value = Some(x);
            self.velocity = 0.0;
            return x;
        }
        let alpha = |cutoff: f64| 1.0 / (1.0 + 1.0 / (2.0 * std::f64::consts::PI * cutoff * dt));
        let raw_velocity = (x - previous) / dt;
        self.velocity += alpha(self.d_cutoff) * (raw_velocity - self.velocity);
        let cutoff = self.min_cutoff + self.beta * self.velocity.abs();
        let smoothed = previous + alpha(cutoff) * (x - previous);
        self.value = Some(smoothed);
        smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_and_constant_input_pass_through() {
        let mut filter = OneEuroFilter::new(0.8, 0.01);
        assert_eq!(filter.filter(3.0, 0.0), 3.0);
        assert_eq!(filter.filter(3.0, 0.01), 3.0);
    }

    #[test]
    fn higher_beta_tracks_a_step_more_closely() {
        let mut slow = OneEuroFilter::new(1.0, 0.0);
        let mut fast = OneEuroFilter::new(1.0, 1.0);
        for _ in 0..50 {
            slow.filter(0.0, 0.01);
            fast.filter(0.0, 0.01);
        }
        let slow_step = slow.filter(100.0, 0.01);
        let fast_step = fast.filter(100.0, 0.01);
        assert!(slow_step < 20.0, "slow filter should lag, got {slow_step}");
        assert!(
            fast_step > 80.0,
            "fast filter should catch the step, got {fast_step}"
        );
        assert!(fast_step > slow_step);
    }
}
