//! Motion Studio's bindings -> FlexInput HIDMaestro report contract.
//! Keyboard/mouse layouts follow FlexInput keymouse_hm.rs at c69f53ad.
use crate::{engine::PadState, profile::GamepadTarget};
use anyhow::{bail, Result};
#[cfg(test)]
use flexinput_hidmaestro::encode::{button, Hat};
use flexinput_hidmaestro::{
    encode::{GamepadState, PinState},
    Profile,
};
use std::collections::BTreeSet;

pub fn keyboard(held: &BTreeSet<String>) -> Result<[u8; 8]> {
    let mut report = [0; 8];
    let mut keys = Vec::new();
    for key in held.iter().filter_map(|s| s.strip_prefix("key:")) {
        let info = crate::keyboard::key(key)
            .ok_or_else(|| anyhow::anyhow!("Unsupported HID key: {key}"))?;
        if info.modifier != 0 {
            report[0] |= info.modifier;
        } else {
            keys.push(
                info.hid_usage()
                    .ok_or_else(|| anyhow::anyhow!("Unsupported HID key: {key}"))?,
            );
        }
    }
    // The fixed upstream boot-keyboard descriptor has six non-modifier slots.
    // Reject overflow instead of dropping held keys or emitting ErrorRollOver.
    if keys.len() > 6 {
        bail!("HIDMaestro 键盘最多同时按下 6 个普通键；请减少同时触发的映射");
    }
    report[2..2 + keys.len()].copy_from_slice(&keys);
    Ok(report)
}

pub fn mouse(held: &BTreeSet<String>, dx: i16, dy: i16, horizontal: i8, vertical: i8) -> [u8; 7] {
    let mut r = [0; 7];
    for (bit, name) in [
        "mouse:left",
        "mouse:right",
        "mouse:middle",
        "mouse:x1",
        "mouse:x2",
    ]
    .iter()
    .enumerate()
    {
        if held.contains(*name) {
            r[0] |= 1 << bit;
        }
    }
    r[1..3].copy_from_slice(&dx.to_le_bytes());
    r[3..5].copy_from_slice(&dy.to_le_bytes());
    r[5] = vertical as u8;
    r[6] = horizontal as u8;
    r
}

use crate::flex_output::pad_pins;
pub fn gamepad(p: PadState) -> GamepadState {
    let mut pins = PinState::new();
    pad_pins(p, |pin, value| {
        flexinput_virtual::apply_pin(&mut pins, pin, value)
    });
    pins.state()
}
pub fn profile_json(target: GamepadTarget) -> Option<&'static str> {
    use flexinput_hidmaestro::profile::presets::*;
    Some(match target {
        GamepadTarget::None => return None,
        GamepadTarget::Xbox360 => XBOX360_JSON,
        GamepadTarget::DualShock4 => DUALSHOCK_4_V2_JSON,
    })
}
pub fn profile(target: GamepadTarget) -> Option<Profile> {
    profile_json(target)
        .map(|json| Profile::from_json(json).expect("pinned native HIDMaestro profile"))
}
/// Initial/release frames use the exact native PinState and encoder. Normal
/// output is written by the original HidMaestroDevice::flush implementation.
pub fn pad_report(profile: &Profile, p: PadState) -> (Vec<u8>, Option<[u8; 14]>) {
    let state = gamepad(p);
    if profile.requires_xusb_companion {
        (
            vec![0; profile.input_report_size],
            Some(flexinput_hidmaestro::gip_from_state(&state)),
        )
    } else {
        let mut bytes = flexinput_hidmaestro::encode_report(profile, &state);
        if profile.report.report_id != 0 {
            bytes.remove(0);
        }
        (bytes, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_lock_uses_a_normal_key_slot_and_releases_to_neutral() {
        let held = ["key:CAPSLOCK", "key:SHIFT", "key:A"]
            .map(String::from)
            .into_iter()
            .collect();
        let r = keyboard(&held).unwrap();
        assert_eq!(r[0], 2, "Caps Lock is a normal key, not a modifier");
        assert_eq!(r[1], 0);
        assert_eq!(&r[2..4], &[4, 0x39]);
        assert_eq!(&r[4..], &[0; 4]);
        assert_eq!(keyboard(&BTreeSet::new()).unwrap(), [0; 8]);
    }
    #[test]
    fn extended_keyboard_reports_distinguish_locations_and_right_modifiers() {
        for (name, usage) in [
            ("ESC", 0x29),
            ("ENTER", 0x28),
            ("NUMPADENTER", 0x58),
            ("1", 0x1e),
            ("NUMPAD1", 0x59),
            ("NUMLOCK", 0x53),
            ("SCROLLLOCK", 0x47),
            ("CONTEXT", 0x65),
            ("F24", 0x73),
            ("SEMICOLON", 0x33),
            ("PRINTSCREEN", 0x46),
            ("HOME", 0x4a),
        ] {
            let held = [format!("key:{name}")].into_iter().collect();
            assert_eq!(
                keyboard(&held).unwrap(),
                [0, 0, usage, 0, 0, 0, 0, 0],
                "{name}"
            );
        }
        let held = ["key:RCTRL", "key:RSHIFT", "key:RALT", "key:RWIN", "key:ESC"]
            .map(String::from)
            .into_iter()
            .collect();
        assert_eq!(keyboard(&held).unwrap(), [0xf0, 0, 0x29, 0, 0, 0, 0, 0]);
        for key in crate::keyboard::keys() {
            assert!(
                keyboard(&[format!("key:{}", key.name)].into_iter().collect()).is_ok(),
                "{}",
                key.name
            );
        }
    }
    #[test]
    fn keys_modifiers_arrows_and_rollover_are_explicit() {
        let held = [
            "key:CTRL",
            "key:SHIFT",
            "key:ALT",
            "key:A",
            "key:UP",
            "key:0",
            "key:F11",
        ]
        .map(String::from)
        .into_iter()
        .collect();
        let r = keyboard(&held).unwrap();
        assert_eq!(r[0], 7);
        assert!(r.contains(&4));
        assert!(r.contains(&0x52));
        assert!(r.contains(&0x44));
        let seven = ['A', 'B', 'C', 'D', 'E', 'F', 'G']
            .map(|c| format!("key:{c}"))
            .into_iter()
            .collect();
        assert!(keyboard(&seven).is_err());
        assert_eq!(keyboard(&BTreeSet::new()).unwrap(), [0; 8]);
        for name in [
            "SPACE",
            "ENTER",
            "ESC",
            "TAB",
            "SHIFT",
            "CTRL",
            "ALT",
            "UP",
            "DOWN",
            "LEFT",
            "RIGHT",
            "BACKSPACE",
            "DELETE",
        ] {
            assert!(
                keyboard(&[format!("key:{name}")].into_iter().collect()).is_ok(),
                "{name}"
            );
        }
    }
    #[test]
    fn side_buttons_and_signed_wheels_match_upstream_descriptor() {
        let held = ["mouse:x1", "mouse:x2", "mouse:left"]
            .map(String::from)
            .into_iter()
            .collect();
        assert_eq!(
            mouse(&held, -32768, 32767, -2, 3),
            [25, 0, 128, 255, 127, 3, 254]
        );
        assert_eq!(mouse(&BTreeSet::new(), 0, 0, 0, 0), [0; 7]);
        use flexinput_hidmaestro::profile::presets::*;
        let p = Profile::from_json(MOUSE_JSON).unwrap();
        assert_eq!(p.report.byte_size(), 7);
        assert_eq!(p.report.field(1, 0x30).unwrap().bit_offset, 8);
        assert_eq!(p.report.field(1, 0x31).unwrap().bit_offset, 24);
        assert_eq!(p.report.field(1, 0x38).unwrap().bit_offset, 40);
        assert_eq!(p.report.field(0x0c, 0x238).unwrap().bit_offset, 48);
        assert_eq!(
            Profile::from_json(KEYBOARD_JSON)
                .unwrap()
                .report
                .byte_size(),
            8
        );
    }
    #[test]
    fn pads_preserve_axes_buttons_and_neutral_state() {
        let p = PadState {
            buttons: 0x1000 | 0x400 | 1 | 8,
            left_x: i16::MIN,
            left_y: i16::MAX,
            right_x: i16::MAX,
            right_y: i16::MIN,
            left_trigger: 255,
            touchpad_click: true,
            ..Default::default()
        };
        let s = gamepad(p);
        assert_eq!(
            (
                s.left_stick_x,
                s.left_stick_y,
                s.right_stick_x,
                s.right_stick_y
            ),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(s.hat, Hat::NorthEast);
        assert!(s.buttons & button::TOUCHPAD != 0);
        for target in [GamepadTarget::Xbox360, GamepadTarget::DualShock4] {
            let profile = profile(target).unwrap();
            let (r, gip) = pad_report(&profile, PadState::default());
            if target == GamepadTarget::Xbox360 {
                assert!(r.iter().all(|b| *b == 0));
                assert!(gip.is_some());
                let (_, gip) = pad_report(&profile, p);
                let g = gip.unwrap();
                // Original SDK GIP uses unsigned axes first, followed by triggers.
                assert_eq!(&g[2..4], &0u16.to_le_bytes());
                assert_eq!(&g[6..8], &u16::MAX.to_le_bytes());
                assert_eq!(&g[8..10], &1020u16.to_le_bytes());
            } else {
                assert_eq!(&r[..4], &[127, 127, 127, 127]); // Original DS4 encoder center.
                assert!(gip.is_none());
                assert_eq!(r[4] & 15, 8);
            }
        }
    }
    #[test]
    fn ds4_rounding_retains_every_trigger_value_and_cancels_opposing_dpad() {
        let profile = profile(GamepadTarget::DualShock4).unwrap();
        for trigger in 0..=255 {
            let (r, _) = pad_report(
                &profile,
                PadState {
                    buttons: 15,
                    left_trigger: trigger,
                    right_trigger: 255 - trigger,
                    ..Default::default()
                },
            );
            assert_eq!(r[7], trigger);
            assert_eq!(r[8], 255 - trigger);
            assert_eq!(r[4] & 15, 8);
        }
    }
}
