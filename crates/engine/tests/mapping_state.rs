//! Synthetic input only: no physical backend and no desktop output.
use motion_core::{
    engine::{Engine, InputEvent},
    layers::{Layer, LayerActivation},
    profile::{Action, GamepadTarget, Profile, BUTTONS},
    storage::parse_profile,
    timing::SampleTime,
};
use serde_json::json;

fn base() -> Profile {
    parse_profile(include_str!("../../../examples/blank.toml")).unwrap()
}
fn action(mode: &str) -> Action {
    serde_json::from_value(json!({"name":"Run", "input":"south", "output":"key:SPACE", "toggle":true, "trigger":mode, "trigger_ms":100})).unwrap()
}
fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: ms * 1_000_000,
    }
}
fn event(ms: u64, input: &str, down: bool) -> InputEvent {
    InputEvent::Button {
        time: time(ms),
        index: BUTTONS.iter().position(|p| *p == input).unwrap() as u8,
        down,
    }
}
fn press(e: &mut Engine, ms: u64, down: bool) {
    e.process(&event(ms, "south", down));
}
fn held(e: &Engine) -> bool {
    e.frame.held.contains("key:SPACE")
}
fn engine(mode: &str) -> Engine {
    let mut p = base();
    p.actions = vec![action(mode)];
    Engine::new(p).unwrap()
}
fn layer(input: &str, mode: &str) -> Layer {
    serde_json::from_value(json!({"id":input,"name":input,"input":input,"activation_mode":mode}))
        .unwrap()
}
fn layers(s: &mut LayerActivation, p: &Profile, edges: bool, inputs: &[(&str, f32)]) -> Vec<usize> {
    s.sample(p, edges, |pin, _| {
        inputs
            .iter()
            .find(|(k, _)| *k == pin)
            .map_or(0., |(_, v)| *v)
    })
}

#[test]
fn toggle_holds_until_next_press_and_reset_clears_it() {
    let mut e = engine("down");
    press(&mut e, 1, true);
    assert!(held(&e));
    press(&mut e, 2, false);
    e.advance_input(time(80).host_ns);
    assert!(held(&e));
    assert_eq!(e.mapping_status()[0].reason, "latched");
    press(&mut e, 81, true);
    assert!(!held(&e));
    press(&mut e, 82, false);
    press(&mut e, 83, true);
    assert!(held(&e));
    e.reset();
    assert!(!held(&e));
    assert!(!e.mapping_status()[0].latched);
}
#[test]
fn snapshot_and_layer_entry_require_release_before_toggling() {
    let mut e = engine("down");
    e.process_snapshot(&event(1, "south", true));
    e.advance_input(time(20).host_ns);
    assert!(!held(&e));
    press(&mut e, 21, false);
    press(&mut e, 22, true);
    assert!(held(&e));
    let mut p = e.profile.clone();
    p.actions[0].output = "key:E".into();
    e.apply_layer(p).unwrap();
    assert!(e.frame.held.is_empty());
    press(&mut e, 23, false);
    press(&mut e, 24, true);
    assert!(e.frame.held.contains("key:E"));
}
#[test]
fn overlapping_edge_pulses_toggle_once_per_recognized_gesture() {
    for mode in ["on_press", "on_release", "short"] {
        let mut e = engine(mode);
        press(&mut e, 1, true);
        press(&mut e, 11, false);
        assert!(held(&e), "{mode}");
        press(&mut e, 12, true);
        press(&mut e, 13, false);
        assert!(!held(&e), "{mode}");
    }
}
#[test]
fn long_and_double_toggle_only_after_recognition() {
    let mut e = engine("long");
    press(&mut e, 1, true);
    assert!(!held(&e));
    assert_eq!(e.mapping_status()[0].reason, "waiting_long");
    e.advance_input(time(101).host_ns);
    assert!(held(&e));
    press(&mut e, 102, false);
    press(&mut e, 103, true);
    e.advance_input(time(203).host_ns);
    assert!(!held(&e));
    let mut e = engine("double");
    press(&mut e, 1, true);
    press(&mut e, 11, false);
    assert!(!held(&e));
    assert_eq!(e.mapping_status()[0].reason, "waiting_double");
    press(&mut e, 21, true);
    press(&mut e, 22, false);
    assert!(held(&e));
    press(&mut e, 31, true);
    press(&mut e, 32, false);
    press(&mut e, 41, true);
    assert!(!held(&e));
}
#[test]
fn toggle_turbo_uses_a_separate_output_clock() {
    let mut p = base();
    let mut a = action("down");
    a.turbo_ms = Some(100);
    p.actions = vec![a];
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 2, false);
    assert!(held(&e));
    e.advance_input(time(61).host_ns);
    assert!(!held(&e));
    assert!(e.mapping_status()[0].latched);
    e.advance_input(time(111).host_ns);
    assert!(held(&e));
    press(&mut e, 112, true);
    assert!(!held(&e));
    e.advance_input(time(221).host_ns);
    assert!(!held(&e));
}
#[test]
fn digital_virtual_buttons_share_toggle_semantics_while_analog_is_rejected() {
    let mut p = base();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.gamepad_bindings = Some(
        serde_json::from_value(json!([{"input":"south","output":"east","toggle":true}])).unwrap(),
    );
    let mut e = Engine::new(p.clone()).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 2, false);
    assert_ne!(e.frame.pad.unwrap().buttons & 0x2000, 0);
    press(&mut e, 3, true);
    assert_eq!(e.frame.pad.unwrap().buttons & 0x2000, 0);
    p.gamepad_bindings = Some(
        serde_json::from_value(
            json!([{"input":"left_trigger","output":"left_trigger","toggle":true}]),
        )
        .unwrap(),
    );
    assert!(p.validate().is_err());
}
#[test]
fn chord_claim_cancels_latch_and_repress_without_idle_tick_rearms_it() {
    let mut p = base();
    p.actions = vec![
        action("down"),
        serde_json::from_value(
            json!({"name":"Chord","input":"left_shoulder+south","output":"key:E"}),
        )
        .unwrap(),
    ];
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    assert!(!held(&e));
    assert_eq!(e.mapping_status()[0].reason, "waiting_chord");
    e.process(&event(2, "left_shoulder", true));
    assert!(!held(&e));
    assert_eq!(e.mapping_status()[0].reason, "consumed");
    e.process(&event(20, "left_shoulder", false));
    press(&mut e, 21, false);
    press(&mut e, 22, true);
    e.advance_input(time(103).host_ns);
    assert!(held(&e));
}
#[test]
fn unchanged_latches_survive_layer_setting_edits_but_removed_bindings_release() {
    let mut e = engine("down");
    press(&mut e, 1, true);
    press(&mut e, 2, false);
    let mut p = e.profile.clone();
    p.gyro.sensitivity += 1.;
    e.apply_layer(p.clone()).unwrap();
    assert!(held(&e));
    p.actions.push(
        serde_json::from_value(json!({"name":"Other","input":"east","output":"key:E"})).unwrap(),
    );
    e.apply_layer(p.clone()).unwrap();
    assert!(held(&e));
    p.actions.remove(0);
    e.apply_layer(p).unwrap();
    assert!(!held(&e));
}
#[test]
fn continuous_scroll_latch_stops_on_next_press_and_shared_outputs_remain_held() {
    let mut p = base();
    let mut a = action("down");
    a.output = "key:CTRL+wheel:down".into();
    a.repeat_ms = Some(50);
    p.actions = vec![a];
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    assert_eq!(e.frame.scroll, [0, -1]);
    press(&mut e, 2, false);
    e.advance(0.03, time(32).host_ns);
    e.advance(0.03, time(62).host_ns);
    assert_eq!(e.frame.scroll, [0, -1]);
    assert!(e.frame.held.contains("key:CTRL"));
    press(&mut e, 63, true);
    assert!(e.frame.held.is_empty());
    e.advance(0.03, time(93).host_ns);
    assert_eq!(e.frame.scroll, [0, 0]);
    let mut p = base();
    p.actions = vec![
        action("down"),
        serde_json::from_value(json!({"name":"Shared","input":"east","output":"key:SPACE"}))
            .unwrap(),
    ];
    let mut e = Engine::new(p).unwrap();
    press(&mut e, 1, true);
    press(&mut e, 2, false);
    e.process(&event(3, "east", true));
    press(&mut e, 4, true);
    assert!(held(&e));
    e.process(&event(5, "east", false));
    assert!(!held(&e));
}

#[test]
fn release_catchup_long_gestures_are_distinct_even_when_pulses_overlap() {
    let mut e = engine("long");
    press(&mut e, 1, true);
    press(&mut e, 101, false);
    assert!(held(&e));
    press(&mut e, 102, true);
    press(&mut e, 202, false);
    assert!(!held(&e));
}

#[test]
fn profile_handoff_does_not_keep_a_latched_output() {
    let mut e = engine("down");
    press(&mut e, 1, true);
    assert!(held(&e));
    assert!(motion_core::engine::PressHandoff::new(e).is_empty());
}
#[test]
fn toggle_layers_preserve_priority_and_independent_hold_layers() {
    let mut p = base();
    p.layers = vec![
        layer("left_shoulder", "toggle"),
        layer("right_shoulder", "hold"),
    ];
    let mut s = LayerActivation::default();
    assert_eq!(layers(&mut s, &p, true, &[("left_shoulder", 1.)]), [0]);
    assert_eq!(layers(&mut s, &p, true, &[("right_shoulder", 1.)]), [0, 1]);
    assert_eq!(
        layers(
            &mut s,
            &p,
            true,
            &[("left_shoulder", 1.), ("right_shoulder", 1.)]
        ),
        [1]
    );
    assert!(layers(&mut s, &p, true, &[]).is_empty());
}
#[test]
fn layer_exit_and_global_reset_require_releasing_the_activation_input() {
    let mut p = base();
    p.layers = vec![
        layer("left_shoulder", "toggle"),
        layer("right_shoulder", "hold"),
    ];
    p.layers[0].exit_input = Some("east".into());
    p.layer_reset_input = Some("start".into());
    let mut s = LayerActivation::default();
    assert_eq!(layers(&mut s, &p, true, &[("left_shoulder", 1.)]), [0]);
    assert!(layers(&mut s, &p, true, &[("left_shoulder", 1.), ("east", 1.)]).is_empty());
    assert!(layers(&mut s, &p, true, &[("left_shoulder", 1.)]).is_empty());
    layers(&mut s, &p, true, &[]);
    assert_eq!(
        layers(
            &mut s,
            &p,
            true,
            &[("left_shoulder", 1.), ("right_shoulder", 1.)]
        ),
        [0, 1]
    );
    assert!(layers(
        &mut s,
        &p,
        true,
        &[("left_shoulder", 1.), ("right_shoulder", 1.), ("start", 1.)]
    )
    .is_empty());
    assert!(layers(
        &mut s,
        &p,
        true,
        &[("left_shoulder", 1.), ("right_shoulder", 1.)]
    )
    .is_empty());
    layers(&mut s, &p, true, &[]);
    assert_eq!(layers(&mut s, &p, true, &[("right_shoulder", 1.)]), [1]);
}
#[test]
fn layer_snapshot_and_analog_hysteresis_do_not_create_toggle_edges() {
    let mut p = base();
    p.layers = vec![layer("left_trigger", "toggle")];
    let mut s = LayerActivation::default();
    assert!(layers(&mut s, &p, false, &[("left_trigger", 1.)]).is_empty());
    assert!(layers(&mut s, &p, true, &[("left_trigger", 1.)]).is_empty());
    layers(&mut s, &p, true, &[]);
    assert_eq!(layers(&mut s, &p, true, &[("left_trigger", 0.6)]), [0]);
    assert_eq!(layers(&mut s, &p, true, &[("left_trigger", 0.45)]), [0]);
    assert_eq!(layers(&mut s, &p, true, &[("left_trigger", 0.6)]), [0]);
    layers(&mut s, &p, true, &[]);
    assert!(layers(&mut s, &p, true, &[("left_trigger", 0.6)]).is_empty());
}
#[test]
fn old_profiles_default_to_momentary_behavior_and_new_settings_roundtrip() {
    let mut p = base();
    p.layers = vec![layer("left_shoulder", "toggle")];
    p.layers[0].exit_input = Some("east".into());
    p.layer_reset_input = Some("start".into());
    p.actions = vec![action("down")];
    let saved = toml::to_string(&p).unwrap();
    let loaded = parse_profile(&saved).unwrap();
    assert!(loaded.actions[0].toggle);
    assert_eq!(loaded.layer_reset_input, p.layer_reset_input);
    assert_eq!(loaded.layers[0].exit_input, p.layers[0].exit_input);
    let old: Action =
        serde_json::from_value(json!({"name":"old","input":"south","output":"key:SPACE"})).unwrap();
    assert!(!old.toggle);
}
