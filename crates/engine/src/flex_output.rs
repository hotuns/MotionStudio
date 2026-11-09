//! FlexInput's VirtualDevice/pin contract over Studio-owned output transports.
//! SendInput compatibility and HID lifetime ownership remain explicit adapters.
use crate::{
    engine::PadState,
    output::{MouseTracePoint, OutputStats, Sink},
};
use anyhow::{bail, Result};
use flexinput_core::Signal;
use flexinput_virtual::{layouts, SinkPin, VirtualDevice};
use std::{collections::BTreeSet, sync::OnceLock};

pub struct FlexVirtualSink<S: Sink + Send> {
    inner: S,
    pad: PadState,
    pad_dirty: bool,
    scroll: [i32; 2],
    held: BTreeSet<String>,
    error: Option<anyhow::Error>,
    feedback_sequence: u64,
}
impl<S: Sink + Send> FlexVirtualSink<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            pad: PadState::default(),
            pad_dirty: false,
            scroll: [0; 2],
            held: BTreeSet::new(),
            error: None,
            feedback_sequence: 0,
        }
    }
    fn record(&mut self, result: Result<()>) {
        if let Err(error) = result {
            if self.error.is_none() {
                self.error = Some(error);
            }
        }
    }
    fn checked(&mut self) -> Result<()> {
        self.error.take().map_or(Ok(()), Err)
    }
    fn submit(&mut self, pin: &str, signal: Signal) -> Result<()> {
        VirtualDevice::send(self, pin, signal);
        self.checked()
    }
}

fn combined_pins() -> &'static [SinkPin] {
    static PINS: OnceLock<Vec<SinkPin>> = OnceLock::new();
    PINS.get_or_init(|| {
        let mut seen = BTreeSet::new();
        layouts::KEYMOUSE_DEFAULT_PINS
            .iter()
            .chain(layouts::DS4_SINK_PINS)
            .filter(|p| supported_pin(p.id) && seen.insert(p.id))
            .map(|p| SinkPin {
                id: p.id,
                display_name: p.display_name,
                signal_type: p.signal_type,
            })
            .collect()
    })
}
fn pad_pin(pin: &str) -> bool {
    PAD_BUTTONS.iter().any(|p| p.0 == pin)
        || matches!(
            pin,
            "btn_touchpad"
                | "btn_lt_dig"
                | "btn_rt_dig"
                | "left_stick_x"
                | "left_stick_y"
                | "right_stick_x"
                | "right_stick_y"
                | "left_trigger"
                | "right_trigger"
        )
}
fn supported_pin(pin: &str) -> bool {
    pin.starts_with("key_")
        || pad_pin(pin)
        || matches!(
            pin,
            "mouse_move"
                | "mouse_left"
                | "mouse_right"
                | "mouse_middle"
                | "mouse_back"
                | "mouse_forward"
                | "scroll_up"
                | "scroll_down"
                | "scroll_left"
                | "scroll_right"
        )
}

pub(crate) const PAD_BUTTONS: &[(&str, u16)] = &[
    ("dpad_up", 1),
    ("dpad_down", 2),
    ("dpad_left", 4),
    ("dpad_right", 8),
    ("btn_start", 0x10),
    ("btn_back", 0x20),
    ("btn_ls", 0x40),
    ("btn_rs", 0x80),
    ("btn_lb", 0x100),
    ("btn_rb", 0x200),
    ("btn_guide", 0x400),
    ("btn_south", 0x1000),
    ("btn_east", 0x2000),
    ("btn_west", 0x4000),
    ("btn_north", 0x8000),
];

impl<S: Sink + Send> VirtualDevice for FlexVirtualSink<S> {
    fn source_pins(&self) -> &'static [flexinput_virtual::SourcePin] {
        use crate::profile::GamepadTarget;
        match self.inner.gamepad_status().map(|pad| pad.target) {
            Some(GamepadTarget::Xbox360) => layouts::XINPUT_SOURCE_PINS,
            Some(GamepadTarget::DualShock4) => layouts::DS4_SOURCE_PINS,
            _ => &[],
        }
    }
    fn feedback_sequence(&self) -> u64 {
        self.feedback_sequence
    }
    fn poll_outputs(&mut self) -> Vec<(&'static str, Signal)> {
        let Some(frame) = <Self as Sink>::feedback(self) else {
            return vec![];
        };
        let mut pins = vec![
            ("rumble_strong", Signal::Float(frame.rumble[0])),
            ("rumble_weak", Signal::Float(frame.rumble[1])),
        ];
        if let Some(colour) = frame.lightbar {
            for (pin, value) in ["lightbar_r", "lightbar_g", "lightbar_b"]
                .into_iter()
                .zip(colour)
            {
                pins.push((pin, Signal::Float(value)));
            }
        }
        pins
    }
    fn id(&self) -> &str {
        "studio.output"
    }
    fn display_name(&self) -> &str {
        "Motion Studio output"
    }
    fn sink_pins(&self) -> &'static [SinkPin] {
        combined_pins()
    }
    fn send(&mut self, pin: &str, value: Signal) {
        if matches!(value, Signal::Float(f) if !f.is_finite())
            || matches!(value, Signal::Vec2(v) if !v.is_finite())
        {
            self.record(bail_result("Non-finite FlexInput output signal"));
            return;
        }
        let result = match pin {
            "mouse_move" => {
                if let Signal::Vec2(v) = value {
                    // Upstream displacement pins use +Y up; transports use screen Y.
                    if v.abs().max_element() > 1_000_000. {
                        bail_result("Oversized mouse displacement")
                    } else {
                        self.inner.mouse(v.x.round() as i32, -v.y.round() as i32)
                    }
                } else {
                    bail_result("Invalid mouse displacement signal")
                }
            }
            "scroll_right" | "scroll_left" | "scroll_up" | "scroll_down" => {
                let n = flexinput_virtual::scroll_notches(value);
                let axis = usize::from(pin == "scroll_up" || pin == "scroll_down");
                let sign = if pin == "scroll_left" || pin == "scroll_down" {
                    -1
                } else {
                    1
                };
                match self.scroll[axis].checked_add(n.saturating_mul(sign)) {
                    Some(count) => {
                        self.scroll[axis] = count;
                        Ok(())
                    }
                    None => bail_result("Oversized scroll output"),
                }
            }
            _ if pin.starts_with("key_") || pin.starts_with("mouse_") => {
                let name = if let Some(key) = pin.strip_prefix("key_") {
                    format!("key:{key}")
                } else {
                    match pin {
                        "mouse_left" => "mouse:left",
                        "mouse_right" => "mouse:right",
                        "mouse_middle" => "mouse:middle",
                        "mouse_back" => "mouse:x1",
                        "mouse_forward" => "mouse:x2",
                        _ => "",
                    }
                    .to_string()
                };
                if name.is_empty() {
                    bail_result("Unsupported FlexInput output pin")
                } else {
                    let down = value.as_bool();
                    if down {
                        self.held.insert(name.clone());
                    }
                    let sent = self.inner.button(&name, down);
                    if sent.is_ok() && !down {
                        self.held.remove(&name);
                    }
                    sent
                }
            }
            _ if pad_pin(pin) => {
                self.pad.set(pin, value);
                self.pad_dirty = true;
                Ok(())
            }
            _ => bail_result("Unsupported FlexInput output pin"),
        };
        self.record(result);
    }
    fn flush(&mut self) {
        let scroll = std::mem::take(&mut self.scroll);
        if scroll != [0; 2] {
            let r = self.inner.scroll(scroll[0], scroll[1]);
            self.record(r);
        }
        if self.pad_dirty {
            let result = self.inner.gamepad(self.pad);
            if result.is_ok() {
                self.pad_dirty = false;
            }
            self.record(result);
        }
        let result = self.inner.poll();
        self.record(result);
    }
    fn reset_outputs(&mut self) {
        self.scroll = [0; 2];
        self.pad_dirty = false;
        let result = self.inner.cancel_pending();
        self.record(result);
        for key in crate::bindings::button_order(&self.held, false) {
            let result = self.inner.button(&key, false);
            if result.is_ok() {
                self.held.remove(&key);
            }
            self.record(result);
        }
        self.pad = PadState::default();
        let result = self.inner.gamepad(self.pad);
        self.record(result);
    }
    fn is_connected(&self) -> bool {
        self.error.is_none()
    }
}
fn bail_result(message: &str) -> Result<()> {
    bail!("{message}")
}

impl<S: Sink + Send> Sink for FlexVirtualSink<S> {
    fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
        let frame = self.inner.feedback();
        if let Some(frame) = &frame {
            self.feedback_sequence = frame.sequence;
        }
        frame
    }
    fn gamepad_status(&self) -> Option<crate::output::GamepadStatus> {
        self.inner.gamepad_status()
    }
    fn protected_devices(&self) -> Vec<String> {
        self.inner.protected_devices()
    }
    fn poll(&mut self) -> Result<()> {
        VirtualDevice::flush(self);
        self.checked()
    }
    fn cancel_pending(&mut self) -> Result<()> {
        // Dispatcher owns the held-key release order. Cancel delayed work first.
        self.scroll = [0; 2];
        self.pad_dirty = false;
        self.error.take();
        self.inner.cancel_pending()
    }
    fn stats(&self) -> OutputStats {
        self.inner.stats()
    }
    fn mouse_trace(&self) -> Option<Vec<MouseTracePoint>> {
        self.inner.mouse_trace()
    }
    fn button(&mut self, name: &str, down: bool) -> Result<()> {
        let pin = if let Some(key) = name.strip_prefix("key:") {
            format!("key_{key}")
        } else {
            match name {
                "mouse:left" => "mouse_left",
                "mouse:right" => "mouse_right",
                "mouse:middle" => "mouse_middle",
                "mouse:x1" => "mouse_back",
                "mouse:x2" => "mouse_forward",
                _ => bail!("Unsupported output button: {name}"),
            }
            .to_string()
        };
        self.submit(&pin, Signal::Bool(down))
    }
    fn mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.submit(
            "mouse_move",
            Signal::Vec2(glam::Vec2::new(dx as f32, -(dy as f32))),
        )
    }
    fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<()> {
        self.submit("scroll_right", Signal::Float(horizontal as f32))?;
        self.submit("scroll_up", Signal::Float(vertical as f32))?;
        VirtualDevice::flush(self);
        self.checked()
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()> {
        for &(pin, bit) in PAD_BUTTONS {
            self.submit(pin, Signal::Bool(pad.buttons & bit != 0))?;
        }
        self.submit("btn_touchpad", Signal::Bool(pad.touchpad_click))?;
        self.submit("btn_lt_dig", Signal::Bool(pad.left_trigger_pressed))?;
        self.submit("btn_rt_dig", Signal::Bool(pad.right_trigger_pressed))?;
        for (pin, value) in [
            ("left_stick_x", pad.left_x),
            ("left_stick_y", pad.left_y),
            ("right_stick_x", pad.right_x),
            ("right_stick_y", pad.right_y),
        ] {
            self.submit(
                pin,
                Signal::Float(value as f32 / if value < 0 { 32768. } else { 32767. }),
            )?;
        }
        self.submit(
            "left_trigger",
            Signal::Float(pad.left_trigger as f32 / 255.),
        )?;
        self.submit(
            "right_trigger",
            Signal::Float(pad.right_trigger as f32 / 255.),
        )?;
        VirtualDevice::flush(self);
        self.checked()
    }
}

/// Adapt Studio's desired pad state to original FlexInput pins. Native devices own encoding.
pub(crate) fn pad_pins(p: PadState, mut send: impl FnMut(&str, Signal)) {
    let axis = |v: i16| f32::from(v) / if v < 0 { 32768. } else { 32767. };
    for (pin, bit) in PAD_BUTTONS {
        send(pin, Signal::Bool(p.buttons & bit != 0));
    }
    send("btn_touchpad", Signal::Bool(p.touchpad_click));
    send(
        "left_stick",
        Signal::Vec2(glam::Vec2::new(axis(p.left_x), axis(p.left_y))),
    );
    send(
        "right_stick",
        Signal::Vec2(glam::Vec2::new(axis(p.right_x), axis(p.right_y))),
    );
    send(
        "left_trigger",
        Signal::Float(f32::from(p.left_trigger) / 255.),
    );
    send(
        "right_trigger",
        Signal::Float(f32::from(p.right_trigger) / 255.),
    );
    // Native PinState preserves analog travel when its digital pin follows it.
    send("btn_lt_dig", Signal::Bool(p.left_trigger_pressed));
    send("btn_rt_dig", Signal::Bool(p.right_trigger_pressed));
}
pub(crate) fn write_pad(device: &mut dyn VirtualDevice, p: PadState) {
    pad_pins(p, |pin, value| device.send(pin, value));
}
