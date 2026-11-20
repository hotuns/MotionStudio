//! Adapt Studio's editable profile and two clocks to the native JSM aiming pipeline.
use crate::{
    motion::cc_settings,
    motion_config::{FlickSnap, Smoothing, Space},
    profile::{Gyro, GyroOutput},
};
use flexinput_core::imu::BUS_GYRO_DPS;
use flexinput_engine::eval::{jsm_aim as jsm, jsm_motion_curves};
use glam::{Vec2, Vec3};
use std::collections::HashSet;

#[derive(PartialEq)]
struct Settings {
    aim: jsm::Settings,
    motion: jsm::MotionSettings,
    curves: jsm_motion_curves::Settings,
    analog: jsm::AnalogSettings,
    pad: jsm::PadSettings,
    gesture_mode: jsm::StickMode,
    stick_scale: f32,
}

impl Settings {
    fn from_profile(g: &Gyro) -> Self {
        let m = &g.motion;
        let f = &m.flick;
        let min_sens = (g.sensitivity, g.sensitivity * g.vertical_ratio);
        let fast = g.acceleration.fast_multiplier;
        let stick_speed = f.max_dps * f.aim_sensitivity;
        let response = &g.jsm_response;
        let shared_window = if m.smoothing == Smoothing::OneEuro {
            0.
        } else {
            g.smoothing_ms
        };
        let aim = jsm::Settings {
            min_sens,
            max_sens: (min_sens.0 * fast, min_sens.1 * fast),
            min_threshold: g.acceleration.start_dps,
            max_threshold: g.acceleration.full_dps,
            axis_x: if m.invert_x { -1. } else { 1. },
            axis_y: if g.invert_y { -1. } else { 1. },
            mouse_x_from: if m.space == Space::PitchRoll {
                jsm::AxisMask {
                    z: true,
                    ..Default::default()
                }
            } else {
                jsm::AxisMask::Y
            },
            real_world_calibration: g.counts_per_degree,
            smooth_time: response.smooth_time_ms.unwrap_or(shared_window) / 1000.,
            smooth_threshold: response.smooth_threshold_dps.unwrap_or(m.decay_threshold),
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
            stick_sens: (stick_speed, stick_speed * f.vertical_ratio),
            ..Default::default()
        };
        let motion = jsm::MotionSettings {
            space: match m.space {
                Space::PitchYaw | Space::PitchRoll => jsm::Space::Local,
                Space::Player => jsm::Space::PlayerTurn,
                Space::World => jsm::Space::WorldTurn,
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
            roll_contribution: if m.space == Space::PitchYaw {
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
        analog.left.outer_dz = if f.enabled { 1. - f.deadzone } else { 0. };
        analog.right.inner_dz = 0.;
        analog.right.outer_dz = 0.;
        analog.right.mode = if dest == jsm::Dest::RightStick {
            jsm::StickMode::VirtualStick(1)
        } else {
            jsm::StickMode::Inert
        };
        let gesture_mode = if f.enabled {
            if f.only {
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
            gesture_mode,
            stick_scale: stick_speed / pad.calibration,
        }
    }
}

pub(crate) struct JsmAim {
    settings: Settings,
    aim: jsm::Aim,
    motion: jsm::Motion,
    gravity: jsm::Gravity,
    analog: jsm::Analog,
    pad: jsm::Pad,
    actions: HashSet<jsm::GyroAction>,
    has_sample: bool,
    stick_active: bool,
    pub right: Option<Vec2>,
}

impl JsmAim {
    pub fn new(g: &Gyro) -> Self {
        Self {
            settings: Settings::from_profile(g),
            aim: Default::default(),
            motion: Default::default(),
            gravity: Default::default(),
            analog: Default::default(),
            pad: Default::default(),
            actions: HashSet::with_capacity(2),
            has_sample: false,
            stick_active: false,
            right: None,
        }
    }

    pub fn configure(&mut self, g: &Gyro) {
        let next = Settings::from_profile(g);
        if next != self.settings {
            self.clear_gyro();
            self.settings = next;
        }
    }

    pub fn clear_gyro(&mut self) {
        self.aim.reset_sampled_gyro();
        self.motion = Default::default();
        self.gravity = Default::default();
        self.has_sample = false;
        self.right = None;
        if !self.stick_active {
            self.analog = Default::default();
        }
    }

    pub fn clear_stick(&mut self) {
        if self.stick_active {
            self.aim.reset_sticks();
            self.analog = Default::default();
            self.stick_active = false;
            self.right = None;
        }
    }

    pub fn sample(&mut self, raw: Vec3, accel: Option<Vec3>, dt: f32, tracking: [bool; 2]) -> Vec2 {
        let s = &self.settings;
        self.gravity = self.motion.tick(accel, dt);
        self.actions.clear();
        if tracking[0] {
            self.actions.insert(jsm::GyroAction::TrackballX);
        }
        if tracking[1] {
            self.actions.insert(jsm::GyroAction::TrackballY);
        }
        let rate = raw * BUS_GYRO_DPS;
        self.has_sample = true;
        self.aim.sample_motion(
            &s.aim,
            &s.pad,
            &s.motion,
            &s.curves,
            self.gravity,
            dt,
            jsm::Gyro {
                roll: rate.x,
                pitch: rate.y,
                yaw: rate.z,
            },
            &self.actions,
        )
    }

    pub fn output(&mut self, dt: f32, gesture: Option<Vec2>, mapped_right: Vec2) -> Vec2 {
        if gesture.is_none() {
            self.clear_stick();
        }
        if !self.has_sample && gesture.is_none() {
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
            analog.left.mode = jsm::StickMode::Inert;
            pad.out[1].virtual_scale = s.stick_scale;
        } else {
            input.sticks[0] = gesture.unwrap_or_default().into();
        }
        self.analog.tick(&analog, dt, &input);
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
            .tick(
                &pad,
                dt,
                &self.analog,
                s.aim.stick_power,
                aimed.gyro_dps,
                aimed.flick_dps,
                None,
            )
            .sticks[1];
        aimed.mouse
    }
}
