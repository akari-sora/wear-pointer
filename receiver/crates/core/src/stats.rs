//! Sequence loss and relative one-way delay.
//!
//! Latency uses WatchPointer's clock estimate: the offset is the minimum of
//! `local_time - unwrapped_watch_time` over packets seen so far (the one that
//! spent the least time in the radio). Reported latency is the excess over
//! that minimum. It is not an NTP-style absolute delay. The first packet of a
//! connection therefore reads about 0 ms.

use crate::FRESH_GAP_S;

#[derive(Clone, Debug)]
pub struct Timing {
    /// No aim sample for `FRESH_GAP_S`, or this is the first aim.
    pub fresh: bool,
    /// Seconds of watch time since the previous aim. `None` when `fresh`.
    pub watch_dt_s: Option<f64>,
    /// Milliseconds above the fastest packet seen on this link.
    pub latency_ms: f64,
    /// Packets missing between this sequence and the previous one.
    pub lost: u32,
}

#[derive(Clone, Debug)]
pub struct LinkStats {
    pub packets: u64,
    pub lost: u64,
    last_seq: Option<u16>,
    last_local_s: Option<f64>,
    unwrapped_us: u64,
    last_watch_us: Option<u32>,
    offset_s: Option<f64>,
    last_aim_us: Option<u32>,
    last_aim_local_s: Option<f64>,
    window_packets: u32,
    window_lost: u32,
    latency_sum_ms: f64,
    latency_min_ms: f64,
    latency_max_ms: f64,
    latency_count: u32,
    pub remote_samples: Option<u16>,
    pub remote_notify_ok: Option<u16>,
    pub remote_notify_drop: Option<u16>,
}

impl Default for LinkStats {
    fn default() -> Self {
        Self {
            packets: 0,
            lost: 0,
            last_seq: None,
            last_local_s: None,
            unwrapped_us: 0,
            last_watch_us: None,
            offset_s: None,
            last_aim_us: None,
            last_aim_local_s: None,
            window_packets: 0,
            window_lost: 0,
            latency_sum_ms: 0.0,
            latency_min_ms: f64::MAX,
            latency_max_ms: 0.0,
            latency_count: 0,
            remote_samples: None,
            remote_notify_ok: None,
            remote_notify_drop: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WindowStats {
    pub packets: u32,
    pub lost: u32,
    pub latency_avg_ms: f64,
    pub latency_min_ms: f64,
    pub latency_max_ms: f64,
}

impl LinkStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// `is_aim` selects the dt used by the 1€ filter. Every packet updates
    /// loss, the shared clock, and the latency estimate.
    pub fn observe(&mut self, sequence: u16, watch_us: u32, local_s: f64, is_aim: bool) -> Timing {
        let lost = match self.last_seq {
            Some(prev) => {
                let gap = sequence.wrapping_sub(prev);
                if gap == 0 {
                    0
                } else {
                    u32::from(gap) - 1
                }
            }
            None => 0,
        };
        self.last_seq = Some(sequence);
        self.packets += 1;
        self.lost += u64::from(lost);
        self.window_packets += 1;
        self.window_lost += lost;

        if let Some(prev) = self.last_watch_us {
            self.unwrapped_us = self
                .unwrapped_us
                .wrapping_add(u64::from(watch_us.wrapping_sub(prev)));
        } else {
            self.unwrapped_us = u64::from(watch_us);
        }
        self.last_watch_us = Some(watch_us);
        self.last_local_s = Some(local_s);

        let watch_s = self.unwrapped_us as f64 / 1_000_000.0;
        let sample_offset = local_s - watch_s;
        self.offset_s = Some(match self.offset_s {
            Some(current) => current.min(sample_offset),
            None => sample_offset,
        });
        let latency_ms = (sample_offset - self.offset_s.unwrap_or(sample_offset)) * 1000.0;
        self.latency_sum_ms += latency_ms;
        self.latency_min_ms = self.latency_min_ms.min(latency_ms);
        self.latency_max_ms = self.latency_max_ms.max(latency_ms);
        self.latency_count += 1;

        let fresh = match self.last_aim_local_s {
            Some(prev) => local_s - prev > FRESH_GAP_S,
            None => true,
        };
        let watch_dt_s = if is_aim && !fresh {
            self.last_aim_us
                .map(|prev| f64::from(watch_us.wrapping_sub(prev)) / 1_000_000.0)
        } else {
            None
        };
        if is_aim {
            self.last_aim_us = Some(watch_us);
            self.last_aim_local_s = Some(local_s);
        }

        Timing {
            fresh: is_aim && fresh,
            watch_dt_s,
            latency_ms,
            lost,
        }
    }

    /// Map a watch timestamp near the latest packet onto local seconds.
    pub fn watch_to_local(&self, watch_us: u32) -> Option<f64> {
        let last = self.last_watch_us?;
        let offset = self.offset_s?;
        let signed = watch_us.wrapping_sub(last) as i32;
        let watch_s = self.unwrapped_us as f64 / 1_000_000.0 + f64::from(signed) / 1_000_000.0;
        Some(watch_s + offset)
    }

    pub fn take_window(&mut self) -> WindowStats {
        let avg = if self.latency_count == 0 {
            0.0
        } else {
            self.latency_sum_ms / f64::from(self.latency_count)
        };
        let min = if self.latency_count == 0 {
            0.0
        } else {
            self.latency_min_ms
        };
        let stats = WindowStats {
            packets: self.window_packets,
            lost: self.window_lost,
            latency_avg_ms: avg,
            latency_min_ms: min,
            latency_max_ms: self.latency_max_ms,
        };
        self.window_packets = 0;
        self.window_lost = 0;
        self.latency_sum_ms = 0.0;
        self.latency_min_ms = f64::MAX;
        self.latency_max_ms = 0.0;
        self.latency_count = 0;
        stats
    }

    pub fn note_remote(&mut self, samples: u16, notifies_ok: u16, notifies_drop: u16) {
        self.remote_samples = Some(samples);
        self.remote_notify_ok = Some(notifies_ok);
        self.remote_notify_drop = Some(notifies_drop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_gaps_count_as_loss_and_wrap_does_not() {
        let mut stats = LinkStats::new();
        assert_eq!(stats.observe(1, 0, 1.0, true).lost, 0);
        assert_eq!(stats.observe(2, 10_000, 1.01, true).lost, 0);
        assert_eq!(stats.observe(5, 20_000, 1.02, true).lost, 2);
        assert_eq!(stats.lost, 2);
        assert_eq!(stats.observe(5, 20_000, 1.02, true).lost, 0);
        assert_eq!(stats.observe(0, 30_000, 1.03, true).lost, 65530);
        // 5 -> 0 is a wrap of a u16: gap = 65531, lost = 65530. That is the
        // honest reading of a jump. A real wrap of one step is seq 65535 -> 0.
        let mut wrapped = LinkStats::new();
        wrapped.observe(65535, 0, 1.0, true);
        assert_eq!(wrapped.observe(0, 10_000, 1.01, true).lost, 0);
        assert_eq!(wrapped.observe(2, 20_000, 1.02, true).lost, 1);
    }

    #[test]
    fn latency_is_relative_to_the_fastest_packet() {
        let mut stats = LinkStats::new();
        let first = stats.observe(1, 0, 10.0, true);
        assert!(first.latency_ms.abs() < 1e-6);
        let second = stats.observe(2, 10_000, 10.020, true);
        assert!((second.latency_ms - 10.0).abs() < 1e-6);
        assert!((second.watch_dt_s.unwrap() - 0.010).abs() < 1e-9);
    }

    #[test]
    fn click_timestamp_maps_back_before_the_latest_aim() {
        let mut stats = LinkStats::new();
        stats.observe(1, 1_000_000, 5.0, true);
        let local = stats.watch_to_local(980_000).unwrap();
        // 20 ms of watch time before the aim, plus the offset of 4.0 s
        // (local 5.0 - watch 1.0). 980_000 us = 0.98 s, + 4.0 = 4.98.
        assert!((local - 4.98).abs() < 1e-6);
    }
}
