use anyhow::{bail, Result};
use motion_core::{
    engine::{Engine, Frame, InputEvent, PadState},
    output::{Dispatcher, Sink},
    profile::{Action, Profile, BUTTONS},
    storage::parse_profile,
    timing::SampleTime,
};
use std::{cell::RefCell, rc::Rc};
fn action(input: &str, output: &str) -> Action {
    Action {
        name: input.into(),
        input: input.into(),
        output: output.into(),
        threshold: 0.5,
        repeat_ms: None,
        trigger: Default::default(),
        trigger_ms: 250,
        sustain: true,
        turbo_ms: None,
    }
}
fn profile(actions: Vec<Action>) -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    p.actions = actions;
    p
}
fn event(input: &str, down: bool) -> InputEvent {
    InputEvent::Button {
        time: SampleTime {
            host_ns: 1_000_000,
            sensor_ns: 1_000_000,
        },
        index: BUTTONS.iter().position(|v| *v == input).unwrap() as u8,
        down,
    }
}
fn press(e: &mut Engine, input: &str, down: bool) -> Frame {
    e.process(&event(input, down)).clone()
}

#[test]
fn chords_require_all_inputs_and_more_specific_actions_override_their_subsets() {
    let mut e = Engine::new(profile(vec![
        action("south", "key:SPACE"),
        action("left_shoulder+south", "key:CTRL+key:SHIFT+key:K"),
        action("left_shoulder+south+east", "mouse:x2"),
    ]))
    .unwrap();
    assert!(press(&mut e, "left_shoulder", true).held.is_empty());
    let f = press(&mut e, "south", true);
    assert!(!f.held.contains("key:SPACE"));
    assert_eq!(f.held.len(), 3);
    assert!(f.held.contains("key:SHIFT"));
    let f = press(&mut e, "east", true);
    assert_eq!(f.held, ["mouse:x2".into()].into());
    let f = press(&mut e, "east", false);
    assert_eq!(f.held.len(), 3);
    let f = press(&mut e, "left_shoulder", false);
    assert_eq!(f.held, ["key:SPACE".into()].into());
    assert!(press(&mut e, "south", false).held.is_empty());
}

#[test]
fn analog_members_use_the_action_threshold_and_release_any_member() {
    let mut e = Engine::new(profile(vec![action("left_trigger+south", "mouse:x1")])).unwrap();
    press(&mut e, "south", true);
    let axis = |value| InputEvent::Axis {
        time: SampleTime {
            host_ns: 1_000_000,
            sensor_ns: 0,
        },
        index: 4,
        value,
    };
    assert!(e.process(&axis(10000)).held.is_empty());
    assert!(e.process(&axis(25000)).held.contains("mouse:x1"));
    assert!(press(&mut e, "south", false).held.is_empty());
}

#[test]
fn scroll_is_a_pulse_per_binding_and_never_repeats_from_gyro_or_idle_frames() {
    let mut e = Engine::new(profile(vec![
        action("south", "wheel:up"),
        action("east", "wheel:up"),
    ]))
    .unwrap();
    assert_eq!(press(&mut e, "south", true).scroll, [0, 1]);
    assert_eq!(
        press(&mut e, "east", true).scroll,
        [0, 1],
        "a second binding still gets its own scroll pulse"
    );
    for n in 1..10 {
        assert_eq!(e.advance(0.004, n * 4_000_000).scroll, [0; 2]);
    }
    assert_eq!(press(&mut e, "south", false).scroll, [0; 2]);
    assert_eq!(press(&mut e, "east", false).scroll, [0; 2]);
    assert_eq!(press(&mut e, "east", true).scroll, [0, 1]);
}

#[test]
fn releasing_a_chord_modifier_does_not_scroll_the_still_held_single_binding() {
    let mut e = Engine::new(profile(vec![
        action("south", "wheel:down"),
        action("left_shoulder+south", "key:E"),
    ]))
    .unwrap();
    press(&mut e, "left_shoulder", true);
    assert_eq!(press(&mut e, "south", true).scroll, [0; 2]);
    assert_eq!(press(&mut e, "left_shoulder", false).scroll, [0; 2]);
    press(&mut e, "south", false);
    assert_eq!(press(&mut e, "south", true).scroll, [0, -1]);
}

#[test]
fn continuous_scroll_is_time_based_and_disconnect_and_snapshots_cannot_replay_it() {
    let mut a = action("south", "key:CTRL+wheel:right");
    a.repeat_ms = Some(100);
    let mut e = Engine::new(profile(vec![a])).unwrap();
    assert_eq!(press(&mut e, "south", true).scroll, [1, 0]);
    let mut n = 0;
    for i in 1..9 {
        n += e.advance(0.025, i * 25_000_000).scroll[0];
    }
    assert_eq!(n, 2);
    assert_eq!(e.advance(0.2, 500_000_000).scroll, [0; 2]);
    e.process(&InputEvent::Disconnected {
        time: SampleTime {
            host_ns: 501_000_000,
            sensor_ns: 0,
        },
    });
    assert!(e.frame.held.is_empty());
    assert_eq!(e.advance(0.025, 525_000_000).scroll, [0; 2]);
    assert_eq!(e.process_snapshot(&event("south", true)).scroll, [0; 2]);
    for i in 1..9 {
        assert_eq!(
            e.advance(0.025, 525_000_000 + i * 25_000_000).scroll,
            [0; 2]
        );
    }
    press(&mut e, "south", false);
    assert_eq!(press(&mut e, "south", true).scroll, [1, 0]);
}

#[derive(Debug, PartialEq)]
enum Sent {
    Button(String, bool),
    Scroll(i32, i32),
}
struct Spy {
    seen: Rc<RefCell<Vec<Sent>>>,
    fail_scroll: bool,
}
impl Sink for Spy {
    fn button(&mut self, name: &str, down: bool) -> Result<()> {
        self.seen.borrow_mut().push(Sent::Button(name.into(), down));
        Ok(())
    }
    fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
        Ok(())
    }
    fn gamepad(&mut self, _: PadState) -> Result<()> {
        Ok(())
    }
    fn scroll(&mut self, h: i32, v: i32) -> Result<()> {
        self.seen.borrow_mut().push(Sent::Scroll(h, v));
        if self.fail_scroll {
            bail!("scroll failed")
        };
        Ok(())
    }
}

#[test]
fn modifiers_press_before_main_keys_and_release_last_and_shared_modifiers_stay_held() {
    let mut e = Engine::new(profile(vec![
        action("south", "key:CTRL+key:SHIFT+key:K"),
        action("east", "key:CTRL+mouse:x1"),
    ]))
    .unwrap();
    let seen = Rc::new(RefCell::new(vec![]));
    let mut out = Dispatcher::new(Spy {
        seen: seen.clone(),
        fail_scroll: false,
    });
    out.apply(&press(&mut e, "south", true)).unwrap();
    assert_eq!(
        &*seen.borrow(),
        &[
            Sent::Button("key:CTRL".into(), true),
            Sent::Button("key:SHIFT".into(), true),
            Sent::Button("key:K".into(), true)
        ]
    );
    out.apply(&press(&mut e, "east", true)).unwrap();
    out.apply(&press(&mut e, "south", false)).unwrap();
    assert!(!seen
        .borrow()
        .contains(&Sent::Button("key:CTRL".into(), false)));
    out.reset().unwrap();
    let sent = seen.borrow();
    assert_eq!(
        &sent[sent.len() - 2..],
        &[
            Sent::Button("mouse:x1".into(), false),
            Sent::Button("key:CTRL".into(), false)
        ]
    );
}

#[test]
fn ctrl_wheel_presses_ctrl_first_and_output_failure_still_releases_it() {
    let mut e = Engine::new(profile(vec![action("south", "key:CTRL+wheel:up")])).unwrap();
    let seen = Rc::new(RefCell::new(vec![]));
    {
        let mut out = Dispatcher::new(Spy {
            seen: seen.clone(),
            fail_scroll: true,
        });
        assert!(out.apply(&press(&mut e, "south", true)).is_err());
    }
    assert_eq!(
        &*seen.borrow(),
        &[
            Sent::Button("key:CTRL".into(), true),
            Sent::Scroll(0, 1),
            Sent::Button("key:CTRL".into(), false)
        ]
    );
}

#[test]
fn caps_lock_survives_save_and_sends_one_press_release_per_controller_press() {
    let p = profile(vec![action("south", "key:CAPSLOCK")]);
    let stored = toml::to_string_pretty(&p).unwrap();
    let mut e = Engine::new(parse_profile(&stored).unwrap()).unwrap();
    let seen = Rc::new(RefCell::new(vec![]));
    let mut out = Dispatcher::new(Spy {
        seen: seen.clone(),
        fail_scroll: false,
    });
    out.apply(&press(&mut e, "south", true)).unwrap();
    for n in 1..=10 {
        out.apply(e.advance(0.004, n * 4_000_000)).unwrap();
    }
    assert_eq!(
        &*seen.borrow(),
        &[Sent::Button("key:CAPSLOCK".into(), true)]
    );
    out.apply(&press(&mut e, "south", false)).unwrap();
    out.apply(&press(&mut e, "south", true)).unwrap();
    out.reset().unwrap();
    assert_eq!(
        &*seen.borrow(),
        &[
            Sent::Button("key:CAPSLOCK".into(), true),
            Sent::Button("key:CAPSLOCK".into(), false),
            Sent::Button("key:CAPSLOCK".into(), true),
            Sent::Button("key:CAPSLOCK".into(), false),
        ]
    );
}

#[test]
fn chord_validation_rejects_ambiguous_or_unsupported_expressions_and_preserves_old_profiles() {
    for (input, output) in [
        ("south+south", "key:A"),
        ("left_stick_up+left_stick_down", "key:A"),
        ("south", "key:CTRL+key:F12"),
        ("south", "wheel:up+wheel:down"),
        ("south", "key:A+"),
        ("south", "key:CTRL+key:ALT+key:SHIFT+key:A+key:B"),
    ] {
        assert!(
            Engine::new(profile(vec![action(input, output)])).is_err(),
            "{input} -> {output}"
        );
    }
    let p = profile(vec![action("south+left_shoulder", "key:CTRL+mouse:x2")]);
    let text = toml::to_string_pretty(&p).unwrap();
    let back = parse_profile(&text).unwrap();
    assert_eq!(back.actions[0].input, "south+left_shoulder");
    assert!(back.actions[0].repeat_ms.is_none());
    let mut p = profile(vec![action("south", "key:A")]);
    p.actions[0].repeat_ms = Some(100);
    assert!(p.validate().is_err());
}
