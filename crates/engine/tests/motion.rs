use motion_core::{
    engine::{Ds4ImuState, Engine, InputEvent},
    motion_config::*,
    profile::{Action, GamepadTarget, GyroActivationMode, GyroOutput, Profile, TimestampSource},
    storage::parse_profile,
    timing::SampleTime,
};
use motion_filters::{FilterConfig, Stage};

fn profile() -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/gyro-mouse.toml")).unwrap();
    p.stick_deadzone = 0.; // Motion tests isolate upstream motion from source calibration.
    p.gyro.enabled = true;
    p.gyro.activation = "always".into();
    p.gyro.counts_per_degree = 10.;
    p.gyro.timestamp_source = TimestampSource::Sensor;
    p
}
fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: ms * 1_000_000,
    }
}
fn gyro(e: &mut Engine, ms: u64, roll: f32, pitch: f32, yaw: f32) {
    e.process(&InputEvent::Gyro {
        time: time(ms),
        value: [pitch.to_radians(), -yaw.to_radians(), -roll.to_radians()],
    });
}
fn accel(e: &mut Engine, ms: u64, a: [f32; 3]) {
    e.process(&InputEvent::Accel {
        time: time(ms),
        value: [-a[1] * 9.80665, a[2] * 9.80665, -a[0] * 9.80665],
    });
}
fn axis(e: &mut Engine, index: u8, value: i16) {
    e.process(&InputEvent::Axis {
        time: time(1),
        index,
        value,
    });
}
fn button(e: &mut Engine, index: u8, down: bool) {
    e.process(&InputEvent::Button {
        time: time(1),
        index,
        down,
    });
}
fn gamepad(p: &mut Profile) {
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.gyro.output = GyroOutput::RightStick;
}

#[test]
fn legacy_aim_engine_choices_load_and_drop_out_of_saved_profiles_and_layers() {
    let source = include_str!("../../../examples/gyro-mouse.toml");
    for choice in ["studio", "jsm"] {
        let legacy = format!(r#"{source}
aim_engine = "{choice}"
[gyro.jsm_response]
smooth_threshold_dps = 14.0
[[layers]]
id = "aim"
name = "Aim"
input = "left_shoulder"
[layers.settings.gyro]
aim_engine = "{choice}"
sensitivity = 2.0
"#);
        let profile = parse_profile(&legacy).unwrap();
        let saved = toml::to_string(&profile).unwrap();
        assert!(!saved.contains("aim_engine"));
        let reopened = parse_profile(&saved).unwrap();
        let aim = motion_core::layers::resolve(&reopened, &[0]).unwrap();
        assert_eq!(aim.gyro.sensitivity, 2.0);
        assert_eq!(aim.gyro.jsm_response.smooth_threshold_dps, Some(14.0));
        assert!(aim.gyro.uses_jsm_aim());
    }
}

#[test]
fn jsm_mouse_integrates_each_sample_once_on_both_axes() {
    let mut p = profile();
    p.gyro.pause_input = Some("south".into());
    let mut e = Engine::new(p).unwrap();
    gyro(&mut e, 10, 0., 20., 30.);
    gyro(&mut e, 20, 0., 20., 30.);
    let [x, y] = e.frame.mouse_delta;
    assert!((x - 3.).abs() < 1e-4 && (y + 2.).abs() < 1e-4);
    assert_eq!(e.advance_output(0.002, time(21).host_ns).mouse_delta, [0.; 2]);
    gyro(&mut e, 25, 0., 20., 30.);
    assert!((e.mouse_total[0] - 4.5).abs() < 1e-4 && (e.mouse_total[1] + 3.).abs() < 1e-4);
    button(&mut e, 0, true);
    gyro(&mut e, 30, 0., 20., 30.);
    gyro(&mut e, 40, 0., 20., 30.);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
}

#[test]
fn jsm_virtual_flick_uses_full_stick_speed_and_releases_when_flick_is_disabled() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.flick.smooth_ms = 2000.; // A mouse duration cannot slow a native stick flick.
    let mut e = Engine::new(p.clone()).unwrap();
    axis(&mut e, 2, i16::MAX);
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
    e.advance_output(0.002, time(2).host_ns);
    assert_eq!(e.frame.pad.unwrap().right_x, i16::MAX);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    axis(&mut e, 2, 0);
    gyro(&mut e, 3, 0., 0., 0.);
    gyro(&mut e, 4, 0., 0., 0.);
    e.advance_output(0.002, time(4).host_ns);
    assert_eq!(e.frame.pad.unwrap().right_x, i16::MAX);
    p.gyro.motion.flick.enabled = false;
    e.apply_layer(p).unwrap();
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
    assert_eq!(e.advance_output(0.002, time(6).host_ns).pad.unwrap().right_x, 0);
}

#[test]
fn ds4_sensor_passthrough_bypasses_aim_and_clears_on_pause_and_stale_accel() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::DualShock4;
    p.gyro.output = GyroOutput::Ds4Imu;
    p.gyro.pause_input = Some("south".into());
    p.gyro.sensitivity = 3.;
    p.gyro.motion.cutoff_dps = 100.;
    p.gyro.motion.custom_filters.push(FilterConfig {
        id: "aim-raw".into(),
        name: "Aim raw filter".into(),
        stage: Stage::RawGyro,
        source: "fn process(s, p) { [0.0, 0.0, 0.0] }".into(),
        ..Default::default()
    });
    let mut e = Engine::new(p).unwrap();
    assert_eq!(e.frame.ds4_imu, Ds4ImuState::Neutral);

    accel(&mut e, 10, [0., 0., 1.]);
    gyro(&mut e, 11, 1., 2., 3.);
    gyro(&mut e, 12, 1., 2., 3.);
    let Ds4ImuState::Sample(sample) = e.frame.ds4_imu else {
        panic!("missing gyro sample");
    };
    assert_eq!(sample.timestamp_ns, 12_000_000);
    for (got, expected) in sample.gyro_dps.into_iter().zip([1., 2., 3.]) {
        assert!((got - expected).abs() < 1e-4);
    }
    assert!((sample.accel_g[2] - 1.).abs() < 1e-5);
    assert_eq!(e.frame.mouse_delta, [0., 0.]);
    assert_eq!(e.frame.pad.unwrap().right_x, 0);

    button(&mut e, 0, true);
    assert_eq!(e.frame.ds4_imu, Ds4ImuState::Neutral);
    button(&mut e, 0, false);
    gyro(&mut e, 14, 1., 2., 3.);
    gyro(&mut e, 15, 1., 2., 3.);
    gyro(&mut e, 59, 1., 2., 3.);
    assert!(matches!(e.frame.ds4_imu, Ds4ImuState::Sample(_)));
    e.advance_output(0.001, 60_000_000);
    assert_eq!(e.frame.ds4_imu, Ds4ImuState::Neutral);
}

#[test]
fn ds4_filters_three_axes_only_while_enabled_and_restart_after_a_toggle() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::DualShock4;
    p.gyro.output = GyroOutput::Ds4Imu;
    p.gyro.activation = "south".into();
    p.gyro.activation_mode = GyroActivationMode::Toggle;
    p.gyro.ds4_imu_filter.smoothing_ms = 10.;
    p.gyro.ds4_imu_filter.use_custom_filters = true;
    for (id, stage, source) in [
        ("raw", Stage::RawGyro, "fn process(s, p) { [s.x*2.0, s.y*2.0, s.z*2.0] }"),
        ("aim", Stage::AfterBuiltin, "fn process(s, p) { [0.0, 0.0, 0.0] }"),
    ] {
        p.gyro.motion.custom_filters.push(FilterConfig {
            id: id.into(),
            name: id.into(),
            stage,
            source: source.into(),
            ..Default::default()
        });
    }
    let mut e = Engine::new(p.clone()).unwrap();
    let check = |e: &Engine, ms, expected: [f32; 3]| {
        let Ds4ImuState::Sample(sample) = e.frame.ds4_imu else {
            panic!("missing filtered sample");
        };
        assert_eq!(sample.timestamp_ns, time(ms).sensor_ns);
        for (actual, expected) in sample.gyro_dps.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
        }
        assert!((sample.accel_g[2] - 1.).abs() < 1e-5);
    };

    accel(&mut e, 10, [0., 0., 1.]);
    gyro(&mut e, 11, 2., 4., 6.);
    gyro(&mut e, 12, 2., 4., 6.);
    assert_eq!(e.frame.ds4_imu, Ds4ImuState::Neutral);
    assert_eq!(e.motion.custom_filters.statuses()[0].calls, 0);

    button(&mut e, 0, true);
    gyro(&mut e, 13, 2., 4., 6.);
    gyro(&mut e, 14, 2., 4., 6.);
    check(&e, 14, [4., 8., 12.]);
    gyro(&mut e, 24, 4., 8., 12.);
    check(&e, 24, [4., 8., 12.].map(|v| v * (2. - (-1.0_f32).exp())));
    let status = e.motion.custom_filters.statuses();
    assert_eq!(status[0].calls, 2);
    assert_eq!(status[1].calls, 0);

    button(&mut e, 0, false);
    button(&mut e, 0, true);
    assert_eq!(e.frame.ds4_imu, Ds4ImuState::Neutral);
    button(&mut e, 0, false);
    button(&mut e, 0, true);
    gyro(&mut e, 25, 5., 10., 15.);
    gyro(&mut e, 26, 5., 10., 15.);
    check(&e, 26, [10., 20., 30.]);

    p.gyro.ds4_imu_filter.noise_gate = 2.;
    e.sensor_calibration.gyro_noise_dps = 1.;
    e.apply_layer(p).unwrap();
    assert_eq!(e.frame.ds4_imu, Ds4ImuState::Neutral);
    gyro(&mut e, 27, 0.1, 0.2, 0.3);
    check(&e, 27, [0.; 3]);
}

#[test]
fn sdl_acceleration_uses_si_units_and_the_same_physical_frame_as_the_gyro() {
    let mut e = Engine::new(profile()).unwrap();
    assert!(e.motion.telemetry.orientation_quat.is_none());
    for ms in 1..100 {
        accel(&mut e, ms, [0., 0., 1.]);
        gyro(&mut e, ms, 0., 0., 0.);
    }
    let t = &e.motion.telemetry;
    assert!(t.accel_fresh);
    assert!((t.accel_g[2] - 1.).abs() < 1e-5);
    assert!(t.quantities[3..].iter().all(|v| v.abs() < 1e-4));
    for ms in 100..200 {
        gyro(&mut e, ms, 0., 90., 0.);
    }
    assert!((e.motion.telemetry.orientation[1] - 9.).abs() < 0.05);
    let pose = glam::Quat::from_array(e.motion.telemetry.orientation_quat.unwrap());
    assert!(pose.is_normalized());
    let pitch = (pose * glam::Vec3::X).z.asin().to_degrees();
    assert!((pitch - 9.).abs() < 0.05);
}

#[test]
fn player_and_world_project_a_turn_correctly_with_the_pad_tilted_45_degrees() {
    for space in [Space::Player, Space::World] {
        let mut p = profile();
        p.gyro.motion.space = space;
        let mut e = Engine::new(p).unwrap();
        let k = 0.5f32.sqrt();
        for ms in 1..15001 {
            accel(&mut e, ms, [k, 0., k]);
            gyro(&mut e, ms, 0., 0., 0.);
        }
        for ms in 15001..15012 {
            accel(&mut e, ms, [k, 0., k]);
            gyro(&mut e, ms, -100. * k, 0., 100. * k);
        }
        assert!(
            (e.frame.mouse_delta[0] - 1.).abs() < 0.01,
            "{space:?} {:?}",
            e.frame.mouse_delta
        );
        assert!(e.frame.mouse_delta[1].abs() < 1e-5);
    }
}

#[test]
fn missing_stale_and_out_of_order_acceleration_do_not_drive_gravity_dependent_aim() {
    let mut p = profile();
    p.gyro.motion.space = Space::Player;
    let mut e = Engine::new(p).unwrap();
    gyro(&mut e, 1, 0., 0., 90.);
    gyro(&mut e, 2, 0., 0., 90.);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    accel(&mut e, 3, [0., 0., 1.]);
    gyro(&mut e, 3, 0., 0., 90.);
    assert!(e.frame.mouse_delta[0] > 0.);
    accel(&mut e, 2, [0., 1., 0.]);
    gyro(&mut e, 4, 0., 0., 90.);
    assert!((e.frame.mouse_delta[0] - 0.9).abs() < 0.01);
    for ms in 5..60 {
        gyro(&mut e, ms, 0., 0., 90.);
    }
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    assert!(!e.motion.telemetry.accel_fresh);
}

#[test]
fn local_roll_axis_and_roll_contribution_can_drive_horizontal_aim() {
    for (space, share, roll) in [(Space::PitchRoll, 0., 90.), (Space::YawPlusRoll, 100., 90.)] {
        let mut p = profile();
        p.gyro.motion.space = space;
        p.gyro.motion.roll_contribution = share;
        let mut e = Engine::new(p).unwrap();
        gyro(&mut e, 1, roll, 0., 0.);
        gyro(&mut e, 2, roll, 0., 0.);
        assert!((e.frame.mouse_delta[0] - 0.9).abs() < 0.001);
    }
}

#[test]
fn steering_holds_deflection_while_stationary_and_resets_from_a_button() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.mode = MotionMode::Steering;
    p.gyro.motion.steering_range = 90.;
    p.gyro.motion.reset_ease_ms = 0.;
    p.gyro.motion.reset_input = Some("south".into());
    let mut e = Engine::new(p).unwrap();
    for ms in 1..1002 {
        gyro(&mut e, ms, 0., 0., 90.);
    }
    assert!(e.frame.pad.unwrap().right_x > 32700);
    for ms in 1002..1102 {
        gyro(&mut e, ms, 0., 0., 0.);
    }
    assert!(e.frame.pad.unwrap().right_x > 32700);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    button(&mut e, 0, true);
    gyro(&mut e, 1102, 0., 0., 0.);
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
}

#[test]
fn neutral_forward_resets_heading_without_resetting_the_clock_or_tilt() {
    let mut p = profile();
    p.gyro.motion.neutral_input = Some("south".into());
    let mut e = Engine::new(p).unwrap();
    for ms in 1..102 {
        gyro(&mut e, ms, 0., 0., 90.);
    }
    assert!((e.motion.telemetry.orientation[0] - 9.).abs() < 0.05);
    button(&mut e, 0, true);
    gyro(&mut e, 102, 0., 0., 0.);
    assert!(
        e.motion.telemetry.orientation[0].abs() < 0.05,
        "{:?}",
        e.motion.telemetry.orientation
    );
}

#[test]
fn legacy_edge_turns_never_add_displacement_to_native_angular_aim() {
    let mut p = profile();
    p.gyro.motion.edge.mode = EdgeMode::Ramp;
    p.gyro.motion.edge.zone = 5.;
    p.gyro.motion.edge.blend = 1.;
    p.gyro.motion.edge.ramp = 5.;
    p.gyro.motion.edge.max_dps = 100.;
    p.gyro.motion.edge.gyro_keep = 0.;
    p.gyro.motion.neutral_input = Some("south".into());
    let mut e = Engine::new(p).unwrap();
    for ms in 1..202 {
        gyro(&mut e, ms, 0., 0., 90.);
    }
    assert!(e.frame.mouse_delta[0] > 0.);
    gyro(&mut e, 202, 0., 0., 0.);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    button(&mut e, 0, true);
    gyro(&mut e, 203, 0., 0., 0.);
    assert!(e.frame.mouse_delta[0].abs() < 1e-4);
    e.advance(0.004, 300_000_000);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
}

#[test]
fn gyro_rotates_the_stick_preserving_its_length_and_centering_drops_the_offset() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.mode = MotionMode::StickRotation;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 3, -26214);
    for ms in 1..102 {
        gyro(&mut e, ms, 0., 0., 90.);
    }
    let pad = e.frame.pad.unwrap();
    let mag = (f32::from(pad.right_x).powi(2) + f32::from(pad.right_y).powi(2)).sqrt() / 32767.;
    assert!((mag - 0.8).abs() < 1e-3);
    assert!(pad.right_x > 4000 && pad.right_y > 25000);
    assert!((e.motion.telemetry.rotation_offset - 9.).abs() < 0.02);
    axis(&mut e, 3, 0);
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
    assert_eq!(e.frame.pad.unwrap().right_y, 0);
    axis(&mut e, 3, -26214);
    gyro(&mut e, 102, 0., 0., 0.);
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
}

#[test]
fn a_layer_switching_motion_modes_releases_the_previous_stick_output() {
    let mut pointer = profile();
    gamepad(&mut pointer);
    let mut rotation = pointer.clone();
    rotation.gyro.motion.mode = MotionMode::StickRotation;
    let mut e = Engine::new(pointer.clone()).unwrap();
    gyro(&mut e, 1, 0., 0., 90.);
    gyro(&mut e, 2, 0., 0., 90.);
    assert!(e.frame.pad.unwrap().right_x > 0);

    e.apply_layer(rotation).unwrap();
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
    axis(&mut e, 3, -26214);
    for ms in 3..104 {
        gyro(&mut e, ms, 0., 0., 90.);
    }
    assert!(e.frame.pad.unwrap().right_x > 4000);

    e.apply_layer(pointer).unwrap();
    let pad = e.frame.pad.unwrap();
    assert_eq!(pad.right_x, 0);
    assert!(pad.right_y > 25000);
    axis(&mut e, 3, 0);
    assert_eq!(e.frame.pad.unwrap().right_y, 0);
}

#[test]
fn lowering_a_layer_threshold_does_not_press_a_held_trigger_again() {
    let mut p = profile();
    p.gyro.activation = "left_trigger".into();
    p.gyro.activation_mode = GyroActivationMode::Toggle;
    p.gyro.activation_threshold = 0.7;
    p.gyro.motion.turns = vec![TurnBinding {
        input: "left_trigger".into(),
        duration_ms: 0.,
        ..Default::default()
    }];
    let mut e = Engine::new(p.clone()).unwrap();
    axis(&mut e, 4, 16384);
    p.gyro.activation_threshold = 0.2;
    e.apply_layer(p).unwrap();
    assert_eq!(e.advance(0.01, 10_000_000).mouse_delta, [0.; 2]);
    gyro(&mut e, 11, 0., 0., 90.);
    gyro(&mut e, 12, 0., 0., 90.);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);

    axis(&mut e, 4, 0);
    axis(&mut e, 4, 16384);
    assert!(e.advance(0.01, 20_000_000).mouse_delta[0] > 0.);
    gyro(&mut e, 21, 0., 0., 90.);
    gyro(&mut e, 22, 0., 0., 90.);
    assert!(e.frame.mouse_delta[0] > 0.);
}

#[test]
fn relative_rotation_returns_to_the_thumb_and_takeover_releases_in_the_outer_zone() {
    for relative in [true, false] {
        let mut p = profile();
        gamepad(&mut p);
        p.gyro.motion.mode = MotionMode::StickRotation;
        p.gyro.motion.rotation.relative = relative;
        p.gyro.motion.rotation.return_ms = 50.;
        p.gyro.motion.rotation.takeover = true;
        let mut e = Engine::new(p).unwrap();
        axis(&mut e, 3, -26214);
        for ms in 1..102 {
            gyro(&mut e, ms, 0., 0., 90.);
        }
        assert!(e.motion.telemetry.rotation_taken);
        for ms in 102..602 {
            gyro(&mut e, ms, 0., 0., 0.);
        }
        if relative {
            assert!(e.motion.telemetry.rotation_offset.abs() < 0.01);
        } else {
            assert!(e.motion.telemetry.rotation_offset > 8.);
        }
        axis(&mut e, 3, -32768);
        gyro(&mut e, 602, 0., 0., 0.);
        assert!(!e.motion.telemetry.rotation_taken);
    }
}

#[test]
fn tilt_actions_hold_release_on_silence_and_can_drive_an_analog_axis() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.bindings = vec![
        MotionBinding::default(),
        MotionBinding {
            output: "pad:left_stick_x".into(),
            ..Default::default()
        },
    ];
    let mut e = Engine::new(p).unwrap();
    for ms in 1..5 {
        accel(&mut e, ms, [0., -0.5, 0.75f32.sqrt()]);
        gyro(&mut e, ms, 0., 0., 0.);
    }
    assert!(e.frame.held.contains("key:Q"));
    assert!((e.frame.pad.unwrap().left_x as f32 / 32767. - 2. / 3.).abs() < 0.001);
    button(&mut e, 1, true);
    assert!(e.frame.held.contains("key:Q"));
    e.advance(0.004, 60_000_000);
    assert!(!e.frame.held.contains("key:Q"));
    assert_eq!(e.frame.pad.unwrap().left_x, 0);
}

#[test]
fn motion_toggle_and_turbo_use_sensor_edges_and_pause_releases_them() {
    let mut p = profile();
    p.gyro.pause_input = Some("south".into());
    p.gyro.motion.bindings = vec![
        MotionBinding {
            mode: BindingMode::Toggle,
            ..Default::default()
        },
        MotionBinding {
            output: "key:E".into(),
            mode: BindingMode::Turbo,
            turbo_ms: 20.,
            ..Default::default()
        },
    ];
    let mut e = Engine::new(p).unwrap();
    let mut pulses = 0;
    for ms in 1..80 {
        accel(&mut e, ms, [0., -0.5, 0.75f32.sqrt()]);
        gyro(&mut e, ms, 0., 0., 0.);
        if e.frame.held.contains("key:E") {
            pulses += 1;
        }
    }
    assert!(e.frame.held.contains("key:Q"));
    assert!(pulses > 20 && pulses < 60);
    button(&mut e, 0, true);
    assert!(e.frame.held.is_empty());
}

#[test]
fn flick_90_degrees_is_paid_out_once_and_suppresses_the_original_stick() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.gyro.motion.flick.enabled = true;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 32767);
    let mut total = 0.;
    for n in 1..101 {
        total += e.advance(0.01, n * 10_000_000).mouse_delta[0];
        assert_eq!(e.frame.pad.unwrap().right_x, 0);
    }
    assert!((total - 900.).abs() < 0.01, "{total}");
    assert_eq!(e.advance(0.01, 1_010_000_000).mouse_delta, [0.; 2]);
}

#[test]
fn a_complete_flick_between_output_slots_is_recognized_then_paid_out() {
    let mut p = profile();
    p.gyro.motion.flick.enabled = true;
    let mut e = Engine::new(p).unwrap();
    e.advance_input(1_000_000);
    let mut counts = 0.;
    for (ms, value) in [(2, 32767), (14, 0)] {
        e.process(&InputEvent::Axis {
            time: time(ms),
            index: 2,
            value,
        });
        e.advance_input(time(ms).host_ns);
        assert_eq!(e.frame.mouse_delta, [0.; 2]);
    }
    for ms in (16..=160).step_by(2) {
        e.advance_input(ms * 1_000_000);
        if ms % 16 == 0 {
            counts += e.advance_output(0.016, ms * 1_000_000).mouse_delta[0];
        }
    }
    assert!((counts - 900.).abs() < 0.01, "{counts}");
}

#[test]
fn flick_source_block_applies_to_normal_mappings_and_keeps_physical_controls() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.stick_mouse.enabled = true;
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.flick.enable_input = Some("right_stick_right".into());
    let direction = Action {
        name: "Right".into(),
        input: "right_stick_right".into(),
        output: "key:D".into(),
        threshold: 0.5,
        repeat_ms: None,
        trigger: Default::default(),
        trigger_ms: 250,
        sustain: true,
        turbo_ms: None,
        allow_simultaneous: false,
        chord_wait_ms: 80,
        toggle: false,
    };
    p.actions = vec![
        Action {
            name: "Outer ring".into(),
            input: "right_stick_outer".into(),
            output: "key:O".into(),
            ..direction.clone()
        },
        direction,
    ];
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 32767);
    assert_eq!(e.input_value("right_stick_right"), 1.);
    assert!(!e.frame.held.contains("key:D"));
    assert!(e.frame.held.contains("key:O"));
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
    let total: f64 = (1..101)
        .map(|n| e.advance(0.01, n * 10_000_000).mouse_delta[0])
        .sum();
    assert!((total - 900.).abs() < 0.01, "{total}");
}

#[test]
fn aim_without_flick_keeps_native_stick_in_deadzone_mode_and_survives_a_layer_change() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.gyro.motion.flick.stick_aim = true;
    p.gyro.motion.flick.suppress = Suppress::Deadzone;
    let mut e = Engine::new(p.clone()).unwrap();
    axis(&mut e, 2, 32767);
    let frame = e.advance(0.01, 10_000_000);
    assert_eq!(frame.pad.unwrap().right_x, 32767);
    assert!(frame.mouse_delta[0] > 0.);

    p.gyro.motion.flick.aim_sensitivity = 0.5;
    e.apply_layer(p).unwrap();
    let frame = e.advance(0.01, 20_000_000);
    assert_eq!(frame.pad.unwrap().right_x, 32767);
    assert!(frame.mouse_delta[0] > 0.);
}

#[test]
fn native_quadratic_flick_payout_uses_elapsed_time_for_both_directions() {
    for strength in [0., 0.5, 1.] {
        for (value, direction) in [(32767, 1.), (-32768, -1.)] {
            let mut p = profile();
            p.gyro.motion.flick.enabled = true;
            p.gyro.motion.flick.stick_aim = true;
            p.gyro.motion.flick.release_ease = strength;
            let mut engine = Engine::new(p).unwrap();
            axis(&mut engine, 2, value);
            assert_eq!(engine.advance_output(0.001, time(1).host_ns).mouse_delta, [0.; 2]);
            let mut elapsed = 0.0_f32;
            let mut total = 0.0;
            for dt in [0.002, 0.005, 0.013, 0.020, 0.011, 0.019, 0.031] {
                elapsed += dt;
                let delta = engine.advance(dt, (elapsed * 1e9) as u64).mouse_delta[0];
                assert!(delta * direction >= -0.0001, "reverse step {delta}");
                total += delta;
                let progress = (elapsed / 0.1).min(1.);
                let expected = f64::from(1. - (1. - progress).powi(2)) * 900. * direction;
                assert!(
                    (total - expected).abs() < 0.002,
                    "{strength}: {total} vs {expected}"
                );
            }
            assert!((total - 900. * direction).abs() < 0.002);
            assert_eq!(engine.advance(0.01, 120_000_000).mouse_delta, [0.; 2]);
        }
    }
}

#[test]
fn flick_snaps_only_the_initial_angle_and_keeps_sweeps_continuous() {
    for (snap_mode, strength, expected) in [
        (FlickSnap::None, 1., 60.),
        (FlickSnap::Four, 1., 90.),
        (FlickSnap::Eight, 1., 45.),
        (FlickSnap::Four, 0.5, 75.),
        (FlickSnap::Four, 0., 60.),
    ] {
        for direction in [-1., 1.] {
            let mut p = profile();
            let f = &mut p.gyro.motion.flick;
            f.enabled = true;
            f.snap_mode = snap_mode;
            f.snap_strength = strength;
            let mut e = Engine::new(p).unwrap();
            // Set the sub-threshold Y component first, then cross the rim at 60°.
            for (index, value) in [(3, -16384), (2, (28377. * direction) as i16)] {
                e.process(&InputEvent::Axis {
                    time: time(1),
                    index,
                    value,
                });
            }
            let mut initial = 0.;
            for ms in (10..=150).step_by(10) {
                initial += e.advance(0.01, ms * 1_000_000).mouse_delta[0];
            }
            assert!(
                (initial - expected * 10. * direction).abs() < 0.1,
                "{snap_mode:?}, {strength}: {initial}"
            );
            // From 60° to 80° on the rim: 20°, even with four/eight-way snapping.
            for (index, value) in [(3, -5690), (2, (32269. * direction) as i16)] {
                e.process(&InputEvent::Axis {
                    time: time(151),
                    index,
                    value,
                });
            }
            let mut sweep = 0.;
            for ms in (160..=350).step_by(10) {
                sweep += e.advance(0.01, ms * 1_000_000).mouse_delta[0];
            }
            assert!((sweep - 200. * direction).abs() < 0.1, "sweep {sweep}");
        }
    }
}

#[test]
fn flick_only_finishes_the_initial_turn_but_never_emits_sweep_or_stabiliser_tail() {
    for stabilise_ms in [0., 50.] {
        let mut p = profile();
        let f = &mut p.gyro.motion.flick;
        f.enabled = true;
        f.only = true;
        f.stabilise_ms = stabilise_ms;
        let mut e = Engine::new(p).unwrap();
        axis(&mut e, 2, 32767);
        let mut total = 0.;
        for ms in (10..=150).step_by(10) {
            if ms == 20 {
                // Sweep while the initial 90° is still being paid out.
                e.process(&InputEvent::Axis {
                    time: time(ms),
                    index: 3,
                    value: 32767,
                });
                e.process(&InputEvent::Axis {
                    time: time(ms),
                    index: 2,
                    value: 0,
                });
            }
            if ms == 40 {
                // Releasing the stick must not truncate that initial turn.
                e.process(&InputEvent::Axis {
                    time: time(ms),
                    index: 3,
                    value: 0,
                });
            }
            total += e.advance(0.01, ms * 1_000_000).mouse_delta[0];
        }
        assert!((total - 900.).abs() < 0.01, "{stabilise_ms}: {total}");
        assert_eq!(e.advance(0.01, 160_000_000).mouse_delta, [0.; 2]);
        e.process(&InputEvent::Axis {
            time: time(161),
            index: 2,
            value: -32768,
        });
        let mut next = 0.;
        for ms in (170..=310).step_by(10) {
            next += e.advance(0.01, ms * 1_000_000).mouse_delta[0];
        }
        assert!((next + 900.).abs() < 0.01, "rearmed turn {next}");
    }
}

#[test]
fn flick_snap_settings_roundtrip_and_layer_changes_require_recenter() {
    let mut p = profile();
    p.gyro.motion.flick.enabled = true;
    p.layers.push(
        serde_json::from_value(serde_json::json!({
            "id":"snap", "name":"Snap", "input":"left_shoulder",
            "settings":{"gyro":{"motion":{"flick":{
                "only":true, "snap_mode":"eight", "snap_strength":0.75
            }}}}
        }))
        .unwrap(),
    );
    let restored = parse_profile(&toml::to_string_pretty(&p).unwrap()).unwrap();
    assert!(!restored.gyro.motion.flick.only);
    assert_eq!(restored.gyro.motion.flick.snap_mode, FlickSnap::None);
    let mut layer = motion_core::layers::resolve(&restored, &[0]).unwrap();
    assert!(layer.gyro.motion.flick.only);
    assert_eq!(layer.gyro.motion.flick.snap_mode, FlickSnap::Eight);
    assert_eq!(layer.gyro.motion.flick.snap_strength, 0.75);
    let mut e = Engine::new(restored).unwrap();
    axis(&mut e, 2, 32767);
    e.advance_output(0.001, time(1).host_ns);
    assert!(e.advance(0.01, 10_000_000).mouse_delta[0] > 0.);
    e.apply_layer(layer.clone()).unwrap();
    assert_eq!(e.advance(0.01, 20_000_000).mouse_delta, [0.; 2]);
    for strength in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
        layer.gyro.motion.flick.snap_strength = strength;
        assert!(layer.validate().is_err());
    }
}

#[test]
fn jsm_default_response_preserves_explicit_calibration_and_filter_values_on_load() {
    let mut p = profile();
    p.gyro = Default::default();
    let native = flexinput_engine::eval::JsmAimSettings::default();
    assert_eq!(
        p.gyro.counts_per_degree,
        native.real_world_calibration / native.in_game_sens
    );
    assert_eq!(p.gyro.smoothing_ms, 0.);
    assert_eq!(p.gyro.acceleration.fast_multiplier, 1.);
    assert_eq!(p.gyro.acceleration.curve, Curve::Linear);
    assert_eq!(p.gyro.motion.angle_snap, 0.);
    p.gyro.counts_per_degree = 12.;
    p.gyro.smoothing_ms = 16.;
    p.gyro.sensitivity = 2.5;
    p.gyro.acceleration.power_reference = 60.;
    p.gyro.motion.angle_snap_ease = true;
    let restored = parse_profile(&toml::to_string_pretty(&p).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
}

#[test]
fn flick_to_virtual_stick_preserves_the_angle_even_when_it_needs_more_time() {
    for pause_at in [None, Some(0), Some(2)] {
        let mut p = profile();
        gamepad(&mut p);
        p.gyro.activation = "left_trigger".into();
        p.gyro.pause_input = Some("south".into());
        p.gyro.stick_full_scale_dps = 90.;
        p.gyro.motion.flick.enabled = true;
        p.gyro.motion.flick.smooth_ms = 10.;
        p.gyro.motion.flick.release_ease = 1.;
        let mut e = Engine::new(p).unwrap();
        if pause_at == Some(0) {
            button(&mut e, 0, true);
        }
        axis(&mut e, 2, 32767);
        let mut degrees = 0.;
        for n in 1..201 {
            if pause_at == Some(n) {
                button(&mut e, 0, true);
            }
            let f = e.advance(0.01, n * 10_000_000);
            degrees += f.pad.unwrap().right_x as f32 / 32767. * 90. * 0.01;
            assert_eq!(f.mouse_delta, [0.; 2]);
        }
        assert!((degrees - 90.).abs() < 0.1, "pause={pause_at:?}: {degrees}");
    }
}

#[test]
fn a_layer_changing_trackball_settings_preserves_a_flick_in_progress() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.stick_full_scale_dps = 90.;
    p.gyro.motion.flick.enabled = true;
    let mut e = Engine::new(p.clone()).unwrap();
    axis(&mut e, 2, 32767);
    let mut degrees = 0.;
    for n in 1..201 {
        if n == 2 {
            p.gyro.motion.trackball.decay = 2.;
            e.apply_layer(p.clone()).unwrap();
        }
        let pad = e.advance(0.01, n * 10_000_000).pad.unwrap();
        degrees += pad.right_x as f32 / 32767. * 90. * 0.01;
    }
    assert!((degrees - 90.).abs() < 0.1, "{degrees}");
}

#[test]
fn disabling_flick_cancels_the_remaining_stick_angle() {
    for via_layer in [false, true] {
        let mut p = profile();
        gamepad(&mut p);
        p.gyro.stick_full_scale_dps = 90.;
        p.gyro.motion.flick.enabled = true;
        p.gyro.motion.flick.enable_input = Some("east".into());
        let mut e = Engine::new(p.clone()).unwrap();
        button(&mut e, 1, true);
        axis(&mut e, 2, 32767);
        assert!(e.advance(0.01, 10_000_000).pad.unwrap().right_x > 0);
        axis(&mut e, 2, 0);
        if via_layer {
            let mut disabled = p.clone();
            disabled.gyro.enabled = false;
            e.apply_layer(disabled).unwrap();
        } else {
            button(&mut e, 1, false);
        }
        assert_eq!(e.frame.pad.unwrap().right_x, 0);
        assert_eq!(e.advance(0.01, 20_000_000).pad.unwrap().right_x, 0);
        if via_layer {
            e.apply_layer(p).unwrap();
        } else {
            button(&mut e, 1, true);
        }
        assert_eq!(e.advance(0.01, 30_000_000).pad.unwrap().right_x, 0);
    }
}

#[test]
fn stick_aim_continues_while_gyro_is_paused_and_restores_native_input_when_disabled() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.output = GyroOutput::Mouse;
    p.gyro.pause_input = Some("south".into());
    let mut native = Engine::new(p.clone()).unwrap();
    axis(&mut native, 2, 16384);
    let native_x = native.frame.pad.unwrap().right_x;
    p.gyro.motion.flick.stick_aim = true;
    p.gyro.motion.flick.enable_input = Some("east".into());
    p.stick_deadzone = 0.;
    p.gyro.motion.flick.deadzone = 1.; // Isolate rate conversion from inner/outer deadzones.
    let mut e = Engine::new(p).unwrap();
    button(&mut e, 1, true);
    axis(&mut e, 2, 16384);
    let f = e.advance(0.01, 10_000_000);
    assert!((f.mouse_delta[0] - 18.).abs() < 0.01);
    assert_eq!(f.pad.unwrap().right_x, 0);
    assert!((e.advance(0.01, 20_000_000).mouse_delta[0] - 18.).abs() < 0.01);
    button(&mut e, 0, true);
    let paused = e.advance(0.01, 30_000_000);
    assert!((paused.mouse_delta[0] - 18.).abs() < 0.01);
    assert_eq!(paused.pad.unwrap().right_x, 0);
    button(&mut e, 1, false);
    assert_eq!(e.frame.pad.unwrap().right_x, native_x);
    assert_eq!(e.advance(0.01, 40_000_000).mouse_delta, [0.; 2]);
}

#[test]
fn bound_stick_turns_survive_gyro_route_changes_and_wait_for_available_capacity() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.turns = vec![TurnBinding {
        input: "south".into(),
        duration_ms: 0.,
        output: AimOutput::RightStick,
        ..Default::default()
    }];
    let mut e = Engine::new(p.clone()).unwrap();
    e.process_snapshot(&InputEvent::Button {
        time: time(1),
        index: 0,
        down: true,
    });
    assert_eq!(e.advance(0.01, 10_000_000).pad.unwrap().right_x, 0);
    button(&mut e, 0, false);
    axis(&mut e, 2, 32767);
    button(&mut e, 0, true);
    for n in 2..10 {
        e.advance(0.01, n * 10_000_000);
    }
    p.gyro.output = GyroOutput::Mouse;
    e.apply_layer(p).unwrap();
    axis(&mut e, 2, 0);
    let mut total = 0.;
    for n in 10..110 {
        total += e.advance(0.01, n * 10_000_000).pad.unwrap().right_x as f32 / 32767. * 360. * 0.01;
    }
    assert!((total - 180.).abs() < 0.05, "{total}");
}

#[test]
fn an_opposing_stick_turn_uses_the_same_clamped_axis_as_the_capacity_check() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.turns = vec![TurnBinding {
        input: "south".into(),
        degrees: -90.,
        duration_ms: 0.,
        output: AimOutput::RightStick,
        ..Default::default()
    }];
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 32767);
    button(&mut e, 0, true);
    gyro(&mut e, 1, 0., 0., 180.);
    let mut degrees = 0.;
    for ms in (10..=400).step_by(10) {
        gyro(&mut e, ms, 0., 0., 180.);
        let x = e.advance(0.01, ms * 1_000_000).pad.unwrap().right_x;
        degrees += (x as f32 / 32767. - 1.) * 360. * 0.01;
    }
    assert!((degrees + 90.).abs() < 0.05, "{degrees}");
}

#[test]
fn trackball_coasts_while_held_and_stops_when_released_or_disconnected() {
    let mut p = profile();
    p.gyro.motion.trackball.horizontal_input = Some("south".into());
    let mut e = Engine::new(p).unwrap();
    for ms in 1..130 {
        gyro(&mut e, ms, 0., 0., 100.);
    }
    button(&mut e, 0, true);
    gyro(&mut e, 130, 0., 0., 0.);
    let first = e.frame.mouse_delta[0];
    assert!(first > 0.9);
    for ms in 131..230 {
        gyro(&mut e, ms, 0., 0., 0.);
    }
    assert!(e.frame.mouse_delta[0] > 0. && e.frame.mouse_delta[0] < first);
    button(&mut e, 0, false);
    gyro(&mut e, 230, 0., 0., 0.);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    e.process(&InputEvent::Disconnected { time: time(231) });
    assert!(e.frame.held.is_empty());
}

#[test]
fn all_curves_and_filters_are_finite_and_custom_points_change_the_response() {
    for curve in [
        Curve::Linear,
        Curve::Natural,
        Curve::Power,
        Curve::Quadratic,
        Curve::Sigmoid,
        Curve::Jump,
        Curve::Custom,
    ] {
        for smoothing in [Smoothing::Ema, Smoothing::Decay, Smoothing::OneEuro] {
            let mut p = profile();
            p.gyro.acceleration.curve = curve;
            p.gyro.acceleration.fast_multiplier = 3.;
            p.gyro.acceleration.points = vec![[0., 1.], [50., 3.]];
            p.gyro.motion.smoothing = smoothing;
            p.gyro.smoothing_ms = 20.;
            let mut e = Engine::new(p).unwrap();
            for ms in 1..500 {
                gyro(&mut e, ms, 0., 0., if ms < 100 { 0. } else { 100. });
                assert!(e.frame.mouse_delta.iter().all(|v| v.is_finite()));
            }
            assert!(
                e.frame.mouse_delta[0] >= 1. && e.frame.mouse_delta[0] <= 3.001,
                "{curve:?} {smoothing:?} {:?}",
                e.frame.mouse_delta
            );
        }
    }
}

#[test]
fn one_euro_reduces_micro_jitter_and_angle_snap_preserves_horizontal_motion() {
    let mut p = profile();
    p.gyro.motion.smoothing = Smoothing::OneEuro;
    p.gyro.motion.one_euro_beta = 0.;
    p.gyro.motion.one_euro_cutoff = 1.;
    let mut e = Engine::new(p).unwrap();
    for ms in 1..501 {
        gyro(&mut e, ms, 0., 0., 0.);
    }
    let mut movement = 0.;
    for ms in 501..1000 {
        gyro(&mut e, ms, 0., 0., if ms % 2 == 0 { 1. } else { -1. });
        if ms > 900 {
            movement += e.frame.mouse_delta[0].abs();
        }
    }
    assert!(movement < 0.02, "{movement}");
    let mut p = profile();
    p.gyro.motion.angle_snap = 10.;
    p.gyro.motion.angle_snap_ease = false;
    let mut e = Engine::new(p).unwrap();
    gyro(&mut e, 1, 0., 2., 100.);
    gyro(&mut e, 2, 0., 2., 100.);
    assert!(e.frame.mouse_delta[1].abs() < 1e-6);
    assert!(e.frame.mouse_delta[0] > 1.);
}

#[test]
fn measurement_uses_base_scale_and_rejects_gaps_and_stick_saturation() {
    let mut p = profile();
    p.gyro.sensitivity = 4.;
    p.gyro.acceleration.fast_multiplier = 3.;
    let mut e = Engine::new(p).unwrap();
    e.begin_measurement("yaw").unwrap();
    let mut total = 0.;
    for ms in 1..1002 {
        if ms == 501 {
            // Local angle measurement only depends on the gyro stream.
            accel(&mut e, ms, [f32::NAN, 0., 0.]);
        }
        gyro(&mut e, ms, 0., 0., 100.);
        total += e.frame.mouse_delta[0];
    }
    assert!((total - 1000.).abs() < 0.1);
    assert!((e.motion.telemetry.measured_degrees - 100.).abs() < 0.01);
    assert!(e.motion.telemetry.measurement_error.is_none());
    gyro(&mut e, 1002, 0., 0., 100.);
    gyro(&mut e, 1002, 0., 0., 100.);
    assert!(e.motion.telemetry.measurement_error.is_some());
    e.end_measurement();
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.stick_full_scale_dps = 60.;
    let mut e = Engine::new(p).unwrap();
    e.begin_measurement("yaw").unwrap();
    for ms in 1..10 {
        gyro(&mut e, ms, 0., 0., 120.);
    }
    assert!((e.motion.telemetry.measurement_peak - 2.).abs() < 0.01);
}

#[test]
fn profiles_round_trip_advanced_settings_and_reject_invalid_or_conflicting_values() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.space = Space::Player;
    p.gyro.motion.edge.mode = EdgeMode::Both;
    p.gyro.motion.bindings = vec![MotionBinding::default()];
    p.gyro.motion.turns = vec![TurnBinding::default()];
    p.gyro.acceleration.curve = Curve::Custom;
    p.gyro.motion.flick.release_ease = 0.65;
    let text = toml::to_string_pretty(&p).unwrap();
    let restored: Profile = toml::from_str(&text).unwrap();
    restored.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
    p.gyro.acceleration.points = vec![[1., 1.], [0., 2.]];
    assert!(p.validate().is_err());
    p.gyro.acceleration.points = vec![[0., 1.], [10., 2.]];
    p.gyro.motion.mode = MotionMode::StickRotation;
    assert!(p.validate().is_err());
    p.gyro.motion.edge.mode = EdgeMode::Off;
    p.gyro.motion.flick.enabled = true;
    assert!(p.validate().is_err());
}

#[test]
fn flick_release_curve_defaults_layers_and_validation() {
    let mut p = profile();
    assert_eq!(p.gyro.motion.flick.release_ease, 0.5);
    p.layers.push(
        serde_json::from_value(serde_json::json!({
            "id": "aim", "name": "Aim", "input": "south",
            "settings": {"gyro": {"motion": {"flick": {"release_ease": 1.}}}}
        }))
        .unwrap(),
    );
    p.validate().unwrap();
    let resolved = motion_core::layers::resolve(&p, &[0]).unwrap();
    assert_eq!(resolved.gyro.motion.flick.release_ease, 1.);
    for strength in [
        -0.1,
        flexinput_engine::eval::MAX_FLICK_RELEASE_EASE + 0.1,
        f32::NAN,
        f32::INFINITY,
    ] {
        p.gyro.motion.flick.release_ease = strength;
        assert!(p.validate().is_err());
    }
}

#[test]
fn device_mounting_is_a_rotation_and_not_an_unchecked_scale() {
    let mut c = SensorCalibration::default();
    c.validate().unwrap();
    c.mounting[0] = 2.;
    assert!(c.validate().is_err());
    c.mounting = [-1., 0., 0., 0., 1., 0., 0., 0., 1.];
    assert!(c.validate().is_err());
    c.mounting = [0., -1., 0., 1., 0., 0., 0., 0., 1.];
    c.validate().unwrap();
}

#[test]
fn aim_activation_preserves_motion_toggle_and_pause_releases_it_even_with_aim_off() {
    let mut p = profile();
    p.gyro.activation = "left_trigger".into();
    p.gyro.pause_input = Some("south".into());
    p.gyro.motion.bindings = vec![MotionBinding {
        mode: BindingMode::Toggle,
        ..Default::default()
    }];
    let mut e = Engine::new(p).unwrap();
    for ms in 1..10 {
        accel(&mut e, ms, [0., -0.5, 0.75f32.sqrt()]);
        gyro(&mut e, ms, 0., 0., 0.);
    }
    assert!(e.frame.held.contains("key:Q"));
    axis(&mut e, 4, 32767);
    assert!(e.frame.held.contains("key:Q"));
    axis(&mut e, 4, 0);
    assert!(e.frame.held.contains("key:Q"));
    button(&mut e, 0, true);
    assert!(e.frame.held.is_empty());
}

#[test]
fn native_virtual_flick_saturates_the_combined_rate_and_finishes_on_schedule() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.flick.enabled = true;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 32767);
    for ms in 1..1001 {
        gyro(&mut e, ms, 0., 0., 180.);
        if ms % 10 == 0 {
            let x = e.advance(0.01, ms * 1_000_000).pad.unwrap().right_x;
            if ms <= 250 { assert_eq!(x, i16::MAX); }
            if ms >= 280 { assert!((x as f32 / 32767. - 0.5).abs() < 1e-4); }
        }
    }
}

#[test]
fn pausing_a_measurement_stops_output_and_marks_the_result_invalid() {
    let mut p = profile();
    p.gyro.pause_input = Some("south".into());
    let mut e = Engine::new(p).unwrap();
    e.begin_measurement("yaw").unwrap();
    assert!(e.begin_measurement("yaw").is_err());
    for ms in 1..10 {
        gyro(&mut e, ms, 0., 0., 100.);
    }
    button(&mut e, 0, true);
    gyro(&mut e, 10, 0., 0., 100.);
    gyro(&mut e, 11, 0., 0., 100.);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    assert!(e.motion.telemetry.measurement_error.is_some());
}

#[test]
fn a_held_flick_snapshot_waits_for_center_before_starting_a_turn() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.output = GyroOutput::Mouse;
    p.gyro.motion.flick.enabled = true;
    let mut e = Engine::new(p).unwrap();
    e.process_snapshot(&InputEvent::Axis {
        time: time(1),
        index: 2,
        value: 32767,
    });
    for n in 1..20 {
        assert_eq!(e.advance(0.01, n * 10_000_000).mouse_delta, [0.; 2]);
        assert_eq!(e.frame.pad.unwrap().right_x, 0);
    }
    axis(&mut e, 2, 0);
    e.advance(0.01, 200_000_000);
    axis(&mut e, 2, 32767);
    e.advance(0.01, 210_000_000);
    assert!(e.advance(0.01, 220_000_000).mouse_delta[0] > 0.);
}

#[test]
fn flick_pause_defaults_roundtrips_and_is_exclusive_with_hold_enable_in_layers() {
    let mut p = profile();
    assert!(p.gyro.motion.flick.pause_input.is_none());
    p.gyro.motion.flick.pause_input = Some("east".into());
    let loaded = parse_profile(&toml::to_string_pretty(&p).unwrap()).unwrap();
    assert_eq!(
        loaded.gyro.motion.flick.pause_input.as_deref(),
        Some("east")
    );
    p.layers.push(
        serde_json::from_value(serde_json::json!({
            "id":"hold", "name":"Hold", "input":"left_shoulder",
            "settings":{"gyro":{"motion":{"flick":{
                "pause_input":{"clear":true}, "enable_input":"south"
            }}}}
        }))
        .unwrap(),
    );
    p.validate().unwrap();
    let hold = motion_core::layers::resolve(&p, &[0]).unwrap();
    assert!(hold.gyro.motion.flick.pause_input.is_none());
    assert_eq!(
        hold.gyro.motion.flick.enable_input.as_deref(),
        Some("south")
    );
    assert_eq!(
        motion_core::layers::resolve(&p, &[])
            .unwrap()
            .gyro
            .motion
            .flick
            .pause_input
            .as_deref(),
        Some("east")
    );
    p.layers[0].settings = serde_json::from_value(serde_json::json!({
        "gyro":{"motion":{"flick":{"enable_input":"south"}}}
    }))
    .unwrap();
    assert!(
        p.validate().is_err(),
        "a layer cannot inherit pause while adding enable"
    );
    p.layers.clear();
    for enable in ["east", "south"] {
        p.gyro.motion.flick.enable_input = Some(enable.into());
        assert!(
            p.validate().is_err(),
            "both controls are invalid even on different buttons"
        );
    }
    p.gyro.motion.flick.enable_input = None;
    p.gyro.motion.flick.pause_input = Some("missing_input".into());
    assert!(p.validate().is_err());
}

#[test]
fn flick_pause_cancels_remaining_mouse_and_stick_turns_without_replaying_on_resume() {
    for output in [GyroOutput::Mouse, GyroOutput::RightStick] {
        let mut p = profile();
        gamepad(&mut p);
        p.gyro.output = output;
        p.gyro.stick_full_scale_dps = 90.;
        p.gyro.motion.flick.enabled = true;
        p.gyro.motion.flick.pause_input = Some("east".into());
        let mut e = Engine::new(p).unwrap();
        axis(&mut e, 2, 32767);
        e.advance_output(0.001, time(1).host_ns);
        let first = e.advance(0.01, 10_000_000);
        assert!(first.mouse_delta[0] > 0. || first.pad.unwrap().right_x > 0);
        axis(&mut e, 2, 0);
        button(&mut e, 1, true);
        assert_eq!(e.frame.pad.unwrap().right_x, 0);
        for n in 2..12 {
            let paused = e.advance(0.01, n * 10_000_000);
            assert_eq!(paused.mouse_delta, [0.; 2]);
            assert_eq!(paused.pad.unwrap().right_x, 0);
        }
        button(&mut e, 1, false);
        let resumed = e.advance(0.01, 120_000_000);
        assert_eq!(resumed.mouse_delta, [0.; 2]);
        assert_eq!(resumed.pad.unwrap().right_x, 0);
        axis(&mut e, 2, 32767);
        e.advance(0.01, 130_000_000);
        let fresh = e.advance(0.01, 140_000_000);
        assert!(fresh.mouse_delta[0] > 0. || fresh.pad.unwrap().right_x > 0);
    }
}

#[test]
fn releasing_flick_pause_with_the_stick_at_the_rim_waits_for_recenter() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.output = GyroOutput::Mouse;
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.flick.pause_input = Some("east".into());
    let mut e = Engine::new(p).unwrap();
    button(&mut e, 1, true);
    axis(&mut e, 2, 32767);
    let paused = e.advance(0.01, 10_000_000);
    assert_eq!(paused.mouse_delta, [0.; 2]);
    assert_eq!(
        paused.pad.unwrap().right_x,
        32767,
        "pause restores the original stick"
    );
    button(&mut e, 1, false);
    for n in 2..12 {
        let resumed = e.advance(0.01, n * 10_000_000);
        assert_eq!(resumed.mouse_delta, [0.; 2]);
        assert_eq!(resumed.pad.unwrap().right_x, 0);
    }
    axis(&mut e, 2, 0);
    e.advance(0.01, 120_000_000);
    axis(&mut e, 2, 32767);
    e.advance(0.01, 130_000_000);
    assert!(e.advance(0.01, 140_000_000).mouse_delta[0] > 0.);
}

#[test]
fn flick_pause_stops_companion_aim_but_keeps_gyro_and_resumes_on_release() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.output = GyroOutput::Mouse;
    p.gyro.motion.flick.stick_aim = true;
    p.gyro.motion.flick.pause_input = Some("east".into());
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 16384);
    assert!(e.advance(0.01, 10_000_000).mouse_delta[0] > 0.);
    button(&mut e, 1, true);
    let paused = e.advance(0.01, 20_000_000);
    assert_eq!(paused.mouse_delta, [0.; 2]);
    assert!(paused.pad.unwrap().right_x > 16000);
    gyro(&mut e, 30, 0., 0., 90.);
    gyro(&mut e, 40, 0., 0., 90.);
    assert!(
        e.frame.mouse_delta[0] > 0.,
        "Flick pause must not pause gyro"
    );
    button(&mut e, 1, false);
    let resumed = e.advance(0.01, 50_000_000);
    assert!(resumed.mouse_delta[0] > 0.);
    assert_eq!(resumed.pad.unwrap().right_x, 0);
}

#[test]
fn flick_pause_applies_when_standalone_and_when_a_pause_binding_layer_is_entered() {
    let mut p = profile();
    p.gyro.enabled = false;
    p.gyro.motion.flick.standalone = true;
    p.gyro.motion.flick.stick_aim = true;
    let mut e = Engine::new(p.clone()).unwrap();
    button(&mut e, 1, true);
    axis(&mut e, 2, 16384);
    assert!(e.advance(0.01, 10_000_000).mouse_delta[0] > 0.);
    p.gyro.motion.flick.pause_input = Some("east".into());
    e.apply_layer(p).unwrap();
    assert_eq!(e.advance(0.01, 20_000_000).mouse_delta, [0.; 2]);
    button(&mut e, 1, false);
    assert!(e.advance(0.01, 30_000_000).mouse_delta[0] > 0.);
}
