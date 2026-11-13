//! Configuration and FIFO wiring for FlexInput's original route_touchpad sink.
use crate::{
    controller_input::SOURCE,
    engine::{node, GraphRunner},
};
use anyhow::{ensure, Result};
use flexinput_core::{PinMapExt, SigMap, Signal};
use flexinput_engine::graph::SinkTarget;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TouchpadConfig {
    pub enabled: bool,
    pub pointer_speed: f32,
    pub scroll_speed: f32,
    pub natural_scroll: bool,
}
impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pointer_speed: 1.,
            scroll_speed: 1.,
            natural_scroll: true,
        }
    }
}
impl TouchpadConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.pointer_speed, self.scroll_speed]
                .iter()
                .all(|v| v.is_finite() && (0.1..=5.).contains(v)),
            "触摸板速度必须在 0.1–5 之间"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct TouchContact {
    pub active: bool,
    pub x: f32,
    pub y: f32,
}
pub type Contacts = [[TouchContact; 2]; 2];
pub const SWIPE_PINS: [&str; 4] = [
    "touch_swipe_up",
    "touch_swipe_down",
    "touch_swipe_left",
    "touch_swipe_right",
];
const SWIPE_BUS: &str = "studio.touch_swipes";
const SWIPE_OUTPUTS: [&str; 4] = ["dpad_up", "dpad_down", "dpad_left", "dpad_right"];

// Only graph wiring and lifecycle live here. FlexInput Touch Zones owns all
// direction detection, distance thresholds, latching and pulse timing.
struct NativeSwipes {
    graph: GraphRunner,
    last_ns: Option<u64>,
    armed: bool,
}
impl NativeSwipes {
    fn new() -> Self {
        let mut zones = node(0, "module.touch_zones", 1);
        zones.params.extend([
            ("zone_mode".into(), json!("mapping")),
            ("_automap_device_id".into(), json!(SOURCE)),
            ("zone_maps".into(), json!((0..2).flat_map(|field| {
                ["up", "down", "left", "right"].into_iter().enumerate().map(move |(i, direction)|
                    json!({"f":field,"z":0,"in":[format!("tz_swipe_{direction}")],"out":[SWIPE_OUTPUTS[i]],"mode":"down"}))
            }).collect::<Vec<_>>())),
        ]);
        let pins = SWIPE_OUTPUTS.map(String::from).to_vec();
        let mut sink = node(1, "device.sink", 0);
        sink.sink_target = Some(SinkTarget {
            device_id: SWIPE_BUS.into(),
            pin_ids: pins.clone(),
            automap_source: Some(("touchmap:0".into(), pins)),
            ..Default::default()
        });
        Self {
            graph: GraphRunner::new(vec![zones, sink]),
            last_ns: None,
            armed: true,
        }
    }
    fn reset(&mut self) {
        self.graph.state.clear();
        self.graph.result.same_input = false;
        self.last_ns = None;
        self.armed = false;
    }
    fn tick(
        &mut self,
        contacts: [TouchContact; 2],
        split: bool,
        now_ns: u64,
        snapshot: bool,
    ) -> [bool; 4] {
        let neutral = contacts.iter().all(|c| !c.active);
        if snapshot
            || self
                .last_ns
                .is_some_and(|last| now_ns < last || now_ns - last > 100_000_000)
        {
            self.reset();
        }
        let dt = self
            .last_ns
            .map_or(0., |last| now_ns.saturating_sub(last) as f32 / 1e9);
        self.last_ns = Some(now_ns);
        if neutral {
            self.armed = true;
        }
        if snapshot || !self.armed {
            return [false; 4];
        }
        self.graph.graph.nodes[0].params.insert(
            "field_mode".into(),
            json!(if split { "split" } else { "single" }),
        );
        let mut signals = SigMap::default();
        for (i, c) in contacts.into_iter().enumerate() {
            let prefix = if i == 0 { "touch1" } else { "touch2" };
            signals.set_pin(SOURCE, &format!("{prefix}_active"), Signal::Bool(c.active));
            signals.set_pin(SOURCE, &format!("{prefix}_x"), Signal::Float(c.x * 2. - 1.));
            signals.set_pin(SOURCE, &format!("{prefix}_y"), Signal::Float(1. - c.y * 2.));
        }
        self.graph.eval(&signals, dt);
        SWIPE_OUTPUTS.map(|pin| {
            self.graph
                .result
                .sink_outputs
                .get_pin(SWIPE_BUS, pin)
                .is_some_and(|v| v.as_bool())
        })
    }
}
pub fn fields(contacts: &Contacts, split: bool) -> [TouchContact; 2] {
    if split {
        contacts.map(|pad| pad.into_iter().find(|c| c.active).unwrap_or_default())
    } else {
        contacts[0]
    }
}
#[derive(Default)]
pub(crate) struct TouchOutput {
    pub mouse: [f64; 2],
    pub scroll: [i32; 2],
}
pub(crate) struct NativeTouchpad {
    swipes: NativeSwipes,
    config: TouchpadConfig,
    graph: GraphRunner,
    signals: SigMap,
    pub contacts: Contacts,
    pub held: BTreeSet<String>,
    split: bool,
    armed: bool,
    last_ns: u64,
    scroll: [f32; 2],
}
impl NativeTouchpad {
    fn graph(config: &TouchpadConfig) -> GraphRunner {
        let pins = [
            "touch1_active",
            "touch1_x",
            "touch1_y",
            "touch2_active",
            "touch2_x",
            "touch2_y",
            "btn_touchpad",
            "btn_misc2",
            flexinput_core::automap::TOUCH_PADS_PIN,
        ]
        .map(String::from)
        .to_vec();
        let mut source = node(0, "device.source", pins.len());
        source.device_id = Some(SOURCE.into());
        source.output_pin_ids = pins.clone();
        source.params.insert("deadzone".into(), json!(0.));
        let mut sink = node(1, "device.sink", 0);
        sink.sink_target = Some(SinkTarget {
            device_id: "virtual.keymouse.studio_touchpad".into(),
            pin_ids: [
                "mouse_move",
                "scroll_move_x",
                "scroll_move_y",
                "mouse_left",
                "mouse_right",
                "mouse_middle",
            ]
            .map(String::from)
            .to_vec(),
            automap_source: Some((SOURCE.into(), pins)),
            ..Default::default()
        });
        sink.params.extend([
            ("route_touchpad".into(), json!(true)),
            ("touchpad_speed".into(), json!(config.pointer_speed)),
            ("touchpad_scroll_speed".into(), json!(config.scroll_speed)),
            (
                "touchpad_reverse_scroll".into(),
                json!(!config.natural_scroll),
            ),
        ]);
        GraphRunner::new(vec![source, sink])
    }
    pub fn new(config: TouchpadConfig) -> Self {
        Self {
            swipes: NativeSwipes::new(),
            graph: Self::graph(&config),
            config,
            signals: SigMap::default(),
            contacts: Default::default(),
            held: BTreeSet::new(),
            split: false,
            armed: true,
            last_ns: 0,
            scroll: [0.; 2],
        }
    }
    pub fn configure(&mut self, config: TouchpadConfig) {
        self.graph = Self::graph(&config);
        self.config = config;
        self.baseline();
    }
    pub fn set_count(&mut self, count: u16) {
        self.split = count >= 2;
        self.baseline();
    }
    pub fn fields(&self) -> [TouchContact; 2] {
        fields(&self.contacts, self.split)
    }
    pub fn baseline(&mut self) {
        self.swipes.reset();
        self.armed = false;
        self.graph.state.clear();
        self.held.clear();
        self.scroll = [0.; 2];
        self.last_ns = 0;
    }
    pub fn reset(&mut self) {
        self.contacts = Default::default();
        self.baseline();
    }
    pub fn swipe_signals(&mut self, now_ns: u64, snapshot: bool) -> [bool; 4] {
        self.swipes
            .tick(self.fields(), self.split, now_ns, snapshot)
    }
    pub fn contact(&mut self, pad: u8, finger: u8, active: bool, x: f32, y: f32) -> bool {
        let Some(slot) = self
            .contacts
            .get_mut(usize::from(pad))
            .and_then(|pad| pad.get_mut(usize::from(finger)))
        else {
            return false;
        };
        if active
            && (!x.is_finite()
                || !y.is_finite()
                || !(0.0..=1.).contains(&x)
                || !(0.0..=1.).contains(&y))
        {
            self.reset();
            return false;
        }
        *slot = TouchContact {
            active,
            x: if active { x } else { 0. },
            y: if active { y } else { 0. },
        };
        true
    }
    pub fn tick(&mut self, now_ns: u64, clicks: [bool; 2], snapshot: bool) -> TouchOutput {
        let mut out = TouchOutput::default();
        let neutral =
            !self.contacts.iter().flatten().any(|c| c.active) && !clicks.into_iter().any(|v| v);
        if snapshot {
            self.baseline();
            self.armed = neutral;
            return out;
        }
        if self.last_ns != 0 && (now_ns < self.last_ns || now_ns - self.last_ns > 100_000_000) {
            self.baseline();
        }
        let dt = if self.last_ns == 0 {
            0.
        } else {
            now_ns.saturating_sub(self.last_ns) as f32 / 1e9
        };
        self.last_ns = now_ns;
        if neutral {
            self.armed = true;
        }
        if !self.config.enabled || !self.armed {
            self.held.clear();
            return out;
        }
        self.signals.clear();
        for (index, c) in fields(&self.contacts, self.split).into_iter().enumerate() {
            let prefix = if index == 0 { "touch1" } else { "touch2" };
            self.signals
                .set_pin(SOURCE, &format!("{prefix}_active"), Signal::Bool(c.active));
            self.signals
                .set_pin(SOURCE, &format!("{prefix}_x"), Signal::Float(c.x * 2. - 1.));
            self.signals
                .set_pin(SOURCE, &format!("{prefix}_y"), Signal::Float(1. - c.y * 2.));
        }
        self.signals
            .set_pin(SOURCE, "btn_touchpad", Signal::Bool(clicks[0]));
        self.signals
            .set_pin(SOURCE, "btn_misc2", Signal::Bool(clicks[1]));
        if self.split {
            self.signals.set_pin(
                SOURCE,
                flexinput_core::automap::TOUCH_PADS_PIN,
                Signal::Float(2.),
            );
        }
        self.graph.eval(&self.signals, dt);
        let bus = &self.graph.result.sink_outputs;
        let id = "virtual.keymouse.studio_touchpad";
        if let Some(Signal::Vec2(v)) = bus.get_pin(id, "mouse_move") {
            out.mouse = [f64::from(v.x), -f64::from(v.y)];
        }
        for (i, pin) in ["scroll_move_x", "scroll_move_y"].into_iter().enumerate() {
            self.scroll[i] += bus.get_pin(id, pin).map_or(0., |s| s.as_float());
            let whole = flexinput_virtual::scroll_notches(Signal::Float(self.scroll[i]));
            out.scroll[i] = whole;
            self.scroll[i] -= whole as f32;
        }
        self.held.clear();
        for (pin, name) in [
            ("mouse_left", "mouse:left"),
            ("mouse_right", "mouse:right"),
            ("mouse_middle", "mouse:middle"),
        ] {
            if bus.get_pin(id, pin).is_some_and(|s| s.as_bool()) {
                self.held.insert(name.into());
            }
        }
        out
    }
}
