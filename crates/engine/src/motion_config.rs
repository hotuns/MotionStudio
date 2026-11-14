//! User-facing motion settings. Physical units are degrees, seconds and g.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

macro_rules! choices {
    ($name:ident, $first:ident $(, $rest:ident)*) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { #[default] $first, $($rest),* }
    };
}
choices!(Space, PitchYaw, PitchRoll, Player, World);
choices!(MotionMode, Pointer, Steering, StickRotation);
choices!(Smoothing, Ema, Decay, OneEuro);
choices!(Curve, Linear, Natural, Power, Quadratic, Sigmoid, Jump, Custom);
choices!(EdgeMode, Off, Ramp, Momentum, Both);
choices!(Stick, Right, Left);
choices!(AimOutput, Mouse, RightStick, Both);
choices!(Suppress, Full, Deadzone, Off);
choices!(BindingMode, Hold, Toggle, Turbo);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MotionConfig {
    pub space: Space,
    pub mode: MotionMode,
    pub hold_angles: [f32; 3],
    pub roll_contribution: f32,
    pub invert_x: bool,
    pub reset_input: Option<String>,
    pub neutral_input: Option<String>,
    pub steering_range: f32,
    pub steering_exclude_y: bool,
    pub recenter_strength: f32,
    pub reset_ease_ms: f32,
    pub drift_correction: bool,
    pub smoothing: Smoothing,
    pub decay_threshold: f32,
    pub one_euro_cutoff: f32,
    pub one_euro_beta: f32,
    pub angle_snap: f32,
    pub angle_snap_ease: bool,
    pub brake_strength: f32,
    pub brake_threshold: f32,
    pub cutoff_dps: f32,
    pub recovery_ms: f32,
    pub noise_gate: f32,
    pub edge: Edge,
    pub rotation: Rotation,
    pub flick: Flick,
    pub trackball: Trackball,
    pub bindings: Vec<MotionBinding>,
    pub turns: Vec<TurnBinding>,
    pub custom_filters: Vec<motion_filters::FilterConfig>,
}
impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            space: Space::PitchYaw,
            mode: MotionMode::Pointer,
            hold_angles: [0.0; 3],
            roll_contribution: 0.0,
            invert_x: false,
            reset_input: None,
            neutral_input: None,
            steering_range: 30.0,
            steering_exclude_y: false,
            recenter_strength: 0.0,
            reset_ease_ms: 250.0,
            drift_correction: false,
            smoothing: Smoothing::Ema,
            decay_threshold: 60.0,
            one_euro_cutoff: 6.0,
            one_euro_beta: 0.3,
            angle_snap: 0.0,
            angle_snap_ease: true,
            brake_strength: 0.0,
            brake_threshold: 25.0,
            cutoff_dps: 0.0,
            recovery_ms: 0.0,
            noise_gate: 0.0,
            edge: Edge::default(),
            rotation: Rotation::default(),
            flick: Flick::default(),
            trackball: Trackball::default(),
            bindings: vec![],
            turns: vec![],
            custom_filters: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Edge {
    pub mode: EdgeMode,
    pub zone: f32,
    pub blend: f32,
    pub ramp: f32,
    pub max_dps: f32,
    pub momentum: f32,
    pub decay_s: f32,
    pub gyro_keep: f32,
    pub horizontal_only: bool,
}
impl Default for Edge {
    fn default() -> Self {
        Self {
            mode: EdgeMode::Off,
            zone: 30.0,
            blend: 15.0,
            ramp: 30.0,
            max_dps: 180.0,
            momentum: 1.0,
            decay_s: 0.0,
            gyro_keep: 1.0,
            horizontal_only: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rotation {
    pub stick: Stick,
    pub sensitivity: f32,
    pub deadzone: f32,
    pub smooth_ms: f32,
    pub relative: bool,
    pub return_ms: f32,
    pub takeover: bool,
    pub takeover_dps: f32,
    pub outer: f32,
}
impl Default for Rotation {
    fn default() -> Self {
        Self {
            stick: Stick::Right,
            sensitivity: 1.0,
            deadzone: 0.2,
            smooth_ms: 0.0,
            relative: false,
            return_ms: 250.0,
            takeover: false,
            takeover_dps: 10.0,
            outer: 0.9,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flick {
    pub enabled: bool,
    pub standalone: bool,
    pub stick: Stick,
    #[serde(rename = "output", skip_serializing)]
    _legacy_output: serde::de::IgnoredAny,
    pub enable_input: Option<String>,
    pub deadzone: f32,
    pub release_ratio: f32,
    pub time_exponent: f32,
    pub disengage_ms: f32,
    pub smooth_ms: f32,
    pub forward_deadzone: f32,
    pub release_ease: f32,
    pub stabilise_ms: f32,
    pub speed_gate_ms: f32,
    pub slow_lock: bool,
    pub suppress: Suppress,
    pub stick_aim: bool,
    pub aim_sensitivity: f32,
    pub max_dps: f32,
    pub vertical_ratio: f32,
}
impl Default for Flick {
    fn default() -> Self {
        Self {
            enabled: false,
            standalone: false,
            stick: Stick::Right,
            _legacy_output: serde::de::IgnoredAny,
            enable_input: None,
            deadzone: 0.85,
            release_ratio: 0.5,
            time_exponent: 0.0,
            disengage_ms: 10.0,
            smooth_ms: 100.0,
            forward_deadzone: 0.0,
            release_ease: 0.5,
            stabilise_ms: 0.0,
            speed_gate_ms: 0.0,
            slow_lock: false,
            suppress: Suppress::Full,
            stick_aim: false,
            aim_sensitivity: 1.0,
            max_dps: 360.0,
            vertical_ratio: 1.0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Trackball {
    pub horizontal_input: Option<String>,
    pub vertical_input: Option<String>,
    pub decay: f32,
}
impl Default for Trackball {
    fn default() -> Self {
        Self {
            horizontal_input: None,
            vertical_input: None,
            decay: 1.0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MotionBinding {
    pub name: String,
    pub input: String,
    pub output: String,
    pub range: f32,
    pub threshold: f32,
    pub mode: BindingMode,
    pub turbo_ms: f32,
    pub curve: Vec<[f32; 2]>,
}
impl Default for MotionBinding {
    fn default() -> Self {
        Self {
            name: "倾斜动作".into(),
            input: "imu_tilt_left".into(),
            output: "key:Q".into(),
            range: 45.0,
            threshold: 0.4,
            mode: BindingMode::Hold,
            turbo_ms: 100.0,
            curve: vec![[0.0, 0.0], [1.0, 1.0]],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TurnBinding {
    pub name: String,
    pub input: String,
    pub degrees: f32,
    pub duration_ms: f32,
    pub output: AimOutput,
}
impl Default for TurnBinding {
    fn default() -> Self {
        Self {
            name: "转身".into(),
            input: "right_stick_click".into(),
            degrees: 180.0,
            duration_ms: 100.0,
            output: AimOutput::Mouse,
        }
    }
}

/// Local, per-device calibration in the SDL sensor frame. The mounting matrix is
/// a proper rotation, applied equally to the gyro and accelerometer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SensorCalibration {
    pub accel_offset: [f32; 3],
    pub accel_scale: [f32; 3],
    pub gyro_sign: [f32; 3],
    pub accel_sign: [f32; 3],
    pub mounting: [f32; 9],
    pub gyro_noise_dps: f32,
    pub accel_noise_g: f32,
    pub accel_faces: [[Option<f32>; 2]; 3],
}
impl Default for SensorCalibration {
    fn default() -> Self {
        Self {
            accel_offset: [0.0; 3],
            accel_scale: [1.0; 3],
            gyro_sign: [1.0; 3],
            accel_sign: [1.0; 3],
            mounting: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            gyro_noise_dps: 0.0,
            accel_noise_g: 0.0,
            accel_faces: [[None; 2]; 3],
        }
    }
}
pub fn range(name: &str, v: f32, lo: f32, hi: f32) -> Result<()> {
    if !v.is_finite() || v < lo || v > hi {
        bail!("{name} 需为 {lo}–{hi}");
    }
    Ok(())
}
pub fn points(name: &str, p: &[[f32; 2]], max_x: f32, max_y: f32) -> Result<()> {
    if p.len() < 2 || p.len() > 32 {
        bail!("{name} 需包含 2–32 个控制点");
    }
    for (i, pt) in p.iter().enumerate() {
        range(name, pt[0], 0.0, max_x)?;
        range(name, pt[1], 0.0, max_y)?;
        if i > 0 && pt[0] <= p[i - 1][0] {
            bail!("{name} 的横坐标必须严格递增");
        }
    }
    Ok(())
}
impl SensorCalibration {
    pub fn validate(&self) -> Result<()> {
        for v in self.accel_offset {
            range("加速度偏移", v, -5.0, 5.0)?;
        }
        for v in self.accel_scale {
            range("加速度比例", v, 0.5, 2.0)?;
        }
        for v in self.accel_faces.iter().flatten().flatten() {
            range("六面校准读数", *v, -20., 20.)?;
        }
        for v in self.gyro_sign.into_iter().chain(self.accel_sign) {
            if v != 1.0 && v != -1.0 {
                bail!("传感器方向必须为 +1 或 -1");
            }
        }
        let m = glam::Mat3::from_cols_array(&self.mounting).transpose();
        if !m.is_finite()
            || (m.determinant() - 1.0).abs() > 0.02
            || (m.transpose() * m - glam::Mat3::IDENTITY)
                .to_cols_array()
                .iter()
                .any(|v| v.abs() > 0.02)
        {
            bail!("安装方向矩阵必须为正交旋转矩阵");
        }
        range("陀螺仪噪声底", self.gyro_noise_dps, 0.0, 20.0)?;
        range("加速度噪声底", self.accel_noise_g, 0.0, 1.0)
    }
}
impl MotionConfig {
    pub fn needs_accel(&self) -> bool {
        matches!(self.space, Space::Player | Space::World)
            || self.drift_correction
            || self.recenter_strength > 0.0
            || self
                .bindings
                .iter()
                .any(|b| !b.input.starts_with("imu_gyro_"))
            || self
                .custom_filters
                .iter()
                .any(|filter| filter.enabled && filter.requires_accel)
    }
    pub fn validate(&self) -> Result<()> {
        motion_filters::validate_configs(&self.custom_filters)?;
        use crate::profile::{input_pin, valid_output};
        for input in [
            &self.reset_input,
            &self.neutral_input,
            &self.trackball.horizontal_input,
            &self.trackball.vertical_input,
            &self.flick.enable_input,
        ]
        .into_iter()
        .flatten()
        {
            if input_pin(input).is_none() {
                bail!("未知体感控制按键：{input}");
            }
        }
        for angle in self.hold_angles {
            range("握持角度", angle, -180.0, 180.0)?;
        }
        for (n, v, lo, hi) in [
            ("横滚贡献", self.roll_contribution, -100., 100.),
            ("姿态满杆角度", self.steering_range, 1., 180.),
            ("回中强度", self.recenter_strength, 0., 4.),
            ("回中时间", self.reset_ease_ms, 0., 2000.),
            ("衰减平滑阈值", self.decay_threshold, 0.1, 2000.),
            ("One Euro 截止频率", self.one_euro_cutoff, 0.01, 100.),
            ("One Euro 速度系数", self.one_euro_beta, 0., 10.),
            ("角度吸附", self.angle_snap, 0., 45.),
            ("减速刹车", self.brake_strength, 0., 1.),
            ("刹车阈值", self.brake_threshold, 0.01, 1000.),
            ("低速截止", self.cutoff_dps, 0., 100.),
            ("截止恢复时间", self.recovery_ms, 0., 1000.),
            ("噪声底宽度", self.noise_gate, 0., 10.),
            ("惯性衰减", self.trackball.decay, 0., 20.),
            ("边缘角度", self.edge.zone, 0., 170.),
            ("边缘过渡", self.edge.blend, 0., 170.),
            ("边缘提速区间", self.edge.ramp, 0., 170.),
            ("边缘最大速度", self.edge.max_dps, 0., 4000.),
            ("边缘惯性", self.edge.momentum, 0., 10.),
            ("边缘惯性衰减", self.edge.decay_s, 0., 30.),
            ("边缘体感保留", self.edge.gyro_keep, 0., 1.),
            ("旋转灵敏度", self.rotation.sensitivity, 0.01, 100.),
            ("旋转死区", self.rotation.deadzone, 0., 0.95),
            ("方向稳定", self.rotation.smooth_ms, 0., 1000.),
            ("相对回归时间", self.rotation.return_ms, 0., 5000.),
            ("接管速度", self.rotation.takeover_dps, 0., 2000.),
            ("外圈交还阈值", self.rotation.outer, 0.05, 1.),
            ("Flick 死区", self.flick.deadzone, 0.05, 0.99),
            ("Flick 追踪下限比例", self.flick.release_ratio, 0., 1.),
            ("Flick 退出延时", self.flick.disengage_ms, 0., 60.),
            ("Flick 时间", self.flick.smooth_ms, 0., 2000.),
            ("Flick 时间指数", self.flick.time_exponent, 0., 2.),
            (
                "Flick 缓出强度",
                self.flick.release_ease,
                0.,
                flexinput_engine::eval::MAX_FLICK_RELEASE_EASE,
            ),
            ("正前方死角", self.flick.forward_deadzone, 0., 180.),
            ("Flick 稳定", self.flick.stabilise_ms, 0., 1000.),
            ("快推判定时间", self.flick.speed_gate_ms, 0., 2000.),
            ("摇杆瞄准倍率", self.flick.aim_sensitivity, 0.01, 100.),
            ("摇杆最大转速", self.flick.max_dps, 1., 4000.),
            ("摇杆纵向比例", self.flick.vertical_ratio, 0.01, 10.),
        ] {
            range(n, v, lo, hi)?;
        }
        if self.mode == MotionMode::StickRotation
            && (self.flick.enabled || self.flick.stick_aim)
            && self.rotation.stick == self.flick.stick
        {
            bail!("体感旋转与 Flick／摇杆瞄准不能同时使用同一摇杆");
        }
        if self.mode != MotionMode::Pointer && self.edge.mode != EdgeMode::Off {
            bail!("边缘转向需要选择角速度瞄准模式");
        }
        if self.rotation.takeover && self.rotation.outer <= self.rotation.deadzone {
            bail!("外圈交还阈值必须高于旋转死区");
        }
        if self.bindings.len() > 64 || self.turns.len() > 32 {
            bail!("体感动作最多 64 个，定角转身最多 32 个");
        }
        for b in &self.bindings {
            if b.name.trim().is_empty()
                || flexinput_core::imu::imu_pin(&b.input).is_none()
                || !(valid_output(&b.output)
                    || b.output
                        .strip_prefix("pad:")
                        .is_some_and(|p| crate::profile::PAD_PINS.contains(&p)))
            {
                bail!("无效体感动作：{}", b.name);
            }
            range("动作范围", b.range, 0.01, 4000.)?;
            range("动作阈值", b.threshold, 0., 0.99)?;
            range("连发周期", b.turbo_ms, 20., 2000.)?;
            points("动作曲线", &b.curve, 1., 1.)?;
        }
        for b in &self.turns {
            if b.name.trim().is_empty() || input_pin(&b.input).is_none() {
                bail!("无效转身动作：{}", b.name);
            }
            range("转身角度", b.degrees, -720., 720.)?;
            range("转身时间", b.duration_ms, 0., 5000.)?;
        }
        Ok(())
    }
}
