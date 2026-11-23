//! Direction geometry and profile lifecycle, without devices or desktop output.
use motion_core::{
    controller_input::StickDirections,
    engine::{Engine, InputEvent},
    layers,
    profile::{Profile, Trigger},
    storage::parse_profile,
    timing::SampleTime,
};
use serde_json::json;

fn profile() -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    p.input.deadzone_left = Some(0.);
    p.input.deadzone_right = Some(0.);
    p.input.lstick_directions = StickDirections::EightWay;
    p.input.rstick_directions = StickDirections::EightWay;
    for (side, outputs) in [
        ("left", ["A", "D", "W", "S"]),
        ("right", ["J", "L", "I", "K"]),
    ] {
        for (direction, output) in ["left", "right", "up", "down"].into_iter().zip(outputs) {
            p.actions.push(
                serde_json::from_value(json!({
                    "name": format!("{side} {direction}"),
                    "input": format!("{side}_stick_{direction}"),
                    "output": format!("key:{output}"),
                    "threshold": 0.5,
                    "trigger_ms": 100,
                }))
                .unwrap(),
            );
        }
    }
    p
}

fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: ms * 1_000_000,
    }
}

fn stick(e: &mut Engine, side: u8, x: i16, y: i16, ms: u64) {
    for (index, value) in [(side * 2, x), (side * 2 + 1, y)] {
        e.process(&InputEvent::Axis {
            time: time(ms),
            index,
            value,
        });
    }
}

fn held(e: &Engine, keys: &[&str]) {
    let expected: std::collections::BTreeSet<String> =
        keys.iter().map(|k| format!("key:{k}")).collect();
    assert_eq!(e.frame.held, expected);
}

#[test]
fn outer_sectors_share_the_ring_and_switch_without_recentering() {
    let mut p = profile();
    p.actions = serde_json::from_value(json!([
        {"name": "Ring", "input": "left_stick_outer", "output": "key:SHIFT"},
        {"name": "Forward", "input": "left_stick_outer_up", "output": "key:W"},
        {"name": "Right", "input": "left_stick_outer_right", "output": "key:D"}
    ])).unwrap();
    p.virtual_gamepad = motion_core::profile::GamepadTarget::Xbox360;
    p.gamepad_bindings = Some(serde_json::from_value(json!([
        {"input": "right_stick_outer_left", "output": "south"}
    ])).unwrap());
    let mut e = Engine::new(p.clone()).unwrap();
    stick(&mut e, 1, -32767, 0, 1);
    stick(&mut e, 0, 0, -32767, 2);
    held(&e, &["SHIFT", "W"]);
    assert_eq!(e.frame.pad.unwrap().buttons, 0x1000);
    // A diagonal still belongs to exactly one native outer sector, even in eight-way WASD mode.
    stick(&mut e, 0, 26000, -26000, 3);
    assert_eq!(e.input_value("left_stick_outer_up") + e.input_value("left_stick_outer_right"), 1.);
    stick(&mut e, 0, 32767, 0, 4);
    held(&e, &["SHIFT", "D"]);
    stick(&mut e, 0, 27000, 0, 5); // Between the 0.85 entry and 0.80 release radii.
    held(&e, &["SHIFT", "D"]);
    let mut layer = p.clone();
    layer.stick_zones.left_outer = 0.99;
    e.apply_layer(layer).unwrap();
    held(&e, &[]);
    assert_eq!(e.layer_input_value("left_stick_outer_right", &p, false), 1.);
    stick(&mut e, 0, 23000, 0, 6);
    assert_eq!(e.layer_input_value("left_stick_outer_right", &p, true), 0.);
    stick(&mut e, 1, 0, 32767, 7);
    assert_eq!(e.input_value("right_stick_outer_down"), 1.);
    assert_eq!(e.frame.pad.unwrap().buttons, 0);
    e.reset();
    assert_eq!(e.input_value("right_stick_outer_down"), 0.);
}

#[test]
fn native_default_preserves_original_predicate_including_boundaries() {
    use flexinput_engine::eval::jsm_stick_directions;
    for radius in [0_f32, 0.3, 1.] {
        for step in 0..3600 {
            let angle = (step as f32 / 10.).to_radians();
            let (x, y) = (radius * angle.cos(), radius * angle.sin());
            let (ax, ay) = (x.abs(), y.abs());
            assert_eq!(
                jsm_stick_directions(x, y),
                [x < -0.5 * ay, x > 0.5 * ay, y > 0.5 * ax, y < -0.5 * ax]
            );
        }
    }
    for (x, y, expected) in [
        (1., 0.5, [false, true, false, false]),
        (0.5, 1., [false, false, true, false]),
        (-1., -0.5, [true, false, false, false]),
        (-0.5, -1., [false, false, false, true]),
    ] {
        assert_eq!(jsm_stick_directions(x, y), expected);
    }
}

#[test]
fn overlap_is_independent_per_stick_and_layer_activation_uses_base_geometry() {
    let mut p = profile();
    p.input.lstick_overlap_degrees = Some(10.);
    p.input.rstick_overlap_degrees = Some(60.);
    let mut e = Engine::new(p.clone()).unwrap();
    // About 20 degrees above right: outside a 10-degree diagonal sector,
    // inside a 60-degree diagonal sector. Magnitude exceeds 0.5 on both.
    stick(&mut e, 0, 28000, -10000, 1);
    stick(&mut e, 1, 28000, -10000, 2);
    held(&e, &["D", "L", "I"]);
    let base = p.clone();
    p.input.lstick_overlap_degrees = Some(60.);
    p.input.rstick_overlap_degrees = Some(10.);
    e.apply_layer(p.clone()).unwrap();
    held(&e, &["D", "W", "L"]);
    assert!(e.layer_input_value("right_stick_up", &base, false) > 0.5);
    assert_eq!(e.layer_input_value("right_stick_up", &p, false), 0.);
    assert_eq!(e.layer_input_value("left_stick_up", &base, false), 0.);
    e.apply_layer(base).unwrap();
    held(&e, &["D", "L", "I"]);
    stick(&mut e, 0, 0, 0, 3);
    stick(&mut e, 1, 0, 0, 4);
    held(&e, &[]);
}

#[test]
fn zero_and_full_overlap_have_no_direction_holes_or_center_output() {
    for (x, y) in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)] {
        assert_eq!(
            flexinput_engine::eval::jsm_stick_directions_with_ratio(x, y, 1.),
            [x < 0., x > 0., false, false]
        );
    }
    for degrees in [0., 90.] {
        let mut p = profile();
        p.input.lstick_overlap_degrees = Some(degrees);
        let mut e = Engine::new(p).unwrap();
        let mut ms = 1;
        for (x, horizontal) in [(-20000, "A"), (20000, "D")] {
            for (y, vertical) in [(-20000, "W"), (20000, "S")] {
                stick(&mut e, 0, x, y, ms);
                if degrees == 0. {
                    // Signed SDL axes use 32767/32768 scales, so equal raw
                    // magnitudes need not produce an exact calibrated tie.
                    assert_eq!(e.frame.held.len(), 1);
                    assert!(
                        e.frame.held.contains(&format!("key:{horizontal}"))
                            || e.frame.held.contains(&format!("key:{vertical}"))
                    );
                } else {
                    held(&e, &[horizontal, vertical]);
                }
                ms += 1;
            }
        }
        for (x, y, key) in [
            (25000, 0, "D"),
            (-25000, 0, "A"),
            (0, -25000, "W"),
            (0, 25000, "S"),
        ] {
            stick(&mut e, 0, x, y, ms);
            held(&e, &[key]);
            ms += 1;
        }
        stick(&mut e, 0, 25000, -1, ms);
        held(&e, if degrees == 0. { &["D"] } else { &["D", "W"] });
        stick(&mut e, 0, 0, 0, ms + 1);
        held(&e, &[]);
    }
}

#[test]
fn changing_overlap_cancels_pending_gesture_only_for_the_affected_stick() {
    let mut p = profile();
    p.input.lstick_overlap_degrees = Some(10.);
    p.actions.retain(|a| a.input.ends_with("_right"));
    for action in &mut p.actions {
        action.trigger = Trigger::Long;
    }
    let mut e = Engine::new(p.clone()).unwrap();
    stick(&mut e, 0, 30000, 0, 1);
    stick(&mut e, 1, 30000, 0, 1);
    e.advance_input(time(51).host_ns);
    held(&e, &[]);
    p.input.lstick_overlap_degrees = Some(80.);
    e.apply_layer(p).unwrap();
    e.advance_input(time(102).host_ns);
    held(&e, &["L"]);
    e.advance_input(time(250).host_ns);
    held(&e, &["L"]);
    stick(&mut e, 0, 0, 0, 251);
    stick(&mut e, 0, 30000, 0, 252);
    e.advance_input(time(353).host_ns);
    held(&e, &["D", "L"]);
}

#[test]
fn axis_threshold_ignores_overlap_and_preserves_pending_gestures() {
    let mut p = profile();
    p.input.lstick_directions = StickDirections::AxisThreshold;
    p.input.lstick_overlap_degrees = Some(0.);
    p.actions.retain(|a| a.input == "left_stick_right");
    p.actions[0].trigger = Trigger::Long;
    let mut e = Engine::new(p.clone()).unwrap();
    stick(&mut e, 0, 28000, -10000, 1);
    e.advance_input(time(51).host_ns);
    p.input.lstick_overlap_degrees = Some(90.);
    e.apply_layer(p).unwrap();
    e.advance_input(time(102).host_ns);
    held(&e, &["D"]);
}

#[test]
fn optional_overlap_round_trips_and_layer_can_restore_native_default() {
    let mut p = profile();
    let old = toml::to_string_pretty(&p).unwrap();
    assert!(!old.contains("overlap_degrees"));
    let old = parse_profile(&old).unwrap();
    assert_eq!(old.input.lstick_overlap_degrees, None);
    assert_eq!(old.input.rstick_overlap_degrees, None);
    p.input.lstick_overlap_degrees = Some(12.5);
    p.input.rstick_overlap_degrees = Some(70.);
    p.layers.push(
        serde_json::from_value(json!({
            "id": "overlap", "name": "Overlap", "input": "left_shoulder",
            "settings": {"input": {"lstick_overlap_degrees": {"clear": true}}}
        }))
        .unwrap(),
    );
    let loaded = parse_profile(&toml::to_string_pretty(&p).unwrap()).unwrap();
    assert_eq!(loaded.input.lstick_overlap_degrees, Some(12.5));
    assert_eq!(loaded.input.rstick_overlap_degrees, Some(70.));
    let active = layers::resolve(&loaded, &[0]).unwrap();
    active.validate().unwrap();
    assert_eq!(active.input.lstick_overlap_degrees, None);
    assert_eq!(active.input.rstick_overlap_degrees, Some(70.));
    p.layers[0].settings =
        serde_json::from_value(json!({"input": {"lstick_overlap_degrees": 91}})).unwrap();
    assert!(p.validate().is_err());
}

#[test]
fn overlap_validation_rejects_nonfinite_and_out_of_range_on_either_stick() {
    for value in [-1., 90.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for left in [true, false] {
            let mut p = profile();
            if left {
                p.input.lstick_overlap_degrees = Some(value);
            } else {
                p.input.rstick_overlap_degrees = Some(value);
            }
            assert!(p.validate().is_err(), "accepted {value} for left={left}");
        }
    }
    for value in [0., 36.87, 90.] {
        let mut p = profile();
        p.input.lstick_overlap_degrees = Some(value);
        p.input.rstick_overlap_degrees = Some(value);
        p.validate().unwrap();
    }
}
