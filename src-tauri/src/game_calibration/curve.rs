use super::{CalibrationAction as Action, CalibrationButton as Button};
use motion_core::{
    protocol::{Command, CurveProbePhase, CurveProbeStatus},
    stick_response::{ResponsePoint, RightStickCurve},
};
use std::sync::Arc;
use uuid::Uuid;

// Measure the reference first. Keep the intermediate points in ascending travel order.
const TRAVEL: [u8; 6] = [100, 25, 50, 75, 10, 90];
const GUIDED_COUNT: usize = 4;
const LAP_COUNTS: [u8; 5] = [1, 3, 5, 10, 20];
const UNORDERED: &str = "测得的转速随推杆量下降或超过满杆转速。请重新测量可疑档位后再保存。";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CurvePhase {
    Idle,
    Align,
    Timing,
    Complete,
    Interrupted,
}

pub(super) struct CurveView {
    pub percent: u8,
    pub laps: u8,
    pub phase: CurvePhase,
    pub samples: Vec<(u8, f64)>,
    pub can_save: bool,
    pub problem: Option<&'static str>,
}

#[derive(Clone, Copy)]
enum LapTime {
    Still,
    Seconds(f64),
}

#[derive(Clone)]
pub(super) struct CurveDraft {
    pub original: Option<Arc<RightStickCurve>>,
    index: usize,
    lap_times: [Option<LapTime>; TRAVEL.len()],
    lap_choice: [usize; TRAVEL.len()],
    pub phase: CurvePhase,
}

impl CurveDraft {
    pub fn new(original: Option<RightStickCurve>) -> Self {
        Self {
            original: original.map(Arc::new),
            index: 0,
            lap_times: [None; TRAVEL.len()],
            lap_choice: [0; TRAVEL.len()],
            phase: CurvePhase::Idle,
        }
    }

    pub fn percent(&self) -> u8 {
        TRAVEL[self.index]
    }

    fn lap_count(&self) -> u8 {
        LAP_COUNTS[self.lap_choice[self.index]]
    }

    fn cycle_laps(&mut self) {
        let choice = &mut self.lap_choice[self.index];
        *choice = (*choice + 1) % LAP_COUNTS.len();
    }

    fn select(&mut self, next: bool) {
        let count = if self.guided_complete() {
            TRAVEL.len()
        } else {
            GUIDED_COUNT
        };
        let step = if next { 1 } else { count - 1 };
        self.index = (self.index + step) % count;
        self.phase = CurvePhase::Idle;
    }

    fn record(&mut self, lap: LapTime) {
        self.lap_times[self.index] = Some(lap);
        self.phase = CurvePhase::Idle;
        if let Some(index) = self.lap_times[..GUIDED_COUNT]
            .iter()
            .position(Option::is_none)
        {
            self.index = index;
        }
    }

    fn guided_complete(&self) -> bool {
        self.lap_times[..GUIDED_COUNT].iter().all(Option::is_some)
    }

    pub fn observe(&mut self, id: Uuid, probe: Option<&CurveProbeStatus>) {
        if !matches!(self.phase, CurvePhase::Align | CurvePhase::Timing) {
            return;
        }
        let matching = probe.filter(|p| p.session_id == id && p.percent == self.percent());
        match matching.map(|p| &p.phase) {
            Some(CurveProbePhase::WaitingForMark) => self.phase = CurvePhase::Align,
            Some(CurveProbePhase::Timing) => self.phase = CurvePhase::Timing,
            Some(CurveProbePhase::Finished { seconds })
                if seconds.is_finite() && *seconds > 0.0 =>
            {
                self.record(LapTime::Seconds(*seconds / f64::from(self.lap_count())));
            }
            None => self.phase = CurvePhase::Idle, // Cancelled from the controller.
            _ => self.phase = CurvePhase::Interrupted,
        }
    }

    fn samples(&self) -> Vec<(u8, f64)> {
        let Some(LapTime::Seconds(reference)) = self.lap_times[0] else {
            return Vec::new();
        };
        let mut samples: Vec<_> = TRAVEL
            .into_iter()
            .zip(self.lap_times)
            .filter_map(|(travel, lap)| {
                let speed = match lap? {
                    LapTime::Still => 0.0,
                    LapTime::Seconds(time) => reference / time * 100.0,
                };
                Some((travel, speed))
            })
            .collect();
        samples.sort_by_key(|&(travel, _)| travel);
        samples
    }

    pub fn view(&self) -> CurveView {
        let samples = self.samples();
        let invalid = samples.windows(2).any(|pair| pair[0].1 > pair[1].1);
        let can_save = self.guided_complete()
            && !invalid
            && !matches!(self.phase, CurvePhase::Align | CurvePhase::Timing);
        let phase =
            if can_save && self.lap_times[self.index].is_some() && self.phase == CurvePhase::Idle {
                CurvePhase::Complete
            } else {
                self.phase
            };
        let problem = if phase == CurvePhase::Interrupted {
            Some("本档测量已中断，摇杆输出已回中。可重新开始这一档。")
        } else if invalid {
            Some(UNORDERED)
        } else {
            None
        };
        CurveView {
            percent: self.percent(),
            laps: self.lap_count(),
            phase,
            samples,
            can_save,
            problem,
        }
    }

    pub fn pad_action(&self, button: Button) -> Option<Action> {
        if matches!(self.phase, CurvePhase::Align | CurvePhase::Timing) {
            return match button {
                Button::Measure => Some(Action::CurveMark),
                Button::Back => Some(Action::CurveCancel),
                Button::Mode if self.phase == CurvePhase::Align && self.percent() != 100 => {
                    Some(Action::CurveZero)
                }
                _ => None,
            };
        }
        match button {
            Button::Measure => Some(Action::CurveStart),
            Button::Mode => Some(Action::CurveLaps),
            Button::Back => Some(Action::Close),
            Button::Apply if self.view().can_save => Some(Action::Apply),
            Button::Less if self.lap_times[0].is_some() => Some(Action::CurvePrev),
            Button::More if self.lap_times[0].is_some() => Some(Action::CurveNext),
            _ => None,
        }
    }

    pub fn perform(
        &mut self,
        action: Action,
        client: &crate::client::Client,
        session_id: Uuid,
        revision: u64,
    ) -> Result<(), String> {
        // Mouse clicks and pad commands must both still belong to the current step.
        if !super::PAD_BUTTONS
            .iter()
            .any(|button| self.pad_action(*button) == Some(action))
        {
            return Ok(());
        }
        match action {
            Action::CurveStart => {
                if let Err(error) = client.request(Command::StartCurveProbe {
                    session_id,
                    expected_revision: revision,
                    percent: self.percent(),
                }) {
                    // A timed-out start may already have driven the virtual stick.
                    let _ = client.request(Command::CancelCurveProbe { session_id });
                    return Err(error);
                }
                self.phase = CurvePhase::Align;
            }
            Action::CurveMark => client.request(Command::MarkCurveProbe { session_id })?,
            Action::CurveCancel | Action::CurveZero => {
                client.request(Command::CancelCurveProbe { session_id })?;
                if action == Action::CurveZero {
                    self.record(LapTime::Still);
                } else {
                    self.phase = CurvePhase::Idle;
                }
            }
            Action::CurvePrev | Action::CurveNext => self.select(action == Action::CurveNext),
            Action::CurveLaps => self.cycle_laps(),
            _ => unreachable!(),
        }
        Ok(())
    }

    pub fn fitted(&self) -> Result<RightStickCurve, String> {
        let view = self.view();
        if !view.can_save {
            return Err(view
                .problem
                .unwrap_or("请完成满杆和三个中间档位的测量。")
                .into());
        }
        let mut points = vec![ResponsePoint(0., 0., 1.)];
        points.extend(
            view.samples
                .iter()
                .map(|&(x, y)| ResponsePoint(f32::from(x) / 100., (y / 100.) as f32, 1.)),
        );
        let curve = RightStickCurve {
            enabled: true,
            points,
            model: motion_core::bezier::CurveModel::Points,
            ..self.original.as_deref().cloned().unwrap_or_default()
        };
        curve.validate().map_err(super::text)?;
        Ok(curve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_laps_generate_forward_points_and_reference_retries_rescale_them() {
        let original = RightStickCurve {
            linearize_stick: true,
            ..Default::default()
        };
        let mut draft = CurveDraft::new(Some(original));
        let session_id = Uuid::new_v4();
        for (percent, presses, laps, seconds) in [
            (100, 3, 10, 20.0),
            (25, 0, 1, 10.0),
            (50, 1, 3, 15.0),
            (75, 2, 5, 12.5),
        ] {
            assert_eq!(draft.percent(), percent);
            for _ in 0..presses {
                draft.cycle_laps();
            }
            assert_eq!(draft.view().laps, laps);
            draft.phase = CurvePhase::Timing;
            assert!(draft.pad_action(Button::Mode).is_none());
            draft.observe(
                session_id,
                Some(&CurveProbeStatus {
                    session_id,
                    percent,
                    phase: CurveProbePhase::Finished { seconds },
                }),
            );
        }
        let fitted = draft.fitted().unwrap();
        assert!(fitted.enabled && fitted.linearize_stick);
        assert_eq!(fitted.points[2], ResponsePoint(0.5, 0.4, 1.0));
        draft.select(true);
        assert_eq!(draft.percent(), 10);
        draft.record(LapTime::Still);
        assert_eq!(
            draft.fitted().unwrap().points[1],
            ResponsePoint(0.1, 0.0, 1.0)
        );
        draft.select(true);
        draft.select(true);
        assert_eq!(draft.percent(), 100);
        assert_eq!(draft.view().laps, 10);
        draft.cycle_laps();
        assert_eq!(draft.view().laps, 20);
        assert_eq!(
            draft.fitted().unwrap().points[3],
            ResponsePoint(0.5, 0.4, 1.0)
        );
        draft.record(LapTime::Seconds(3.0));
        assert!(draft.fitted().is_err()); // 75% would exceed the new full-stick reference.
        draft.record(LapTime::Seconds(2.25));
        assert_eq!(
            draft.fitted().unwrap().points[3],
            ResponsePoint(0.5, 0.45, 1.0)
        );
    }
}
