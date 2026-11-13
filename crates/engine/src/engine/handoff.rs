//! Studio profile lifecycle adapter. Completed continuous presses retain their
//! existing output until release; no trigger recognition is reimplemented here.
use super::*;

struct Press {
    index: usize,
    inputs: Vec<String>,
    output: String,
    threshold: f32,
    active: bool,
}

pub struct PressHandoff {
    engine: Engine,
    presses: Vec<Press>,
}
impl PressHandoff {
    pub fn new(engine: Engine) -> Self {
        let presses = engine
            .mapped_actions
            .iter()
            .zip(&engine.action_states)
            .enumerate()
            .filter(|(_, (action, state))| {
                state.active
                    && state.raw
                    && action.turbo_ms.is_none()
                    && (matches!(action.trigger, Trigger::Down | Trigger::Double)
                        || action.trigger == Trigger::Long && action.sustain)
                    && !action_condition(action)
                        .iter()
                        .any(|input| input.starts_with("touch_swipe_"))
            })
            .filter_map(|(index, (action, _))| {
                let outputs = crate::bindings::output_chord(&action.output);
                if !action.output.starts_with("pad:")
                    && !outputs.is_some_and(|parts| {
                        parts
                            .iter()
                            .any(|output| crate::bindings::wheel_delta(output).is_none())
                    })
                {
                    return None;
                }
                Some(Press {
                    index,
                    inputs: action_condition(action)
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    output: action.output.clone(),
                    threshold: action.threshold,
                    active: true,
                })
            })
            .collect();
        Self { engine, presses }
    }
    pub fn previous(&self) -> &Engine {
        &self.engine
    }
    pub fn is_empty(&self) -> bool {
        self.presses.is_empty()
    }
    pub fn process(&mut self, event: &InputEvent) {
        if !matches!(
            event,
            InputEvent::Button { .. } | InputEvent::Axis { .. } | InputEvent::Touch { .. }
        ) {
            return;
        }
        self.engine.process_snapshot(event);
        for press in &mut self.presses {
            press.active &= self
                .engine
                .controls
                .result
                .sink_outputs
                .get_pin(ACTIONS, &format!("raw:{}", press.index))
                .is_some_and(|value| value.as_bool());
        }
        self.presses.retain(|press| {
            press
                .inputs
                .iter()
                .any(|input| held(&self.engine, input, press.threshold))
        });
    }
    pub fn blocked_inputs(&self) -> BTreeSet<String> {
        let mut blocked = BTreeSet::new();
        for press in &self.presses {
            for input in &press.inputs {
                if !held(&self.engine, input, press.threshold) {
                    continue;
                }
                blocked.insert(input.clone());
                for side in ["left", "right"] {
                    if input.starts_with(&format!("{side}_trigger")) {
                        for suffix in ["", "_soft", "_full"] {
                            blocked.insert(format!("{side}_trigger{suffix}"));
                        }
                    }
                    if input.starts_with(&format!("{side}_stick_")) && !input.ends_with("_click") {
                        blocked.insert(format!("{side}_stick"));
                        for suffix in ["x", "y", "up", "down", "left", "right", "inner", "outer"] {
                            blocked.insert(format!("{side}_stick_{suffix}"));
                        }
                    }
                }
            }
        }
        blocked
    }
    pub fn merge(&self, frame: &mut Frame) {
        for press in self.presses.iter().filter(|press| press.active) {
            if let Some(pin) = press.output.strip_prefix("pad:") {
                let pad = frame.pad.get_or_insert_with(PadState::default);
                pad.set(pin, Signal::Bool(true));
                match pin {
                    "left_trigger" => pad.left_trigger_pressed = true,
                    "right_trigger" => pad.right_trigger_pressed = true,
                    _ => {}
                }
            } else {
                for output in crate::bindings::output_chord(&press.output)
                    .into_iter()
                    .flatten()
                {
                    if crate::bindings::wheel_delta(output).is_none() {
                        frame.held.insert(output.into());
                    }
                }
            }
        }
    }
}
fn held(engine: &Engine, input: &str, threshold: f32) -> bool {
    for side in ["left", "right"] {
        if input.starts_with(&format!("{side}_trigger")) {
            return engine.input_value(&format!("{side}_trigger")) > 0.02;
        }
        if input.starts_with(&format!("{side}_stick_")) && !input.ends_with("_click") {
            return engine.input.stick(&format!("{side}_stick")).length() > threshold;
        }
    }
    engine.input_value(input) > threshold
}

impl Engine {
    /// Seed from the last processed physical state, not a newer SDL snapshot.
    /// Queued FIFO releases remain available to both engines after the switch.
    pub fn inherit_profile_input(&mut self, previous: &Engine, blocked: BTreeSet<String>) {
        self.bias_rad_s = previous.bias_rad_s;
        self.sensor_calibration = previous.sensor_calibration.clone();
        self.raw_signals = previous.raw_signals.clone();
        self.raw_sticks = previous.raw_sticks;
        self.stick_time = previous.stick_time;
        self.action_time_ns = previous.action_time_ns;
        self.touchpad.inherit_contacts(&previous.touchpad);
        self.handoff_blocked = blocked;
        self.refresh_sticks(self.stick_time, false);
        self.activation_pressed = self.gyro_activation_pressed();
        self.turn_held = self
            .profile
            .gyro
            .motion
            .turns
            .iter()
            .map(|turn| self.input_value(&turn.input) > self.profile.gyro.activation_threshold)
            .collect();
        let flick = &self.profile.gyro.motion.flick;
        self.flick_snapshot_blocked =
            flick.enabled && self.stick_value_radial(flick.stick).length() >= flick.deadzone;
        self.eval_controls(false, self.action_time_ns);
    }
    pub fn set_handoff_blocked(&mut self, blocked: BTreeSet<String>) {
        self.handoff_blocked = blocked;
    }
    pub(super) fn mask_handoff_inputs(&mut self) {
        for input in &self.handoff_blocked {
            let pin = input_pin(input).unwrap_or(input);
            if let Some(signal) = self.signals.get_pin_mut(SOURCE, pin) {
                *signal = signal.zeroed();
            }
        }
    }
}
