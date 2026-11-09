use motion_core::{
    engine::{Engine, InputEvent},
    profile::{Action, Profile, Trigger},
    storage::parse_profile,
    timing::SampleTime,
};
fn profile() -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    p.touchpad.enabled = true;
    p
}
#[test]
fn rejected_layer_changes_do_not_reset_a_drag_and_unmodified_layers_keep_it() {
    let mut e = Engine::new(profile()).unwrap();
    touch(&mut e, 10, 0, 0, true, 0.5, 0.5);
    click(&mut e, 20, 20, true);
    let mut bad = e.profile.clone();
    bad.touchpad.pointer_speed = 2.;
    bad.virtual_gamepad = motion_core::profile::GamepadTarget::Xbox360;
    assert!(e.apply_layer(bad).is_err());
    touch(&mut e, 30, 0, 0, true, 0.6, 0.5);
    assert!((e.frame.mouse_delta[0] - 140.).abs() < 0.001);
    assert!(e.frame.held.contains("mouse:left"));
    let mut layer = e.profile.clone();
    layer.gyro.sensitivity = 2.;
    e.apply_layer(layer).unwrap();
    touch(&mut e, 40, 0, 0, true, 0.7, 0.5);
    assert!((e.frame.mouse_delta[0] - 140.).abs() < 0.001);
    assert!(e.frame.held.contains("mouse:left"));
}
#[test]
fn shipped_touch_profiles_load_and_keep_pointer_off_for_touch_to_aim() {
    for source in [
        include_str!("../../../examples/touchpad-mouse.toml"),
        include_str!("../../../examples/touch-to-aim.toml"),
    ] {
        Engine::new(parse_profile(source).unwrap()).unwrap();
    }
    let p = parse_profile(include_str!("../../../examples/touch-to-aim.toml")).unwrap();
    assert_eq!(p.gyro.activation, "touch1");
    assert!(!p.touchpad.enabled);
    assert_eq!(p.layers[0].input, "touch1+left_shoulder");
}
fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: 0,
    }
}
fn touch(e: &mut Engine, ms: u64, pad: u8, finger: u8, active: bool, x: f32, y: f32) {
    e.process(&InputEvent::Touch {
        time: time(ms),
        pad,
        finger,
        active,
        x,
        y,
    });
}
fn click(e: &mut Engine, ms: u64, index: u8, down: bool) {
    e.process(&InputEvent::Button {
        time: time(ms),
        index,
        down,
    });
}
fn advance(e: &mut Engine, ms: u64) {
    e.advance(0.01, ms * 1_000_000);
}
#[test]
fn native_pointer_uses_contact_deltas_once_and_does_not_replay_between_events() {
    let mut e = Engine::new(profile()).unwrap();
    touch(&mut e, 10, 0, 0, true, 0.5, 0.5);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    touch(&mut e, 20, 0, 0, true, 0.6, 0.4);
    assert!((e.frame.mouse_delta[0] - 140.).abs() < 0.001);
    assert!((e.frame.mouse_delta[1] + 140.).abs() < 0.001);
    advance(&mut e, 30);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
}
#[test]
fn native_tap_pulses_release_without_more_touch_events() {
    let mut e = Engine::new(profile()).unwrap();
    touch(&mut e, 10, 0, 0, true, 0.5, 0.5);
    touch(&mut e, 50, 0, 0, false, 0., 0.);
    assert!(e.frame.held.contains("mouse:left"));
    for ms in [60, 70, 80, 90] {
        advance(&mut e, ms);
    }
    assert!(e.frame.held.is_empty());
}
#[test]
fn two_finger_tap_is_right_click_and_scroll_direction_uses_native_setting() {
    let mut e = Engine::new(profile()).unwrap();
    touch(&mut e, 10, 0, 0, true, 0.4, 0.5);
    touch(&mut e, 11, 0, 1, true, 0.6, 0.5);
    touch(&mut e, 30, 0, 0, false, 0., 0.);
    touch(&mut e, 31, 0, 1, false, 0., 0.);
    assert!(e.frame.held.contains("mouse:right"));
    assert!(!e.frame.held.contains("mouse:left"));
    let mut outputs = vec![];
    for natural in [true, false] {
        let mut p = profile();
        p.touchpad.natural_scroll = natural;
        let mut e = Engine::new(p).unwrap();
        touch(&mut e, 10, 0, 0, true, 0.4, 0.5);
        touch(&mut e, 11, 0, 1, true, 0.6, 0.5);
        let mut total = 0;
        for n in 0..10 {
            let y = 0.51 + n as f32 * 0.01;
            touch(&mut e, 20 + n * 2, 0, 0, true, 0.4, y);
            total += e.frame.scroll[1];
            touch(&mut e, 21 + n * 2, 0, 1, true, 0.6, y);
            total += e.frame.scroll[1];
        }
        assert_eq!(e.frame.mouse_delta, [0.; 2]);
        outputs.push(total);
    }
    assert!(outputs[0] != 0);
    assert_eq!(outputs[0], -outputs[1]);
}
#[test]
fn held_click_drags_and_lifting_after_click_cannot_create_an_extra_tap() {
    let mut e = Engine::new(profile()).unwrap();
    touch(&mut e, 10, 0, 0, true, 0.5, 0.5);
    click(&mut e, 20, 20, true);
    assert!(e.frame.held.contains("mouse:left"));
    touch(&mut e, 30, 0, 0, true, 0.6, 0.5);
    assert!(e.frame.mouse_delta[0] > 100.);
    assert!(e.frame.held.contains("mouse:left"));
    click(&mut e, 40, 20, false);
    touch(&mut e, 50, 0, 0, false, 0., 0.);
    assert!(e.frame.held.is_empty());
}
#[test]
fn split_pads_reuse_native_left_scroll_right_pointer_and_middle_chord() {
    let mut e = Engine::new(profile()).unwrap();
    e.set_touchpad_count(2);
    advance(&mut e, 1);
    touch(&mut e, 10, 0, 0, true, 0.5, 0.5);
    touch(&mut e, 20, 0, 0, true, 0.5, 0.6);
    assert_ne!(e.frame.scroll, [0; 2]);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    touch(&mut e, 30, 1, 0, true, 0.5, 0.5);
    touch(&mut e, 40, 1, 0, true, 0.6, 0.5);
    assert!(e.frame.mouse_delta[0] > 100.);
    click(&mut e, 50, 20, true);
    click(&mut e, 51, 21, true);
    assert!(e.frame.held.contains("mouse:middle"));
}
#[test]
fn snapshots_and_enabling_a_held_contact_wait_for_neutral_without_a_fake_tap() {
    let mut e = Engine::new(profile()).unwrap();
    e.process_snapshot(&InputEvent::Touch {
        time: time(1),
        pad: 0,
        finger: 0,
        active: true,
        x: 0.2,
        y: 0.2,
    });
    touch(&mut e, 10, 0, 0, true, 0.8, 0.8);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    touch(&mut e, 20, 0, 0, false, 0., 0.);
    assert!(e.frame.held.is_empty());
    touch(&mut e, 30, 0, 0, true, 0.5, 0.5);
    touch(&mut e, 40, 0, 0, true, 0.6, 0.5);
    assert!(e.frame.mouse_delta[0] > 100.);
    let mut p = e.profile.clone();
    p.touchpad.pointer_speed = 2.;
    e.apply_layer(p).unwrap();
    touch(&mut e, 50, 0, 0, true, 0.7, 0.5);
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
}
#[test]
fn touch_contact_activators_and_chords_work_even_with_pointer_routing_disabled() {
    let mut p = profile();
    p.touchpad.enabled = false;
    p.actions.push(Action {
        name: "contact".into(),
        input: "touch1+left_shoulder".into(),
        output: "key:SPACE".into(),
        threshold: 0.5,
        repeat_ms: None,
        trigger: Trigger::Down,
        trigger_ms: 250,
        sustain: true,
        turbo_ms: None,
    });
    let mut e = Engine::new(p).unwrap();
    click(&mut e, 1, 9, true);
    touch(&mut e, 2, 0, 0, true, 0.5, 0.5);
    assert_eq!(e.input_value("touch1"), 1.);
    assert!(e.frame.held.contains("key:SPACE"));
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
    touch(&mut e, 3, 0, 0, false, 0., 0.);
    assert!(!e.frame.held.contains("key:SPACE"));
}
#[test]
fn invalid_coordinates_cancel_touch_state_without_releasing_another_mapping() {
    let mut p = profile();
    p.actions.push(Action {
        name: "hold".into(),
        input: "south".into(),
        output: "key:SPACE".into(),
        threshold: 0.5,
        repeat_ms: None,
        trigger: Trigger::Down,
        trigger_ms: 250,
        sustain: true,
        turbo_ms: None,
    });
    let mut e = Engine::new(p).unwrap();
    click(&mut e, 1, 0, true);
    touch(&mut e, 10, 0, 0, true, 0.5, 0.5);
    click(&mut e, 20, 20, true);
    touch(&mut e, 30, 0, 0, true, f32::NAN, 0.5);
    assert!(!e.frame.held.contains("mouse:left"));
    assert!(e.frame.held.contains("key:SPACE"));
    assert_eq!(e.frame.mouse_delta, [0.; 2]);
}
