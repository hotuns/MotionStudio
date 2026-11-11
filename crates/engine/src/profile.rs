use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const BUTTONS: &[&str] = &[
    "south",
    "east",
    "west",
    "north",
    "back",
    "guide",
    "start",
    "left_stick_click",
    "right_stick_click",
    "left_shoulder",
    "right_shoulder",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "misc1",
    "right_paddle1",
    "left_paddle1",
    "right_paddle2",
    "left_paddle2",
    "touchpad",
    "misc2",
    "misc3",
    "misc4",
    "misc5",
    "misc6",
];
pub const BUTTON_PINS: &[&str] = &[
    "btn_south",
    "btn_east",
    "btn_west",
    "btn_north",
    "btn_back",
    "btn_guide",
    "btn_start",
    "btn_ls",
    "btn_rs",
    "btn_lb",
    "btn_rb",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "btn_misc1",
    "btn_rp1",
    "btn_lp1",
    "btn_rp2",
    "btn_lp2",
    "btn_touchpad",
    "btn_misc2",
    "btn_misc3",
    "btn_misc4",
    "btn_misc5",
    "btn_misc6",
];
pub const AXES: &[&str] = &[
    "left_stick_x",
    "left_stick_y",
    "right_stick_x",
    "right_stick_y",
    "left_trigger",
    "right_trigger",
];
pub const DIRECTIONS: &[&str] = &[
    "left_stick_left",
    "left_stick_right",
    "left_stick_up",
    "left_stick_down",
    "right_stick_left",
    "right_stick_right",
    "right_stick_up",
    "right_stick_down",
];
pub const ZONES: &[&str] = &[
    "left_stick_outer",
    "left_stick_inner",
    "right_stick_outer",
    "right_stick_inner",
];
pub const TOUCH_PINS: &[&str] = &["touch1_active", "touch2_active"];
pub const PAD_PINS: &[&str] = &[
    "btn_south",
    "btn_east",
    "btn_west",
    "btn_north",
    "btn_back",
    "btn_guide",
    "btn_start",
    "btn_ls",
    "btn_rs",
    "btn_lb",
    "btn_rb",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "left_stick_x",
    "left_stick_y",
    "right_stick_x",
    "right_stick_y",
    "left_trigger",
    "right_trigger",
];

pub fn input_pin(input: &str) -> Option<&str> {
    BUTTONS
        .iter()
        .position(|&b| b == input)
        .map(|i| BUTTON_PINS[i])
        .or_else(|| AXES.contains(&input).then_some(input))
        .or_else(|| {
            crate::controller_input::TRIGGER_STAGE_PINS
                .contains(&input)
                .then_some(input)
        })
        .or_else(|| DIRECTIONS.contains(&input).then_some(input))
        .or_else(|| ZONES.contains(&input).then_some(input))
        .or(match input {
            "touch1" => Some("touch1_active"),
            "touch2" => Some("touch2_active"),
            _ => None,
        })
}
/// Simultaneously held controls, using SDL's screen-down axis convention.
pub fn pressed_inputs(buttons: &[bool], axes: &[f32; 6], threshold: f32) -> Vec<String> {
    let mut held: Vec<_> = BUTTONS
        .iter()
        .zip(buttons)
        .filter(|(_, down)| **down)
        .map(|(name, _)| name.to_string())
        .collect();
    for (index, value) in axes.iter().copied().enumerate() {
        if !value.is_finite() {
            continue;
        }
        let name = match index {
            0 if value < -threshold => Some("left_stick_left"),
            0 if value > threshold => Some("left_stick_right"),
            1 if value < -threshold => Some("left_stick_up"),
            1 if value > threshold => Some("left_stick_down"),
            2 if value < -threshold => Some("right_stick_left"),
            2 if value > threshold => Some("right_stick_right"),
            3 if value < -threshold => Some("right_stick_up"),
            3 if value > threshold => Some("right_stick_down"),
            4 if value > threshold => Some("left_trigger"),
            5 if value > threshold => Some("right_trigger"),
            _ => None,
        };
        if let Some(name) = name {
            held.push(name.into());
        }
    }
    held
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_gamepad_target")]
    pub virtual_gamepad: GamepadTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamepad_bindings: Option<Vec<crate::gamepad_bindings::GamepadBinding>>,
    #[serde(default = "default_deadzone")]
    pub stick_deadzone: f32,
    #[serde(default)]
    pub input: crate::controller_input::InputConfig,
    #[serde(default)]
    pub feedback: crate::feedback::FeedbackConfig,
    #[serde(default)]
    pub touchpad: crate::touchpad::TouchpadConfig,
    #[serde(default)]
    pub stick_mouse: StickMouse,
    #[serde(default)]
    pub gyro: Gyro,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub trigger_stages: crate::controller_input::TriggerStagesConfig,
    #[serde(default)]
    pub stick_zones: StickZones,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stick_rebound_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<crate::layers::Layer>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StickZones {
    pub left_outer: f32,
    pub right_outer: f32,
    pub hysteresis: f32,
}
impl Default for StickZones {
    fn default() -> Self {
        Self {
            left_outer: 0.85,
            right_outer: 0.85,
            hysteresis: 0.05,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StickRegion {
    #[default]
    Center,
    Inner,
    Outer,
}
impl StickRegion {
    pub fn input(self, side: usize) -> Option<&'static str> {
        match (side, self) {
            (0, Self::Inner) => Some("left_stick_inner"),
            (0, Self::Outer) => Some("left_stick_outer"),
            (1, Self::Inner) => Some("right_stick_inner"),
            (1, Self::Outer) => Some("right_stick_outer"),
            _ => None,
        }
    }
}
fn default_deadzone() -> f32 {
    0.12
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GamepadTarget {
    #[default]
    None,
    #[serde(rename = "xbox_360")]
    Xbox360,
    #[serde(rename = "dualshock_4")]
    DualShock4,
}

fn deserialize_gamepad_target<'de, D>(de: D) -> Result<GamepadTarget, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Target(GamepadTarget),
        Legacy(bool),
    }
    Ok(match Value::deserialize(de)? {
        Value::Target(target) => target,
        Value::Legacy(true) => GamepadTarget::Xbox360,
        Value::Legacy(false) => GamepadTarget::None,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gyro {
    pub enabled: bool,
    pub output: GyroOutput,
    pub activation: String,
    pub activation_mode: GyroActivationMode,
    pub activation_threshold: f32,
    pub pause_input: Option<String>,
    pub counts_per_degree: f32,
    pub stick_full_scale_dps: f32,
    pub sensitivity: f32,
    pub vertical_ratio: f32,
    pub invert_y: bool,
    pub timestamp_source: TimestampSource,
    pub smoothing_ms: f32,
    pub noise_threshold: f32,
    pub acceleration: GyroAcceleration,
    pub motion: crate::motion_config::MotionConfig,
}
impl Default for Gyro {
    fn default() -> Self {
        Self {
            enabled: false,
            output: GyroOutput::Mouse,
            activation: "always".into(),
            activation_mode: GyroActivationMode::Hold,
            activation_threshold: 0.2,
            pause_input: None,
            counts_per_degree: 12.0,
            stick_full_scale_dps: 360.0,
            sensitivity: 1.0,
            vertical_ratio: 1.0,
            invert_y: false,
            timestamp_source: TimestampSource::Host,
            smoothing_ms: 0.0,
            noise_threshold: 0.0,
            acceleration: GyroAcceleration::default(),
            motion: Default::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroOutput {
    #[default]
    Mouse,
    RightStick,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroActivationMode {
    #[default]
    Hold,
    Toggle,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GyroAcceleration {
    pub fast_multiplier: f32,
    pub start_dps: f32,
    pub full_dps: f32,
    pub curve: crate::motion_config::Curve,
    pub natural_half: f32,
    pub power_reference: f32,
    pub power_exponent: f32,
    pub sigmoid_mid: f32,
    pub sigmoid_width: f32,
    pub jump_width: f32,
    pub points: Vec<[f32; 2]>,
}

impl Default for GyroAcceleration {
    fn default() -> Self {
        Self {
            fast_multiplier: 1.0,
            start_dps: 40.0,
            full_dps: 180.0,
            curve: Default::default(),
            natural_half: 200.0,
            power_reference: 60.0,
            power_exponent: 2.0,
            sigmoid_mid: 90.0,
            sigmoid_width: 20.0,
            jump_width: 10.0,
            points: vec![[0.0, 1.0], [180.0, 2.0]],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StickMouse {
    pub enabled: bool,
    pub stick: String,
    pub speed: f32,
    pub deadzone: f32,
    pub exponent: f32,
}
impl Default for StickMouse {
    fn default() -> Self {
        Self {
            enabled: false,
            stick: "right".into(),
            speed: 1200.0,
            deadzone: 0.15,
            exponent: 1.6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampSource {
    #[default]
    Host,
    Sensor,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub name: String,
    pub input: String,
    pub output: String,
    #[serde(default = "default_threshold")]
    pub threshold: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_ms: Option<u32>,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default = "default_trigger_ms")]
    pub trigger_ms: u32,
    #[serde(default = "default_sustain")]
    pub sustain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turbo_ms: Option<u32>,
    #[serde(default)]
    pub allow_simultaneous: bool,
}
pub(crate) fn default_trigger_ms() -> u32 {
    250
}
pub(crate) fn default_sustain() -> bool {
    true
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    #[default]
    Down,
    Short,
    Long,
    Double,
    OnPress,
    OnRelease,
}
impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Short => "short",
            Self::Long => "long",
            Self::Double => "double",
            Self::OnPress => "on_press",
            Self::OnRelease => "on_release",
        }
    }
}
pub(crate) fn default_threshold() -> f32 {
    0.5
}

pub(crate) fn validate_activation(
    threshold: f32,
    trigger_ms: u32,
    turbo_ms: Option<u32>,
) -> Result<()> {
    if !(10..=5000).contains(&trigger_ms) || turbo_ms.is_some_and(|v| !(20..=2000).contains(&v)) {
        bail!("动作触发时间需为 10–5000 ms，连发周期需为 20–2000 ms");
    }
    if !threshold.is_finite() || !(0.0..1.0).contains(&threshold) {
        bail!("动作触发阈值必须在 0–1 之间（不含 1）");
    }
    Ok(())
}

pub fn valid_key(key: &str) -> bool {
    crate::keyboard::key(key).is_some()
}
pub fn valid_output(output: &str) -> bool {
    crate::bindings::output_chord(output).is_some()
}
impl Profile {
    pub fn materialize_gamepad_bindings(&mut self) {
        if self.virtual_gamepad != GamepadTarget::None && self.gamepad_bindings.is_none() {
            self.gamepad_bindings = Some(crate::gamepad_bindings::standard_bindings(
                self.virtual_gamepad,
            ));
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Read profile {}", path.display()))?;
        let mut profile: Self = toml::from_str(&text).context("Invalid profile TOML")?;
        profile.materialize_gamepad_bindings();
        profile.validate()?;
        Ok(profile)
    }
    pub fn validate(&self) -> Result<()> {
        if serde_json::to_vec(self)?.len() > 256 * 1024 {
            bail!("配置内容超过 256 KiB，请减少脚本或配置层中的重复内容");
        }
        if self.schema_version != 1 {
            bail!("Unsupported schema_version; expected 1");
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 100 {
            bail!("配置名需为 1–100 个字符");
        }
        self.input.validate()?;
        crate::gamepad_bindings::validate(self.gamepad_bindings.as_deref().unwrap_or_default())?;
        self.trigger_stages.validate()?;
        self.feedback.validate()?;
        self.touchpad.validate()?;
        if !self.stick_deadzone.is_finite() || !(0.0..0.95).contains(&self.stick_deadzone) {
            bail!("stick_deadzone must be between 0 and 0.95 (exclusive)");
        }
        if self.actions.len() > 128 {
            bail!("每个配置最多支持 128 个动作");
        }
        crate::layers::validate(self)?;
        let z = &self.stick_zones;
        if self
            .stick_rebound_ms
            .is_some_and(|ms| !(10..=150).contains(&ms))
        {
            bail!("摇杆防回弹时间需为 10–150 ms");
        }
        if [z.left_outer, z.right_outer]
            .iter()
            .any(|v| !v.is_finite() || !(0.1..=1.).contains(v))
            || !z.hysteresis.is_finite()
            || !(0.0..=0.2).contains(&z.hysteresis)
            || z.hysteresis >= z.left_outer.min(z.right_outer)
        {
            bail!("摇杆外圈阈值或回差无效");
        }
        for action in &self.actions {
            validate_activation(action.threshold, action.trigger_ms, action.turbo_ms)?;
            if action.name.trim().is_empty()
                || crate::bindings::input_chord(&action.input).is_none()
                || !valid_output(&action.output)
            {
                bail!("Invalid action: {:?}; use a supported input and key:SPACE / mouse:left style output", action);
            }
            if let Some(ms) = action.repeat_ms {
                if !(20..=2000).contains(&ms)
                    || !crate::bindings::output_chord(&action.output)
                        .unwrap()
                        .iter()
                        .any(|p| crate::bindings::wheel_delta(p).is_some())
                {
                    bail!(
                        "滚轮连续滚动间隔需为 20–2000 ms，且输出中需包含滚轮：{}",
                        action.name
                    );
                }
            }
        }
        let g = &self.gyro;
        if g.enabled
            && g.motion.mode != crate::motion_config::MotionMode::Pointer
            && g.output != GyroOutput::RightStick
        {
            bail!("姿态控制和体感旋转模式需要选择虚拟右摇杆输出");
        }
        g.motion.validate()?;
        for (n, v, lo, hi) in [
            ("自然曲线半速", g.acceleration.natural_half, 0.01, 4000.),
            (
                "幂曲线参考速度",
                g.acceleration.power_reference,
                0.01,
                4000.,
            ),
            ("幂指数", g.acceleration.power_exponent, 0.01, 10.),
            ("S 曲线中点", g.acceleration.sigmoid_mid, 0., 4000.),
            ("S 曲线宽度", g.acceleration.sigmoid_width, 0.01, 2000.),
            ("跳变宽度", g.acceleration.jump_width, 0., 2000.),
        ] {
            crate::motion_config::range(n, v, lo, hi)?;
        }
        crate::motion_config::points("体感加速曲线", &g.acceleration.points, 4000., 10.)?;
        let needs_pad = g.motion.mode != crate::motion_config::MotionMode::Pointer
            || g.motion
                .turns
                .iter()
                .any(|t| t.output != crate::motion_config::AimOutput::Mouse)
            || g.motion
                .bindings
                .iter()
                .any(|b| b.output.starts_with("pad:"));
        if g.enabled && needs_pad && self.virtual_gamepad == GamepadTarget::None {
            bail!("当前体感模式或动作需要选择虚拟手柄输出");
        }
        if g.enabled
            && g.output == GyroOutput::RightStick
            && self.virtual_gamepad == GamepadTarget::None
        {
            bail!("体感输出到右摇杆需要选择虚拟 Xbox 360 或 DualShock 4");
        }
        if !(30.0..=1440.0).contains(&g.stick_full_scale_dps) {
            bail!("满杆角速度需为 30–1440 °/s");
        }
        let a = &g.acceleration;
        if !(1.0..=4.0).contains(&a.fast_multiplier)
            || !(0.0..=1000.0).contains(&a.start_dps)
            || !(1.0..=2000.0).contains(&a.full_dps)
            || a.full_dps <= a.start_dps
        {
            bail!("快速转动倍率需为 1–4，达到最高倍率的角速度需高于加速起点");
        }
        if !g.smoothing_ms.is_finite()
            || !(0.0..=50.0).contains(&g.smoothing_ms)
            || !g.noise_threshold.is_finite()
            || !(0.0..=5.0).contains(&g.noise_threshold)
        {
            bail!("平滑时间需为 0–50 ms，静止抑制需为 0–5 °/s");
        }
        let s = &self.stick_mouse;
        if !["left", "right"].contains(&s.stick.as_str())
            || !s.speed.is_finite()
            || !(1.0..=10000.0).contains(&s.speed)
            || !s.deadzone.is_finite()
            || !(0.0..0.95).contains(&s.deadzone)
            || !s.exponent.is_finite()
            || !(1.0..=4.0).contains(&s.exponent)
        {
            bail!("摇杆鼠标参数无效");
        }
        if g.activation != "always" && input_pin(&g.activation).is_none() {
            bail!("Unknown gyro activation: {}", g.activation);
        }
        if g.activation == "always" && g.activation_mode == GyroActivationMode::Toggle {
            bail!("按键切换体感需要指定开启按键");
        }
        if let Some(pause_input) = &g.pause_input {
            if input_pin(pause_input).is_none() || pause_input == &g.activation {
                bail!("体感暂停按键必须有效，并与开启按键不同");
            }
        }
        for (name, v, max) in [
            ("counts_per_degree", g.counts_per_degree, 10000.0),
            ("sensitivity", g.sensitivity, 100.0),
            ("vertical_ratio", g.vertical_ratio, 10.0),
        ] {
            if !v.is_finite() || v <= 0.0 || v > max {
                bail!("{name} must be finite and in (0, {max}]");
            }
        }
        if !g.activation_threshold.is_finite() || !(0.0..1.0).contains(&g.activation_threshold) {
            bail!("Invalid gyro activation_threshold");
        }
        Ok(())
    }
}
