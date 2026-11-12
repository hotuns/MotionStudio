use anyhow::Result;
use motion_core::engine::{Engine, Frame, InputEvent, PadState};
use motion_core::motion_config::Stick;
use motion_core::output::{Dispatcher, Sink};
use motion_core::profile::{
    Action, GamepadTarget, Gyro, GyroActivationMode, GyroOutput, Profile, TimestampSource,
};
use motion_core::timing::SampleTime;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

fn profile() -> Profile {
    Profile {
        layers: vec![],
        stick_zones: Default::default(),
        stick_rebound_ms: None,
        schema_version: 1,
        name: "test".into(),
        virtual_gamepad: GamepadTarget::None,
        gamepad_bindings: None,
        stick_deadzone: 0.0,
        input: Default::default(),
        feedback: Default::default(),
        touchpad: Default::default(),
        stick_mouse: Default::default(),
        gyro: Gyro {
            enabled: true,
            activation: "always".into(),
            counts_per_degree: 10.0,
            timestamp_source: TimestampSource::Sensor,
            ..Default::default()
        },
        actions: vec![],
        trigger_stages: Default::default(),
    }
}

#[test]
fn asynchronous_output_failure_is_detected_even_on_idle_frames_and_reset_releases_other_targets() {
    struct AsyncSink {
        events: Rc<RefCell<Vec<String>>>,
        failed: Rc<std::cell::Cell<bool>>,
    }
    impl Sink for AsyncSink {
        fn poll(&mut self) -> Result<()> {
            anyhow::ensure!(!self.failed.get(), "asynchronous I/O fault");
            Ok(())
        }
        fn cancel_pending(&mut self) -> Result<()> {
            self.events.borrow_mut().push("cancel".into());
            anyhow::ensure!(!self.failed.get(), "reset reported earlier fault");
            Ok(())
        }
        fn button(&mut self, key: &str, down: bool) -> Result<()> {
            self.events.borrow_mut().push(format!("{key}:{down}"));
            Ok(())
        }
        fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
            Ok(())
        }
        fn gamepad(&mut self, pad: PadState) -> Result<()> {
            self.events
                .borrow_mut()
                .push(format!("pad:{}", pad.buttons));
            Ok(())
        }
    }
    let events = Rc::new(RefCell::new(vec![]));
    let failed = Rc::new(std::cell::Cell::new(false));
    let mut dispatcher = Dispatcher::new(AsyncSink {
        events: events.clone(),
        failed: failed.clone(),
    });
    dispatcher
        .apply(&Frame {
            held: ["key:A".into()].into_iter().collect(),
            pad: Some(PadState {
                buttons: 1,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    failed.set(true);
    assert!(dispatcher.apply(&Frame::default()).is_err());
    events.borrow_mut().clear();
    assert!(dispatcher.reset().is_err());
    assert_eq!(*events.borrow(), vec!["cancel", "key:A:false", "pad:0"]);
}

#[test]
fn held_stick_keeps_moving_without_new_axis_events_and_stops_after_reset() {
    let mut p = profile();
    p.gyro.enabled = false;
    p.stick_mouse.enabled = true;
    p.stick_mouse.stick = "left".into();
    p.stick_mouse.speed = 1000.0;
    let mut engine = Engine::new(p).unwrap();
    engine.process(&InputEvent::Axis {
        time: time(1),
        index: 0,
        value: 32767,
    });
    let mut total = 0.0;
    for _ in 0..100 {
        total += engine.advance(0.01, 0).mouse_delta[0];
    }
    assert!((total - 1000.0).abs() < 0.01);
    assert_eq!(engine.advance(0.2, 0).mouse_delta, [0.0; 2]);
    engine.reset();
    assert_eq!(engine.advance(0.01, 0).mouse_delta, [0.0; 2]);
}
#[test]
fn smoothing_reduces_a_step_and_soft_threshold_preserves_large_motion() {
    let mut plain = Engine::new(profile()).unwrap();
    let mut filtered_profile = profile();
    filtered_profile.gyro.smoothing_ms = 20.0;
    filtered_profile.gyro.noise_threshold = 0.5;
    let mut filtered = Engine::new(filtered_profile).unwrap();
    for t in 1..10 {
        plain.process(&sample(t, [0.0; 3]));
        filtered.process(&sample(t, [0.0; 3]));
    }
    let a = plain.process(&sample(10, [0.0, -100.0, 0.0])).mouse_delta[0];
    let b = filtered
        .process(&sample(10, [0.0, -100.0, 0.0]))
        .mouse_delta[0];
    assert!(b > 0.0 && b < a);
    filtered.reset();
    filtered.process(&sample(20, [0.0, -0.2, 0.0]));
    assert_eq!(
        filtered.process(&sample(21, [0.0, -0.2, 0.0])).mouse_delta,
        [0.0; 2]
    );
}
fn time(ms: u64) -> SampleTime {
    SampleTime {
        host_ns: ms * 1_000_000,
        sensor_ns: ms * 1_000_000,
    }
}
fn sample(ms: u64, dps: [f32; 3]) -> InputEvent {
    InputEvent::Gyro {
        time: time(ms),
        value: dps.map(f32::to_radians),
    }
}
fn button(index: u8, down: bool) -> InputEvent {
    InputEvent::Button {
        time: time(1),
        index,
        down,
    }
}
fn axis(ms: u64, index: u8, value: i16) -> InputEvent {
    InputEvent::Axis {
        time: time(ms),
        index,
        value,
    }
}
fn binding(input: &str, output: &str) -> Action {
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
        allow_simultaneous: false,
        chord_wait_ms: 80,
    }
}

#[test]
fn rws_calibration_uses_the_configured_stick_without_also_mapping_it() {
    for (stick, name, index, output) in [
        (Stick::Left, "left", 0, GyroOutput::RightStick),
        (Stick::Right, "right", 2, GyroOutput::Mouse),
    ] {
        let mut p = profile();
        p.virtual_gamepad = GamepadTarget::Xbox360;
        p.gyro.output = output;
        p.gyro.stick_full_scale_dps = 1440.;
        p.gyro.motion.flick.stick = stick;
        p.gyro.motion.flick.enabled = output == GyroOutput::Mouse;
        p.stick_mouse.enabled = true;
        p.stick_mouse.stick = name.into();
        p.actions
            .push(binding(&format!("{name}_stick_up"), "key:W"));
        let mut engine = Engine::new(p).unwrap();
        engine.begin_measurement("yaw").unwrap();

        let mut ms = 1;
        let mut mouse_x = 0.;
        for step in 0..=36 {
            let (x, y) = (step as f32 * 10.).to_radians().sin_cos();
            for (axis_index, value) in [(index, x), (index + 1, -y)] {
                let frame = engine.process(&axis(ms, axis_index, (value * 32767.).round() as i16));
                let pad = frame.pad.unwrap();
                let physical_stick = if stick == Stick::Left {
                    (pad.left_x, pad.left_y)
                } else {
                    (pad.right_x, pad.right_y)
                };
                assert_eq!(physical_stick, (0, 0));
                assert!(!frame.held.contains("key:W"));
            }
            for _ in 0..12 {
                ms += 2;
                mouse_x += engine.process(&sample(ms, [0.; 3])).mouse_delta[0];
                assert_eq!(engine.advance(0.002, time(ms).host_ns).mouse_delta, [0.; 2]);
            }
        }
        for _ in 0..100 {
            ms += 2;
            mouse_x += engine.process(&sample(ms, [0.; 3])).mouse_delta[0];
        }
        let result = engine.end_measurement().unwrap();
        assert_eq!(result.output, output);
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!((result.degrees - 360.).abs() < 0.1, "{result:?}");
        if output == GyroOutput::Mouse {
            assert!((mouse_x - 3600.).abs() < 1.);
            assert_eq!(
                engine.advance(0.002, time(ms + 2).host_ns).mouse_delta[0],
                0.
            );
        } else {
            assert_eq!(mouse_x, 0.);
            assert!(result.peak > 0. && result.peak < 1.);
        }
        assert!(engine.end_measurement().is_none());
    }
}

#[test]
fn constant_gyro_samples_are_each_integrated_once_and_button_events_add_no_motion() {
    let mut engine = Engine::new(profile()).unwrap();
    for ms in 1..=1001 {
        engine.process(&sample(ms, [0.0, -90.0, 0.0]));
        assert_eq!(
            engine.process(&button(0, ms % 2 == 0)).mouse_delta,
            [0.0, 0.0]
        );
    }
    assert!(
        (engine.mouse_total[0] - 900.0).abs() < 0.01,
        "{:?}",
        engine.mouse_total
    );
    assert_eq!(engine.gyro_events, 1001);
    assert_eq!(engine.clock.stats.integrated, 1000);
}

#[test]
fn sensor_clock_replay_does_not_depend_on_host_batching() {
    let mut regular = Engine::new(profile()).unwrap();
    let mut batched = Engine::new(profile()).unwrap();
    for ms in 1..=1001 {
        let mut e = sample(ms, [20.0, -25.0, 7.0]);
        regular.process(&e);
        if let InputEvent::Gyro { time, .. } = &mut e {
            time.host_ns = (ms / 8 + 1) * 8_000_000;
        }
        batched.process(&e);
    }
    assert_eq!(regular.mouse_total, batched.mouse_total);
    assert!(regular.mouse_total[0] > 0.0 && regular.mouse_total[1] < 0.0);
}

#[test]
fn zero_backward_and_long_gap_timestamps_do_not_make_a_jump() {
    let mut e = Engine::new(profile()).unwrap();
    for ms in [1, 2, 2, 1, 0, 3, 1000] {
        e.process(&sample(ms, [0.0, -100.0, 0.0]));
    }
    assert!((e.mouse_total[0] - 1.0).abs() < 0.001);
    assert_eq!(e.clock.stats.invalid_timestamps, 2);
    assert_eq!(e.clock.stats.gaps, 1);
    assert_eq!(
        e.process(&sample(1001, [0.0, -100.0, 0.0])).mouse_delta[0].round(),
        1.0
    );
}

#[test]
fn backwards_time_never_integrates_the_same_interval_twice() {
    let mut e = Engine::new(profile()).unwrap();
    for ms in [1, 2, 1, 2, 3] {
        e.process(&sample(ms, [0.0, -100.0, 0.0]));
    }
    assert!((e.mouse_total[0] - 2.0).abs() < 0.001);
}

#[test]
fn stationary_bias_is_device_local_and_removes_only_the_offset() {
    let mut e = Engine::new(profile()).unwrap();
    e.bias_rad_s = [0.01, -0.02, 0.03];
    for ms in 1..=10 {
        assert_eq!(
            e.process(&InputEvent::Gyro {
                time: time(ms),
                value: e.bias_rad_s
            })
            .mouse_delta,
            [0.0; 2]
        );
    }
    assert_eq!(e.mouse_total, [0.0; 2]);
}

#[test]
fn activation_does_not_replay_motion_before_the_trigger_press() {
    let mut p = profile();
    p.gyro.activation = "left_trigger".into();
    let mut e = Engine::new(p).unwrap();
    for ms in 1..=10 {
        e.process(&sample(ms, [0.0, -100.0, 0.0]));
    }
    assert_eq!(e.mouse_total, [0.0; 2]);
    e.process(&InputEvent::Axis {
        time: time(11),
        index: 4,
        value: 32767,
    });
    assert_eq!(
        e.process(&sample(11, [0.0, -100.0, 0.0])).mouse_delta,
        [0.0; 2]
    );
    assert!(e.process(&sample(12, [0.0, -100.0, 0.0])).mouse_delta[0] > 0.0);
    e.process(&InputEvent::Axis {
        time: time(13),
        index: 4,
        value: 0,
    });
    assert_eq!(
        e.process(&sample(14, [0.0, -100.0, 0.0])).mouse_delta,
        [0.0; 2]
    );
}

#[test]
fn toggle_uses_real_press_edges_and_pause_preserves_the_latch() {
    let mut p = profile();
    p.gyro.activation = "left_trigger".into();
    p.gyro.activation_mode = GyroActivationMode::Toggle;
    p.gyro.activation_threshold = 0.5;
    p.gyro.pause_input = Some("east".into());
    let mut e = Engine::new(p).unwrap();
    let moving = |ms| sample(ms, [0.0, -90.0, 0.0]);
    e.process_snapshot(&axis(1, 4, 20000)); // The trigger is held while attaching.
    e.process(&axis(2, 4, 20001));
    e.process(&moving(3));
    assert_eq!(e.process(&moving(4)).mouse_delta, [0.0; 2]);
    e.process(&axis(5, 4, 0));
    e.process(&axis(6, 4, 20000)); // A fresh press turns gyro on.
    assert_eq!(e.process(&moving(7)).mouse_delta, [0.0; 2]);
    assert!(e.process(&moving(8)).mouse_delta[0] > 0.0);
    e.process(&axis(9, 4, 16000)); // Below press, above release.
    e.process(&axis(10, 4, 21000));
    assert!(e.process(&moving(11)).mouse_delta[0] > 0.0);
    e.process(&button(1, true));
    assert_eq!(e.process(&moving(12)).mouse_delta, [0.0; 2]);
    e.process(&button(1, false));
    assert_eq!(e.process(&moving(13)).mouse_delta, [0.0; 2]);
    assert!(e.process(&moving(14)).mouse_delta[0] > 0.0);
    e.process(&axis(15, 4, 0));
    e.process(&axis(16, 4, 21000));
    assert_eq!(e.process(&moving(17)).mouse_delta, [0.0; 2]);
    assert_eq!(e.process(&moving(18)).mouse_delta, [0.0; 2]);
}

#[test]
fn rws_gyro_right_stick_adds_to_physical_axes_and_releases_on_silence() {
    for target in [GamepadTarget::Xbox360, GamepadTarget::DualShock4] {
        let mut p = profile();
        p.virtual_gamepad = target;
        p.gyro.output = GyroOutput::RightStick;
        p.gyro.pause_input = Some("east".into());
        let mut e = Engine::new(p).unwrap();
        let moving = |ms| sample(ms, [90.0, -90.0, 0.0]);
        e.process(&axis(1, 2, 16384));
        assert_eq!(e.process(&moving(2)).pad.unwrap().right_x, 16384);
        let output = e.process(&moving(3));
        assert_eq!(output.mouse_delta, [0.0; 2]);
        let pad = output.pad.unwrap();
        assert!((pad.right_x - 24576).abs() <= 1, "{pad:?}");
        assert!((pad.right_y - 8192).abs() <= 1, "{pad:?}");
        assert_eq!(
            e.process(&button(0, true)).pad.unwrap().right_x,
            pad.right_x
        );
        assert_eq!(
            e.advance(0.004, time(7).host_ns).pad.unwrap().right_x,
            pad.right_x
        );
        assert_eq!(
            e.advance(0.046, time(53).host_ns).pad.unwrap().right_x,
            16384
        );
        assert_eq!(e.frame.pad.unwrap().right_y, 0);
        assert_eq!(e.process(&moving(54)).pad.unwrap().right_x, 16384);
        assert_eq!(e.process(&moving(55)).pad.unwrap().right_x, pad.right_x);
        assert_eq!(e.process(&button(1, true)).pad.unwrap().right_x, 16384);
        assert_eq!(e.process(&moving(56)).pad.unwrap().right_y, 0);
        e.process(&button(1, false));
        assert_eq!(e.process(&moving(57)).pad.unwrap().right_y, 0);
        assert_eq!(e.process(&moving(58)).pad.unwrap().right_y, pad.right_y);
        assert_eq!(e.process(&moving(58)).pad.unwrap().right_y, 0);
        e.process(&InputEvent::Disconnected { time: time(59) });
        assert_eq!(e.frame.pad, Some(PadState::default()));
    }
}

#[test]
fn speed_ramp_leaves_slow_aim_linear_and_caps_the_fast_gain() {
    let mut p = profile();
    p.gyro.acceleration.fast_multiplier = 3.0;
    p.gyro.acceleration.start_dps = 40.0;
    p.gyro.acceleration.full_dps = 160.0;
    let mut e = Engine::new(p).unwrap();
    for (speed, multiplier) in [(20.0, 1.0), (100.0, 2.0), (200.0, 3.0)] {
        e.reset();
        e.process(&sample(1, [0.0, -speed, 0.0]));
        let delta = e.process(&sample(2, [0.0, -speed, 0.0])).mouse_delta[0];
        let expected = f64::from(speed * multiplier) * 0.01;
        assert!((delta - expected).abs() < 0.0001);
    }
}

#[test]
fn two_bindings_to_one_key_hold_until_both_release() {
    let mut p = profile();
    p.actions = vec![binding("south", "key:SPACE"), binding("east", "key:SPACE")];
    let mut e = Engine::new(p).unwrap();
    assert!(e.process(&button(0, true)).held.contains("key:SPACE"));
    e.process(&button(1, true));
    assert!(e.process(&button(0, false)).held.contains("key:SPACE"));
    assert!(e.process(&button(1, false)).held.is_empty());
}

#[test]
fn full_native_automap_preserves_buttons_trigger_and_stick_axes() {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    let mut e = Engine::new(p).unwrap();
    for (index, bit) in [
        (0, 0x1000),
        (1, 0x2000),
        (2, 0x4000),
        (3, 0x8000),
        (4, 0x20),
        (5, 0x400),
        (6, 0x10),
        (7, 0x40),
        (8, 0x80),
        (9, 0x100),
        (10, 0x200),
        (11, 1),
        (12, 2),
        (13, 4),
        (14, 8),
    ] {
        assert_ne!(
            e.process(&button(index, true)).pad.unwrap().buttons & bit,
            0,
            "button {index}"
        );
        assert_eq!(e.process(&button(index, false)).pad.unwrap().buttons, 0);
    }
    for (index, value) in [
        (0, 32767),
        (1, -32768),
        (2, -32768),
        (3, 32767),
        (4, 32767),
        (5, 32767),
    ] {
        e.process(&InputEvent::Axis {
            time: time(1),
            index,
            value,
        });
    }
    assert_eq!(
        e.frame.pad.unwrap(),
        PadState {
            left_x: 32767,
            left_y: 32767,
            right_x: -32768,
            right_y: -32768,
            left_trigger: 255,
            right_trigger: 255,
            ..Default::default()
        }
    );
    e.process(&InputEvent::Disconnected { time: time(2) });
    assert_eq!(e.frame.pad.unwrap(), PadState::default());
}

#[test]
fn touchpad_click_routes_to_dualshock_but_not_xbox() {
    for (target, has_click) in [
        (GamepadTarget::DualShock4, true),
        (GamepadTarget::Xbox360, false),
    ] {
        let mut p = profile();
        p.virtual_gamepad = target;
        let mut e = Engine::new(p).unwrap();
        assert_eq!(
            e.process(&button(20, true)).pad.unwrap().touchpad_click,
            has_click
        );
        assert!(!e.process(&button(20, false)).pad.unwrap().touchpad_click);
    }
}

#[test]
fn stick_to_wasd_releases_on_center_and_disconnect() {
    let mut p = profile();
    p.actions = vec![binding("left_stick_up", "key:W")];
    let mut e = Engine::new(p).unwrap();
    assert!(e
        .process(&InputEvent::Axis {
            time: time(1),
            index: 1,
            value: -32768
        })
        .held
        .contains("key:W"));
    assert!(e
        .process(&InputEvent::Axis {
            time: time(2),
            index: 1,
            value: 0
        })
        .held
        .is_empty());
    e.process(&InputEvent::Axis {
        time: time(3),
        index: 1,
        value: -32768,
    });
    assert!(e
        .process(&InputEvent::Disconnected { time: time(4) })
        .held
        .is_empty());
}

#[derive(Default)]
struct Seen {
    edges: Vec<(String, bool)>,
    motion_at_edge: Vec<[i64; 2]>,
    motion: [i64; 2],
    pad: Option<PadState>,
}
struct Spy(Rc<RefCell<Seen>>);
impl Sink for Spy {
    fn button(&mut self, name: &str, down: bool) -> Result<()> {
        let mut seen = self.0.borrow_mut();
        let motion = seen.motion;
        seen.motion_at_edge.push(motion);
        seen.edges.push((name.into(), down));
        Ok(())
    }
    fn mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.0.borrow_mut().motion[0] += dx as i64;
        self.0.borrow_mut().motion[1] += dy as i64;
        Ok(())
    }
    fn gamepad(&mut self, p: PadState) -> Result<()> {
        self.0.borrow_mut().pad = Some(p);
        Ok(())
    }
}
#[test]
fn scheduled_mouse_drains_once_and_motion_precedes_a_discrete_edge() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut output = Dispatcher::new(Spy(seen.clone())).with_scheduled_output();
    let small = Frame {
        mouse_delta: [0.625, -0.625],
        ..Frame::default()
    };
    output.apply(&small).unwrap();
    output.apply(&small).unwrap();
    assert_eq!(seen.borrow().motion, [0, 0]);
    output.flush_output().unwrap();
    output.flush_output().unwrap();
    assert_eq!(seen.borrow().motion, [1, -1]);

    let movement = Frame {
        mouse_delta: [2.0, -2.0],
        ..Frame::default()
    };
    output.apply(&movement).unwrap();
    let mut pressed = Frame {
        held: BTreeSet::from(["key:W".into()]),
        ..Frame::default()
    };
    output.apply(&pressed).unwrap();
    assert_eq!(seen.borrow().motion_at_edge, [[3, -3]]);
    pressed.mouse_delta = [6.0, -6.0];
    output.apply(&pressed).unwrap();
    output.reset().unwrap();
    output.flush_output().unwrap();
    assert_eq!(seen.borrow().motion, [3, -3]);
}

#[test]
fn scheduled_gamepad_uses_the_latest_axes_and_preserves_short_button_edges() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut output = Dispatcher::new(Spy(seen.clone())).with_scheduled_output();
    let frame = |right_x, buttons| Frame {
        pad: Some(PadState {
            right_x,
            buttons,
            ..Default::default()
        }),
        ..Default::default()
    };
    output.apply(&frame(100, 0)).unwrap();
    output.apply(&frame(200, 0)).unwrap();
    assert_eq!(seen.borrow().pad, None);
    output.flush_output().unwrap();
    assert_eq!(seen.borrow().pad, frame(200, 0).pad);
    assert_eq!(output.pad_reports, 1);

    output.apply(&frame(200, 0x1000)).unwrap();
    assert_eq!(seen.borrow().pad, frame(200, 0x1000).pad);
    output.apply(&frame(200, 0)).unwrap();
    assert_eq!(seen.borrow().pad, frame(200, 0).pad);
    output.apply(&frame(300, 0)).unwrap();
    output.reset().unwrap();
    output.flush_output().unwrap();
    assert_eq!(seen.borrow().pad, Some(PadState::default()));
    assert_eq!(output.pad_reports, 4);
}

#[test]
fn tiny_motion_is_not_lost_and_drop_releases_held_keys_and_pad() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    {
        let mut d = Dispatcher::new(Spy(seen.clone()));
        let frame = Frame {
            held: BTreeSet::from(["key:W".into()]),
            mouse_delta: [0.125, -0.125],
            scroll: [0; 2],
            pad: Some(PadState {
                buttons: 4096,
                ..Default::default()
            }),
        };
        for _ in 0..800 {
            d.apply(&frame).unwrap();
        }
        assert_eq!(d.mouse_counts, [100, -100]);
        assert_eq!(d.preview().mouse.last().unwrap().position, [100, -100]);
        assert_eq!(d.preview().pad, frame.pad);
        let generation = d.preview().generation;
        d.reset().unwrap();
        assert!(d.preview().mouse.is_empty());
        assert_ne!(d.preview().generation, generation);
    }
    let seen = seen.borrow();
    assert_eq!(
        seen.edges,
        [("key:W".into(), true), ("key:W".into(), false)]
    );
    assert_eq!(seen.motion, [100, -100]);
    assert_eq!(seen.pad, Some(PadState::default()));
}

#[test]
fn malformed_config_and_nonfinite_samples_fail_closed() {
    let mut p = profile();
    p.gyro.sensitivity = f32::NAN;
    assert!(Engine::new(p).is_err());
    let mut p = profile();
    p.actions = vec![binding("south", "key:TYPO")];
    assert!(Engine::new(p).is_err());
    let mut e = Engine::new(profile()).unwrap();
    e.process(&sample(1, [0.0, -10.0, 0.0]));
    assert_eq!(
        e.process(&sample(2, [f32::NAN, 0.0, 0.0])).mouse_delta,
        [0.0; 2]
    );
    assert_eq!(e.invalid_values, 1);
}

#[test]
fn a_failed_release_does_not_prevent_releasing_other_keys_and_is_retried_on_drop() {
    struct FailingSink {
        seen: Rc<RefCell<Seen>>,
        failed: bool,
    }
    impl Sink for FailingSink {
        fn button(&mut self, name: &str, down: bool) -> Result<()> {
            if !down && name == "key:A" && !self.failed {
                self.failed = true;
                anyhow::bail!("simulated one-time OS release error");
            }
            self.seen.borrow_mut().edges.push((name.into(), down));
            Ok(())
        }
        fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
            Ok(())
        }
        fn gamepad(&mut self, _: PadState) -> Result<()> {
            Ok(())
        }
    }
    let seen = Rc::new(RefCell::new(Seen::default()));
    {
        let mut d = Dispatcher::new(FailingSink {
            seen: seen.clone(),
            failed: false,
        });
        d.apply(&Frame {
            held: BTreeSet::from(["key:A".into(), "key:W".into()]),
            ..Default::default()
        })
        .unwrap();
        assert!(d.reset().is_err());
        assert!(seen.borrow().edges.contains(&("key:W".into(), false)));
    }
    assert!(seen.borrow().edges.contains(&("key:A".into(), false)));
}

#[test]
fn failed_first_pad_write_still_attempts_to_send_neutral() {
    struct FirstWriteFails(Rc<RefCell<Vec<PadState>>>);
    impl Sink for FirstWriteFails {
        fn button(&mut self, _: &str, _: bool) -> Result<()> {
            Ok(())
        }
        fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
            Ok(())
        }
        fn gamepad(&mut self, pad: PadState) -> Result<()> {
            let mut reports = self.0.borrow_mut();
            reports.push(pad);
            anyhow::ensure!(reports.len() > 1, "simulated first report failure");
            Ok(())
        }
    }
    let reports = Rc::new(RefCell::new(Vec::new()));
    let pad = PadState {
        right_x: 25000,
        ..Default::default()
    };
    {
        let mut dispatch = Dispatcher::new(FirstWriteFails(reports.clone()));
        assert!(dispatch
            .apply(&Frame {
                pad: Some(pad),
                ..Default::default()
            })
            .is_err());
    }
    assert_eq!(*reports.borrow(), [pad, PadState::default()]);
}
