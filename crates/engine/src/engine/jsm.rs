//! Adapt Studio's editable profile and two clocks to the native JSM aiming pipeline.
use super::gamepad_motion::GamepadMotion;
use crate::{
    motion::{cc_settings, filter_sample, filter_screen_rate},
    motion_config::{FlickSnap, Smoothing, Space},
    profile::{Gyro, GyroOutput},
    stick_response::RightStickCurve,
    timing::SampleTime,
};
use flexinput_engine::eval::{jsm_aim as jsm, jsm_motion_curves};
use glam::{Vec2, Vec3};
use motion_filters::{FilterChain, Stage};
use std::collections::HashSet;

#[derive(PartialEq)]
struct Settings {
    aim: jsm::Settings,
    motion: jsm::MotionSettings,
    curves: jsm_motion_curves::Settings,
    analog: jsm::AnalogSettings,
    pad: jsm::PadSettings,
    right_stick_curve: Option<RightStickCurve>,
    gesture_mode: jsm::StickMode,
    stick_scale: f32,
    auto_calibrate: bool,
    fast_multiplier: f32,
}

fn axis_mask(bits: u8) -> jsm::AxisMask {
    jsm::AxisMask {
        x: bits & 1 != 0,
        y: bits & 2 != 0,
        z: bits & 4 != 0,
    }
}

impl Settings {
    fn from_profile(g: &Gyro, inner_deadzone: f32) -> Self {
        let m = &g.motion;
        let f = &m.flick;
        let min_sens = (g.sensitivity, g.sensitivity * g.vertical_ratio);
        let fast = g.acceleration.fast_multiplier;
        let stick_speed = f.max_dps * f.aim_sensitivity;
        let response = &g.jsm_response;
        let legacy_smoothing = g.smoothing_ms > 0. && m.smoothing != Smoothing::OneEuro;
        let defaults = jsm::Settings::default();
        let window_ms = if legacy_smoothing {
            g.smoothing_ms
        } else {
            defaults.smooth_time * 1000.
        };
        let threshold = if legacy_smoothing {
            m.decay_threshold
        } else {
            defaults.smooth_threshold
        };
        let aim = jsm::Settings {
            min_sens,
            max_sens: (min_sens.0 * fast, min_sens.1 * fast),
            min_threshold: g.acceleration.start_dps,
            max_threshold: g.acceleration.full_dps,
            axis_x: if m.invert_x { -1. } else { 1. },
            axis_y: if g.invert_y { -1. } else { 1. },
            mouse_x_from: axis_mask(
                response
                    .mouse_x_axes
                    .unwrap_or(if m.space == Space::PitchRoll { 4 } else { 2 }),
            ),
            mouse_y_from: axis_mask(response.mouse_y_axes.unwrap_or(1)),
            real_world_calibration: g.counts_per_degree,
            smooth_time: response.smooth_time_ms.unwrap_or(window_ms) / 1000.,
            smooth_threshold: response.smooth_threshold_dps.unwrap_or(threshold),
            cutoff_speed: m.cutoff_dps,
            cutoff_recovery: g.jsm_response.cutoff_recovery_dps,
            trackball_decay: m.trackball.decay,
            flick_time: f.smooth_ms / 1000.,
            flick_time_exponent: f.time_exponent,
            flick_deadzone_angle: f.forward_deadzone,
            flick_snap: match f.snap_mode {
                FlickSnap::None => jsm::SnapMode::None,
                FlickSnap::Four => jsm::SnapMode::Four,
                FlickSnap::Eight => jsm::SnapMode::Eight,
            },
            flick_snap_strength: f.snap_strength,
            rotate_smooth_override: f.rotate_smooth_override,
            stick_sens: (stick_speed, stick_speed * f.vertical_ratio),
            ..Default::default()
        };
        let motion = jsm::MotionSettings {
            space: match m.space {
                Space::PitchYaw | Space::PitchRoll => jsm::Space::Local,
                Space::YawPlusRoll => jsm::Space::YawPlusRoll,
                Space::Player => jsm::Space::PlayerTurn,
                Space::PlayerLean => jsm::Space::PlayerLean,
                Space::World => jsm::Space::WorldTurn,
                Space::WorldLean => jsm::Space::WorldLean,
            },
            local_axis_offset: m.hold_angles[0],
            ..Default::default()
        };
        let curves = jsm_motion_curves::Settings {
            decay_smoothing: response
                .decay_smoothing
                .unwrap_or(m.smoothing == Smoothing::Decay),
            one_euro_enabled: response
                .one_euro
                .unwrap_or(m.smoothing == Smoothing::OneEuro),
            one_euro_min_cutoff: m.one_euro_cutoff,
            one_euro_speed_coeff: m.one_euro_beta,
            roll_contribution: if m.space == Space::YawPlusRoll {
                m.roll_contribution
            } else {
                0.
            },
            ..cc_settings(g)
        };
        let dest = if g.output == GyroOutput::RightStick {
            jsm::Dest::RightStick
        } else {
            jsm::Dest::Mouse
        };
        let mut pad = jsm::PadSettings {
            calibration: g.stick_full_scale_dps,
            gyro_dest: dest,
            flick_dest: dest,
            ..Default::default()
        };
        pad.out[1].undeadzone_inner = g.jsm_response.right_undeadzone_inner;
        pad.out[1].undeadzone_outer = g.jsm_response.right_undeadzone_outer;
        pad.out[1].unpower = g.jsm_response.right_unpower;
        // Both inputs are already calibrated by the FlexInput physical source.
        // Slot 0 receives the chosen thumb. Slot 1 carries the Studio-mapped right
        // stick into JSM's rate mixer, including cross-stick and empty layouts.
        let mut analog = jsm::AnalogSettings::default();
        analog.left.inner_dz = 0.;
        // The source has already rescaled travel above the inner deadzone.
        // Express JSM's physical outer boundary in that same coordinate system.
        analog.left.outer_dz =
            ((1. - f.deadzone) / (1. - inner_deadzone)).clamp(0., 1. - f32::EPSILON);
        analog.right.inner_dz = 0.;
        analog.right.outer_dz = 0.;
        analog.right.mode = if dest == jsm::Dest::RightStick {
            jsm::StickMode::VirtualStick(1)
        } else {
            jsm::StickMode::Inert
        };
        let gesture_mode = if f.enabled {
            if f.rotate_only {
                jsm::StickMode::RotateOnly
            } else if f.only {
                jsm::StickMode::FlickOnly
            } else {
                jsm::StickMode::Flick
            }
        } else if f.stick_aim {
            if dest == jsm::Dest::Mouse {
                jsm::StickMode::Aim
            } else {
                jsm::StickMode::VirtualStick(1)
            }
        } else {
            jsm::StickMode::Inert
        };
        Self {
            aim,
            motion,
            curves,
            analog,
            pad,
            right_stick_curve: g.right_stick_curve.as_ref().filter(|curve| curve.enabled).cloned(),
            gesture_mode,
            stick_scale: stick_speed / pad.calibration,
            auto_calibrate: response.auto_calibrate_gyro,
            fast_multiplier: fast,
        }
    }
}

pub(crate) struct JsmAim {
    settings: Settings,
    aim: jsm::Aim,
    motion: GamepadMotion,
    gravity: jsm::Gravity,
    analog: jsm::Analog,
    pad: jsm::Pad,
    actions: HashSet<jsm::GyroAction>,
    has_sample: bool,
    stick_active: bool,
    pending_flick: Option<Vec2>,
    pub right: Option<Vec2>,
}

impl JsmAim {
    pub fn new(g: &Gyro, inner_deadzone: f32) -> Self {
        Self {
            settings: Settings::from_profile(g, inner_deadzone),
            aim: Default::default(),
            motion: Default::default(),
            gravity: Default::default(),
            analog: Default::default(),
            pad: Default::default(),
            actions: HashSet::with_capacity(2),
            has_sample: false,
            stick_active: false,
            pending_flick: None,
            right: None,
        }
    }

    pub fn configure(&mut self, g: &Gyro, inner_deadzone: f32) {
        let next = Settings::from_profile(g, inner_deadzone);
        if next.motion.space != self.settings.motion.space
            || next.pad.gyro_dest != self.settings.pad.gyro_dest
            || next.aim.mouse_x_from != self.settings.aim.mouse_x_from
            || next.aim.mouse_y_from != self.settings.aim.mouse_y_from
        {
            self.clear_rate();
        }
        self.settings = next;
    }

    pub fn flick_threshold(&self) -> f32 {
        1. - self.settings.analog.left.outer_dz
    }

    pub fn clear_gyro(&mut self) {
        self.motion.reset();
        self.gravity = Default::default();
        self.clear_rate();
    }

    pub fn clear_rate(&mut self) {
        self.aim.reset_sampled_gyro();
        self.has_sample = false;
        self.right = None;
    }

    pub fn block_gyro(&mut self) {
        self.aim.block_sampled_gyro();
        self.has_sample = false;
        self.right = None;
    }

    // A thumb can reach the rim and return between two output reports. Capture
    // its first complete vector when an SDL axis report closes. JSM still
    // owns snapping, hysteresis and the entire turn on the output clock.
    pub fn observe_flick(&mut self, thumb: Vec2) {
        if self.pending_flick.is_none()
            && !self.aim.flick_engaged(0)
            && matches!(
                self.settings.gesture_mode,
                jsm::StickMode::Flick | jsm::StickMode::FlickOnly
            )
            && thumb.length() >= self.flick_threshold()
        {
            self.pending_flick = Some(thumb);
        }
    }

    pub fn clear_stick(&mut self) {
        self.pending_flick = None;
        if self.stick_active {
            self.reset_sticks();
        }
    }

    pub fn reset_sticks(&mut self) {
        self.pending_flick = None;
        self.aim.reset_sticks();
        self.analog = Default::default();
        self.stick_active = false;
        self.right = None;
    }

    fn observe(&mut self, raw: Vec3, accel: Option<Vec3>, dt: f32) -> jsm::Gyro {
        let (gyro, gravity) = self
            .motion
            .sample(raw, accel, dt, self.settings.auto_calibrate);
        self.gravity = gravity;
        gyro
    }

    pub fn measurement_rate(&mut self, raw: Vec3, accel: Option<Vec3>, dt: f32) -> Vec2 {
        let gyro = self.observe(raw, accel, dt);
        let s = &self.settings;
        jsm::Aim::project_motion(&s.aim, &s.motion, &s.curves, self.gravity, gyro).into()
    }

    pub fn sample(
        &mut self,
        raw: Vec3,
        accel: Option<Vec3>,
        dt: f32,
        tracking: [bool; 2],
        active: bool,
        filters: Option<(&mut FilterChain, SampleTime)>,
    ) -> Vec2 {
        let mut gyro = self.observe(raw, accel, dt);
        let s = &self.settings;
        self.actions.clear();
        if tracking[0] {
            self.actions.insert(jsm::GyroAction::TrackballX);
        }
        if tracking[1] {
            self.actions.insert(jsm::GyroAction::TrackballY);
        }
        self.has_sample = active;
        let Some((chain, time)) = filters.filter(|(chain, _)| active && chain.has_active()) else {
            return self.aim.sample_motion(
                &s.aim,
                &s.pad,
                &s.motion,
                &s.curves,
                self.gravity,
                dt,
                gyro,
                &self.actions,
                !active,
            );
        };

        // The estimator observes the physical IMU. Scripts see its bias-corrected
        // rate in Studio's existing forward/right/down frame, in degrees/s.
        chain.begin_sample();
        let sample = filter_sample(Vec3::new(-gyro.roll, gyro.pitch, gyro.yaw), accel, dt, time);
        if chain.has_stage(Stage::RawGyro) {
            let [roll, pitch, yaw] = chain.process(Stage::RawGyro, sample).map(|v| v as f32);
            gyro = jsm::Gyro {
                pitch,
                yaw,
                roll: -roll,
            };
        }
        let mut projected =
            jsm::Aim::project_motion(&s.aim, &s.motion, &s.curves, self.gravity, gyro);
        if chain.has_stage(Stage::BeforeBuiltin) {
            projected =
                filter_screen_rate(chain, Stage::BeforeBuiltin, sample, projected.into()).into();
        }

        // The original script API places after_builtin after the acceleration
        // curve but before base sensitivity and the vertical ratio.
        let after = chain.has_stage(Stage::AfterBuiltin);
        let mut aim = s.aim;
        if after {
            aim.min_sens = (1., 1.);
            aim.max_sens = (s.fast_multiplier, s.fast_multiplier);
        }
        self.aim.sample_projected_motion(
            &aim,
            &s.pad,
            &s.curves,
            projected,
            dt,
            gyro,
            &self.actions,
            !active,
            |rate| {
                if after {
                    filter_screen_rate(chain, Stage::AfterBuiltin, sample, rate)
                        * Vec2::from(s.aim.min_sens)
                } else {
                    rate
                }
            },
        )
    }

    pub fn output(&mut self, dt: f32, gesture: Option<Vec2>, mapped_right: Vec2) -> Vec2 {
        if gesture.is_none() {
            self.clear_stick();
        }
        let gesture = self.pending_flick.take().or(gesture);
        let idle = !self.has_sample && gesture.is_none();
        if idle && self.settings.pad.gyro_dest != jsm::Dest::RightStick {
            self.right = None;
            return Vec2::ZERO;
        }
        self.stick_active = gesture.is_some();
        let s = &self.settings;
        let mut analog = s.analog;
        let mut pad = s.pad;
        analog.left.mode = if gesture.is_some() {
            s.gesture_mode
        } else {
            jsm::StickMode::Inert
        };
        let mut input = jsm::PadInput::default();
        input.sticks[1] = mapped_right.into();
        if analog.left.mode == jsm::StickMode::VirtualStick(1) {
            // In native stick-aim mode the selected thumb owns the carrier.
            input.sticks[1] = gesture.unwrap_or_default().into();
            analog.right.outer_dz = analog.left.outer_dz;
            analog.left.mode = jsm::StickMode::Inert;
            pad.out[1].virtual_scale = s.stick_scale;
        } else {
            input.sticks[0] = gesture.unwrap_or_default().into();
        }
        self.analog.tick(&analog, dt, &input);
        if idle {
            // The native mixer reads the previous report's physical direction.
            // Track it while the gyro is gated so the first live report can add
            // gyro to a stick the player was already holding.
            self.right = None;
            return Vec2::ZERO;
        }
        let aimed = self.aim.tick_sampled(
            &s.aim,
            &pad,
            &s.motion,
            &s.curves,
            self.gravity,
            dt,
            jsm::Gyro::default(),
            &self.analog,
            &self.actions,
            &|_| false,
            jsm::JsmGyroClock::Unchanged,
        );
        self.right = self
            .pad
            .tick_with_response(
                &pad,
                dt,
                &self.analog,
                s.aim.stick_power,
                aimed.gyro_dps,
                aimed.flick_dps,
                None,
                [None, s.right_stick_curve.as_ref().map(|curve| curve as &dyn jsm::StickResponse)],
            )
            .sticks[1];
        aimed.mouse
    }
}
