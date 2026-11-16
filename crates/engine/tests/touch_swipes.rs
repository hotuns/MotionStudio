use motion_core::{
    engine::{Engine, InputEvent, PressHandoff},
    profile::{Action, Profile, Trigger},
    storage::parse_profile,
    timing::SampleTime,
    touchpad::SWIPE_PINS,
};

fn profile() -> Profile {
    let mut profile = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    for (input, output) in SWIPE_PINS
        .into_iter()
        .zip(["key:W", "key:S", "key:A", "key:D"])
    {
        profile.actions.push(Action {
            name: input.into(),
            input: input.into(),
            output: output.into(),
            threshold: 0.5,
            repeat_ms: None,
            trigger: Trigger::Down,
            trigger_ms: 250,
            sustain: true,
            turbo_ms: None,
            allow_simultaneous: false,
            chord_wait_ms: 80,
            toggle: false,
        });
    }
    profile
}

fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: 0,
    }
}

fn touch(ms: u64, pad: u8, finger: u8, position: Option<[f32; 2]>) -> InputEvent {
    let [x, y] = position.unwrap_or([0.; 2]);
    InputEvent::Touch {
        time: time(ms),
        pad,
        finger,
        active: position.is_some(),
        x,
        y,
    }
}

fn held(engine: &Engine, expected: &[&str]) {
    assert_eq!(
        engine.frame.held,
        expected.iter().map(|key| (*key).to_string()).collect()
    );
}

#[test]
fn four_directions_hold_without_motion_and_keep_the_first_direction_until_lift() {
    for (target, key) in [
        ([0.5, 0.2], "key:W"),
        ([0.5, 0.8], "key:S"),
        ([0.2, 0.5], "key:A"),
        ([0.8, 0.5], "key:D"),
    ] {
        let mut engine = Engine::new(profile()).unwrap();
        engine.process(&touch(10, 0, 0, Some([0.5, 0.5])));
        engine.process(&touch(15, 0, 0, Some([0.55, 0.55])));
        held(&engine, &[]);
        engine.process(&touch(20, 0, 0, Some(target)));
        held(&engine, &[key]);
        // Advance the drained input clock for a second with no new touch events.
        for ms in (30..=1020).step_by(10) {
            engine.advance_input(ms * 1_000_000);
            held(&engine, &[key]);
        }
        engine.process(&touch(1030, 0, 0, Some([0.5, 0.5])));
        held(&engine, &[key]);
        engine.process(&touch(1040, 0, 0, Some([1. - target[0], 1. - target[1]])));
        held(&engine, &[key]);
        engine.process(&touch(1050, 0, 0, None));
        held(&engine, &[]);
    }
}

#[test]
fn quick_lift_releases_immediately_and_a_new_contact_can_choose_another_direction() {
    let mut engine = Engine::new(profile()).unwrap();
    for (start, target, key) in [(10, [0.8, 0.5], "key:D"), (30, [0.2, 0.5], "key:A")] {
        engine.process(&touch(start, 0, 0, Some([0.5, 0.5])));
        held(&engine, &[]);
        engine.process(&touch(start + 1, 0, 0, Some(target)));
        held(&engine, &[key]);
        engine.process(&touch(start + 2, 0, 0, None));
        held(&engine, &[]);
        engine.advance_input((start + 3) * 1_000_000);
        held(&engine, &[]);
    }
}

#[test]
fn lifting_the_swiping_finger_releases_even_if_another_finger_is_still_touching() {
    let mut engine = Engine::new(profile()).unwrap();
    engine.process(&touch(10, 0, 0, Some([0.5, 0.5])));
    engine.process(&touch(11, 0, 1, Some([0.5, 0.5])));
    engine.process(&touch(20, 0, 0, Some([0.8, 0.5])));
    held(&engine, &["key:D"]);
    engine.process(&touch(21, 0, 0, None));
    held(&engine, &[]);
    engine.process(&touch(30, 0, 0, Some([0.5, 0.5])));
    engine.process(&touch(40, 0, 0, Some([0.8, 0.5])));
    engine.process(&touch(41, 0, 1, Some([0.5, 0.2])));
    held(&engine, &["key:D", "key:W"]);
    engine.process(&touch(42, 0, 0, None));
    held(&engine, &["key:W"]);
    engine.process(&touch(43, 0, 1, None));
    held(&engine, &[]);
}

#[test]
fn matching_directions_from_two_fingers_or_pads_release_only_after_both_lift() {
    for split in [false, true] {
        let mut engine = Engine::new(profile()).unwrap();
        engine.set_touchpad_count(if split { 2 } else { 1 });
        engine.advance_input(1_000_000);
        let (pad, finger) = if split { (1, 0) } else { (0, 1) };
        engine.process(&touch(10, 0, 0, Some([0.5, 0.5])));
        engine.process(&touch(11, pad, finger, Some([0.5, 0.5])));
        engine.process(&touch(20, 0, 0, Some([0.8, 0.5])));
        engine.process(&touch(21, pad, finger, Some([0.8, 0.5])));
        for ms in (30..=300).step_by(10) {
            engine.advance_input(ms * 1_000_000);
            held(&engine, &["key:D"]);
        }
        engine.process(&touch(310, 0, 0, None));
        held(&engine, &["key:D"]);
        engine.process(&touch(320, pad, finger, None));
        held(&engine, &[]);
    }
}

#[test]
fn long_press_counts_from_swipe_recognition_and_releases_on_lift() {
    let mut profile = profile();
    profile.actions[3].trigger = Trigger::Long;
    let mut engine = Engine::new(profile).unwrap();
    engine.process(&touch(10, 0, 0, Some([0.5, 0.5])));
    for ms in (20..=300).step_by(10) {
        engine.advance_input(ms * 1_000_000);
    }
    engine.process(&touch(310, 0, 0, Some([0.8, 0.5])));
    held(&engine, &[]);
    for ms in (320..=550).step_by(10) {
        engine.advance_input(ms * 1_000_000);
        held(&engine, &[]);
    }
    for ms in (560..=700).step_by(10) {
        engine.advance_input(ms * 1_000_000);
        held(&engine, &["key:D"]);
    }
    engine.process(&touch(701, 0, 0, None));
    held(&engine, &[]);
}

#[test]
fn snapshot_gap_invalid_coordinates_and_disconnect_clear_swipe_holds() {
    for reset in ["snapshot", "gap", "invalid", "disconnect"] {
        let mut engine = Engine::new(profile()).unwrap();
        engine.process(&touch(10, 0, 0, Some([0.5, 0.5])));
        engine.process(&touch(20, 0, 0, Some([0.8, 0.5])));
        held(&engine, &["key:D"]);
        let now = match reset {
            "snapshot" => {
                engine.process_snapshot(&touch(30, 0, 0, Some([0.8, 0.5])));
                30
            }
            "gap" => {
                engine.advance_input(200_000_000);
                200
            }
            "invalid" => {
                engine.process(&touch(30, 0, 0, Some([f32::NAN, 0.5])));
                30
            }
            _ => {
                engine.process(&InputEvent::Disconnected { time: time(30) });
                30
            }
        };
        held(&engine, &[]);
        engine.process_snapshot(&touch(now + 1, 0, 0, Some([0.8, 0.5])));
        engine.process(&touch(now + 10, 0, 0, Some([0.2, 0.5])));
        held(&engine, &[]);
        engine.process(&touch(now + 20, 0, 0, None));
        held(&engine, &[]);
        engine.process(&touch(now + 30, 0, 0, Some([0.5, 0.5])));
        engine.process(&touch(now + 40, 0, 0, Some([0.2, 0.5])));
        held(&engine, &["key:A"]);
    }
}

#[test]
fn profile_handoff_clears_swipes_and_waits_for_contact_release() {
    let mut previous = Engine::new(profile()).unwrap();
    previous.process(&touch(10, 0, 0, Some([0.5, 0.5])));
    previous.process(&touch(20, 0, 0, Some([0.8, 0.5])));
    held(&previous, &["key:D"]);
    let handoff = PressHandoff::new(previous);
    assert!(handoff.is_empty());
    let mut next = Engine::new(profile()).unwrap();
    next.inherit_profile_input(handoff.previous(), handoff.blocked_inputs());
    next.process(&touch(30, 0, 0, Some([0.2, 0.5])));
    held(&next, &[]);
    next.process(&touch(40, 0, 0, None));
    next.process(&touch(50, 0, 0, Some([0.5, 0.5])));
    next.process(&touch(60, 0, 0, Some([0.2, 0.5])));
    held(&next, &["key:A"]);
}
