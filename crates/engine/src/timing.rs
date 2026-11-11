use crate::profile::TimestampSource;
#[cfg(windows)]
pub use flexinput_virtual::display_clock;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
pub use {flexinput_engine::HrWaiter, flexinput_virtual::io_schedule};

pub const MAX_GAP_NS: u64 = 50_000_000;
pub const MAX_EVENT_AGE_NS: u64 = 100_000_000;
pub const CONTINUOUS_OUTPUT_INTERVAL: Duration = Duration::from_millis(2);

/// Carry small overruns forward on the absolute schedule, but skip missed
/// slots instead of issuing back-to-back reports after a stall.
pub fn next_output_deadline(deadline: Instant, now: Instant) -> Instant {
    let next = deadline + CONTINUOUS_OUTPUT_INTERVAL;
    if next <= now {
        now + CONTINUOUS_OUTPUT_INTERVAL
    } else {
        next
    }
}

/// Both clocks are retained. SDL's sensor clock may be synthetic; never subtract
/// it from SDL_GetTicksNS. Host selection is deliberately explicit in the profile.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SampleTime {
    pub host_ns: u64,
    pub sensor_ns: u64,
}

#[derive(Default, Debug, Serialize)]
pub struct ClockStats {
    pub samples: u64,
    pub integrated: u64,
    pub invalid_timestamps: u64,
    pub gaps: u64,
    pub integrated_seconds: f64,
}
pub struct SampleClock {
    source: TimestampSource,
    last: Option<u64>,
    pub stats: ClockStats,
}
impl SampleClock {
    pub fn new(source: TimestampSource) -> Self {
        Self {
            source,
            last: None,
            stats: ClockStats::default(),
        }
    }
    pub fn reset(&mut self) {
        self.last = None;
    }
    pub fn step(&mut self, time: SampleTime) -> Option<f32> {
        self.stats.samples += 1;
        let now = match self.source {
            TimestampSource::Host => time.host_ns,
            TimestampSource::Sensor => time.sensor_ns,
        };
        if now == 0 {
            self.last = None;
            self.stats.invalid_timestamps += 1;
            return None;
        }
        let last = self.last.replace(now)?;
        if now <= last {
            self.stats.invalid_timestamps += 1;
            self.last = None;
            return None;
        }
        let delta = now - last;
        if delta > MAX_GAP_NS {
            self.stats.gaps += 1;
            return None;
        }
        self.stats.integrated += 1;
        self.stats.integrated_seconds += delta as f64 * 1e-9;
        Some(delta as f32 * 1e-9)
    }
}

/// Fixed storage: no per-sample heap allocation. Bucket resolution 1 us, >=4096
/// us goes into overflow; percentiles are bucket upper bounds, not promises.
pub struct Histogram {
    buckets: [u64; 4097],
    count: u64,
    max_ns: u64,
}
impl Default for Histogram {
    fn default() -> Self {
        Self {
            buckets: [0; 4097],
            count: 0,
            max_ns: 0,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Distribution {
    pub count: u64,
    pub p50_us: u64,
    pub p99_us: u64,
    pub max_us: f64,
    pub overflow_4096_us: u64,
}
impl Histogram {
    pub fn add(&mut self, ns: u64) {
        let index = ns.div_ceil(1000).min(4096) as usize;
        self.buckets[index] += 1;
        self.count += 1;
        self.max_ns = self.max_ns.max(ns);
    }
    pub fn summary(&self) -> Distribution {
        let percentile = |p: u64| {
            let target = (self.count * p).div_ceil(100).max(1);
            let mut total = 0;
            for (i, &n) in self.buckets.iter().enumerate() {
                total += n;
                if total >= target {
                    return i as u64;
                }
            }
            0
        };
        Distribution {
            count: self.count,
            p50_us: percentile(50),
            p99_us: percentile(99),
            max_us: self.max_ns as f64 / 1000.0,
            overflow_4096_us: self.buckets[4096],
        }
    }
}
