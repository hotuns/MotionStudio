use motion_core::{
    engine::{Engine, InputEvent},
    layers::{self, Layer},
    profile::{Action, Trigger},
    storage::parse_profile,
    timing::SampleTime,
};
fn profile() -> motion_core::profile::Profile {
    parse_profile(include_str!("../../../examples/blank.toml")).unwrap()
}
fn action(mode: Trigger) -> Action {
    Action {
        name: "test".into(),
        input: "south".into(),
        output: "key:SPACE".into(),
        threshold: 0.5,
        repeat_ms: None,
        trigger: mode,
        trigger_ms: 100,
        sustain: true,
        turbo_ms: None,
    }
}
fn button(down: bool) -> InputEvent {
    InputEvent::Button {
        time: SampleTime {
            host_ns: 1,
            sensor_ns: 1,
        },
        index: 0,
        down,
    }
}
fn configured(mode: Trigger) -> Engine {
    let mut p = profile();
    p.actions = vec![action(mode)];
    Engine::new(p).unwrap()
}
fn held(e: &Engine) -> bool {
    e.frame.held.contains("key:SPACE")
}
#[test]
fn long_press_waits_then_holds_and_early_release_never_fires() {
    let mut e = configured(Trigger::Long);
    e.process(&button(true));
    assert!(!held(&e));
    e.advance(0.04, 0);
    e.process(&button(false));
    assert!(!held(&e));
    e.process(&button(true));
    for _ in 0..3 {
        e.advance(0.04, 0);
    }
    assert!(held(&e));
    e.advance(0.04, 0);
    assert!(held(&e));
    e.process(&button(false));
    assert!(!held(&e));
}
#[test]
fn double_press_holds_second_press_beyond_recognition_window_and_releases() {
    let mut e = configured(Trigger::Double);
    e.process(&button(true));
    e.advance(0.02, 0);
    e.process(&button(false));
    e.advance(0.02, 0);
    assert!(!held(&e));
    e.process(&button(true));
    assert!(held(&e));
    for _ in 0..8 {
        e.advance(0.04, 0);
    }
    assert!(held(&e));
    e.process(&button(false));
    assert!(!held(&e));
}
#[test]
fn double_press_expiry_and_reconnect_snapshot_cannot_start_a_gesture() {
    let mut e = configured(Trigger::Double);
    e.process(&button(true));
    e.process(&button(false));
    for _ in 0..4 {
        e.advance(0.04, 0);
    }
    e.process(&button(true));
    assert!(!held(&e));
    e.reset();
    e.process_snapshot(&button(true));
    for _ in 0..4 {
        e.advance(0.04, 0);
    }
    e.process(&button(false));
    assert!(!held(&e));
    e.process(&button(true));
    assert!(!held(&e));
}
#[test]
fn start_release_short_and_turbo_emit_bounded_outputs() {
    let mut e = configured(Trigger::OnPress);
    e.process(&button(true));
    assert!(held(&e));
    for _ in 0..3 {
        e.advance(0.04, 0);
    }
    assert!(!held(&e));
    let mut e = configured(Trigger::OnRelease);
    e.process(&button(true));
    assert!(!held(&e));
    e.process(&button(false));
    assert!(held(&e));
    for _ in 0..3 {
        e.advance(0.04, 0);
    }
    assert!(!held(&e));
    let mut e = configured(Trigger::Short);
    e.process(&button(true));
    e.advance(0.04, 0);
    e.process(&button(false));
    assert!(held(&e));
    e.advance(0.05, 0);
    assert!(!held(&e));
    let mut p = profile();
    let mut a = action(Trigger::Down);
    a.turbo_ms = Some(100);
    p.actions.push(a);
    let mut e = Engine::new(p).unwrap();
    e.process(&button(true));
    assert!(held(&e));
    e.advance(0.05, 0);
    assert!(!held(&e));
    e.process(&button(false));
    assert!(!held(&e));
}
#[test]
fn outer_ring_is_radial_has_hysteresis_and_inner_excludes_neutral() {
    let mut p = profile();
    p.input.deadzone_left = Some(0.); // Isolate native ring geometry from source calibration.
    let mut a = action(Trigger::Down);
    a.input = "left_stick_outer".into();
    p.actions.push(a);
    let mut e = Engine::new(p).unwrap();
    let axis = |index, value| InputEvent::Axis {
        time: SampleTime {
            host_ns: 1,
            sensor_ns: 1,
        },
        index,
        value,
    };
    e.process(&axis(0, 20000));
    assert!(!held(&e));
    e.process(&axis(1, 20000));
    assert!(held(&e));
    e.process(&axis(0, 28000));
    e.process(&axis(1, 0));
    assert!(held(&e));
    e.process(&axis(0, 26600));
    assert!(held(&e));
    e.process(&axis(0, 25000));
    assert!(!held(&e));
    let mut p = profile();
    let mut a = action(Trigger::Down);
    a.input = "left_stick_inner".into();
    p.actions.push(a);
    let mut e = Engine::new(p).unwrap();
    assert!(!held(&e));
    e.process(&axis(0, 10000));
    assert!(held(&e));
    e.process(&axis(0, 0));
    assert!(!held(&e));
}
#[test]
fn layers_merge_only_overrides_preserve_inheritance_and_roundtrip_nullable_fields() {
    let mut p = profile();
    p.actions.push(action(Trigger::Down));
    p.layers.push(Layer {
        activation_threshold: 0.5,
        activation_hysteresis: 0.1,
        id: "ads".into(),
        name: "ADS".into(),
        input: "left_shoulder".into(),
        settings: serde_json::from_value(
            serde_json::json!({"gyro":{"sensitivity":0.5,"pause_input":{"clear":true}}}),
        )
        .unwrap(),
        actions: vec![],
        disabled_inputs: vec![],
    });
    p.validate().unwrap();
    let active = layers::resolve(&p, &[0]).unwrap();
    assert_eq!(active.gyro.sensitivity, 0.5);
    assert_eq!(active.actions.len(), 1);
    assert!(active.gyro.pause_input.is_none());
    assert_eq!(active.virtual_gamepad, p.virtual_gamepad);
    let text = toml::to_string_pretty(&p).unwrap();
    let loaded = parse_profile(&text).unwrap();
    assert_eq!(
        layers::resolve(&loaded, &[0]).unwrap().gyro.sensitivity,
        0.5
    );
    p.layers[0]
        .settings
        .insert("virtual_gamepad".into(), serde_json::json!("dualshock_4"));
    assert!(p.validate().is_err());
}
#[test]
fn layer_validation_rejects_conflicts_hidden_by_a_later_override() {
    let mut p = profile();
    p.gyro.acceleration.start_dps = 10.;
    p.gyro.acceleration.full_dps = 100.;
    for (id, patch) in [
        (
            "slow",
            serde_json::json!({"gyro":{"acceleration":{"start_dps":80}}}),
        ),
        (
            "fast",
            serde_json::json!({"gyro":{"acceleration":{"full_dps":50}}}),
        ),
        (
            "last",
            serde_json::json!({"gyro":{"acceleration":{"full_dps":150}}}),
        ),
    ] {
        p.layers.push(Layer {
            id: id.into(),
            name: id.into(),
            input: "left_shoulder".into(),
            activation_threshold: 0.5,
            activation_hysteresis: 0.1,
            settings: serde_json::from_value(patch).unwrap(),
            actions: vec![],
            disabled_inputs: vec![],
        });
    }
    for active in [&[0][..], &[1], &[2], &[0, 1, 2]] {
        layers::resolve(&p, active).unwrap().validate().unwrap();
    }
    let error = p.validate().unwrap_err();
    assert!(error.to_string().contains("slow + fast"));
}
#[test]
fn shipped_hold_layer_example_loads_with_inherited_aim_and_distinct_activators() {
    let profile = parse_profile(include_str!("../../../examples/hold-layer.toml")).unwrap();
    let active = layers::resolve(&profile, &[0]).unwrap();
    assert_eq!(profile.gyro.sensitivity, 2.);
    assert_eq!(active.gyro.sensitivity, 0.55);
    assert_eq!(active.actions, profile.actions);
    assert!(active.actions.iter().any(|a| a.output == "mouse:right"));
    assert!(active.actions.iter().any(|a| a.trigger == Trigger::Short));
    assert!(active.actions.iter().any(|a| a.trigger == Trigger::Long));
}
#[test]
fn layer_sensitivity_change_preserves_long_press_progress_and_sample_clock() {
    let mut p = profile();
    p.actions.push(action(Trigger::Long));
    p.gyro.enabled = true;
    p.gyro.activation = "always".into();
    p.gyro.smoothing_ms = 0.;
    p.gyro.noise_threshold = 0.;
    p.gyro.motion.noise_gate = 0.;
    p.gyro.acceleration.fast_multiplier = 1.;
    let mut e = Engine::new(p.clone()).unwrap();
    e.process(&button(true));
    e.advance(0.04, 0);
    e.advance(0.04, 0);
    let gyro = |ns| InputEvent::Gyro {
        time: SampleTime {
            host_ns: ns,
            sensor_ns: ns,
        },
        value: [0.1, 0.2, 0.3],
    };
    e.process(&gyro(1_000_000));
    let before = e.process(&gyro(3_000_000)).mouse_delta;
    assert!(before.iter().any(|v| v.abs() > 0.));
    p.gyro.sensitivity *= 0.5;
    e.apply_layer(p).unwrap();
    let after = e.process(&gyro(5_000_000)).mouse_delta;
    for (before, after) in before.into_iter().zip(after) {
        assert!(
            (after - before * 0.5).abs() < 1e-4,
            "lost or replayed gyro interval: {before} -> {after}"
        );
    }
    e.advance(0.03, 5_000_000);
    assert!(held(&e));
}
#[test]
fn action_overrides_do_not_replay_held_buttons_and_inherited_wheel_pulses_survive_switch() {
    let mut p = profile();
    let mut wheel = action(Trigger::Down);
    wheel.input = "left_shoulder".into();
    wheel.output = "wheel:down".into();
    p.actions = vec![wheel, action(Trigger::Down)];
    let mut e = Engine::new(p.clone()).unwrap();
    let lb = InputEvent::Button {
        time: SampleTime {
            host_ns: 1,
            sensor_ns: 1,
        },
        index: 9,
        down: true,
    };
    e.process(&lb);
    assert_eq!(e.frame.scroll, [0, -1]);
    p.actions[1].output = "key:E".into();
    e.apply_layer(p.clone()).unwrap();
    assert_eq!(e.frame.scroll, [0, -1]);
    e.advance(0.01, 0);
    assert_eq!(e.frame.scroll, [0, 0]);
    e.process(&button(true));
    p.actions[1].output = "wheel:up".into();
    e.apply_layer(p).unwrap();
    assert!(e.frame.held.is_empty());
    assert_eq!(e.frame.scroll, [0, 0]);
    e.process(&button(false));
    e.process(&button(true));
    assert_eq!(e.frame.scroll, [0, 1]);
}
#[test]
fn rebound_filter_releases_suppression_even_without_another_axis_change() {
    let mut p = profile();
    p.stick_deadzone = 0.;
    p.stick_rebound_ms = Some(40);
    let mut a = action(Trigger::Down);
    a.input = "left_stick_left".into();
    a.threshold = 0.05;
    p.actions.push(a);
    let mut e = Engine::new(p).unwrap();
    let axis = |ns, value| InputEvent::Axis {
        time: SampleTime {
            host_ns: ns,
            sensor_ns: ns,
        },
        index: 0,
        value,
    };
    e.process(&axis(1_000_000, 30000));
    e.process(&axis(15_000_000, 0));
    e.process(&axis(25_000_000, -6500));
    assert!(!held(&e));
    e.advance(0.035, 60_000_000);
    assert!(held(&e));
    e.process(&axis(65_000_000, 0));
    assert!(!held(&e));
}
