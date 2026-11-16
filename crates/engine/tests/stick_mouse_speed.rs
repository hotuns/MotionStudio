use motion_core::{
    engine::{Engine, InputEvent, PadState},
    output::{Dispatcher, Sink},
    storage::parse_profile,
    timing::SampleTime,
};
use std::{cell::Cell, rc::Rc};

struct Counts(Rc<Cell<[i64; 2]>>);
impl Sink for Counts {
    fn button(&mut self, _: &str, _: bool) -> anyhow::Result<()> {
        Ok(())
    }
    fn mouse(&mut self, dx: i32, dy: i32) -> anyhow::Result<()> {
        let [x, y] = self.0.get();
        self.0.set([x + i64::from(dx), y + i64::from(dy)]);
        Ok(())
    }
    fn gamepad(&mut self, _: PadState) -> anyhow::Result<()> {
        Ok(())
    }
}

fn one_second(speed: f32, exponent: f32, deflection: f32, hz: u64) -> i64 {
    let mut profile = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    profile.stick_mouse.enabled = true;
    profile.stick_mouse.speed = speed;
    profile.stick_mouse.exponent = exponent;
    profile.input.deadzone_right = Some(0.03);
    let mut engine = Engine::new(profile).unwrap();
    engine.process(&InputEvent::Axis {
        time: SampleTime {
            host_ns: 1,
            sensor_ns: 0,
        },
        index: 2,
        value: (deflection * 32767.).round() as i16,
    });
    let counts = Rc::new(Cell::new([0; 2]));
    let mut dispatcher = Dispatcher::new(Counts(counts.clone())).with_scheduled_output();
    for tick in 1..=hz {
        let frame = engine.advance_output(1. / hz as f32, 1 + tick * 1_000_000_000 / hz);
        dispatcher.apply(frame).unwrap();
        dispatcher.flush_output().unwrap();
    }
    assert_eq!(counts.get(), dispatcher.mouse_counts);
    assert_eq!(counts.get()[1], 0);
    counts.get()[0]
}

#[test]
fn full_stick_speed_reaches_the_configured_counts_per_second_at_each_output_rate() {
    for speed in [1000., 4000., 10000.] {
        for hz in [60, 125, 250, 500, 1000, 2000] {
            let total = one_second(speed, 4., 1., hz);
            assert!(
                (total - speed as i64).abs() <= 1,
                "speed={speed} hz={hz}: {total}"
            );
        }
    }
}

#[test]
fn larger_response_exponent_slows_partial_pushes_but_keeps_full_stick_speed() {
    for deflection in [0.25, 0.5, 0.75, 1.] {
        let linear = one_second(4000., 1., deflection, 1000);
        let curved = one_second(4000., 4., deflection, 1000);
        println!("raw={deflection:.2} linear={linear} exponent4={curved} counts/s");
        if deflection < 1. {
            assert!(curved < linear);
        } else {
            assert_eq!(curved, linear);
            assert!((curved - 4000).abs() <= 1);
        }
        // High output rates must preserve sub-count motion through the dispatcher.
        let slower_rate = one_second(4000., 4., deflection, 125);
        assert!((curved - slower_rate).abs() <= 1);
        if deflection == 0.5 {
            assert!((1900..2000).contains(&linear));
            assert!((200..240).contains(&curved));
        }
    }
}
