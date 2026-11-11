//! Studio configuration for the original FlexInput physical-source pipeline.
//! Processing stays inside eval_graph_tick; this module only wires its I/O.
use crate::{
    engine::{node, GraphRunner},
    profile::{Profile, AXES, BUTTON_PINS},
};
use anyhow::{ensure, Result};
use flexinput_core::{PinMapExt, SigMap, Signal};
use flexinput_engine::eval::{
    apply_trigger_range, JsmAnalogSettings, JsmTriggerMode, JsmTriggerStages, JSM_TRIGGER_REST,
};
use flexinput_engine::graph::SinkTarget;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

pub(crate) const SOURCE: &str = "sdl:selected";
const PROCESSED: &str = "studio.processed_input";
pub const RADIAL_BUCKETS: usize = flexinput_engine::eval::STICK_RADIAL_BUCKETS;
pub const TRIGGER_STAGE_PINS: [&str; 4] = [
    "left_trigger_soft",
    "left_trigger_full",
    "right_trigger_soft",
    "right_trigger_full",
];

#[derive(Serialize, Deserialize)]
#[serde(remote = "JsmTriggerMode", rename_all = "snake_case")]
enum TriggerModeDef {
    NoFull,
    NoSkip,
    NoSkipExclusive,
    MustSkip,
    MaySkip,
    MustSkipR,
    MaySkipR,
    Pad(usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TriggerStagesConfig {
    #[serde(with = "TriggerModeDef")]
    pub left_mode: JsmTriggerMode,
    #[serde(with = "TriggerModeDef")]
    pub right_mode: JsmTriggerMode,
    pub soft_threshold: f32,
    pub skip_delay_ms: f32,
}

impl Default for TriggerStagesConfig {
    fn default() -> Self {
        Self {
            left_mode: JsmTriggerMode::NoSkip,
            right_mode: JsmTriggerMode::NoSkip,
            soft_threshold: 0.25,
            skip_delay_ms: 150.,
        }
    }
}

impl TriggerStagesConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.left_mode.pad_side().is_none() && self.right_mode.pad_side().is_none(),
            "双段扳机模式不支持改变虚拟扳机的输出方向"
        );
        ensure!(
            self.soft_threshold == -1. || (0.0..1.0).contains(&self.soft_threshold),
            "轻拉阈值必须在 0–1 之间（不含 1），或为 -1 表示随扳机移动触发"
        );
        ensure!(
            (0.0..=5000.).contains(&self.skip_delay_ms),
            "快速全拉时间窗必须在 0–5000 毫秒之间"
        );
        Ok(())
    }

    fn native_settings(&self) -> JsmAnalogSettings {
        JsmAnalogSettings {
            zl: self.left_mode,
            zr: self.right_mode,
            threshold: self.soft_threshold,
            skip_delay: self.skip_delay_ms / 1000.,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StickDirections {
    #[default]
    AxisThreshold,
    EightWay,
}

impl StickDirections {
    pub(crate) fn values(self, value: Vec2) -> [f32; 4] {
        match self {
            Self::AxisThreshold => [-value.x, value.x, value.y, -value.y].map(|v| v.max(0.)),
            Self::EightWay => {
                let magnitude = value.length().min(1.);
                flexinput_engine::eval::jsm_stick_directions(value.x, value.y).map(|active| {
                    if active {
                        magnitude
                    } else {
                        0.
                    }
                })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadzone_left: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadzone_right: Option<f32>,
    pub lstick_directions: StickDirections,
    pub rstick_directions: StickDirections,
    pub lstick_center: [f32; 2],
    pub rstick_center: [f32; 2],
    pub lstick_radial: Vec<f32>,
    pub rstick_radial: Vec<f32>,
    pub ltrig_min: f32,
    pub ltrig_max: f32,
    pub rtrig_min: f32,
    pub rtrig_max: f32,
    pub digital_triggers: bool,
    pub ltrig_digital_threshold: f32,
    pub rtrig_digital_threshold: f32,
    pub trig_hysteresis: f32,
    pub trig_debounce_ms: f32,
}
impl Default for InputConfig {
    fn default() -> Self {
        use flexinput_core::automap::*;
        Self {
            deadzone_left: None,
            deadzone_right: None,
            lstick_directions: StickDirections::default(),
            rstick_directions: StickDirections::default(),
            lstick_center: [0.; 2],
            rstick_center: [0.; 2],
            lstick_radial: vec![1.; RADIAL_BUCKETS],
            rstick_radial: vec![1.; RADIAL_BUCKETS],
            ltrig_min: 0.,
            ltrig_max: 1.,
            rtrig_min: 0.,
            rtrig_max: 1.,
            digital_triggers: false,
            ltrig_digital_threshold: TRIGGER_DIGITAL_THRESHOLD,
            rtrig_digital_threshold: TRIGGER_DIGITAL_THRESHOLD,
            trig_hysteresis: TRIGGER_HYSTERESIS,
            trig_debounce_ms: TRIGGER_DEBOUNCE_MS,
        }
    }
}
impl InputConfig {
    pub fn validate(&self) -> Result<()> {
        for value in [self.deadzone_left, self.deadzone_right]
            .into_iter()
            .flatten()
        {
            ensure!(
                value.is_finite() && (0.0..0.95).contains(&value),
                "摇杆输入死区必须在 0–0.95 之间（不含 0.95）"
            );
        }
        ensure!(
            self.lstick_center
                .iter()
                .chain(&self.rstick_center)
                .all(|v| v.is_finite() && v.abs() <= 1.),
            "摇杆中心偏移必须为 -1–1 的有限数值"
        );
        for radial in [&self.lstick_radial, &self.rstick_radial] {
            ensure!(
                radial.len() == RADIAL_BUCKETS
                    && radial
                        .iter()
                        .all(|v| v.is_finite() && (0.05..=20.).contains(v)),
                "摇杆圆周校准需要 72 个 0.05–20 的有限倍率"
            );
        }
        for (min, max) in [
            (self.ltrig_min, self.ltrig_max),
            (self.rtrig_min, self.rtrig_max),
        ] {
            ensure!(
                min.is_finite() && max.is_finite() && min >= 0. && max <= 1. && max - min >= 0.01,
                "扳机校准范围必须在 0–1 之内，最大值需至少高出最小值 0.01"
            );
        }
        ensure!(
            [self.ltrig_digital_threshold, self.rtrig_digital_threshold]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "扳机数字阈值必须在 0–1 之间"
        );
        ensure!(
            self.trig_hysteresis.is_finite() && (0.0..=0.5).contains(&self.trig_hysteresis),
            "扳机回差必须在 0–0.5 之间"
        );
        ensure!(
            self.trig_debounce_ms.is_finite()
                && (flexinput_core::automap::TRIGGER_DEBOUNCE_MS..=5000.)
                    .contains(&self.trig_debounce_ms),
            "扳机松开防抖时间必须在 4–5000 毫秒之间"
        );
        Ok(())
    }
    /// Missing side overrides retain old files: a legacy mouse deadzone moves
    /// to its selected stick's physical source; all consumers then share it.
    pub fn deadzone(&self, profile: &Profile, left: bool) -> f32 {
        (if left {
            self.deadzone_left
        } else {
            self.deadzone_right
        })
        .unwrap_or_else(|| {
            if profile.stick_mouse.enabled && (profile.stick_mouse.stick == "left") == left {
                profile.stick_mouse.deadzone
            } else {
                profile.stick_deadzone
            }
        })
    }
    pub fn parameters(&self, profile: &Profile) -> HashMap<String, serde_json::Value> {
        let value = serde_json::to_value(self).expect("serializable physical input configuration");
        let mut params: HashMap<_, _> = value
            .as_object()
            .expect("input configuration object")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        params.remove("deadzone_left");
        params.remove("deadzone_right");
        params.insert("deadzone".into(), json!(self.deadzone(profile, true)));
        params.insert("deadzone_r".into(), json!(self.deadzone(profile, false)));
        params.insert(
            "stick_rebound_enabled".into(),
            json!(profile.stick_rebound_ms.is_some()),
        );
        if let Some(ms) = profile.stick_rebound_ms {
            params.insert("stick_rebound_ms".into(), json!(ms));
        }
        params
    }
}

pub(crate) struct NativeInput {
    runner: GraphRunner,
    pub signals: SigMap,
    trigger_ranges: [(f32, f32); 2],
    trigger_settings: JsmAnalogSettings,
    trigger_stages: JsmTriggerStages,
    trigger_blocked: [bool; 2],
}
impl NativeInput {
    fn trigger_ranges(profile: &Profile) -> [(f32, f32); 2] {
        let input = &profile.input;
        [
            (input.ltrig_min, input.ltrig_max),
            (input.rtrig_min, input.rtrig_max),
        ]
    }

    fn graph(profile: &Profile) -> flexinput_engine::ProcessingGraph {
        let pins: Vec<String> = BUTTON_PINS
            .iter()
            .chain(AXES)
            .copied()
            .chain([
                "left_stick",
                "right_stick",
                "btn_lt_dig",
                "btn_rt_dig",
                "touch1_active",
                "touch2_active",
            ])
            .map(String::from)
            .collect();
        let mut source = node(0, "device.source", pins.len());
        source.device_id = Some(SOURCE.into());
        source.output_pin_ids = pins.clone();
        source.params = profile.input.parameters(profile);
        let mut sink = node(1, "device.sink", 0);
        sink.input_sources = (0..pins.len()).map(|i| Some((0, i))).collect();
        sink.sink_target = Some(SinkTarget {
            device_id: PROCESSED.into(),
            multi_sources: (0..pins.len()).map(|i| vec![(0, i)]).collect(),
            pin_ids: pins,
            ..Default::default()
        });
        flexinput_engine::ProcessingGraph::new(vec![source, sink])
    }
    pub fn new(profile: &Profile) -> Self {
        Self {
            runner: GraphRunner::new(Self::graph(profile).nodes),
            signals: SigMap::default(),
            trigger_ranges: Self::trigger_ranges(profile),
            trigger_settings: profile.trigger_stages.native_settings(),
            trigger_stages: JsmTriggerStages::default(),
            trigger_blocked: [false; 2],
        }
    }
    pub fn configure(&mut self, profile: &Profile) {
        self.runner.graph = Self::graph(profile);
        self.runner.result.same_input = false;
        self.trigger_ranges = Self::trigger_ranges(profile);
        self.trigger_settings = profile.trigger_stages.native_settings();
    }
    pub fn reset(&mut self) {
        self.runner.state.clear();
        self.signals.clear();
        self.runner.result.same_input = false;
        self.trigger_stages = JsmTriggerStages::default();
        self.trigger_blocked = [false; 2];
    }
    pub fn tick(&mut self, raw: &SigMap, dt: f32, snapshot: bool) {
        if snapshot {
            self.runner.state.clear();
        }
        self.runner.eval(raw, dt);
        self.signals.clear();
        for (pin, value) in self.runner.result.sink_outputs.pins_of(PROCESSED) {
            self.signals.set_pin(SOURCE, pin, *value);
        }
        // Read the calibrated travel before the optional digital snap for
        // virtual output. A snapped 0/1 value cannot describe two stages.
        let mut positions = ["left_trigger", "right_trigger"].map(|pin| {
            raw.get_pin(SOURCE, pin)
                .map_or(0., |value| value.as_float())
        });
        if snapshot {
            self.trigger_stages = JsmTriggerStages::default();
        }
        for (side, position) in positions.iter_mut().enumerate() {
            *position = apply_trigger_range(*position, self.trigger_ranges[side]);
            if snapshot || *position <= JSM_TRIGGER_REST {
                self.trigger_blocked[side] = *position > JSM_TRIGGER_REST;
            }
            if self.trigger_blocked[side] {
                *position = 0.;
            }
        }
        let stages = self
            .trigger_stages
            .tick(&self.trigger_settings, dt, positions);
        for (pin, down) in TRIGGER_STAGE_PINS
            .into_iter()
            .zip(stages.into_iter().flatten())
        {
            self.signals.set_pin(SOURCE, pin, Signal::Bool(down));
        }
        // Both forms come from the native calibrated Vec2. Every Studio reader
        // sees its radial correction, rather than mixing raw scalar axes in.
        for stick in ["left_stick", "right_stick"] {
            let value = self.stick(stick);
            self.signals
                .set_pin(SOURCE, &format!("{stick}_x"), Signal::Float(value.x));
            self.signals
                .set_pin(SOURCE, &format!("{stick}_y"), Signal::Float(value.y));
        }
    }
    pub fn stick(&self, pin: &str) -> Vec2 {
        match self.signals.get_pin(SOURCE, pin) {
            Some(Signal::Vec2(v)) => *v,
            _ => Vec2::ZERO,
        }
    }
}
