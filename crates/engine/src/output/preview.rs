use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, VecDeque},
    time::{Duration, Instant},
};

pub(crate) const SAMPLE_MS: u64 = 8;
const RECENT_POINTS: usize = 40;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct MouseTracePoint {
    pub sequence: u64,
    pub time_ms: u64,
    /// Cumulative relative output counts, positive right and down.
    pub position: [i64; 2],
    /// Age of this bucket at the snapshot, on the writer's clock.
    #[serde(default)]
    pub age_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OutputPreview {
    pub generation: u64,
    pub mouse: Vec<MouseTracePoint>,
    #[serde(default)]
    pub key_mouse: KeyMousePreview,
    pub pad: Option<crate::engine::PadState>,
    #[serde(default)]
    pub pad_trace: Vec<PadTracePoint>,
    #[serde(default)]
    pub pad_time_ms: u64,
    #[serde(default)]
    pub sample_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyMouseChange {
    Button { name: String, down: bool },
    Wheel { delta: [i32; 2] },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyMouseEvent {
    pub age_ms: u64,
    #[serde(flatten)]
    pub change: KeyMouseChange,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KeyMousePreview {
    pub held: Vec<String>,
    pub recent: Vec<KeyMouseEvent>,
}

/// Keep fast taps between status publications, separately from long-held keys.
/// All timestamps and edges come from successful writes on the same backend.
#[derive(Default)]
pub(crate) struct KeyMouseTrace {
    held: BTreeSet<String>,
    recent: VecDeque<(Instant, KeyMouseChange)>,
}

impl KeyMouseTrace {
    pub(crate) fn button(&mut self, name: &str, down: bool, at: Instant) {
        if self.held.contains(name) == down {
            return;
        }
        if down {
            self.held.insert(name.to_owned());
        } else {
            self.held.remove(name);
        }
        self.push(at, KeyMouseChange::Button {
            name: name.to_owned(),
            down,
        });
    }
    pub(crate) fn wheel(&mut self, delta: [i32; 2], at: Instant) {
        if delta != [0; 2] {
            self.push(at, KeyMouseChange::Wheel { delta });
        }
    }
    fn push(&mut self, at: Instant, change: KeyMouseChange) {
        if self.recent.len() == 48 {
            self.recent.pop_front();
        }
        self.recent.push_back((at, change));
    }
    pub(crate) fn snapshot(&self) -> KeyMousePreview {
        let now = Instant::now();
        let recent = self.recent.iter()
            .filter_map(|(at, change)| {
                let age = now.saturating_duration_since(*at);
                (age < Duration::from_secs(1)).then(|| KeyMouseEvent {
                    age_ms: age.as_millis() as u64,
                    change: change.clone(),
                })
            })
            .collect();
        KeyMousePreview { held: self.held.iter().cloned().collect(), recent }
    }
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
                age_ms: 0,
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
            age_ms: 0,
        });
    }

    pub(crate) fn points(&self) -> Vec<MouseTracePoint> {
        let now = self.started.elapsed().as_millis() as u64;
        self.points
            .iter()
            .copied()
            .map(|mut point| {
                point.age_ms = now.saturating_sub(point.time_ms);
                point
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct PadTracePoint {
    pub sequence: u64,
    pub time_ms: u64,
    /// Final submitted axes: left X/Y, right X/Y. Positive right/up.
    pub axes: [i16; 4],
}

/// Display-only history of successful pad submissions, bounded independently
/// of the UI refresh rate. No additional reports are emitted for this trace.
pub(crate) struct PadTrace {
    started: Instant,
    points: VecDeque<PadTracePoint>,
}
impl Default for PadTrace {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            points: VecDeque::with_capacity(RECENT_POINTS),
        }
    }
}
impl PadTrace {
    pub(crate) fn clear(&mut self) {
        self.started = Instant::now();
        self.points.clear();
    }
    pub(crate) fn record(&mut self, pad: crate::engine::PadState) {
        let time_ms = self.time_ms() / SAMPLE_MS * SAMPLE_MS;
        let axes = [pad.left_x, pad.left_y, pad.right_x, pad.right_y];
        if let Some(last) = self.points.back_mut() {
            if last.time_ms == time_ms {
                last.axes = axes;
                return;
            }
        }
        let sequence = self.points.back().map_or(0, |point| point.sequence + 1);
        if self.points.len() == RECENT_POINTS {
            self.points.pop_front();
        }
        self.points.push_back(PadTracePoint {
            sequence,
            time_ms,
            axes,
        });
    }
    pub(crate) fn points(&self) -> Vec<PadTracePoint> {
        self.points.iter().copied().collect()
    }
    pub(crate) fn time_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}
