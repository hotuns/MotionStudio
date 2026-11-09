//! Lifecycle routing only; native devices retain their report and transport logic.
use super::*;

pub struct SplitSink<K, P> {
    keys: K,
    pad: P,
}
impl<K, P> SplitSink<K, P> {
    pub fn new(keys: K, pad: P) -> Self {
        Self { keys, pad }
    }
}
impl<K: Sink, P: Sink> Sink for SplitSink<K, P> {
    fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
        self.pad.feedback()
    }
    fn gamepad_status(&self) -> Option<GamepadStatus> {
        self.pad.gamepad_status()
    }
    fn protected_devices(&self) -> Vec<String> {
        let mut ids = self.keys.protected_devices();
        for id in self.pad.protected_devices() {
            if !ids.iter().any(|old| old.eq_ignore_ascii_case(&id)) {
                ids.push(id);
            }
        }
        ids
    }
    fn poll(&mut self) -> Result<()> {
        let keys = self.keys.poll();
        let pad = self.pad.poll();
        keys.and(pad)
    }
    fn cancel_pending(&mut self) -> Result<()> {
        let keys = self.keys.cancel_pending();
        let pad = self.pad.cancel_pending();
        keys.and(pad)
    }
    fn stats(&self) -> OutputStats {
        self.keys.stats()
    }
    fn mouse_trace(&self) -> Option<Vec<MouseTracePoint>> {
        self.keys.mouse_trace()
    }
    fn button(&mut self, name: &str, down: bool) -> Result<()> {
        self.keys.button(name, down)
    }
    fn mouse(&mut self, x: i32, y: i32) -> Result<()> {
        self.keys.mouse(x, y)
    }
    fn scroll(&mut self, h: i32, v: i32) -> Result<()> {
        self.keys.scroll(h, v)
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()> {
        self.pad.gamepad(pad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Fake {
        kind: &'static str,
        events: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }
    impl Sink for Fake {
        fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
            Some(crate::feedback::FeedbackFrame {
                sequence: 17,
                rumble: [0.5, 0.25],
                lightbar: None,
            })
        }
        fn protected_devices(&self) -> Vec<String> {
            vec![self.kind.into()]
        }
        fn poll(&mut self) -> Result<()> {
            self.events
                .lock()
                .unwrap()
                .push(format!("{}:poll", self.kind));
            if self.fail {
                bail!("failure")
            }
            Ok(())
        }
        fn button(&mut self, _: &str, _: bool) -> Result<()> {
            self.events
                .lock()
                .unwrap()
                .push(format!("{}:key", self.kind));
            Ok(())
        }
        fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
            self.events
                .lock()
                .unwrap()
                .push(format!("{}:mouse", self.kind));
            Ok(())
        }
        fn scroll(&mut self, _: i32, _: i32) -> Result<()> {
            self.events
                .lock()
                .unwrap()
                .push(format!("{}:wheel", self.kind));
            Ok(())
        }
        fn gamepad(&mut self, _: PadState) -> Result<()> {
            self.events
                .lock()
                .unwrap()
                .push(format!("{}:pad", self.kind));
            Ok(())
        }
        fn gamepad_status(&self) -> Option<GamepadStatus> {
            Some(GamepadStatus {
                target: crate::profile::GamepadTarget::Xbox360,
                backend: GamepadBackend::Vigem,
                connected: true,
                player_index: Some(2),
                protected_devices: self.protected_devices(),
            })
        }
    }
    #[test]
    fn split_routes_reports_and_forwards_actual_pad_through_both_wrappers() {
        let events = Arc::new(Mutex::new(vec![]));
        let mut split = SplitSink::new(
            Fake {
                kind: "hid",
                events: events.clone(),
                fail: true,
            },
            Fake {
                kind: "pad",
                events: events.clone(),
                fail: false,
            },
        );
        split.button("key:A", true).unwrap();
        split.mouse(1, 2).unwrap();
        split.scroll(0, 1).unwrap();
        split.gamepad(PadState::default()).unwrap();
        assert!(split.poll().is_err());
        assert_eq!(
            *events.lock().unwrap(),
            [
                "hid:key",
                "hid:mouse",
                "hid:wheel",
                "pad:pad",
                "hid:poll",
                "pad:poll"
            ]
        );
        let mut wrapped = crate::flex_output::FlexVirtualSink::new(Box::new(split));
        use flexinput_virtual::VirtualDevice;
        assert!(wrapped
            .source_pins()
            .iter()
            .any(|pin| pin.id == "rumble_strong"));
        assert_eq!(wrapped.poll_outputs()[0].1.as_float(), 0.5);
        assert_eq!(wrapped.feedback_sequence(), 17);
        let mut dispatcher = Dispatcher::new(Box::new(wrapped));
        assert_eq!(dispatcher.feedback().unwrap().sequence, 17);
        assert_eq!(dispatcher.gamepad_status().unwrap().player_index, Some(2));
        assert_eq!(dispatcher.protected_devices(), ["hid", "pad"]);
    }
}
