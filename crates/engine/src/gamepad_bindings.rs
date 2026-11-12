use crate::{
    motion_config::Stick,
    profile::{input_pin, Action, GamepadTarget, Trigger, AXES, PAD_PINS},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::OnceLock};

const MAX_GAMEPAD_BINDINGS: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GamepadBinding {
    pub input: String,
    pub output: String,
    #[serde(default = "crate::profile::default_threshold")]
    pub threshold: f32,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default = "crate::profile::default_trigger_ms")]
    pub trigger_ms: u32,
    #[serde(default = "crate::profile::default_sustain")]
    pub sustain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turbo_ms: Option<u32>,
    #[serde(default)]
    pub allow_simultaneous: bool,
    #[serde(default = "crate::profile::default_chord_wait_ms")]
    pub chord_wait_ms: u32,
}

impl GamepadBinding {
    pub(crate) fn is_analog(&self) -> bool {
        stick(&self.output).is_some()
            || (trigger_button(&self.output).is_some()
                && trigger_button(self.input.trim()).is_some())
    }

    pub(crate) fn action(&self) -> Action {
        Action {
            name: format!("pad:{}", self.output),
            input: self.input.clone(),
            output: format!(
                "pad:{}",
                output_pin(&self.output).expect("digital pad output")
            ),
            threshold: self.threshold,
            repeat_ms: None,
            trigger: self.trigger,
            trigger_ms: self.trigger_ms,
            sustain: self.sustain,
            turbo_ms: self.turbo_ms,
            allow_simultaneous: self.allow_simultaneous,
            chord_wait_ms: self.chord_wait_ms,
        }
    }
}

#[derive(Deserialize)]
struct Layout {
    buttons: Vec<String>,
    sticks: Vec<String>,
    triggers: Vec<String>,
    dualshock_4: Vec<String>,
}

pub(crate) fn standard_bindings(target: GamepadTarget) -> Vec<GamepadBinding> {
    static LAYOUT: OnceLock<Layout> = OnceLock::new();
    let layout = LAYOUT.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/virtual-gamepad-layout.json"))
            .expect("built-in virtual gamepad layout")
    });
    layout
        .buttons
        .iter()
        .chain(&layout.sticks)
        .chain(&layout.triggers)
        .chain(
            layout
                .dualshock_4
                .iter()
                .filter(|_| target == GamepadTarget::DualShock4),
        )
        .map(|name| GamepadBinding {
            input: name.clone(),
            output: name.clone(),
            threshold: crate::profile::default_threshold(),
            trigger: Trigger::Down,
            trigger_ms: crate::profile::default_trigger_ms(),
            sustain: true,
            turbo_ms: None,
            allow_simultaneous: false,
            chord_wait_ms: crate::profile::default_chord_wait_ms(),
        })
        .collect()
}

pub(crate) fn stick(name: &str) -> Option<Stick> {
    match name {
        "left_stick" => Some(Stick::Left),
        "right_stick" => Some(Stick::Right),
        _ => None,
    }
}

pub(crate) fn stick_pins(stick: Stick) -> [&'static str; 2] {
    match stick {
        Stick::Left => ["left_stick_x", "left_stick_y"],
        Stick::Right => ["right_stick_x", "right_stick_y"],
    }
}

pub(crate) fn trigger_button(name: &str) -> Option<&'static str> {
    match name {
        "left_trigger" => Some("btn_lt_dig"),
        "right_trigger" => Some("btn_rt_dig"),
        _ => None,
    }
}

pub(crate) fn output_pin(name: &str) -> Option<&str> {
    let pin = input_pin(name)?;
    (pin == "btn_touchpad"
        || (PAD_PINS.contains(&pin) && (!AXES.contains(&pin) || trigger_button(name).is_some())))
    .then_some(pin)
}

pub(crate) fn validate(bindings: &[GamepadBinding]) -> Result<()> {
    ensure!(
        bindings.len() <= MAX_GAMEPAD_BINDINGS,
        "每个配置最多支持 64 条手柄映射"
    );
    let mut analog_outputs = HashSet::new();
    for binding in bindings {
        crate::profile::validate_chord_wait(binding.chord_wait_ms)?;
        crate::profile::validate_activation(
            binding.threshold,
            binding.trigger_ms,
            binding.turbo_ms,
        )?;
        if binding.is_analog() {
            ensure!(
                binding.trigger == Trigger::Down
                    && binding.turbo_ms.is_none()
                    && !binding.allow_simultaneous,
                "模拟摇杆和扳机行程映射需要使用按住方式"
            );
        }
        let output = binding.output.as_str();
        if stick(output).is_some() {
            ensure!(stick(&binding.input).is_some(), "虚拟摇杆需要绑定实体摇杆");
        } else {
            ensure!(
                output_pin(output).is_some()
                    && crate::bindings::input_chord(&binding.input).is_some(),
                "手柄映射的输入或输出无效"
            );
        }
        if stick(output).is_some() || trigger_button(output).is_some() {
            ensure!(
                analog_outputs.insert(output),
                "同一个虚拟摇杆或扳机只能绑定一条映射"
            );
        }
    }
    Ok(())
}
