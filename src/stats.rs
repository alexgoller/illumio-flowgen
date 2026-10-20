//! Run counters plus a short sliding window for the measured flow rate.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct Stats {
    pub flows: u64,
    pub datagrams: u64,
    pub wire_bytes: u64,
    pub send_errors: u64,
    pub scan_bursts: u64,
    pub scan_flows: u64,
    pub per_rule: Vec<u64>,
    pub target_rate: f64,
    window: VecDeque<(Instant, u64)>,
}

impl Stats {
    pub fn new(rule_count: usize) -> Self {
        Self {
            per_rule: vec![0; rule_count],
            ..Self::default()
        }
    }

    /// Record the current flow total at `now`; drops samples older than the window.
    pub fn sample(&mut self, now: Instant) {
        self.window.push_back((now, self.flows));
        while let Some(&(t, _)) = self.window.front() {
            if now.duration_since(t) > WINDOW {
                self.window.pop_front();
            } else {
                break;
            }
        }
    }

    /// Flows per second over the sampled window (0 with fewer than two samples).
    pub fn measured_rate(&self) -> f64 {
        match (self.window.front(), self.window.back()) {
            (Some(&(t0, f0)), Some(&(t1, f1))) if t1 > t0 => {
                (f1 - f0) as f64 / (t1 - t0).as_secs_f64()
            }
            _ => 0.0,
        }
    }
}
