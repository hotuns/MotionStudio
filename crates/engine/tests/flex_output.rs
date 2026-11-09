use anyhow::{bail, Result};
use flexinput_core::Signal;
use flexinput_virtual::VirtualDevice;
use motion_core::{
    engine::{Frame, PadState},
    flex_output::FlexVirtualSink,
    output::{Dispatcher, Sink},
};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Button(String, bool),
    Mouse(i32, i32),
    Scroll(i32, i32),
    Pad(PadState),
    Cancel,
}
#[derive(Clone, Default)]
struct Mock {
    calls: Arc<Mutex<Vec<Call>>>,
    fail_poll: Arc<AtomicBool>,
}
impl Mock {
    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }
}
impl Sink for Mock {
    fn poll(&mut self) -> Result<()> {
        if self.fail_poll.load(Ordering::Relaxed) {
            bail!("async failure")
        }
        Ok(())
    }
    fn cancel_pending(&mut self) -> Result<()> {
        self.record(Call::Cancel);
        Ok(())
    }
    fn button(&mut self, key: &str, down: bool) -> Result<()> {
        self.record(Call::Button(key.into(), down));
        Ok(())
    }
    fn mouse(&mut self, x: i32, y: i32) -> Result<()> {
        self.record(Call::Mouse(x, y));
        Ok(())
    }
    fn scroll(&mut self, x: i32, y: i32) -> Result<()> {
        self.record(Call::Scroll(x, y));
        Ok(())
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()> {
        self.record(Call::Pad(pad));
        Ok(())
    }
}

#[test]
fn flex_pins_preserve_chords_side_buttons_wheel_direction_and_pad_bits() {
    let mock = Mock::default();
    let mut out = Dispatcher::new(FlexVirtualSink::new(mock.clone()));
    let pad = PadState {
        buttons: 0xf7ff,
        touchpad_click: true,
        left_trigger_pressed: false,
        right_trigger_pressed: true,
        left_trigger: 1,
        right_trigger: 254,
        left_x: -32768,
        left_y: 32767,
        right_x: -1,
        right_y: 1,
    };
    out.apply(&Frame {
        held: BTreeSet::from(["key:CTRL".into(), "mouse:x1".into(), "mouse:x2".into()]),
        mouse_delta: [999_999., -999_999.],
        scroll: [-2, 3],
        pad: Some(pad),
    })
    .unwrap();
    out.reset().unwrap();
    let calls = mock.calls.lock().unwrap();
    let position = |c: Call| calls.iter().position(|x| x == &c).unwrap();
    assert!(position(Call::Button("key:CTRL".into(), true)) < position(Call::Scroll(-2, 3)));
    assert!(position(Call::Scroll(-2, 3)) < position(Call::Button("key:CTRL".into(), false)));
    assert!(calls.contains(&Call::Button("mouse:x1".into(), true)));
    assert!(calls.contains(&Call::Button("mouse:x2".into(), false)));
    assert!(calls.contains(&Call::Mouse(999_999, -999_999)));
    assert!(calls.contains(&Call::Pad(pad)));
    assert_eq!(calls.last(), Some(&Call::Pad(PadState::default())));
}

#[test]
fn flex_contract_keeps_fractional_displacement_exactly_once() {
    let mock = Mock::default();
    let mut out = Dispatcher::new(FlexVirtualSink::new(mock.clone()));
    let frame = Frame {
        mouse_delta: [0.251, -0.251],
        ..Default::default()
    };
    for _ in 0..1000 {
        out.apply(&frame).unwrap();
    }
    out.reset().unwrap();
    let total = mock
        .calls
        .lock()
        .unwrap()
        .iter()
        .fold([0; 2], |mut sum, call| {
            if let Call::Mouse(x, y) = call {
                sum[0] += x;
                sum[1] += y;
            }
            sum
        });
    assert_eq!(total, [251, -251]);
}

#[test]
fn async_failure_still_cancels_motion_releases_keys_and_neutralizes_pad() {
    let mock = Mock::default();
    let mut out = Dispatcher::new(FlexVirtualSink::new(mock.clone()));
    out.apply(&Frame {
        held: BTreeSet::from(["key:W".into(), "key:CTRL".into()]),
        pad: Some(PadState {
            buttons: 4096,
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();
    mock.fail_poll.store(true, Ordering::Relaxed);
    assert!(out.apply(&Frame::default()).is_err());
    assert!(out.reset().is_err());
    let calls = mock.calls.lock().unwrap();
    let tail = &calls[calls.iter().position(|c| *c == Call::Cancel).unwrap()..];
    assert_eq!(
        tail,
        [
            Call::Cancel,
            Call::Button("key:W".into(), false),
            Call::Button("key:CTRL".into(), false),
            Call::Pad(PadState::default())
        ]
    );
}

#[test]
fn sdk_reset_discards_unsent_scroll_and_releases_all_held_outputs() {
    let mock = Mock::default();
    let mut device = FlexVirtualSink::new(mock.clone());
    device.send("key_CTRL", Signal::Bool(true));
    device.send("mouse_back", Signal::Bool(true));
    device.send("scroll_up", Signal::Float(4.));
    device.reset_outputs();
    device.flush();
    let calls = mock.calls.lock().unwrap();
    assert!(!calls.iter().any(|c| matches!(c, Call::Scroll(..))));
    assert!(calls.contains(&Call::Button("key:CTRL".into(), false)));
    assert!(calls.contains(&Call::Button("mouse:x1".into(), false)));
    assert!(calls.contains(&Call::Pad(PadState::default())));
    assert!(!device
        .sink_pins()
        .iter()
        .any(|p| matches!(p.id, "gyro_x" | "trackpad_scroll_y")));
}
