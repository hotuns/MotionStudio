//! End-to-end contracts: original physical-source processing reaches every consumer.
use motion_core::{
    engine::{Engine, InputEvent},
    profile::{GamepadTarget, Profile},
    storage::parse_profile,
    timing::SampleTime,
};
fn profile() -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    p.input.deadzone_left = Some(0.);
    p.input.deadzone_right = Some(0.);
    p
}
fn axis(e: &mut Engine, ms: u64, index: u8, value: f32) {
    let value = (value * 32767.).round() as i16;
    e.process(&InputEvent::Axis {
        time: SampleTime {
            host_ns: ms * 1_000_000,
            sensor_ns: ms * 1_000_000,
        },
        index,
        value,
    });
}
#[test]
fn calibration_and_side_deadzones_reach_axes_directions_and_virtual_pad_once() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.input.deadzone_left = Some(0.1);
    p.input.deadzone_right = Some(0.2);
    p.input.rstick_center = [0.1, 0.];
    p.input.rstick_radial.fill(1.25);
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 1, 0, 0.55);
    axis(&mut e, 2, 2, 0.58);
    let x = ((0.58 - 0.1) * 1.25 - 0.2) / 0.8;
    assert!((e.processed_axes()[0] - 0.5).abs() < 0.0001);
    assert!((e.processed_axes()[2] - x).abs() < 0.0001);
    assert!((e.input_value("right_stick_right") - x).abs() < 0.0001);
    assert!((f32::from(e.frame.pad.unwrap().right_x) / 32767. - x).abs() < 0.0001);
    axis(&mut e, 3, 2, 0.18);
    assert_eq!(e.processed_axes()[2], 0.);
    assert_eq!(e.frame.pad.unwrap().right_x, 0);
}
#[test]
fn resting_offset_cannot_move_flick_companion_aim_or_poison_the_speed_gate() {
    let mut p = profile();
    p.gyro.enabled = true;
    p.gyro.activation = "always".into();
    p.gyro.counts_per_degree = 10.;
    p.input.deadzone_right = Some(0.2);
    let f = &mut p.gyro.motion.flick;
    f.enabled = true;
    f.stick_aim = true;
    f.speed_gate_ms = 80.;
    f.slow_lock = true;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 1, 2, 0.18);
    for ms in (10..=1000).step_by(10) {
        assert_eq!(e.advance(0.01, ms * 1_000_000).mouse_delta, [0.; 2]);
    }
    axis(&mut e, 1001, 2, 1.);
    let mut total = 0.;
    for ms in (1010..=1500).step_by(10) {
        total += e.advance(0.01, ms * 1_000_000).mouse_delta[0];
    }
    assert!(
        (total - 900.).abs() < 0.01,
        "native Flick after neutral: {total}"
    );
}
#[test]
fn flick_outer_boundary_uses_the_native_processed_stick_travel() {
    let mut p = profile();
    p.gyro.enabled = true;
    p.gyro.activation = "always".into();
    p.gyro.counts_per_degree = 10.;
    p.input.deadzone_right = Some(0.2);
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.flick.deadzone = 0.8;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 1, 2, 0.8);
    assert_eq!(e.advance(0.01, 10_000_000).mouse_delta, [0.; 2]);
    axis(&mut e, 20, 2, 1.);
    let mut total = 0.;
    for ms in (30..=500).step_by(10) {
        total += e.advance(0.01, ms * 1_000_000).mouse_delta[0];
    }
    assert!((total - 900.).abs() < 0.01, "{total}");
}
#[test]
fn digital_trigger_calibration_hysteresis_and_timed_release_use_original_pipeline() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.input.digital_triggers = true;
    p.input.ltrig_min = 0.1;
    p.input.ltrig_max = 0.9;
    p.input.ltrig_digital_threshold = 0.5;
    p.input.trig_hysteresis = 0.02;
    p.input.trig_debounce_ms = 8.;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 1, 4, 0.7);
    assert_eq!(e.processed_axes()[4], 1.);
    assert_eq!(e.frame.pad.unwrap().left_trigger, 255);
    axis(&mut e, 2, 4, 0.495);
    assert_eq!(e.processed_axes()[4], 1.);
    axis(&mut e, 3, 4, 0.1);
    assert_eq!(e.processed_axes()[4], 1.);
    e.advance(0.004, 7_000_000);
    assert_eq!(e.processed_axes()[4], 1.);
    e.advance(0.006, 13_000_000);
    assert_eq!(e.processed_axes()[4], 0.);
    assert_eq!(e.frame.pad.unwrap().left_trigger, 0);
}
#[test]
fn a_layer_keeps_its_base_activation_when_its_input_deadzone_changes() {
    let mut base = profile();
    base.input.deadzone_left = Some(0.1);
    let mut e = Engine::new(base.clone()).unwrap();
    axis(&mut e, 1, 0, 0.6);
    let before = e.layer_input_value("left_stick_right", &base, false);
    let mut layer = base.clone();
    layer.input.deadzone_left = Some(0.8);
    e.apply_layer(layer).unwrap();
    assert_eq!(e.input_value("left_stick_right"), 0.);
    assert!((e.layer_input_value("left_stick_right", &base, true) - before).abs() < 0.00001);
}

#[test]
fn ds4_automap_keeps_analog_and_digital_trigger_states_and_releases_both() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::DualShock4;
    let mut e = Engine::new(p).unwrap();
    axis(&mut e, 1, 4, 0.75);
    assert!(e.frame.pad.unwrap().left_trigger_pressed);
    assert!(e.frame.pad.unwrap().left_trigger > 180);
    assert!(!e.frame.pad.unwrap().right_trigger_pressed);
    axis(&mut e, 2, 4, 0.);
    e.advance(0.01, 12_000_000);
    assert!(!e.frame.pad.unwrap().left_trigger_pressed);
    assert_eq!(e.frame.pad.unwrap().left_trigger, 0);
}
#[test]
fn ordinary_mouse_native_response_preserves_legacy_curve_with_bounded_interpolation_error() {
    for exponent in [1., 1.5, 2., 3., 4.] {
        let mut p = profile();
        p.stick_mouse.enabled = true;
        p.stick_mouse.stick = "left".into();
        p.stick_mouse.speed = 1000.;
        p.stick_mouse.exponent = exponent;
        p.input.deadzone_left = Some(0.2);
        let mut e = Engine::new(p).unwrap();
        for i in 0..=100 {
            let raw = i as f32 / 100.;
            axis(&mut e, 1 + i, 0, raw);
            let x = e.processed_axes()[0];
            let expected = x.powf(exponent);
            let actual = e.advance(0.01, (1 + i) * 1_000_000).mouse_delta[0] as f32 / 10.;
            assert!(
                (actual - expected).abs() < 0.0001,
                "exponent={exponent} raw={raw}: {actual} vs {expected}"
            );
        }
    }
}
