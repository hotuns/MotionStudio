//! Stage responses to synthetic input. These never touch the live device or profile.
use super::{cc, cc_settings, cutoff_recovery, suppress_idle, AimSmoothing, MotionRuntime};
use crate::{profile::Gyro, timing::SampleTime};
use anyhow::{ensure, Result};
use flexinput_core::imu::{BUS_ACCEL_G, BUS_GYRO_DPS};
use glam::{Vec2, Vec3};
use serde::Serialize;
use std::sync::OnceLock;

/// Horizontal coordinate, reference value, processed value.
type Point = [f32; 3];

pub fn flick_release(strength: f32) -> Result<Vec<Point>> {
    crate::motion_config::range(
        "Flick 缓出强度",
        strength,
        0.,
        flexinput_engine::eval::MAX_FLICK_RELEASE_EASE,
    )?;
    Ok((0..=100)
        .map(|index| {
            let progress = index as f32 / 100.;
            [
                index as f32,
                index as f32,
                100. * flexinput_engine::eval::flick_release_progress(progress, strength),
            ]
        })
        .collect())
}

#[derive(Serialize)]
pub struct StabilityPreview {
    pub smoothing: Vec<Point>,
    pub idle: Vec<Point>,
    pub recovery: Vec<Point>,
    pub snap: Vec<Point>,
    pub brake: Vec<Point>,
    pub drift: Vec<Point>,
    pub measured_noise: bool,
    pub noise_floor: f32,
}

pub fn preview(gyro: &Gyro, sensor_noise: Option<f32>) -> Result<StabilityPreview> {
    gyro.motion.validate()?;
    ensure!(
        (0.0..=50.0).contains(&gyro.smoothing_ms) && (0.0..=5.0).contains(&gyro.noise_threshold),
        "平滑时间需为 0–50 ms，静止抑制需为 0–5 °/s"
    );
    let measured = sensor_noise.filter(|noise| *noise > 0.0 && *noise <= 20.0);
    let noise_floor = measured.unwrap_or(0.25);
    let settings = cc_settings(gyro);
    Ok(StabilityPreview {
        smoothing: smoothing(gyro),
        idle: idle(gyro, noise_floor),
        recovery: recovery(gyro),
        snap: snap(&settings),
        brake: brake(&settings),
        drift: drift(gyro.motion.drift_correction),
        measured_noise: measured.is_some(),
        noise_floor,
    })
}

fn smoothing(gyro: &Gyro) -> Vec<Point> {
    let mut filter = AimSmoothing::default();
    (0..=200)
        .map(|index| {
            let time_ms = index * 2;
            let wobble = (time_ms as f32 * std::f32::consts::TAU / 20.).sin() * 5.;
            let input = match time_ms {
                40..=158 => 32. + wobble,
                220..=338 => 105. + wobble,
                _ => 0.,
            };
            let output = filter.tick(gyro, Vec2::new(input, 0.), 0.002).x;
            [time_ms as f32, input, output]
        })
        .collect()
}

fn plot_limit(minimum: f32, needed: f32) -> f32 {
    minimum * 2_f32.powf((needed / minimum).max(1.).log2().ceil())
}

fn idle(gyro: &Gyro, noise: f32) -> Vec<Point> {
    let m = &gyro.motion;
    let end = plot_limit(
        5.,
        gyro.noise_threshold + 2. * noise * m.noise_gate + 1.5 * m.cutoff_dps,
    );
    (0..=160)
        .map(|index| {
            let input = end * index as f32 / 160.;
            let gated = suppress_idle(gyro, Vec2::new(input, 0.), noise);
            // A steady input with recovery already completed.
            let mut gain = 1.;
            let output = cutoff_recovery(gyro, gated, gated.length(), 0., &mut gain).x;
            [input, input, output]
        })
        .collect()
}

fn recovery(gyro: &Gyro) -> Vec<Point> {
    let speed = gyro.motion.cutoff_dps + 1.;
    let input = Vec2::new(speed, 0.);
    let step_ms = plot_limit(160., gyro.motion.recovery_ms * 1.25) / 120.;
    let mut gain = 0.;
    (0..=120)
        .map(|index| {
            let dt = if index == 0 { 0. } else { step_ms / 1000. };
            let output = cutoff_recovery(gyro, input, speed, dt, &mut gain).x;
            [step_ms * index as f32, 100., output / speed * 100.]
        })
        .collect()
}

fn snap(settings: &cc::Settings) -> Vec<Point> {
    (0..=180)
        .map(|index| {
            let angle = index as f32 / 2.;
            let (sin, cos) = angle.to_radians().sin_cos();
            let (x, y) = cc::angle_snap(settings, cos, sin);
            [angle, angle, y.atan2(x).to_degrees().clamp(0., 90.)]
        })
        .collect()
}

fn brake(settings: &cc::Settings) -> Vec<Point> {
    let end = plot_limit(20., settings.brake_threshold * 4.);
    let speed = 30.;
    (0..=120)
        .map(|index| {
            let drop = end * index as f32 / 120.;
            let mut brake = cc::Brake::default();
            brake.tick(settings, 0.010, speed + drop, speed + drop);
            let gain = brake.tick(settings, 0.010, speed, speed);
            [drop, 100., gain * 100.]
        })
        .collect()
}

fn drift(enabled: bool) -> Vec<Point> {
    // The scenario is fixed. Sample the existing 3DOF node once and reuse its
    // result while sliders change in the other previews.
    static CORRECTED: OnceLock<Vec<Point>> = OnceLock::new();
    CORRECTED
        .get_or_init(|| {
            let mut gyro = Gyro::default();
            gyro.motion.drift_correction = true;
            let mut runtime = MotionRuntime::builtin(&gyro);
            let gravity = Some(Vec3::Z / BUS_ACCEL_G);
            let mut tick = |raw, accel| {
                runtime.sample(
                    &gyro,
                    raw,
                    accel,
                    0.010,
                    false,
                    false,
                    [false; 2],
                    0.,
                    0.,
                    SampleTime {
                        host_ns: 0,
                        sensor_ns: 0,
                    },
                    false,
                );
                runtime.telemetry.orientation[1].abs()
            };
            tick(Vec3::ZERO, gravity);
            // Accumulate a tilt error while gravity is unavailable, then
            // restore the stationary 1 g reference.
            let mut error = 0.;
            for _ in 0..20 {
                error = tick(Vec3::Y * (30. / BUS_GYRO_DPS), None);
            }
            let mut points = vec![[0., error, error]];
            points.extend(
                (1..=100).map(|index| [index as f32 * 10., error, tick(Vec3::ZERO, gravity)]),
            );
            points
        })
        .iter()
        .map(|&[time, reference, corrected]| {
            [time, reference, if enabled { corrected } else { reference }]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion_config::Smoothing;

    #[test]
    fn previews_follow_the_selected_stages_and_keep_the_reference() {
        let mut gyro = Gyro::default();
        let baseline = preview(&gyro, None).unwrap();
        assert_eq!(baseline.noise_floor, 0.25);
        assert!(!baseline.measured_noise);
        assert!(baseline.smoothing.iter().all(|p| p[1] == p[2]));
        assert!(baseline.drift.iter().all(|p| p[1] == p[2]));

        gyro.smoothing_ms = 30.;
        gyro.noise_threshold = 1.;
        gyro.motion.noise_gate = 2.;
        gyro.motion.cutoff_dps = 2.;
        gyro.motion.recovery_ms = 160.;
        gyro.motion.angle_snap = 15.;
        gyro.motion.angle_snap_ease = false;
        gyro.motion.brake_strength = 0.6;
        gyro.motion.drift_correction = true;
        let curves = preview(&gyro, Some(0.4)).unwrap();
        assert!(curves.measured_noise);
        assert_eq!(curves.smoothing[20][1], baseline.smoothing[20][1]);
        assert!(curves.smoothing[20][2] < 5.);
        assert!(curves.idle.iter().any(|p| p[0] > 2. && p[2] == 0.));
        assert!((curves.recovery[30][2] - 50.).abs() < 0.01);
        assert_eq!(curves.snap[20], [10., 10., 0.]);
        assert!((curves.brake.last().unwrap()[2] - 40.).abs() < 0.01);
        assert!(curves.drift[0][2] > 5.);
        assert!(curves.drift.last().unwrap()[2] < 0.01);

        gyro.motion.smoothing = Smoothing::Decay;
        let decay = preview(&gyro, Some(0.4)).unwrap();
        assert!((decay.smoothing[110][2] - decay.smoothing[110][1]).abs() < 0.01);
        gyro.motion.smoothing = Smoothing::OneEuro;
        gyro.motion.one_euro_beta = 0.;
        let euro = preview(&gyro, Some(0.4)).unwrap();
        assert_ne!(euro.smoothing, curves.smoothing);
        assert_ne!(euro.smoothing, decay.smoothing);
    }
}
