//! Studio's unordered chord deadline/lifecycle policy. Timed activators and
//! withheld output replay remain FlexInput PressGate / OutputHoldBack.
use crate::{bindings::input_chord, profile::Action};

#[derive(Clone, Copy, Default)]
pub(crate) struct Disposition {
    pub waiting: bool,
    pub consumed: bool,
    pub rejected: bool,
}

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Idle,
    Pending(u64),
    Matched {
        lifted: bool,
    },
    Expired,
}

struct Chord {
    inputs: Vec<String>,
    threshold: f32,
    wait_ns: u64,
    phase: Phase,
    clear_next: bool,
}

#[derive(Default)]
pub(crate) struct ChordWait {
    chords: Vec<Chord>,
    // For each digital action: its chord, plus strict supersets.
    owners: Vec<Option<usize>>,
    supersets: Vec<Vec<usize>>,
}

impl ChordWait {
    pub fn new(actions: &[Action]) -> Self {
        let inputs: Vec<Vec<String>> = actions
            .iter()
            .map(|a| {
                let mut parts: Vec<_> = input_chord(&a.input)
                    .unwrap()
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                parts.sort();
                parts
            })
            .collect();
        let mut this = Self::default();
        for (action, parts) in actions.iter().zip(&inputs) {
            if parts.len() < 2 {
                this.owners.push(None);
                continue;
            }
            let index = this
                .chords
                .iter()
                .position(|c| c.inputs == *parts && c.threshold == action.threshold)
                .unwrap_or_else(|| {
                    this.chords.push(Chord {
                        inputs: parts.clone(),
                        threshold: action.threshold,
                        wait_ns: 0,
                        phase: Phase::Idle,
                        clear_next: false,
                    });
                    this.chords.len() - 1
                });
            this.chords[index].wait_ns = this.chords[index]
                .wait_ns
                .max(u64::from(action.chord_wait_ms) * 1_000_000);
            this.owners.push(Some(index));
        }
        this.supersets = inputs
            .iter()
            .map(|parts| {
                this.chords
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        c.inputs.len() > parts.len() && parts.iter().all(|p| c.inputs.contains(p))
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .collect();
        this
    }

    pub fn reset(&mut self) {
        for chord in &mut self.chords {
            chord.phase = Phase::Idle;
            chord.clear_next = false;
        }
    }

    pub fn reconfigure(&mut self, actions: &[Action]) {
        let mut next = Self::new(actions);
        for chord in &mut next.chords {
            if let Some(old) = self.chords.iter().find(|c| {
                c.inputs == chord.inputs
                    && c.threshold == chord.threshold
                    && c.wait_ns == chord.wait_ns
            }) {
                chord.phase = old.phase;
                chord.clear_next = old.clear_next;
            }
        }
        *self = next;
    }

    pub fn sample(&mut self, now_ns: u64, allow_press: bool, value: impl Fn(&str) -> f32) {
        for chord in &mut self.chords {
            if chord.clear_next {
                chord.phase = Phase::Idle;
                chord.clear_next = false;
            }
            // Start at analog travel onset as well: otherwise an analog
            // passthrough could already cast a skill before the threshold.
            let engaged = chord.inputs.iter().any(|p| value(p) > 0.0);
            let complete = chord.inputs.iter().all(|p| value(p) > chord.threshold);
            if !allow_press && engaged && matches!(chord.phase, Phase::Idle) {
                chord.phase = Phase::Expired;
            }
            chord.phase = match chord.phase {
                Phase::Idle if complete => Phase::Matched { lifted: false },
                Phase::Idle if engaged && chord.wait_ns > 0 => Phase::Pending(now_ns),
                Phase::Pending(start) if now_ns.saturating_sub(start) > chord.wait_ns => {
                    Phase::Expired
                }
                Phase::Pending(_) if complete => Phase::Matched { lifted: false },
                Phase::Pending(start) if now_ns.saturating_sub(start) == chord.wait_ns => {
                    Phase::Expired
                }
                Phase::Matched { lifted } => Phase::Matched {
                    lifted: lifted || !complete,
                },
                phase => phase,
            };
            if !engaged {
                // Keep the claim for this final release edge too, preventing
                // release activators from leaking when the chord ends.
                if matches!(chord.phase, Phase::Pending(_)) {
                    chord.phase = Phase::Idle;
                }
                chord.clear_next = true;
            }
        }
    }

    fn suppression(&self, groups: impl Iterator<Item = usize>) -> Disposition {
        let mut result = Disposition::default();
        for index in groups {
            match self.chords[index].phase {
                Phase::Pending(_) => result.waiting = true,
                Phase::Matched { .. } => result.consumed = true,
                _ => {}
            }
        }
        result
    }

    pub fn action(&self, index: usize) -> Disposition {
        let mut result = self.suppression(self.supersets[index].iter().copied());
        if let Some(owner) = self.owners[index] {
            result.rejected = !matches!(self.chords[owner].phase, Phase::Matched { lifted: false });
        }
        result
    }

    pub fn analog(&self, input: &str) -> Disposition {
        self.suppression(
            self.chords
                .iter()
                .enumerate()
                .filter(|(_, c)| c.inputs.iter().any(|p| p == input))
                .map(|(i, _)| i),
        )
    }
}

#[derive(Default)]
pub(crate) struct AnalogHoldBack {
    gate: flexinput_engine::eval::OutputHoldBack,
    replay_value: f32,
}

impl AnalogHoldBack {
    pub fn sample(&mut self, value: f32, disposition: Disposition, now_ns: u64) -> f32 {
        if disposition.consumed {
            *self = Self::default();
            return 0.0;
        }
        if value > 0.0 {
            self.replay_value = if disposition.waiting {
                self.replay_value.max(value)
            } else {
                value
            };
        }
        if self
            .gate
            .sample_at(value > 0.0, disposition.waiting, now_ns)
        {
            if value > 0.0 {
                value
            } else {
                self.replay_value
            }
        } else {
            if !disposition.waiting {
                self.replay_value = 0.0;
            }
            0.0
        }
    }
}
