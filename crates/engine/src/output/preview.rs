use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, time::Instant};

const SAMPLE_MS: u64 = 8;
const RECENT_POINTS: usize = 40;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct MouseTracePoint {
    pub sequence: u64,
    pub time_ms: u64,
    /// Cumulative relative output counts, positive right and down.
    pub position: [i64; 2],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OutputPreview {
    pub generation: u64,
    pub mouse: Vec<MouseTracePoint>,
    pub pad: Option<crate::engine::PadState>,
}

/// Coalesces successful reports into display samples without touching their
/// delivery cadence. A short overlapping tail tolerates skipped UI snapshots.
pub(crate) struct MouseTrace {
    started: Instant,
    position: [i64; 2],
    points: VecDeque<MouseTracePoint>,
}

impl Default for MouseTrace {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            position: [0; 2],
            points: VecDeque::with_capacity(RECENT_POINTS),
        }
    }
}

impl MouseTrace {
    pub(crate) fn clear(&mut self) {
        self.started = Instant::now();
        self.position = [0; 2];
        self.points.clear();
    }

    pub(crate) fn record(&mut self, counts: [i32; 2], at: Instant) {
        if counts == [0; 2] {
            return;
        }
        let elapsed = at.saturating_duration_since(self.started).as_millis() as u64;
        let time_ms = elapsed / SAMPLE_MS * SAMPLE_MS;
        if self.points.is_empty() {
            self.points.push_back(MouseTracePoint {
                sequence: 0,
                time_ms,
                position: self.position,
            });
        }
        for (axis, count) in self.position.iter_mut().zip(counts) {
            *axis = axis.saturating_add(i64::from(count));
        }
        let last = self.points.back_mut().unwrap();
        if last.sequence > 0 && last.time_ms == time_ms {
            last.position = self.position;
            return;
        }
        let sequence = last.sequence + 1;
        if self.points.len() == RECENT_POINTS {
            self.points.pop_front();
        }
        self.points.push_back(MouseTracePoint {
            sequence,
            time_ms,
            position: self.position,
        });
    }

    pub(crate) fn points(&self) -> Vec<MouseTracePoint> {
        self.points.iter().copied().collect()
    }
}
