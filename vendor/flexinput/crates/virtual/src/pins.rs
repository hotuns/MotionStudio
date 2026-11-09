//! Original FlexInput apply_pin function, c69f53ad (MIT). Visibility only.
use flexinput_core::Signal;
pub fn apply_pin(pins: &mut flexinput_hidmaestro::encode::PinState, pin: &str, value: Signal) {
    match (pin, value) {
        ("left_stick", Signal::Vec2(v)) => {
            pins.set("left_stick_x", v.x, true);
            pins.set("left_stick_y", v.y, true);
        }
        ("right_stick", Signal::Vec2(v)) => {
            pins.set("right_stick_x", v.x, true);
            pins.set("right_stick_y", v.y, true);
        }
        ("dpad", Signal::Vec2(v)) => {
            pins.set("dpad_left", 0.0, v.x < -0.5);
            pins.set("dpad_right", 0.0, v.x > 0.5);
            pins.set("dpad_up", 0.0, v.y > 0.5);
            pins.set("dpad_down", 0.0, v.y < -0.5);
        }
        _ => pins.set(pin, value.as_float(), value.as_bool()),
    }
}
