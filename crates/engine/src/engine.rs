use crate::profile::{
    input_pin, Action, GamepadTarget, GyroActivationMode, GyroOutput, Profile, StickRegion,
    StickZones, Trigger, AXES, BUTTON_PINS, DIRECTIONS, PAD_PINS, ZONES,
};
use crate::timing::{SampleClock, SampleTime, MAX_GAP_NS};
use crate::{
    gamepad_bindings as pad_binding,
    motion::{self, GameAngleMeasurement, MotionRuntime},
    motion_config::*,
};
use anyhow::{bail, Result};
use flexinput_core::{imu::BUS_GYRO_DPS, PinMapExt, SigMap, Signal};
use flexinput_engine::{
    eval::{sample_curve, PressCompetition, TickOutput},
    eval_graph_tick,
    graph::SinkTarget,
    NodeSnap, NodeState, ProcessingGraph,
};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};

use crate::controller_input::{NativeInput, SOURCE, TRIGGER_STAGE_PINS};
const ACTIONS: &str = "studio.actions";
const PAD: &str = "studio.pad";
const GYRO: &str = "studio.gyro";
const GYRO_AIM: &str = "aim_output";

fn stick_region(
    layout: &mut flexinput_core::area::AreaLayout,
    value: Vec2,
    limit: f32,
    hysteresis: f32,
    was_outer: bool,
) -> StickRegion {
    layout.edges[0].pos = limit - if was_outer { hysteresis } else { 0. };
    if layout.band_of(value.x, value.y) > 0 {
        StickRegion::Outer
    } else if value.length() > 0. {
        StickRegion::Inner
    } else {
        StickRegion::Center
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    Button {
        time: SampleTime,
        index: u8,
        down: bool,
    },
    Axis {
        time: SampleTime,
        index: u8,
        value: i16,
    },
    Touch {
        time: SampleTime,
        pad: u8,
        finger: u8,
        active: bool,
        x: f32,
        y: f32,
    },
    Gyro {
        time: SampleTime,
        value: [f32; 3],
    },
    Accel {
        time: SampleTime,
        value: [f32; 3],
    },
    Disconnected {
        time: SampleTime,
    },
}
impl InputEvent {
    pub fn time(&self) -> SampleTime {
        match *self {
            Self::Button { time, .. }
            | Self::Axis { time, .. }
            | Self::Touch { time, .. }
            | Self::Gyro { time, .. }
            | Self::Accel { time, .. }
            | Self::Disconnected { time } => time,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Frame {
    pub held: BTreeSet<String>,
    pub mouse_delta: [f64; 2],
    /// Horizontal/right and vertical/up scroll, in wheel detents.
    pub scroll: [i32; 2],
    pub pad: Option<PadState>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadState {
    pub buttons: u16,
    pub touchpad_click: bool,
    pub left_trigger_pressed: bool,
    pub right_trigger_pressed: bool,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

fn axis_to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * if value < 0.0 { 32768.0 } else { 32767.0 }).round() as i16
}

fn axis_from_i16(value: i16) -> f32 {
    f32::from(value) / if value < 0 { 32768.0 } else { 32767.0 }
}

impl PadState {
    pub fn set(&mut self, pin: &str, sig: Signal) {
        let f = sig.as_float();
        match pin {
            "left_stick_x" => self.left_x = axis_to_i16(f),
            "left_stick_y" => self.left_y = axis_to_i16(f),
            "right_stick_x" => self.right_x = axis_to_i16(f),
            "right_stick_y" => self.right_y = axis_to_i16(f),
            "btn_touchpad" => self.touchpad_click = sig.as_bool(),
            "btn_lt_dig" => self.left_trigger_pressed = sig.as_bool(),
            "btn_rt_dig" => self.right_trigger_pressed = sig.as_bool(),
            "left_trigger" => self.left_trigger = (f.clamp(0.0, 1.0) * 255.0).round() as u8,
            "right_trigger" => self.right_trigger = (f.clamp(0.0, 1.0) * 255.0).round() as u8,
            _ => {
                let bit = match pin {
                    "dpad_up" => 0x0001,
                    "dpad_down" => 0x0002,
                    "dpad_left" => 0x0004,
                    "dpad_right" => 0x0008,
                    "btn_start" => 0x0010,
                    "btn_back" => 0x0020,
                    "btn_ls" => 0x0040,
                    "btn_rs" => 0x0080,
                    "btn_lb" => 0x0100,
                    "btn_rb" => 0x0200,
                    "btn_guide" => 0x0400,
                    "btn_south" => 0x1000,
                    "btn_east" => 0x2000,
                    "btn_west" => 0x4000,
                    "btn_north" => 0x8000,
                    _ => return,
                };
                if sig.as_bool() {
                    self.buttons |= bit;
                } else {
                    self.buttons &= !bit;
                }
            }
        }
    }
}

pub(crate) fn node(id: usize, module: &str, outputs: usize) -> NodeSnap {
    static DESCRIPTORS: std::sync::OnceLock<
        HashMap<&'static str, flexinput_core::ModuleDescriptor>,
    > = std::sync::OnceLock::new();
    let descriptor = DESCRIPTORS
        .get_or_init(|| {
            flexinput_modules::all_modules()
                .into_iter()
                .map(|m| (m.descriptor.id, m.descriptor))
                .collect()
        })
        .get(module);
    NodeSnap {
        node_uid: id,
        module_id: module.into(),
        params: HashMap::new(),
        // Dynamic outlets (measurement etc.) may extend the base descriptor.
        n_outputs: descriptor.map_or(outputs, |d| outputs.max(d.outputs.len())),
        input_sources: vec![],
        device_id: None,
        output_pin_ids: descriptor.map_or_else(Vec::new, |d| {
            d.outputs.iter().map(|p| p.name.clone()).collect()
        }),
        aux_f32_override: None,
        sink_target: None,
        inline_subgraph: None,
    }
}
fn source(pins: Vec<String>, deadzone: f32) -> NodeSnap {
    let mut n = node(0, "device.source", pins.len());
    n.device_id = Some(SOURCE.into());
    n.output_pin_ids = pins;
    n.params.insert("deadzone".into(), json!(deadzone));
    n
}

pub(crate) struct GraphRunner {
    pub(crate) graph: ProcessingGraph,
    pub(crate) state: HashMap<usize, NodeState>,
    pub(crate) result: TickOutput,
}
impl GraphRunner {
    pub(crate) fn new(nodes: Vec<NodeSnap>) -> Self {
        let mut result = TickOutput::default();
        result.skip_display = true;
        Self {
            graph: ProcessingGraph::new(nodes),
            state: HashMap::new(),
            result,
        }
    }
    pub(crate) fn eval(&mut self, signals: &SigMap, dt: f32) {
        eval_graph_tick(&self.graph, &mut self.state, signals, dt, &mut self.result);
    }
}

fn button_condition(
    nodes: &mut Vec<NodeSnap>,
    pins: &[String],
    inputs: &[&str],
    threshold: f32,
) -> usize {
    let constant = nodes.len();
    let mut n = node(constant, "module.constant", 1);
    n.params.insert("value".into(), json!(threshold));
    nodes.push(n);
    let mut condition = None;
    for &input in inputs {
        let pin = input_pin(input).expect("validated input");
        let index = pins.iter().position(|p| p == pin).expect("source pin");
        let compare = nodes.len();
        let mut n = node(compare, "logic.greater_than", 1);
        n.input_sources = vec![Some((0, index)), Some((constant, 0))];
        nodes.push(n);
        condition = Some(if let Some(previous) = condition {
            let and = nodes.len();
            let mut n = node(and, "logic.and", 1);
            n.input_sources = vec![Some((previous, 0)), Some((compare, 0))];
            nodes.push(n);
            and
        } else {
            compare
        });
    }
    condition.expect("validated chord")
}

fn mapped_actions(profile: &Profile) -> Vec<Action> {
    let mut actions = profile.actions.clone();
    if profile.virtual_gamepad != GamepadTarget::None {
        actions.extend(
            profile
                .gamepad_bindings
                .iter()
                .flatten()
                .filter(|binding| !binding.is_analog())
                .filter(|binding| {
                    binding.output != "touchpad"
                        || profile.virtual_gamepad == GamepadTarget::DualShock4
                })
                .map(|binding| binding.action()),
        );
    }
    actions
}

fn action_condition(action: &Action) -> Vec<&str> {
    let mut chord = crate::bindings::input_chord(&action.input).expect("validated chord");
    chord.sort_unstable();
    chord
}

fn analog_condition(chord: &[&str]) -> bool {
    chord
        .iter()
        .any(|pin| AXES.contains(pin) || DIRECTIONS.contains(pin))
}

fn same_competitors(action: &Action, old: &[Action], new: &[Action]) -> bool {
    if action.allow_simultaneous {
        return true;
    }
    let chord = action_condition(action);
    let analog = analog_condition(&chord);
    let peer = |other: &&Action| {
        !other.allow_simultaneous
            && matches!(
                other.trigger,
                Trigger::Short | Trigger::Long | Trigger::Double
            )
            && (!analog || action.threshold == other.threshold)
            && action_condition(other) == chord
    };
    let old: Vec<_> = old.iter().filter(peer).collect();
    let mut new: Vec<_> = new.iter().filter(peer).collect();
    // Reordering rows preserves progress; changing a timed command cancels
    // the whole pending decision, even if its recognition window is unchanged.
    old.len() == new.len()
        && old.into_iter().all(|action| {
            if let Some(index) = new.iter().position(|other| *other == action) {
                new.swap_remove(index);
                true
            } else {
                false
            }
        })
}

fn controls(profile: &Profile, actions: &[Action]) -> (GraphRunner, Vec<PressCompetition>) {
    let pins: Vec<String> = BUTTON_PINS
        .iter()
        .chain(AXES)
        .chain(DIRECTIONS)
        .chain(ZONES)
        .chain(crate::profile::TOUCH_PINS)
        .chain(&TRIGGER_STAGE_PINS)
        .map(|p| p.to_string())
        .collect();
    let mut nodes = vec![source(pins.clone(), 0.)];
    let chords: Vec<_> = actions.iter().map(action_condition).collect();
    let competitions = actions
        .iter()
        .zip(&chords)
        .map(|(action, chord)| {
            let analog = analog_condition(chord);
            let mut competition = PressCompetition::default();
            for (other, _) in actions.iter().zip(&chords).filter(|(other, inputs)| {
                *inputs == chord
                    && (!analog || action.threshold == other.threshold)
                    && (std::ptr::eq(action, *other)
                        || (!action.allow_simultaneous && !other.allow_simultaneous))
            }) {
                let ms = other.trigger_ms;
                match other.trigger {
                    Trigger::Short => {
                        competition.short_ms =
                            Some(competition.short_ms.map_or(ms, |old| old.max(ms)))
                    }
                    Trigger::Long => {
                        competition.long_ms =
                            Some(competition.long_ms.map_or(ms, |old| old.min(ms)))
                    }
                    Trigger::Double => {
                        competition.double_ms =
                            Some(competition.double_ms.map_or(ms, |old| old.max(ms)))
                    }
                    _ => {}
                }
            }
            competition
        })
        .collect();
    let conditions: Vec<_> = actions
        .iter()
        .zip(&chords)
        .map(|(action, inputs)| button_condition(&mut nodes, &pins, inputs, action.threshold))
        .collect();
    let mut action_pins = vec![];
    let mut action_sources = vec![];
    for (i, _action) in actions.iter().enumerate() {
        let mut supersets = None;
        for (other, c) in chords.iter().enumerate() {
            if c.len() > chords[i].len() && chords[i].iter().all(|v| c.contains(v)) {
                supersets = Some(if let Some(previous) = supersets {
                    let or = nodes.len();
                    let mut n = node(or, "logic.or", 1);
                    n.input_sources = vec![Some((previous, 0)), Some((conditions[other], 0))];
                    nodes.push(n);
                    or
                } else {
                    conditions[other]
                });
            }
        }
        let active = if let Some(superset) = supersets {
            let not = nodes.len();
            let mut n = node(not, "logic.not", 1);
            n.input_sources = vec![Some((superset, 0))];
            nodes.push(n);
            let and = nodes.len();
            let mut n = node(and, "logic.and", 1);
            n.input_sources = vec![Some((conditions[i], 0)), Some((not, 0))];
            nodes.push(n);
            and
        } else {
            conditions[i]
        };
        action_pins.extend([format!("{i}"), format!("raw:{i}")]);
        action_sources.extend([vec![(active, 0)], vec![(conditions[i], 0)]]);
    }
    if !action_pins.is_empty() {
        let mut n = node(nodes.len(), "device.sink", 0);
        n.sink_target = Some(SinkTarget {
            device_id: ACTIONS.into(),
            pin_ids: action_pins,
            multi_sources: action_sources,
            ..Default::default()
        });
        nodes.push(n);
    }
    if profile.virtual_gamepad != GamepadTarget::None {
        let mut pad_pins: Vec<String> = PAD_PINS.iter().map(|s| s.to_string()).collect();
        if profile.virtual_gamepad == GamepadTarget::DualShock4 {
            pad_pins.push("btn_touchpad".into());
            pad_pins.extend(["btn_lt_dig".into(), "btn_rt_dig".into()]);
        }
        let mut sources = vec![Vec::new(); pad_pins.len()];
        let source_pin = |name: &str| {
            (
                0,
                pins.iter().position(|pin| pin == name).expect("source pin"),
            )
        };
        let mut wire = |output: &str, source| {
            if let Some(index) = pad_pins.iter().position(|pin| pin == output) {
                sources[index].push(source);
            }
        };
        for binding in profile
            .gamepad_bindings
            .iter()
            .flatten()
            .filter(|binding| binding.is_analog())
        {
            let input = binding.input.trim();
            let output = binding.output.as_str();
            if let Some(output_stick) = pad_binding::stick(output) {
                let input_stick = pad_binding::stick(input).expect("validated stick binding");
                for (input, output) in pad_binding::stick_pins(input_stick)
                    .into_iter()
                    .zip(pad_binding::stick_pins(output_stick))
                {
                    wire(output, source_pin(input));
                }
                continue;
            }
            let output_pin = pad_binding::output_pin(output).expect("validated analog trigger");
            let output_trigger = pad_binding::trigger_button(output);
            let from = source_pin(input_pin(input).expect("validated analog trigger"));
            wire(output_pin, from);
            if let Some(digital) = output_trigger {
                // The DS4 digital bit uses the same calibrated signal as its travel.
                wire(digital, from);
            }
        }
        let mut n = node(nodes.len(), "device.sink", 0);
        n.sink_target = Some(SinkTarget {
            device_id: PAD.into(),
            pin_ids: pad_pins,
            multi_sources: sources,
            ..Default::default()
        });
        nodes.push(n);
    }
    (GraphRunner::new(nodes), competitions)
}

fn gyro_graph(profile: &Profile) -> GraphRunner {
    let mut rws = node(1, "processing.rws", 5);
    rws.input_sources = vec![Some((0, 0)), Some((0, 1))];
    rws.params = HashMap::from([
        ("scale".into(), json!(profile.gyro.counts_per_degree)),
        ("rws".into(), json!(profile.gyro.sensitivity)),
        ("gyro_vh_ratio".into(), json!(profile.gyro.vertical_ratio)),
        (
            "stick_out_dps".into(),
            json!(profile.gyro.stick_full_scale_dps),
        ),
        ("flick_enabled".into(), json!(false)),
        ("stick_aim_enabled".into(), json!(false)),
    ]);
    let mut sink = node(2, "device.sink", 0);
    sink.sink_target = Some(SinkTarget {
        device_id: GYRO.into(),
        pin_ids: vec![GYRO_AIM.into(), "measured".into(), "peak".into()],
        multi_sources: vec![
            vec![(
                1,
                usize::from(profile.gyro.output == GyroOutput::RightStick),
            )],
            vec![(1, 3)],
            vec![(1, 4)],
        ],
        ..Default::default()
    });
    GraphRunner::new(vec![
        source(vec!["aim_rate".into(), "calibration_stick".into()], 0.0),
        rws,
        sink,
    ])
}

/// Simple Studio profiles are compiled into FlexInput graphs, never a GUI canvas.
/// The controls graph evaluates edges; the gyro graph runs once per new sample.
/// Buttons and idle wakeups cannot integrate an old gyro again.
pub struct Engine {
    pub profile: Profile,
    pub clock: SampleClock,
    pub frame: Frame,
    pub gyro_events: u64,
    pub invalid_values: u64,
    pub mouse_total: [f64; 2],
    pub bias_rad_s: [f32; 3],
    pub sensor_calibration: SensorCalibration,
    pub motion: MotionRuntime,
    signals: SigMap,
    raw_signals: SigMap,
    input: NativeInput,
    base_input: NativeInput,
    stick_mouse_graph: GraphRunner,
    touchpad: crate::touchpad::NativeTouchpad,
    gyro_signals: SigMap,
    controls: GraphRunner,
    gyro: GraphRunner,
    active: bool,
    last_paused: bool,
    activation_pressed: bool,
    gyro_latched: bool,
    physical_pad: Option<PadState>,
    gyro_stick: Vec2,
    last_stick_gyro_ns: Option<u64>,
    accel: Option<(u64, Vec3)>,
    binding_states: Vec<BindingState>,
    motion_held: BTreeSet<String>,
    motion_pad: Vec<(String, f32)>,
    controls_held: BTreeSet<String>,
    mapped_actions: Vec<Action>,
    action_states: Vec<ActionState>,
    action_time_ns: u64,
    rotated_stick: Option<(Stick, Vec2)>,
    flick: GraphRunner,
    flick_stick: Vec2,
    flick_snapshot_blocked: bool,
    turn_held: Vec<bool>,
    turns: Vec<PendingTurn>,
    turn_stick_owed: f32,
    measuring: Option<String>,
    raw_sticks: [Vec2; 2],
    zone_layouts: [flexinput_core::area::AreaLayout; 2],
    stick_time: f64,
}
#[derive(Default)]
struct BindingState {
    pressed: bool,
    latched: bool,
    phase: f32,
    emitting: bool,
}
#[derive(Default)]
struct ActionState {
    raw: bool,
    active: bool,
    repeat_armed: bool,
    elapsed: f32,
    gate: flexinput_engine::eval::PressGate,
    competition: PressCompetition,
    blocked: bool,
    pending_scroll: [i32; 2],
}
struct PendingTurn {
    degrees: f32,
    seconds: f32,
    output: AimOutput,
}

fn flick_graph(profile: &Profile) -> GraphRunner {
    let f = &profile.gyro.motion.flick;
    let mut src = node(0, "device.source", 2);
    src.device_id = Some(SOURCE.into());
    src.output_pin_ids = vec![
        if f.stick == Stick::Left {
            "left_stick".into()
        } else {
            "right_stick".into()
        },
        "flick_on".into(),
    ];
    src.params.insert("deadzone".into(), json!(0.));
    let mut rws = node(1, "processing.rws", 5);
    rws.input_sources = vec![None, Some((0, 0)), Some((0, 1))];
    rws.params = HashMap::from([
        ("scale".into(), json!(profile.gyro.counts_per_degree)),
        ("rws".into(), json!(1.)),
        (
            "stick_out_dps".into(),
            json!(profile.gyro.stick_full_scale_dps),
        ),
        // Use the Flick pin as a relative-angle source for the shared output.
        ("flick_output".into(), json!("mouse")),
        ("_rws_flick_out_wired".into(), json!(f.enabled)),
        ("stick_aim_enabled".into(), json!(f.stick_aim)),
        ("flick_deadzone".into(), json!(f.deadzone)),
        ("flick_smooth_ms".into(), json!(f.smooth_ms)),
        ("flick_release_ease".into(), json!(f.release_ease)),
        ("flick_fwd_dz_deg".into(), json!(f.forward_deadzone)),
        ("flick_stabilise_ms".into(), json!(f.stabilise_ms)),
        ("flick_speed_ms".into(), json!(f.speed_gate_ms)),
        ("flick_slow_lock".into(), json!(f.slow_lock)),
        ("stick_aim_rws".into(), json!(f.aim_sensitivity)),
        ("max_rate_dps".into(), json!(f.max_dps)),
        ("stick_vh_ratio".into(), json!(f.vertical_ratio)),
    ]);
    let mut sink = node(2, "device.sink", 0);
    sink.sink_target = Some(SinkTarget {
        device_id: GYRO.into(),
        pin_ids: vec![
            "flick_mouse".into(),
            "flick_pad".into(),
            "flick_angle".into(),
        ],
        multi_sources: vec![
            vec![(1, 0)],
            vec![(1, 1)],
            if f.enabled { vec![(1, 2)] } else { vec![] },
        ],
        ..Default::default()
    });
    GraphRunner::new(vec![src, rws, sink])
}
// Legacy exponent is configuration converted to an upstream vector-response
// curve. The original RWS integrates its output; Studio owns no mouse filter.
fn stick_mouse_graph(profile: &Profile) -> GraphRunner {
    let m = &profile.stick_mouse;
    let mut src = source(vec![format!("{}_stick", m.stick)], 0.);
    src.node_uid = 0;
    let mut curve = node(1, "module.vec_response_curve", 1);
    curve.input_sources = vec![Some((0, 0))];
    let points: Vec<_> = (0..=128)
        .map(|i| {
            let x = i as f32 / 128.;
            [x, x.powf(m.exponent)]
        })
        .collect();
    curve.params.insert("points".into(), json!(points));
    let mut rate = node(2, "processing.rws", 5);
    rate.input_sources = vec![Some((1, 0)), None, None];
    rate.params = HashMap::from([
        ("scale".into(), json!(m.speed / BUS_GYRO_DPS)),
        ("rws".into(), json!(1.)),
        ("flick_enabled".into(), json!(false)),
        ("stick_aim_enabled".into(), json!(false)),
    ]);
    let mut sink = node(3, "device.sink", 0);
    sink.input_sources = vec![Some((2, 0))];
    sink.sink_target = Some(SinkTarget {
        device_id: "studio.stick_mouse".into(),
        pin_ids: vec!["mouse_move".into()],
        multi_sources: vec![vec![(2, 0)]],
        ..Default::default()
    });
    GraphRunner::new(vec![src, curve, rate, sink])
}
impl Engine {
    pub fn new(mut profile: Profile) -> Result<Self> {
        profile.materialize_gamepad_bindings();
        profile.validate()?;
        let mut signals = SigMap::default();
        for pin in BUTTON_PINS {
            signals.set_pin(SOURCE, pin, Signal::Bool(false));
        }
        for pin in AXES.iter().chain(DIRECTIONS).chain(ZONES) {
            signals.set_pin(SOURCE, pin, Signal::Float(0.0));
        }
        for stick in ["left_stick", "right_stick"] {
            signals.set_pin(SOURCE, stick, Signal::Vec2(Vec2::ZERO));
        }
        let raw_signals = signals.clone();
        let mapped_actions = mapped_actions(&profile);
        let (controls, competitions) = controls(&profile, &mapped_actions);
        let mut this = Self {
            input: NativeInput::new(&profile),
            base_input: NativeInput::new(&profile),
            stick_mouse_graph: stick_mouse_graph(&profile),
            touchpad: crate::touchpad::NativeTouchpad::new(profile.touchpad.clone()),
            raw_signals,
            controls,
            mapped_actions,
            action_states: competitions
                .into_iter()
                .map(|competition| ActionState {
                    competition,
                    ..Default::default()
                })
                .collect(),
            action_time_ns: 0,
            gyro: gyro_graph(&profile),
            motion: MotionRuntime::new(&profile.gyro)?,
            sensor_calibration: SensorCalibration::default(),
            flick: flick_graph(&profile),
            binding_states: (0..profile.gyro.motion.bindings.len())
                .map(|_| BindingState::default())
                .collect(),
            turn_held: vec![false; profile.gyro.motion.turns.len()],
            clock: SampleClock::new(profile.gyro.timestamp_source),
            frame: Frame::default(),
            physical_pad: (profile.virtual_gamepad != GamepadTarget::None).then(PadState::default),
            profile,
            signals,
            gyro_signals: SigMap::default(),
            active: false,
            last_paused: false,
            activation_pressed: false,
            gyro_latched: false,
            gyro_events: 0,
            invalid_values: 0,
            mouse_total: [0.0; 2],
            bias_rad_s: [0.0; 3],
            gyro_stick: Vec2::ZERO,
            last_stick_gyro_ns: None,
            accel: None,
            motion_held: BTreeSet::new(),
            motion_pad: vec![],
            controls_held: BTreeSet::new(),
            rotated_stick: None,
            flick_stick: Vec2::ZERO,
            flick_snapshot_blocked: false,
            turns: vec![],
            turn_stick_owed: 0.0,
            measuring: None,
            raw_sticks: [Vec2::ZERO; 2],
            stick_time: 0.,
            zone_layouts: [0, 1].map(|_| {
                let mut geometry = flexinput_core::area::AreaLayout::default_for(
                    flexinput_core::area::Shape::Circle,
                );
                geometry.edges = vec![flexinput_core::area::Border::hard(0.85)];
                geometry
            }),
        };
        this.refresh_sticks(0., false);
        this.eval_controls(false, 0);
        Ok(this)
    }
    pub fn graph_node_count(&self) -> usize {
        self.controls.graph.nodes.len() + self.gyro.graph.nodes.len()
    }
    /// Layer changes keep the sample clock, input bus, untouched activators,
    /// pending turns and motion filters. The caller keeps the same dispatcher.
    pub fn apply_layer(&mut self, mut profile: Profile) -> Result<()> {
        profile.materialize_gamepad_bindings();
        profile.validate()?;
        let touch_changed = profile.touchpad != self.profile.touchpad;
        if profile.virtual_gamepad != self.profile.virtual_gamepad {
            bail!("配置层不能改变虚拟输出设备");
        }
        self.motion
            .custom_filters
            .reconfigure(&profile.gyro.motion.custom_filters)?;
        if touch_changed {
            self.touchpad.configure(profile.touchpad.clone());
        }
        let input_changed =
            profile.input.parameters(&profile) != self.profile.input.parameters(&self.profile);
        if input_changed {
            self.input.configure(&profile);
        }
        if serde_json::to_value(&profile.stick_mouse)?
            != serde_json::to_value(&self.profile.stick_mouse)?
        {
            self.stick_mouse_graph = stick_mouse_graph(&profile);
        }
        let directions_changed = [
            profile.input.lstick_directions != self.profile.input.lstick_directions,
            profile.input.rstick_directions != self.profile.input.rstick_directions,
        ];
        let controls_changed = profile.actions != self.profile.actions
            || profile.stick_deadzone != self.profile.stick_deadzone
            || profile.gamepad_bindings != self.profile.gamepad_bindings
            || directions_changed.iter().any(|changed| *changed);
        if controls_changed {
            let actions = mapped_actions(&profile);
            let (controls, competitions) = controls(&profile, &actions);
            let mut old_states: Vec<_> = std::mem::take(&mut self.action_states)
                .into_iter()
                .map(Some)
                .collect();
            self.frame.scroll = [0; 2];
            self.action_states = actions
                .iter()
                .zip(competitions)
                .map(|(action, competition)| {
                    let previous = self
                        .mapped_actions
                        .iter()
                        .enumerate()
                        .find(|(i, old)| *old == action && old_states[*i].is_some())
                        .map(|(i, _)| i);
                    let geometry_changed =
                        ["left", "right"]
                            .iter()
                            .zip(directions_changed)
                            .any(|(side, changed)| {
                                changed
                                    && action.input.split('+').any(|part| {
                                        part.trim()
                                            .strip_prefix(&format!("{side}_stick_"))
                                            .is_some_and(|suffix| {
                                                matches!(suffix, "left" | "right" | "up" | "down")
                                            })
                                    })
                            });
                    if let Some(index) = previous.filter(|&index| {
                        !geometry_changed
                            && old_states[index].as_ref().unwrap().competition == competition
                            && same_competitors(action, &self.mapped_actions, &actions)
                    }) {
                        let state = old_states[index].take().unwrap();
                        for (value, delta) in self.frame.scroll.iter_mut().zip(state.pending_scroll)
                        {
                            *value += delta;
                        }
                        state
                    } else {
                        ActionState {
                            raw: true,
                            active: true,
                            blocked: action.trigger != Trigger::Down
                                || action.turbo_ms.is_some()
                                || !competition.is_empty()
                                || previous.is_some_and(|i| {
                                    !old_states[i].as_ref().unwrap().competition.is_empty()
                                }),
                            competition,
                            ..Default::default()
                        }
                    }
                })
                .collect();
            self.controls = controls;
            self.mapped_actions = actions;
        }
        if std::mem::discriminant(&profile.gyro.timestamp_source)
            != std::mem::discriminant(&self.profile.gyro.timestamp_source)
        {
            self.clock = SampleClock::new(profile.gyro.timestamp_source);
        }
        if serde_json::to_value(&profile.gyro.motion)?
            != serde_json::to_value(&self.profile.gyro.motion)?
        {
            self.motion.rebuild_builtin(&profile.gyro);
            self.flick = flick_graph(&profile);
            self.binding_states = (0..profile.gyro.motion.bindings.len())
                .map(|_| BindingState::default())
                .collect();
            self.turn_held = profile
                .gyro
                .motion
                .turns
                .iter()
                .map(|turn| self.input_value(&turn.input) > profile.gyro.activation_threshold)
                .collect();
            self.turns.clear();
            self.turn_stick_owed = 0.;
            self.flick_stick = Vec2::ZERO;
            self.flick_snapshot_blocked =
                self.stick_value(profile.gyro.motion.flick.stick).length()
                    >= profile.gyro.motion.flick.deadzone;
        }
        if profile.gyro.activation != self.profile.gyro.activation
            || profile.gyro.activation_mode != self.profile.gyro.activation_mode
        {
            self.activation_pressed =
                self.input_value(&profile.gyro.activation) > profile.gyro.activation_threshold;
            self.gyro_latched = false;
        }
        let gyro_output_changed = profile.gyro.output != self.profile.gyro.output;
        if gyro_output_changed {
            self.gyro = gyro_graph(&profile);
            self.gyro_stick = Vec2::ZERO;
            self.flick_stick = Vec2::ZERO;
            self.turn_stick_owed = 0.;
        } else {
            for (key, value) in [
                ("scale", profile.gyro.counts_per_degree),
                ("rws", profile.gyro.sensitivity),
                ("gyro_vh_ratio", profile.gyro.vertical_ratio),
                ("stick_out_dps", profile.gyro.stick_full_scale_dps),
            ] {
                self.gyro.graph.nodes[1]
                    .params
                    .insert(key.into(), json!(value));
            }
        }
        let flick_params = &mut self.flick.graph.nodes[1].params;
        flick_params.insert("scale".into(), json!(profile.gyro.counts_per_degree));
        flick_params.insert(
            "stick_out_dps".into(),
            json!(profile.gyro.stick_full_scale_dps),
        );
        self.profile = profile;
        self.refresh_sticks(self.stick_time, true);
        if input_changed || gyro_output_changed {
            self.flick.state.clear();
            self.flick_snapshot_blocked = self
                .stick_value(self.profile.gyro.motion.flick.stick)
                .length()
                >= self.profile.gyro.motion.flick.deadzone;
        }
        self.eval_controls(true, self.action_time_ns);
        if touch_changed {
            self.advance_touch((self.stick_time * 1e9) as u64, true);
        }
        Ok(())
    }
    fn refresh_sticks(&mut self, time: f64, filter: bool) {
        let time = self.stick_time.max(time);
        let dt = (time - self.stick_time).clamp(0., 0.05) as f32;
        self.stick_time = time;
        let previous: [bool; 2] = ["left_stick_outer", "right_stick_outer"].map(|pin| {
            self.signals
                .get_pin(SOURCE, pin)
                .is_some_and(|s| s.as_bool())
        });
        for (i, prefix) in ["left", "right"].into_iter().enumerate() {
            self.raw_signals.set_pin(
                SOURCE,
                &format!("{prefix}_stick"),
                Signal::Vec2(self.raw_sticks[i]),
            );
        }
        self.input.tick(&self.raw_signals, dt, !filter);
        self.base_input.tick(&self.raw_signals, dt, !filter);
        self.signals.clone_from(&self.input.signals);
        for (i, (prefix, stick, limit)) in [
            ("left", Stick::Left, self.profile.stick_zones.left_outer),
            ("right", Stick::Right, self.profile.stick_zones.right_outer),
        ]
        .into_iter()
        .enumerate()
        {
            let pin = format!("{prefix}_stick");
            // Studio's controls and gyro run in separate graphs. Keep the
            // original processed vector on the input bus for the RWS sweep.
            let value = if self.measuring.is_some() && stick == self.profile.gyro.motion.flick.stick
            {
                self.signals.set_pin(SOURCE, &pin, Signal::Vec2(Vec2::ZERO));
                self.signals
                    .set_pin(SOURCE, &format!("{pin}_x"), Signal::Float(0.));
                self.signals
                    .set_pin(SOURCE, &format!("{pin}_y"), Signal::Float(0.));
                Vec2::ZERO
            } else {
                self.input.stick(&pin)
            };
            let directions = [
                self.profile.input.lstick_directions,
                self.profile.input.rstick_directions,
            ][i];
            for (direction, value) in ["left", "right", "up", "down"]
                .into_iter()
                .zip(directions.values(value))
            {
                self.signals.set_pin(
                    SOURCE,
                    &format!("{prefix}_stick_{direction}"),
                    Signal::Float(value),
                );
            }
            let region = stick_region(
                &mut self.zone_layouts[i],
                value,
                limit,
                self.profile.stick_zones.hysteresis,
                previous[i],
            );
            self.signals.set_pin(
                SOURCE,
                &format!("{prefix}_stick_outer"),
                Signal::Bool(region == StickRegion::Outer),
            );
            self.signals.set_pin(
                SOURCE,
                &format!("{prefix}_stick_inner"),
                Signal::Bool(region == StickRegion::Inner),
            );
        }
    }
    pub fn processed_axes(&self) -> [f32; 6] {
        let mut axes: [f32; 6] = std::array::from_fn(|i| {
            input_pin(AXES[i])
                .and_then(|pin| self.input.signals.get_pin(SOURCE, pin))
                .map_or(0., |signal| signal.as_float())
        });
        axes[1] = -axes[1];
        axes[3] = -axes[3];
        axes
    }

    pub fn base_trigger_inputs(&self) -> [Option<&'static str>; 4] {
        TRIGGER_STAGE_PINS.map(|pin| {
            self.base_input
                .signals
                .get_pin(SOURCE, pin)
                .is_some_and(|v| v.as_bool())
                .then_some(pin)
        })
    }

    /// Global shortcuts use the base profile's calibrated stick and ring bounds.
    pub fn base_stick_regions(
        &mut self,
        zones: &StickZones,
        previous: [StickRegion; 2],
    ) -> [StickRegion; 2] {
        let limits = [zones.left_outer, zones.right_outer];
        std::array::from_fn(|i| {
            let value = self.base_input.stick(["left_stick", "right_stick"][i]);
            let layout = &mut self.zone_layouts[i];
            let old_limit = layout.edges[0].pos;
            let region = stick_region(
                layout,
                value,
                limits[i],
                zones.hysteresis,
                previous[i] == StickRegion::Outer,
            );
            layout.edges[0].pos = old_limit;
            region
        })
    }
    pub fn reset(&mut self) {
        self.touchpad.reset();
        self.stick_time = 0.;
        self.action_time_ns = 0;
        self.input.reset();
        self.base_input.reset();
        self.stick_mouse_graph.state.clear();
        self.raw_sticks = [Vec2::ZERO; 2];
        for value in self.raw_signals.values_mut() {
            *value = value.zeroed();
        }
        self.clock.reset();
        self.controls.state.clear();
        for state in &mut self.action_states {
            *state = ActionState {
                competition: state.competition,
                ..Default::default()
            };
        }
        self.clear_gyro_motion();
        self.accel = None;
        self.flick.state.clear();
        self.flick_stick = Vec2::ZERO;
        self.flick_snapshot_blocked = false;
        self.turns.clear();
        self.turn_held.fill(false);
        self.turn_stick_owed = 0.;
        self.measuring = None;
        self.gyro.graph.nodes[1].params.remove("cal_measure");
        self.activation_pressed = false;
        self.gyro_latched = false;
        self.last_paused = false;
        self.frame.mouse_delta = [0.0; 2];
        self.frame.scroll = [0; 2];
        self.refresh_sticks(0., false);
        self.eval_controls(false, 0);
    }
    fn update_pad(&mut self) {
        self.frame.pad = self.physical_pad.map(|mut pad| {
            if self.measuring.is_some() {
                pad.right_x = 0;
                pad.right_y = 0;
            }
            let f = &self.profile.gyro.motion.flick;
            let suppress_flick = self.flick_active()
                && (f.suppress == Suppress::Full
                    || (f.suppress == Suppress::Deadzone
                        && self.stick_value(f.stick).length() >= f.deadzone));
            if suppress_flick || self.rotated_stick.is_some() {
                for binding in self.profile.gamepad_bindings.iter().flatten() {
                    let (Some(input), Some(output)) = (
                        pad_binding::stick(&binding.input),
                        pad_binding::stick(&binding.output),
                    ) else {
                        continue;
                    };
                    let value = self
                        .rotated_stick
                        .filter(|(stick, _)| *stick == input)
                        .map(|(_, value)| value)
                        .or_else(|| (suppress_flick && input == f.stick).then_some(Vec2::ZERO));
                    if let Some(value) = value {
                        for (pin, component) in pad_binding::stick_pins(output)
                            .into_iter()
                            .zip([value.x, value.y])
                        {
                            pad.set(pin, Signal::Float(component));
                        }
                    }
                }
            }
            let aim = self.gyro_stick + self.flick_stick;
            pad.right_x = axis_to_i16(axis_from_i16(pad.right_x) + aim.x);
            pad.right_y = axis_to_i16(axis_from_i16(pad.right_y) + aim.y);
            for (pin, v) in &self.motion_pad {
                match pin.as_str() {
                    "left_stick_x" => pad.left_x = axis_to_i16(axis_from_i16(pad.left_x) + v),
                    "left_stick_y" => pad.left_y = axis_to_i16(axis_from_i16(pad.left_y) + v),
                    "right_stick_x" => pad.right_x = axis_to_i16(axis_from_i16(pad.right_x) + v),
                    "right_stick_y" => pad.right_y = axis_to_i16(axis_from_i16(pad.right_y) + v),
                    "left_trigger" => {
                        pad.left_trigger = pad.left_trigger.max((v.max(0.) * 255.).round() as u8)
                    }
                    "right_trigger" => {
                        pad.right_trigger = pad.right_trigger.max((v.max(0.) * 255.).round() as u8)
                    }
                    _ => {
                        if *v > 0. {
                            pad.set(pin, Signal::Bool(true));
                        }
                    }
                }
            }
            pad
        });
        self.frame.held = self
            .controls_held
            .union(&self.motion_held)
            .cloned()
            .chain(self.touchpad.held.iter().cloned())
            .collect();
    }
    fn clear_gyro_motion(&mut self) {
        let measurement = self
            .measuring
            .as_ref()
            .map(|_| self.motion.telemetry.clone());
        self.motion.reset(&self.profile.gyro);
        if let Some(t) = measurement {
            self.motion.measurement_mode();
            self.motion.telemetry.measuring = self.measuring.clone();
            self.motion.telemetry.measured_degrees = t.measured_degrees;
            self.motion.telemetry.measurement_peak = t.measurement_peak;
            self.motion.telemetry.measurement_error = t.measurement_error;
        }
        self.gyro.state.clear();
        self.gyro_stick = Vec2::ZERO;
        self.last_stick_gyro_ns = None;
        self.rotated_stick = None;
        self.motion_held.clear();
        self.motion_pad.clear();
        for state in &mut self.binding_states {
            *state = BindingState::default();
        }
        self.update_pad();
    }
    fn pressed(&self, input: &Option<String>) -> bool {
        input
            .as_deref()
            .is_some_and(|i| self.input_value(i) > self.profile.gyro.activation_threshold)
    }
    fn paused(&self) -> bool {
        self.pressed(&self.profile.gyro.pause_input)
    }
    fn flick_active(&self) -> bool {
        let g = &self.profile.gyro;
        let f = &g.motion.flick;
        g.enabled
            && !self.paused()
            && self.measuring.is_none()
            && (f.enabled || f.stick_aim)
            && (f.enable_input.is_none() || self.pressed(&f.enable_input))
    }
    fn stick_value(&self, stick: Stick) -> Vec2 {
        let (x, y) = match stick {
            Stick::Left => ("left_stick_x", "left_stick_y"),
            Stick::Right => ("right_stick_x", "right_stick_y"),
        };
        Vec2::new(self.input_value(x), self.input_value(y))
    }
    pub fn input_value(&self, input: &str) -> f32 {
        input_pin(input)
            .and_then(|pin| self.signals.get_pin(SOURCE, pin))
            .map_or(0.0, |signal| signal.as_float())
    }
    /// Hold modifiers use physical values and base ring geometry. A layer may
    /// change filtering, deadzones or rings without changing its own trigger.
    pub fn layer_input_value(&self, input: &str, base: &Profile, active: bool) -> f32 {
        let (index, part) = if let Some(part) = input.strip_prefix("left_stick_") {
            (0, part)
        } else if let Some(part) = input.strip_prefix("right_stick_") {
            (1, part)
        } else {
            return input_pin(input)
                .and_then(|pin| self.base_input.signals.get_pin(SOURCE, pin))
                .map_or(0., |v| v.as_float());
        };
        let value = self.base_input.stick(if index == 0 {
            "left_stick"
        } else {
            "right_stick"
        });
        let directions =
            [base.input.lstick_directions, base.input.rstick_directions][index].values(value);
        match part {
            "left" => directions[0],
            "right" => directions[1],
            "up" => directions[2],
            "down" => directions[3],
            "inner" | "outer" => {
                let mut geometry = self.zone_layouts[index].clone();
                let previous_outer = if part == "outer" { active } else { !active };
                let limit = [base.stick_zones.left_outer, base.stick_zones.right_outer][index];
                let region = stick_region(
                    &mut geometry,
                    value,
                    limit,
                    base.stick_zones.hysteresis,
                    previous_outer,
                );
                let pressed = region
                    == if part == "outer" {
                        StickRegion::Outer
                    } else {
                        StickRegion::Inner
                    };
                if pressed {
                    1.
                } else {
                    0.
                }
            }
            _ => self.input_value(input),
        }
    }
    fn eval_controls(&mut self, allow_toggle: bool, now_ns: u64) {
        self.action_time_ns = self.action_time_ns.max(now_ns);
        self.controls.eval(&self.signals, 0.0);
        self.controls_held.clear();
        if let Some(pad) = &mut self.physical_pad {
            *pad = PadState::default();
            for (pin, &value) in self.controls.result.sink_outputs.pins_of(PAD) {
                pad.set(pin, value);
            }
        }
        for (i, (action, state)) in self
            .mapped_actions
            .iter()
            .zip(&mut self.action_states)
            .enumerate()
        {
            let get = |pin: &str| {
                self.controls
                    .result
                    .sink_outputs
                    .get_pin(ACTIONS, pin)
                    .is_some_and(|v| v.as_bool())
            };
            let condition = get(&i.to_string());
            let raw = get(&format!("raw:{i}"));
            let immediate = action.trigger == Trigger::Down && state.competition.is_empty();
            if !immediate && raw && (!allow_toggle || !condition) {
                state.blocked = true;
            }
            let active = if state.blocked {
                if !raw {
                    state.blocked = false;
                    state.gate = Default::default();
                }
                false
            } else {
                state.gate.sample_at(
                    condition,
                    action.trigger.as_str(),
                    action.trigger_ms,
                    action.sustain,
                    action.turbo_ms,
                    state.competition,
                    self.action_time_ns,
                )
            };
            if active {
                if let Some(pin) = action.output.strip_prefix("pad:") {
                    if let Some(pad) = &mut self.physical_pad {
                        pad.set(pin, Signal::Bool(true));
                        match pin {
                            "left_trigger" => pad.left_trigger_pressed = true,
                            "right_trigger" => pad.right_trigger_pressed = true,
                            _ => {}
                        }
                    }
                } else {
                    for output in crate::bindings::output_chord(&action.output).unwrap() {
                        if crate::bindings::wheel_delta(output).is_none() {
                            self.controls_held.insert(output.into());
                        }
                    }
                }
            }
            let rising = if immediate && action.turbo_ms.is_none() {
                raw && !state.raw
            } else {
                !state.active
            };
            if active && rising && allow_toggle {
                for output in crate::bindings::output_chord(&action.output)
                    .into_iter()
                    .flatten()
                {
                    if let Some(delta) = crate::bindings::wheel_delta(output) {
                        for (i, d) in delta.into_iter().enumerate() {
                            self.frame.scroll[i] += d;
                            state.pending_scroll[i] += d;
                        }
                    }
                }
                state.repeat_armed = action.repeat_ms.is_some();
                state.elapsed = 0.;
            }
            if !active {
                state.repeat_armed = false;
                state.elapsed = 0.;
            }
            state.raw = raw;
            state.active = active;
        }
        let gyro = &self.profile.gyro;
        let toggle = gyro.activation_mode == GyroActivationMode::Toggle;
        // A lower release threshold prevents an analog input from repeatedly
        // toggling while it hovers around the press threshold.
        let release_band = if toggle && self.activation_pressed {
            0.1
        } else {
            0.0
        };
        let pressed = self.input_value(&gyro.activation)
            > (gyro.activation_threshold - release_band).max(0.0);
        if toggle && allow_toggle && pressed && !self.activation_pressed {
            self.gyro_latched = !self.gyro_latched;
        }
        self.activation_pressed = pressed;
        let paused = gyro
            .pause_input
            .as_deref()
            .is_some_and(|input| self.input_value(input) > gyro.activation_threshold);
        let active = gyro.enabled
            && !paused
            && (gyro.activation == "always" || if toggle { self.gyro_latched } else { pressed });
        if paused != self.last_paused {
            self.clock.reset();
            self.clear_gyro_motion();
            if self.measuring.is_some() {
                self.motion.telemetry.measurement_error = Some("测量期间已暂停，请重新测量".into());
            }
        } else if active != self.active && self.measuring.is_none() {
            self.clock.reset();
            self.motion.reset_filters();
            self.gyro.state.clear();
            self.gyro_stick = Vec2::ZERO;
            self.rotated_stick = None;
        } else {
            self.update_pad();
        }
        self.last_paused = paused;
        self.active = active;
        if let Some((stick, _)) = self.rotated_stick {
            if self.profile.gyro.motion.mode == MotionMode::StickRotation
                && self.stick_value(stick).length() < self.profile.gyro.motion.rotation.deadzone
            {
                self.rotated_stick = None;
                self.motion.rotate(&self.profile.gyro, Vec2::ZERO, 0., 0.);
            }
        }
        for i in 0..self.profile.gyro.motion.turns.len() {
            let t = &self.profile.gyro.motion.turns[i];
            let pressed = self.input_value(&t.input) > self.profile.gyro.activation_threshold;
            if allow_toggle
                && pressed
                && !self.turn_held[i]
                && self.profile.gyro.enabled
                && !self.paused()
                && self.measuring.is_none()
            {
                self.turns.push(PendingTurn {
                    degrees: t.degrees,
                    seconds: t.duration_ms / 1000.,
                    output: t.output,
                });
            }
            self.turn_held[i] = pressed;
        }
        if self.paused() {
            self.flick.state.clear();
            self.flick_stick = Vec2::ZERO;
            self.turns.clear();
            self.turn_stick_owed = 0.;
        }
        self.update_pad();
    }
    pub fn process(&mut self, event: &InputEvent) -> &Frame {
        self.process_event(event, true)
    }
    /// Seed controls from a device snapshot. A held control is not a press
    /// edge and must not toggle gyro on after a reload or reconnect.
    pub fn process_snapshot(&mut self, event: &InputEvent) -> &Frame {
        self.process_event(event, false)
    }
    fn clear_frame_deltas(&mut self) {
        self.frame.mouse_delta = [0.0; 2];
        self.frame.scroll = [0; 2];
        for state in &mut self.action_states {
            state.pending_scroll = [0; 2];
        }
    }
    fn process_event(&mut self, event: &InputEvent, allow_toggle: bool) -> &Frame {
        self.clear_frame_deltas();
        let now_ns = event.time().host_ns;
        match *event {
            InputEvent::Button { index, down, .. } => {
                if let Some(pin) = BUTTON_PINS.get(index as usize) {
                    self.raw_signals.set_pin(SOURCE, pin, Signal::Bool(down));
                    self.refresh_sticks(now_ns as f64 / 1e9, allow_toggle);
                    self.eval_controls(allow_toggle, now_ns);
                }
            }
            InputEvent::Axis { index, value, .. } => {
                if let Some(pin) = AXES.get(index as usize) {
                    let mut v = axis_from_i16(value);
                    if index == 1 || index == 3 {
                        v = -v;
                    } // SDL: down +; Flex/XInput: up +.
                    if index >= 4 {
                        v = v.max(0.0);
                    }
                    self.raw_signals.set_pin(SOURCE, pin, Signal::Float(v));
                    if index < 4 {
                        self.raw_sticks[index as usize / 2][index as usize % 2] = v;
                    }
                    self.refresh_sticks(now_ns as f64 / 1e9, allow_toggle);
                    let f = &self.profile.gyro.motion.flick;
                    let length = self.stick_value(f.stick).length();
                    if !allow_toggle && f.enabled && length >= f.deadzone {
                        self.flick_snapshot_blocked = true;
                    }
                    if allow_toggle && length < f.deadzone * 0.8 {
                        self.flick_snapshot_blocked = false;
                    }
                    self.eval_controls(allow_toggle, now_ns);
                }
            }
            InputEvent::Touch {
                pad,
                finger,
                active,
                x,
                y,
                ..
            } => {
                self.touchpad.contact(pad, finger, active, x, y);
                self.refresh_touch_signals();
                self.refresh_sticks(now_ns as f64 / 1e9, allow_toggle);
                self.eval_controls(allow_toggle, now_ns);
            }
            InputEvent::Gyro { time, value } => {
                self.process_gyro(time, value);
            }
            InputEvent::Accel { time, value } => {
                if value.iter().all(|v| v.is_finite() && v.abs() < 200.) && time.host_ns > 0 {
                    if self.accel.is_none_or(|(last, _)| time.host_ns >= last) {
                        let c = &self.sensor_calibration;
                        self.accel = Some((
                            time.host_ns,
                            motion::accel_bus(motion::calibrated(
                                value,
                                c.accel_offset,
                                c.accel_scale,
                                c.accel_sign,
                                c,
                            )),
                        ));
                    }
                } else {
                    self.accel = None;
                    self.clear_gyro_motion();
                }
            }
            InputEvent::Disconnected { .. } => self.reset(),
        }
        if matches!(
            event,
            InputEvent::Touch { .. } | InputEvent::Button { .. } | InputEvent::Axis { .. }
        ) {
            self.advance_touch(now_ns, !allow_toggle);
        }
        &self.frame
    }

    pub fn set_touchpad_count(&mut self, count: u16) {
        self.touchpad.set_count(count);
        self.refresh_touch_signals();
        self.advance_touch((self.stick_time * 1e9) as u64, true);
    }
    fn refresh_touch_signals(&mut self) {
        for (pin, contact) in crate::profile::TOUCH_PINS
            .iter()
            .zip(self.touchpad.fields())
        {
            self.raw_signals
                .set_pin(SOURCE, pin, Signal::Bool(contact.active));
        }
    }
    pub fn touch_contacts(&self) -> crate::touchpad::Contacts {
        self.touchpad.contacts
    }
    fn advance_touch(&mut self, now_ns: u64, snapshot: bool) {
        let now_ns = if now_ns == 0 {
            (self.stick_time * 1e9) as u64
        } else {
            now_ns
        };
        let clicks = ["btn_touchpad", "btn_misc2"].map(|pin| {
            self.raw_signals
                .get_pin(SOURCE, pin)
                .is_some_and(|s| s.as_bool())
        });
        let out = self.touchpad.tick(now_ns, clicks, snapshot);
        if self.measuring.is_none() {
            for (value, delta) in self.frame.mouse_delta.iter_mut().zip(out.mouse) {
                *value += delta;
            }
        }
        for (value, delta) in self.frame.scroll.iter_mut().zip(out.scroll) {
            *value += delta;
        }
        self.update_pad();
    }

    fn process_gyro(&mut self, time: SampleTime, value: [f32; 3]) {
        self.gyro_events += 1;
        if !value.iter().all(|v| v.is_finite() && v.abs() < 100.) {
            self.invalid_values += 1;
            self.clock.reset();
            self.clear_gyro_motion();
            if self.measuring.is_some() {
                self.motion.telemetry.measurement_error = Some("收到无效采样，请重新测量".into());
            }
            return;
        }
        // Even an equal-valued sample is a new interval.
        let previous_errors = self.clock.stats.invalid_timestamps + self.clock.stats.gaps;
        let Some(dt) = self.clock.step(time) else {
            self.clear_gyro_motion();
            if self.measuring.is_some()
                && self.clock.stats.invalid_timestamps + self.clock.stats.gaps > previous_errors
            {
                self.motion.telemetry.measurement_error = Some("采样时间中断，请重新测量".into());
            }
            return;
        };
        if time.host_ns == 0 {
            self.clear_gyro_motion();
            return;
        }
        if !self.profile.gyro.enabled {
            return;
        }
        self.last_stick_gyro_ns = Some(time.host_ns);
        let accel = self
            .accel
            .filter(|(last, _)| {
                time.host_ns
                    .checked_sub(*last)
                    .is_some_and(|age| age < MAX_GAP_NS)
            })
            .map(|(_, v)| v);
        if self.profile.gyro.motion.needs_accel() && accel.is_none() {
            self.clear_gyro_motion();
            if self.measuring.is_some() {
                self.motion.telemetry.measurement_error = Some("加速度采样中断，请重新测量".into());
            }
            return;
        }
        let c = &self.sensor_calibration;
        let raw = motion::gyro_bus(motion::calibrated(
            value,
            self.bias_rad_s,
            [1.; 3],
            c.gyro_sign,
            c,
        ));
        let reset = self.pressed(&self.profile.gyro.motion.reset_input);
        let neutral = self.pressed(&self.profile.gyro.motion.neutral_input);
        let tracking = [
            self.pressed(&self.profile.gyro.motion.trackball.horizontal_input),
            self.pressed(&self.profile.gyro.motion.trackball.vertical_input),
        ];
        let result = self.motion.sample(
            &self.profile.gyro,
            raw,
            accel,
            dt,
            reset,
            neutral,
            tracking,
            c.gyro_noise_dps,
            c.accel_noise_g,
            time,
            self.measuring.is_none(),
        );
        if self.paused() && self.measuring.is_some() {
            self.gyro_stick = Vec2::ZERO;
            self.update_pad();
            return;
        }
        if !self.paused() && self.measuring.is_none() {
            self.update_motion_bindings(dt);
        } else {
            self.motion_held.clear();
            self.motion_pad.clear();
        }
        if !self.active && self.measuring.is_none() {
            self.update_pad();
            return;
        }
        if self.measuring.is_none() {
            if let Some(steering) = result.steering {
                self.gyro_stick = steering;
                self.update_pad();
                return;
            }
            if self.profile.gyro.motion.mode == MotionMode::StickRotation {
                let stick = self.profile.gyro.motion.rotation.stick;
                let value = self.stick_value(stick);
                let rotated = self
                    .motion
                    .rotate(&self.profile.gyro, value, result.rate.x, dt);
                self.rotated_stick = Some((stick, rotated));
                self.update_pad();
                return;
            }
        }
        let rate = if self.measuring.is_some() {
            result.physical_rate
        } else {
            result.rate
        };
        self.gyro_signals
            .set_pin(SOURCE, "aim_rate", Signal::Vec2(rate / BUS_GYRO_DPS));
        if self.measuring.as_deref() == Some("yaw") {
            let pin = match self.profile.gyro.motion.flick.stick {
                Stick::Left => "left_stick",
                Stick::Right => "right_stick",
            };
            self.gyro_signals.set_pin(
                SOURCE,
                "calibration_stick",
                Signal::Vec2(self.input.stick(pin)),
            );
        }
        self.gyro.eval(&self.gyro_signals, dt);
        if self.measuring.is_some() {
            self.motion.telemetry.measured_degrees = self
                .gyro
                .result
                .sink_outputs
                .get_pin(GYRO, "measured")
                .map_or(0., |s| s.as_float());
            self.motion.telemetry.measurement_peak = self
                .gyro
                .result
                .sink_outputs
                .get_pin(GYRO, "peak")
                .map_or(0., |s| s.as_float());
            self.motion.telemetry.measuring = self.measuring.clone();
        }
        let Some(&Signal::Vec2(aim)) = self.gyro.result.sink_outputs.get_pin(GYRO, GYRO_AIM) else {
            return;
        };
        match self.profile.gyro.output {
            GyroOutput::Mouse => {
                self.frame.mouse_delta = [f64::from(aim.x), f64::from(aim.y)];
                self.mouse_total[0] += f64::from(aim.x);
                self.mouse_total[1] += f64::from(aim.y);
            }
            GyroOutput::RightStick if time.host_ns != 0 => {
                // RWS uses screen Y (down positive); gamepad Y is up positive.
                self.gyro_stick = Vec2::new(aim.x, -aim.y);
                self.last_stick_gyro_ns = Some(time.host_ns);
                self.update_pad();
            }
            GyroOutput::RightStick => {
                self.clock.reset();
                self.clear_gyro_motion();
            }
        }
        self.update_pad();
    }

    fn update_motion_bindings(&mut self, dt: f32) {
        self.motion_held.clear();
        self.motion_pad.clear();
        for (b, state) in self
            .profile
            .gyro
            .motion
            .bindings
            .iter()
            .zip(&mut self.binding_states)
        {
            let p = flexinput_core::imu::imu_pin(&b.input).expect("validated motion input");
            let physical = p.physical(&self.motion.reading);
            let value = sample_curve(&b.curve, (physical.abs() / b.range).clamp(0., 1.), &[])
                * physical.signum();
            let pressed =
                value.abs() > (b.threshold - if state.pressed { 0.05 } else { 0. }).max(0.);
            if pressed && !state.pressed {
                state.latched = !state.latched;
                state.phase = 0.;
            }
            if !pressed {
                state.phase = 0.;
            } else {
                state.phase = (state.phase + dt) % (b.turbo_ms / 1000.);
            }
            state.pressed = pressed;
            let on = match b.mode {
                BindingMode::Hold => pressed,
                BindingMode::Toggle => state.latched,
                BindingMode::Turbo => pressed && state.phase < b.turbo_ms / 2000.,
            };
            if on {
                if let Some(pin) = b.output.strip_prefix("pad:") {
                    self.motion_pad.push((
                        pin.into(),
                        if b.mode == BindingMode::Hold {
                            if pin.contains("stick_")
                                && (pin.ends_with("_x") || pin.ends_with("_y"))
                            {
                                value
                            } else {
                                value.abs()
                            }
                        } else {
                            1.
                        },
                    ));
                } else {
                    for output in crate::bindings::output_chord(&b.output).unwrap() {
                        if let Some(delta) = crate::bindings::wheel_delta(output) {
                            if !state.emitting {
                                for (v, d) in self.frame.scroll.iter_mut().zip(delta) {
                                    *v += d;
                                }
                            }
                        } else {
                            self.motion_held.insert(output.into());
                        }
                    }
                }
            }
            state.emitting = on;
        }
    }

    pub fn begin_measurement(&mut self, axis: &str) -> Result<()> {
        if self.measuring.is_some() {
            bail!("请先结束或取消当前转角测量");
        }
        if !["yaw", "pitch"].contains(&axis)
            || !self.profile.gyro.enabled
            || self.profile.gyro.motion.mode != MotionMode::Pointer
            || self.paused()
        {
            bail!("转角测量需要启用角速度瞄准模式，并选择水平或垂直轴");
        }
        self.clear_gyro_motion();
        self.clock.reset();
        self.turns.clear();
        self.turn_stick_owed = 0.;
        self.flick.state.clear();
        self.flick_stick = Vec2::ZERO;
        self.measuring = Some(axis.into());
        self.motion.measurement_mode();
        self.gyro.graph.nodes[1]
            .params
            .insert("cal_measure".into(), json!(axis));
        self.gyro.graph.nodes[1].params.insert(
            "cal_output".into(),
            json!(if self.profile.gyro.output == GyroOutput::Mouse {
                "mouse"
            } else {
                "stick"
            }),
        );
        self.motion.telemetry.measuring = self.measuring.clone();
        self.refresh_sticks(self.stick_time, true);
        self.eval_controls(false, self.action_time_ns);
        Ok(())
    }
    pub fn is_measuring(&self) -> bool {
        self.measuring.is_some()
    }
    pub fn reset_pose(&mut self) {
        self.clear_gyro_motion();
        self.clock.reset();
        if self.measuring.is_some() {
            self.motion.telemetry.measurement_error = Some("测量期间姿态已重置，请重新测量".into());
        }
    }
    pub fn end_measurement(&mut self) -> Option<GameAngleMeasurement> {
        let axis = self.measuring.take()?;
        let gyro = &self.profile.gyro;
        let result = GameAngleMeasurement {
            axis,
            degrees: self.motion.telemetry.measured_degrees,
            peak: self.motion.telemetry.measurement_peak,
            error: self.motion.telemetry.measurement_error.clone(),
            output: gyro.output,
            scale: match gyro.output {
                GyroOutput::Mouse => gyro.counts_per_degree,
                GyroOutput::RightStick => gyro.stick_full_scale_dps,
            },
        };
        self.gyro.graph.nodes[1].params.remove("cal_measure");
        self.gyro.state.clear();
        self.gyro_stick = Vec2::ZERO;
        self.clock.reset();
        self.motion.telemetry.measuring = None;
        self.refresh_sticks(self.stick_time, true);
        let flick = &self.profile.gyro.motion.flick;
        self.flick_snapshot_blocked = self.stick_value(flick.stick).length() >= flick.deadzone;
        self.eval_controls(false, self.action_time_ns);
        Some(result)
    }

    fn advance_aim(&mut self, dt: f32) {
        if !self.profile.gyro.enabled || self.paused() || self.measuring.is_some() {
            self.flick_stick = Vec2::ZERO;
            self.update_pad();
            return;
        }
        let f = &self.profile.gyro.motion.flick;
        let value = if self.flick_snapshot_blocked {
            Vec2::ZERO
        } else {
            self.stick_value(f.stick)
        };
        let on = self.flick_active();
        self.gyro_signals.set_pin(
            SOURCE,
            if f.stick == Stick::Left {
                "left_stick"
            } else {
                "right_stick"
            },
            Signal::Vec2(value),
        );
        self.gyro_signals
            .set_pin(SOURCE, "flick_on", Signal::Bool(on));
        self.flick.eval(&self.gyro_signals, dt);
        self.flick_stick = Vec2::ZERO;
        if !on {
            self.flick.state.clear();
        } else {
            let outputs = &self.flick.result.sink_outputs;
            let flick_counts = match outputs.get_pin(GYRO, "flick_angle") {
                Some(Signal::Vec2(v)) => v.x,
                _ => 0.,
            };
            match self.profile.gyro.output {
                GyroOutput::Mouse => {
                    if let Some(Signal::Vec2(v)) = outputs.get_pin(GYRO, "flick_mouse") {
                        self.frame.mouse_delta[0] += f64::from(v.x);
                        self.frame.mouse_delta[1] -= f64::from(v.y);
                    }
                    self.frame.mouse_delta[0] += f64::from(flick_counts);
                }
                GyroOutput::RightStick => {
                    self.turn_stick_owed += flick_counts / self.profile.gyro.counts_per_degree;
                    if let Some(Signal::Vec2(v)) = outputs.get_pin(GYRO, "flick_pad") {
                        self.flick_stick = *v;
                    }
                }
            }
        }
        self.turns.retain_mut(|t| {
            let step = if t.seconds <= dt {
                t.degrees
            } else {
                t.degrees * dt / t.seconds
            };
            t.degrees -= step;
            t.seconds = (t.seconds - dt).max(0.);
            if t.output != AimOutput::RightStick {
                self.frame.mouse_delta[0] += f64::from(step * self.profile.gyro.counts_per_degree);
            }
            if t.output != AimOutput::Mouse {
                self.turn_stick_owed += step;
            }
            t.seconds > 0.
        });
        self.update_pad();
        if self.turn_stick_owed != 0. {
            let base = self.frame.pad.map_or(0., |p| axis_from_i16(p.right_x));
            let cap = self.profile.gyro.stick_full_scale_dps * dt;
            let room = (1. - base * self.turn_stick_owed.signum()).max(0.) * cap;
            let emit = self.turn_stick_owed.abs().min(room) * self.turn_stick_owed.signum();
            self.flick_stick.x += emit / cap;
            self.turn_stick_owed -= emit;
            self.update_pad();
        }
    }

    fn tick_input(&mut self, now_ns: u64) {
        self.refresh_sticks(now_ns as f64 / 1e9, true);
        self.eval_controls(true, now_ns);
    }

    /// Advance mapping timers through a drained input timestamp. Returns
    /// whether the frame contains a discrete output change to dispatch now.
    pub fn advance_input(&mut self, now_ns: u64) -> bool {
        self.clear_frame_deltas();
        let previous_held = std::mem::take(&mut self.frame.held);
        let previous_pad = self.frame.pad;
        self.tick_input(now_ns);
        self.frame.held != previous_held
            || self.frame.pad != previous_pad
            || self.frame.scroll != [0; 2]
    }

    /// Combined tick for callers supplying a single input and output clock.
    pub fn advance(&mut self, dt: f32, now_ns: u64) -> &Frame {
        self.clear_frame_deltas();
        if dt.is_finite() && dt > 0. && dt <= 0.05 {
            let input_ns = if now_ns == 0 {
                let elapsed_ns = (f64::from(dt) * 1e9).round() as u64;
                self.action_time_ns.saturating_add(elapsed_ns)
            } else {
                now_ns
            };
            self.tick_input(input_ns);
        }
        self.tick_output(dt, now_ns)
    }

    /// Advance continuous output without deciding gestures ahead of SDL.
    pub fn advance_output(&mut self, dt: f32, now_ns: u64) -> &Frame {
        self.clear_frame_deltas();
        self.tick_output(dt, now_ns)
    }

    fn tick_output(&mut self, dt: f32, now_ns: u64) -> &Frame {
        if self
            .last_stick_gyro_ns
            .is_some_and(|last| now_ns.checked_sub(last).is_none_or(|age| age >= MAX_GAP_NS))
        {
            self.clock.reset();
            self.clear_gyro_motion();
            if self.measuring.is_some() {
                self.motion.telemetry.measurement_error = Some("体感采样超时，请重新测量".into());
            }
        }
        if !dt.is_finite() || dt <= 0. || dt > 0.05 {
            for state in &mut self.action_states {
                state.elapsed = 0.;
            }
            self.flick.state.clear();
            self.flick_stick = Vec2::ZERO;
            self.turns.clear();
            self.turn_stick_owed = 0.;
            self.update_pad();
            return &self.frame;
        }
        for (action, state) in self.mapped_actions.iter().zip(&mut self.action_states) {
            if !state.active || !state.repeat_armed {
                continue;
            }
            if let Some(ms) = action.repeat_ms {
                let period = ms as f32 / 1000.;
                state.elapsed += dt;
                if state.elapsed >= period {
                    let count = (state.elapsed / period).floor() as i32;
                    state.elapsed %= period;
                    for output in crate::bindings::output_chord(&action.output).unwrap() {
                        if let Some(delta) = crate::bindings::wheel_delta(output) {
                            for (i, d) in delta.into_iter().enumerate() {
                                self.frame.scroll[i] += d * count;
                                state.pending_scroll[i] += d * count;
                            }
                        }
                    }
                }
            }
        }
        self.advance_aim(dt);
        self.advance_touch(now_ns, false);
        let options = &self.profile.stick_mouse;
        if self.measuring.is_some() || !options.enabled {
            return &self.frame;
        }
        self.stick_mouse_graph.eval(&self.signals, dt);
        if let Some(Signal::Vec2(delta)) = self
            .stick_mouse_graph
            .result
            .sink_outputs
            .get_pin("studio.stick_mouse", "mouse_move")
        {
            self.frame.mouse_delta[0] += f64::from(delta.x);
            self.frame.mouse_delta[1] -= f64::from(delta.y);
        }
        &self.frame
    }
}
