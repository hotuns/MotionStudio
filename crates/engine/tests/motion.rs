use motion_core::{
    engine::{Engine, InputEvent},
    motion_config::*,
    profile::{GamepadTarget, GyroOutput, Profile, TimestampSource},
    storage::parse_profile,
    timing::SampleTime,
};

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
    for (space, share, roll) in [(Space::PitchRoll, 0., 90.), (Space::PitchYaw, 100., -90.)] {
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
fn edge_ramp_keeps_turning_outside_the_zone_and_neutral_stops_it() {
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
    gyro(&mut e, 202, 0., 0., 0.);
    assert!((e.frame.mouse_delta[0] - 1.).abs() < 0.05);
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
fn flick_release_curve_follows_time_progress_with_irregular_ticks_and_both_directions() {
    for strength in [0., 0.5, 1.] {
        for (value, direction) in [(32767, 1.), (-32768, -1.)] {
            let mut p = profile();
            p.gyro.motion.flick.enabled = true;
            p.gyro.motion.flick.stick_aim = true;
            p.gyro.motion.flick.release_ease = strength;
            let mut engine = Engine::new(p).unwrap();
            axis(&mut engine, 2, value);
            let mut elapsed = 0.0_f32;
            let mut total = 0.0;
            for dt in [0.002, 0.005, 0.013, 0.020, 0.011, 0.019, 0.031] {
                elapsed += dt;
                let delta = engine.advance(dt, (elapsed * 1e9) as u64).mouse_delta[0];
                assert!(delta * direction >= -0.0001, "reverse step {delta}");
                total += delta;
                let progress = (elapsed / 0.1).min(1.);
                let expected =
                    f64::from(progress + strength * progress * (1. - progress)) * 900. * direction;
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
fn flick_to_virtual_stick_preserves_the_angle_even_when_it_needs_more_time() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.stick_full_scale_dps = 90.;
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.flick.output = AimOutput::RightStick;
    p.gyro.motion.flick.smooth_ms = 10.;
    p.gyro.motion.flick.release_ease = 1.;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 32767);
    let mut degrees = 0.;
    for n in 1..201 {
        let f = e.advance(0.01, n * 10_000_000);
        degrees += f.pad.unwrap().right_x as f32 / 32767. * 90. * 0.01;
        assert_eq!(f.mouse_delta, [0.; 2]);
    }
    assert!((degrees - 90.).abs() < 0.1, "{degrees}");
}

#[test]
fn stick_aim_runs_between_axis_events_and_pause_restores_native_input() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.pause_input = Some("south".into());
    let mut native = Engine::new(p.clone()).unwrap();
    axis(&mut native, 2, 16384);
    let native_x = native.frame.pad.unwrap().right_x;
    p.gyro.motion.flick.stick_aim = true;
    p.stick_deadzone = 0.; // Isolate RWS timing from source deadzone.
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 16384);
    let f = e.advance(0.01, 10_000_000);
    assert!((f.mouse_delta[0] - 18.).abs() < 0.01);
    assert_eq!(f.pad.unwrap().right_x, 0);
    assert!((e.advance(0.01, 20_000_000).mouse_delta[0] - 18.).abs() < 0.01);
    button(&mut e, 0, true);
    assert_eq!(e.frame.pad.unwrap().right_x, native_x);
    assert_eq!(e.advance(0.01, 30_000_000).mouse_delta, [0.; 2]);
}

#[test]
fn bound_turns_wait_for_available_stick_capacity_and_snapshot_is_not_a_press() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.turns = vec![TurnBinding {
        input: "south".into(),
        duration_ms: 0.,
        output: AimOutput::RightStick,
        ..Default::default()
    }];
    let mut e = Engine::new(p).unwrap();
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
    axis(&mut e, 2, 0);
    let mut total = 0.;
    for n in 10..110 {
        total += e.advance(0.01, n * 10_000_000).pad.unwrap().right_x as f32 / 32767. * 360. * 0.01;
    }
    assert!((total - 180.).abs() < 0.05, "{total}");
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
    assert_eq!(p.gyro.motion.flick.release_ease, 0.);
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
    for strength in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
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
fn a_flick_shares_stick_capacity_with_gyro_without_losing_degrees() {
    let mut p = profile();
    gamepad(&mut p);
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.flick.output = AimOutput::RightStick;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 2, 32767);
    let mut extra = 0.;
    for ms in 1..1001 {
        gyro(&mut e, ms, 0., 0., 180.);
        if ms % 10 == 0 {
            extra += (e.advance(0.01, ms * 1_000_000).pad.unwrap().right_x as f32 / 32767. - 0.5)
                * 360.
                * 0.01;
        }
    }
    assert!((extra - 90.).abs() < 0.05, "{extra}");
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
    assert!(e.advance(0.01, 210_000_000).mouse_delta[0] > 0.);
}
