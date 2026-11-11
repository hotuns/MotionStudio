//! Pure engine regressions; these never open devices or send desktop input.
use motion_core::{
    controller_input::StickDirections,
    engine::{Engine, InputEvent, PadState},
    gamepad_bindings::GamepadBinding,
    profile::{Action, GamepadTarget, Profile, Trigger},
    storage::parse_profile,
    timing::SampleTime,
};
use serde_json::json;

fn profile() -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.gamepad_bindings = Some(vec![]);
    p
}

fn key(mode: Trigger) -> Action {
    serde_json::from_value(json!({
        "name":"keyboard", "input":"south", "output":"key:SPACE",
        "trigger":mode, "trigger_ms":100,
    }))
    .unwrap()
}

fn pad(output: &str, mode: Trigger, ms: u32) -> GamepadBinding {
    serde_json::from_value(json!({
        "input":"south", "output":output, "trigger":mode, "trigger_ms":ms,
    }))
    .unwrap()
}

fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: ms * 1_000_000,
    }
}

fn press(engine: &mut Engine, ms: u64, down: bool) {
    engine.process(&InputEvent::Button {
        time: time(ms),
        index: 0,
        down,
    });
}

fn buttons(engine: &Engine) -> u16 {
    engine.frame.pad.unwrap().buttons
}

#[test]
fn short_long_and_double_compete_across_keyboard_and_virtual_buttons() {
    let mut p = profile();
    p.actions.push(key(Trigger::Down));
    p.gamepad_bindings = Some(vec![
        pad("south", Trigger::Short, 100),
        pad("east", Trigger::Long, 250),
        pad("west", Trigger::Double, 200),
    ]);

    let mut e = Engine::new(p.clone()).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 41, false);
    assert!(e.frame.held.is_empty());
    assert_eq!(buttons(&e), 0);
    e.advance_input(time(201).host_ns);
    assert_eq!(buttons(&e), 0); // Second press still allowed at the deadline.
    e.advance_input(time(201).host_ns + 1);
    assert_eq!(buttons(&e), 0x1000);
    assert!(e.frame.held.is_empty());
    e.advance_input(time(242).host_ns);
    assert_eq!(buttons(&e), 0);

    let mut e = Engine::new(p.clone()).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 41, false);
    press(&mut e, 81, true);
    assert_eq!(buttons(&e), 0x4000);
    e.advance_input(time(700).host_ns);
    assert_eq!(buttons(&e), 0x4000); // Holding the second press cannot turn into Long.
    assert!(e.frame.held.is_empty());
    press(&mut e, 701, false);
    assert_eq!(buttons(&e), 0);
    e.advance_input(time(1000).host_ns);
    assert!(e.frame.held.is_empty());

    let mut e = Engine::new(p.clone()).unwrap();
    press(&mut e, 1, true);
    e.advance_input(time(251).host_ns);
    assert_eq!(buttons(&e), 0x2000);
    assert!(e.frame.held.is_empty());
    press(&mut e, 260, false);
    assert_eq!(buttons(&e), 0);
    assert!(e.frame.held.is_empty());

    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 211, false); // Too long for Short, too short for Long.
    assert!(e.frame.held.contains("key:SPACE"));
    assert_eq!(buttons(&e), 0);
}

#[test]
fn simultaneous_regular_binding_starts_immediately_and_coexists_with_long() {
    let mut p = profile();
    let mut regular = key(Trigger::Down);
    regular.allow_simultaneous = true;
    p.actions.push(regular);
    p.gamepad_bindings = Some(vec![pad("east", Trigger::Long, 100)]);
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    assert!(e.frame.held.contains("key:SPACE"));
    assert_eq!(buttons(&e), 0);
    e.advance_input(time(101).host_ns);
    assert!(e.frame.held.contains("key:SPACE"));
    assert_eq!(buttons(&e), 0x2000);
    press(&mut e, 102, false);
    assert!(e.frame.held.is_empty());
    assert_eq!(buttons(&e), 0);
}

#[test]
fn opting_long_into_simultaneous_mode_also_stops_it_from_delaying_regular() {
    let mut p = profile();
    p.actions.push(key(Trigger::Down));
    let mut long = pad("east", Trigger::Long, 100);
    long.allow_simultaneous = true;
    p.gamepad_bindings = Some(vec![long]);
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    assert!(e.frame.held.contains("key:SPACE"));
    e.advance_input(time(101).host_ns);
    assert!(e.frame.held.contains("key:SPACE"));
    assert_eq!(buttons(&e), 0x2000);
}

#[test]
fn double_deadline_and_late_repress_do_not_drop_a_completed_single_tap() {
    let mut p = profile();
    p.actions.push(key(Trigger::Short));
    p.gamepad_bindings = Some(vec![pad("east", Trigger::Double, 100)]);
    for extra in [0, 1] {
        let mut e = Engine::new(p.clone()).unwrap();
        press(&mut e, 1, true);
        press(&mut e, 41, false);
        e.advance_output(0.004, time(500).host_ns); // Never advances recognition.
        let ns = time(101).host_ns + extra;
        e.process(&InputEvent::Button {
            time: SampleTime {
                host_ns: ns,
                sensor_ns: ns,
            },
            index: 0,
            down: true,
        });
        assert_eq!(buttons(&e), if extra == 0 { 0x2000 } else { 0 });
        assert_eq!(e.frame.held.contains("key:SPACE"), extra != 0);
    }
}

#[test]
fn virtual_edge_pulses_and_turbo_advance_on_the_input_clock() {
    for mode in [Trigger::OnPress, Trigger::OnRelease] {
        let mut p = profile();
        p.gamepad_bindings = Some(vec![pad("east", mode, 100)]);
        let mut e = Engine::new(p).unwrap();
        press(&mut e, 1, true);
        assert_eq!(
            buttons(&e),
            if mode == Trigger::OnPress { 0x2000 } else { 0 }
        );
        press(&mut e, 2, false);
        assert_eq!(buttons(&e), 0x2000);
        e.advance_input(time(103).host_ns);
        assert_eq!(buttons(&e), 0);
    }
    let mut p = profile();
    let mut turbo = pad("east", Trigger::Long, 100);
    turbo.turbo_ms = Some(100);
    p.gamepad_bindings = Some(vec![turbo]);
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    e.advance_input(time(101).host_ns);
    assert_eq!(buttons(&e), 0x2000);
    e.advance_input(time(161).host_ns);
    assert_eq!(buttons(&e), 0);
    e.advance_input(time(211).host_ns);
    assert_eq!(buttons(&e), 0x2000);
    e.process(&InputEvent::Disconnected { time: time(220) });
    assert_eq!(e.frame.pad, Some(PadState::default()));
}

#[test]
fn layer_change_cancels_a_changed_gamepad_gesture_and_snapshot_needs_release() {
    let mut p = profile();
    p.actions.push(key(Trigger::Down));
    p.gamepad_bindings = Some(vec![pad("south", Trigger::Double, 100)]);
    let mut e = Engine::new(p.clone()).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 21, false);
    p.gamepad_bindings.as_mut().unwrap()[0].output = "east".into();
    e.apply_layer(p).unwrap();
    press(&mut e, 41, true);
    assert_eq!(buttons(&e), 0);
    press(&mut e, 61, false);
    press(&mut e, 81, true);
    assert_eq!(buttons(&e), 0x2000);
    e.advance_input(time(250).host_ns);
    assert!(e.frame.held.is_empty());
    assert_eq!(buttons(&e), 0x2000);
    e.reset();
    e.process_snapshot(&InputEvent::Button {
        time: time(301),
        index: 0,
        down: true,
    });
    press(&mut e, 321, false);
    press(&mut e, 341, true);
    assert_eq!(buttons(&e), 0);
}

#[test]
fn legacy_analog_bindings_and_new_activation_options_roundtrip() {
    let mut p = profile();
    let analog: GamepadBinding = serde_json::from_value(json!({
        "input":"left_trigger", "output":"right_trigger",
    }))
    .unwrap();
    assert_eq!(analog.trigger, Trigger::Down);
    assert!(!analog.allow_simultaneous);
    let mut digital = pad("east", Trigger::Double, 180);
    digital.allow_simultaneous = true;
    p.gamepad_bindings = Some(vec![analog, digital]);
    let loaded = parse_profile(&toml::to_string_pretty(&p).unwrap()).unwrap();
    assert_eq!(loaded.gamepad_bindings, p.gamepad_bindings);
    let mut e = Engine::new(loaded).unwrap();
    e.process(&InputEvent::Axis {
        time: time(1),
        index: 4,
        value: 16384,
    });
    let output = e.frame.pad.unwrap();
    assert!((127..=129).contains(&output.right_trigger));
    assert_eq!(output.left_trigger, 0);
    p.gamepad_bindings.as_mut().unwrap()[0].trigger = Trigger::Double;
    assert!(p.validate().is_err());
}

#[test]
fn eight_way_uses_radial_threshold_and_keeps_legacy_layout_available() {
    let mut p = profile();
    p.input.deadzone_left = Some(0.);
    p.input.lstick_directions = StickDirections::EightWay;
    p.actions = [("left_stick_up", "key:W"), ("left_stick_right", "key:D")]
        .map(|(input, output)| Action {
            input: input.into(),
            output: output.into(),
            ..key(Trigger::Down)
        })
        .into();
    let diagonal = |e: &mut Engine| {
        // Each component < 0.5 but radial magnitude > 0.5.
        e.process(&InputEvent::Axis {
            time: time(1),
            index: 0,
            value: 14000,
        });
        e.process(&InputEvent::Axis {
            time: time(2),
            index: 1,
            value: -14000,
        });
    };
    let mut e = Engine::new(p.clone()).unwrap();
    diagonal(&mut e);
    assert!(e.frame.held.contains("key:W"));
    assert!(e.frame.held.contains("key:D"));
    assert_eq!(e.frame.held.len(), 2);
    let base = p.clone();
    p.input.lstick_directions = StickDirections::AxisThreshold;
    e.apply_layer(p.clone()).unwrap();
    assert!(e.frame.held.is_empty());
    // Layer activation uses base geometry even after the active layout changes.
    assert!(e.layer_input_value("left_stick_up", &base, false) > 0.5);
    let mut legacy = Engine::new(p).unwrap();
    diagonal(&mut legacy);
    assert!(legacy.frame.held.is_empty());
}
