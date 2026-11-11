//! SDL-to-FlexInput motion adapter. The upstream 3DOF, curve, filter and
//! stick-rotation algorithms run on every timed gyro event, outside the UI.
use crate::timing::SampleTime;
use crate::{
    engine::{node, GraphRunner},
    motion_config::*,
    motion_curves as cc,
    profile::Gyro,
};
use flexinput_core::{
    imu::{ImuEstimator, ImuReading, BUS_ACCEL_G, BUS_GYRO_DPS},
    PinMapExt, SigMap, Signal,
};
use flexinput_engine::{
    eval::{sample_curve, RotationSettings, StickRotation, Takeover},
    graph::SinkTarget,
};
use glam::{Mat3, Quat, Vec2, Vec3};
use motion_filters::{FilterChain, FilterStatus, Sample as FilterSample, Stage as FilterStage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};

pub mod stability_preview;

const DEVICE: &str = "motion.sensor";
const SINK: &str = "motion.result";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameAngleMeasurement {
    pub axis: String,
    pub degrees: f32,
    pub peak: f32,
    pub error: Option<String>,
    pub output: crate::profile::GyroOutput,
    pub scale: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MotionTelemetry {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<FilterStatus>,
    pub accel_g: [f32; 3],
    pub accel_fresh: bool,
    pub orientation: [f32; 3],
    /// FlexInput's canonical Z-up quaternion, in XYZW order.
    pub orientation_quat: Option<[f32; 4]>,
    pub quantities: [f32; 8],
    pub rotation_offset: f32,
    pub rotation_taken: bool,
    pub measured_degrees: f32,
    pub measurement_peak: f32,
    pub measuring: Option<String>,
    pub measurement_error: Option<String>,
}

pub struct MotionRuntime {
    pub custom_filters: FilterChain,
    graph: GraphRunner,
    bus: SigMap,
    imu: ImuEstimator,
    pub reading: ImuReading,
    pub telemetry: MotionTelemetry,
    smoothing: AimSmoothing,
    brake: cc::Brake,
    recovery: f32,
    history: VecDeque<(Vec2, f32)>,
    coast: Vec2,
    was_tracking: [bool; 2],
    rotation: StickRotation,
}
pub struct MotionSample {
    pub rate: Vec2,
    pub physical_rate: Vec2,
    pub steering: Option<Vec2>,
}

#[derive(Default)]
struct AimSmoothing {
    ema: Option<Vec2>,
    one_euro: [cc::OneEuro; 2],
    decay: cc::Decay,
}

impl AimSmoothing {
    fn tick(&mut self, g: &Gyro, mut rate: Vec2, dt: f32) -> Vec2 {
        let m = &g.motion;
        match m.smoothing {
            Smoothing::Ema => {
                if g.smoothing_ms > 0. {
                    rate = self
                        .ema
                        .unwrap_or(rate)
                        .lerp(rate, 1. - (-dt / (g.smoothing_ms / 1000.)).exp());
                }
                self.ema = Some(rate);
                rate
            }
            Smoothing::Decay => {
                let (x, y) = self.decay.smooth(
                    rate.x,
                    rate.y,
                    dt,
                    g.smoothing_ms / 1000.,
                    m.decay_threshold,
                );
                Vec2::new(x, y)
            }
            Smoothing::OneEuro => Vec2::new(
                self.one_euro[0].filter(rate.x, dt, m.one_euro_cutoff, m.one_euro_beta),
                self.one_euro[1].filter(rate.y, dt, m.one_euro_cutoff, m.one_euro_beta),
            ),
        }
    }
}

fn suppress_idle(g: &Gyro, mut rate: Vec2, sensor_noise: f32) -> Vec2 {
    let mag = rate.length();
    if mag > 0. {
        rate *= (mag - g.noise_threshold).max(0.) / mag;
    }
    if g.motion.noise_gate > 0. && sensor_noise > 0. {
        rate *=
            flexinput_engine::eval::gyro_dz_ease(rate.length(), sensor_noise * g.motion.noise_gate);
    }
    rate
}

fn cutoff_recovery(g: &Gyro, mut rate: Vec2, speed: f32, dt: f32, recovery: &mut f32) -> Vec2 {
    let m = &g.motion;
    if m.cutoff_dps > 0. && speed <= m.cutoff_dps {
        *recovery = 0.;
        rate = Vec2::ZERO;
    } else if m.recovery_ms > 0. {
        *recovery = (*recovery + dt / (m.recovery_ms / 1000.)).min(1.);
        rate *= *recovery;
    }
    rate
}

pub fn calibrated(
    value: [f32; 3],
    offset: [f32; 3],
    scale: [f32; 3],
    signs: [f32; 3],
    c: &SensorCalibration,
) -> Vec3 {
    let v = (Vec3::from(value) - Vec3::from(offset)) * Vec3::from(scale) * Vec3::from(signs);
    Mat3::from_cols_array(&c.mounting).transpose() * v
}
/// SDL: right/up/back, rad/s and m/s². Flex gyro: forward/right/down,
/// accel: forward/left/up, normalized at 2000°/s and 8g respectively.
pub fn gyro_bus(v: Vec3) -> Vec3 {
    Vec3::from(flexinput_devices::sdl_backend::sdl_gyro_to_canonical(
        v.to_array(),
    )) * (180.0 / std::f32::consts::PI / BUS_GYRO_DPS)
}
pub fn accel_bus(v: Vec3) -> Vec3 {
    Vec3::from(flexinput_devices::sdl_backend::sdl_accel_to_canonical(
        v.to_array(),
    )) / (9.80665 * BUS_ACCEL_G)
}

impl MotionRuntime {
    pub fn new(g: &Gyro) -> anyhow::Result<Self> {
        let mut runtime = Self::builtin(g);
        runtime.custom_filters = FilterChain::new(&g.motion.custom_filters)?;
        Ok(runtime)
    }
    fn builtin(g: &Gyro) -> Self {
        let m = &g.motion;
        let mut src = node(0, "device.source", 1);
        src.device_id = Some(DEVICE.into());
        src.output_pin_ids = vec!["reset".into()];
        let mut gyro = node(1, "processing.gyro_3dof", 7);
        gyro.input_sources = vec![None, Some((0, 0))];
        gyro.params = HashMap::from([
            ("_automap_device_id".into(), json!(DEVICE)),
            (
                "family".into(),
                json!(if m.mode == MotionMode::Steering {
                    "steering"
                } else {
                    "pointer"
                }),
            ),
            (
                "axis".into(),
                json!(match m.space {
                    Space::PitchYaw => "pitch_yaw",
                    Space::PitchRoll => "pitch_roll",
                    Space::Player => "player",
                    Space::World => "world",
                }),
            ),
            ("hold_pitch".into(), json!(m.hold_angles[0])),
            ("hold_yaw".into(), json!(m.hold_angles[1])),
            ("hold_roll".into(), json!(m.hold_angles[2])),
            ("roll_contribution".into(), json!(m.roll_contribution)),
            ("orient_source".into(), json!("computed")),
            (
                "orient_drift".into(),
                json!(if m.drift_correction { 1.0 } else { 0.0 }),
            ),
            ("recenter_strength".into(), json!(m.recenter_strength)),
            ("reset_ease_in".into(), json!(m.reset_ease_ms / 1000.0)),
            ("steering_exclude_y".into(), json!(m.steering_exclude_y)),
            (
                "edge_mode".into(),
                serde_json::to_value(m.edge.mode).unwrap(),
            ),
            ("edge_zone_deg".into(), json!(m.edge.zone)),
            ("edge_blend_deg".into(), json!(m.edge.blend)),
            ("edge_ramp_deg".into(), json!(m.edge.ramp)),
            ("edge_max_dps".into(), json!(m.edge.max_dps)),
            ("edge_momentum".into(), json!(m.edge.momentum)),
            ("edge_decay_s".into(), json!(m.edge.decay_s)),
            ("edge_gyro_keep".into(), json!(m.edge.gyro_keep)),
            ("edge_x_only".into(), json!(m.edge.horizontal_only)),
        ]);
        let mut sink = node(2, "device.sink", 0);
        sink.sink_target = Some(SinkTarget {
            device_id: SINK.into(),
            pin_ids: vec!["aim".into(), "orientation".into()],
            multi_sources: vec![vec![(1, 0)], vec![(1, 5)]],
            ..Default::default()
        });
        Self {
            custom_filters: FilterChain::default(),
            graph: GraphRunner::new(vec![src, gyro, sink]),
            bus: SigMap::default(),
            imu: ImuEstimator::default(),
            reading: ImuReading::default(),
            telemetry: MotionTelemetry::default(),
            smoothing: AimSmoothing::default(),
            brake: cc::Brake::default(),
            recovery: 0.0,
            history: VecDeque::new(),
            coast: Vec2::ZERO,
            was_tracking: [false; 2],
            rotation: StickRotation::default(),
        }
    }
    pub fn reset(&mut self, g: &Gyro) {
        let mut custom = std::mem::take(&mut self.custom_filters);
        custom.reset_state();
        *self = Self::builtin(g);
        self.custom_filters = custom;
    }
    pub fn rebuild_builtin(&mut self, g: &Gyro) {
        let custom = std::mem::take(&mut self.custom_filters);
        *self = Self::builtin(g);
        self.custom_filters = custom;
    }
    pub fn reset_filters(&mut self) {
        self.custom_filters.reset_state();
        self.smoothing = AimSmoothing::default();
        self.brake = cc::Brake::default();
        self.recovery = 0.;
        self.history.clear();
        self.coast = Vec2::ZERO;
        self.was_tracking = [false; 2];
        self.rotation = StickRotation::default();
    }
    pub fn measurement_mode(&mut self) {
        self.graph.graph.nodes[1]
            .params
            .insert("edge_mode".into(), json!("off"));
    }
    #[allow(clippy::too_many_arguments)]
    pub fn sample(
        &mut self,
        g: &Gyro,
        gyro: Vec3,
        accel: Option<Vec3>,
        dt: f32,
        reset: bool,
        neutral: bool,
        tracking: [bool; 2],
        sensor_noise: f32,
        accel_noise: f32,
        time: SampleTime,
        apply_custom: bool,
    ) -> MotionSample {
        self.custom_filters.begin_sample();
        let custom_sample = FilterSample {
            value: gyro.to_array().map(|v| f64::from(v * BUS_GYRO_DPS)),
            raw: gyro.to_array().map(|v| f64::from(v * BUS_GYRO_DPS)),
            accel_g: accel.map(|a| [a.x, -a.y, -a.z].map(|v| f64::from(v * BUS_ACCEL_G))),
            dt: f64::from(dt),
            host_ns: time.host_ns,
            sensor_ns: time.sensor_ns,
        };
        let gyro = if apply_custom && self.custom_filters.has_stage(FilterStage::RawGyro) {
            Vec3::from_array(
                self.custom_filters
                    .process(FilterStage::RawGyro, custom_sample)
                    .map(|v| (v / f64::from(BUS_GYRO_DPS)) as f32),
            )
        } else {
            gyro
        };
        for (pin, v) in ["gyro_x", "gyro_y", "gyro_z"]
            .into_iter()
            .zip(gyro.to_array())
        {
            self.bus.set_pin(DEVICE, pin, Signal::Float(v));
        }
        for (pin, v) in ["accel_x", "accel_y", "accel_z"]
            .into_iter()
            .zip(accel.unwrap_or(Vec3::ZERO).to_array())
        {
            self.bus.set_pin(DEVICE, pin, Signal::Float(v));
        }
        self.bus.set_pin(DEVICE, "reset", Signal::Bool(reset));
        // This is the upstream per-device Set Neutral Forward contract.
        self.bus
            .set_pin(DEVICE, "__orient_neutral", Signal::Bool(neutral));
        self.graph.eval(&self.bus, dt);
        let raw = match self.graph.result.sink_outputs.get_pin(SINK, "aim") {
            Some(Signal::Vec2(v)) => *v * BUS_GYRO_DPS,
            _ => Vec2::ZERO,
        };
        if let Some(Signal::Vec4(v)) = self.graph.result.sink_outputs.get_pin(SINK, "orientation") {
            let q = Quat::from_xyzw(v.x, v.y, v.z, v.w);
            self.telemetry.orientation_quat = Some(q.to_array());
            let f = q * Vec3::X;
            let right = q * Vec3::NEG_Y;
            self.telemetry.orientation = [
                (-f.y).atan2(f.x).to_degrees(),
                f.z.atan2(f.truncate().length()).to_degrees(),
                (-right.z).clamp(-1., 1.).asin().to_degrees(),
            ];
        }
        let m = &g.motion;
        // Use the same neutral hold convention for bindable tilt/acceleration.
        let hold = Quat::from_rotation_z(m.hold_angles[1].to_radians())
            * Quat::from_rotation_y(m.hold_angles[0].to_radians())
            * Quat::from_rotation_x(m.hold_angles[2].to_radians());
        let held_accel = accel.map(|a| {
            let v = hold * Vec3::new(a.x, -a.y, -a.z);
            Vec3::new(v.x, -v.y, -v.z)
        });
        self.reading = self.imu.tick(hold * gyro, held_accel, dt);
        if m.noise_gate > 0. && accel_noise > 0. {
            let mag = Vec3::new(self.reading.0[3], self.reading.0[4], self.reading.0[5]).length();
            let gain = flexinput_engine::eval::gyro_dz_ease(mag, accel_noise * m.noise_gate);
            for v in &mut self.reading.0[3..6] {
                *v *= gain;
            }
        }
        self.telemetry.quantities = self.reading.0;
        self.telemetry.accel_fresh = accel.is_some();
        self.telemetry.accel_g = accel
            .unwrap_or(Vec3::ZERO)
            .to_array()
            .map(|v| v * BUS_ACCEL_G);
        if m.mode == MotionMode::Steering {
            let signs = Vec2::new(
                if m.invert_x { -1. } else { 1. },
                if g.invert_y { -1. } else { 1. },
            );
            return MotionSample {
                rate: Vec2::ZERO,
                physical_rate: Vec2::ZERO,
                steering: Some(
                    (Vec2::new(raw.x, raw.y * g.vertical_ratio) / m.steering_range
                        * g.sensitivity
                        * signs)
                        .clamp(Vec2::splat(-1.), Vec2::ONE),
                ),
            };
        }
        // Positive screen Y points down; the canonical pitch is nose-up.
        let mut rate = Vec2::new(raw.x, -raw.y);
        if apply_custom && self.custom_filters.has_stage(FilterStage::BeforeBuiltin) {
            let value = self.custom_filters.process(
                FilterStage::BeforeBuiltin,
                FilterSample {
                    value: [f64::from(rate.x), f64::from(rate.y), 0.],
                    ..custom_sample
                },
            );
            rate = Vec2::new(value[0] as f32, value[1] as f32);
        }
        rate = suppress_idle(g, rate, sensor_noise);
        let settings = cc_settings(g);
        rate = self.smoothing.tick(g, rate, dt);
        let speed = rate.length();
        rate = cutoff_recovery(g, rate, speed, dt, &mut self.recovery);
        // A 125 ms, time-weighted history keeps trackball behavior independent of Hz.
        self.history.push_back((rate, dt));
        let mut total: f32 = self.history.iter().map(|(_, t)| *t).sum();
        while total > 0.125 && self.history.len() > 1 {
            let excess = total - 0.125;
            let front = self.history.front_mut().unwrap();
            if front.1 > excess {
                front.1 -= excess;
                break;
            }
            total -= front.1;
            self.history.pop_front();
        }
        let duration: f32 = self.history.iter().map(|(_, t)| *t).sum();
        let average = self
            .history
            .iter()
            .fold(Vec2::ZERO, |a, (v, t)| a + *v * *t)
            / duration.max(1e-6);
        for i in 0..2 {
            if tracking[i] {
                if !self.was_tracking[i] {
                    self.coast[i] = average[i];
                }
                self.coast[i] *= (-dt * m.trackball.decay).exp2();
                rate[i] = self.coast[i];
            }
        }
        self.was_tracking = tracking;
        let speed_after = rate.length();
        let a = &g.acceleration;
        let gain = if a.curve == Curve::Custom {
            sample_curve(&a.points, speed_after, &[])
        } else {
            let omega = (speed_after - a.start_dps).max(0.);
            cc::sensitivity(
                &settings,
                omega,
                (omega / (a.full_dps - a.start_dps)).clamp(0., 1.),
                a.full_dps - a.start_dps,
                1.,
                a.fast_multiplier,
            )
        };
        rate *= gain * self.brake.tick(&settings, dt, speed, speed_after);
        let (x, y) = cc::angle_snap(&settings, rate.x, rate.y);
        rate = Vec2::new(x, y);
        if m.invert_x {
            rate.x = -rate.x;
        }
        if g.invert_y {
            rate.y = -rate.y;
        }
        if apply_custom && self.custom_filters.has_stage(FilterStage::AfterBuiltin) {
            let value = self.custom_filters.process(
                FilterStage::AfterBuiltin,
                FilterSample {
                    value: [f64::from(rate.x), f64::from(rate.y), 0.],
                    ..custom_sample
                },
            );
            rate = Vec2::new(value[0] as f32, value[1] as f32);
        }
        MotionSample {
            rate,
            physical_rate: Vec2::new(raw.x, -raw.y),
            steering: None,
        }
    }
    pub fn rotate(&mut self, g: &Gyro, stick: Vec2, rate: f32, dt: f32) -> Vec2 {
        let r = &g.motion.rotation;
        let out = self.rotation.tick(
            stick,
            rate * r.sensitivity * g.sensitivity,
            dt,
            &RotationSettings {
                deadzone: r.deadzone,
                smooth_s: r.smooth_ms / 1000.,
                return_s: r.relative.then_some(r.return_ms / 1000.),
                takeover: r.takeover.then_some(Takeover {
                    speed_dps: r.takeover_dps,
                    outer: r.outer,
                }),
            },
        );
        self.telemetry.rotation_offset = out.offset_deg;
        self.telemetry.rotation_taken = out.taken;
        out.out
    }
}
fn cc_settings(g: &Gyro) -> cc::Settings {
    let a = &g.acceleration;
    let m = &g.motion;
    cc::Settings {
        curve: match a.curve {
            Curve::Natural => cc::Curve::Natural,
            Curve::Power => cc::Curve::Power,
            Curve::Quadratic => cc::Curve::Quadratic,
            Curve::Sigmoid => cc::Curve::Sigmoid,
            Curve::Jump => cc::Curve::Jump,
            _ => cc::Curve::Linear,
        },
        natural_vhalf: a.natural_half,
        power_vref: a.power_reference,
        power_exponent: a.power_exponent,
        sigmoid_mid: a.sigmoid_mid,
        sigmoid_width: a.sigmoid_width,
        jump_tau: a.jump_width,
        angle_snap: m.angle_snap,
        angle_snap_ease: m.angle_snap_ease,
        brake_strength: m.brake_strength,
        brake_threshold: m.brake_threshold,
        ..Default::default()
    }
}
