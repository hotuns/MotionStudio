use crate::sdl::{Device, InputOrigin};
use anyhow::{bail, Result};

#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    One(u32),
    Choose(Vec<u32>),
}

/// Select by capabilities, never by enumeration order, controller name, or
/// VID/PID alone. Two interfaces of one pad can share the same VID/PID; so can
/// two separate controllers. An unknown name does not disqualify a gyro source.
pub fn select_device(devices: &[Device], requested: Option<u32>, gyro: bool) -> Result<Selection> {
    let usable = |d: &&Device| {
        d.origin == InputOrigin::Physical
            && d.gamepad
            && d.open_error.is_none()
            && (!gyro || d.gyro)
    };
    if let Some(id) = requested {
        let Some(device) = devices.iter().find(|d| d.id == id) else {
            bail!("Device {id} not present; run `list` again");
        };
        if !device.gamepad {
            bail!("Device {id} ({}) is visible as a raw joystick, but SDL has no gamepad mapping for it; no input will be emitted", device.name);
        }
        if let Some(error) = &device.open_error {
            bail!("Cannot open device {id} ({}): {error}", device.name);
        }
        if device.origin == InputOrigin::Virtual {
            bail!(
                "Select a physical controller; device {id} ({}) is virtual",
                device.name
            );
        }
        if gyro && !device.gyro {
            bail!("Device {id} ({}) has no gyro. Select the third-party/HID interface with gyro=true, not its XInput compatibility interface", device.name);
        }
        return Ok(Selection::One(id));
    }
    let eligible: Vec<_> = devices.iter().filter(usable).map(|d| d.id).collect();
    match eligible.as_slice() {
        [] if gyro => bail!("No accessible gyro gamepad after discovery. Do not use the XInput-only interface for gyro. Check third-party takeover, other mapper processes and HidHide access for this EXE; run `list` for all SDL devices"),
        [] => bail!("No accessible SDL gamepad. Run `list` for raw/failed devices and check HidHide access for this EXE"),
        [id] => Ok(Selection::One(*id)),
        _ => Ok(Selection::Choose(eligible)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: u32, name: &str, gyro: bool) -> Device {
        Device {
            id,
            name: name.into(),
            vendor: 0x37d7,
            product: 0x2502,
            path: String::new(),
            origin: InputOrigin::Physical,
            gyro,
            gamepad: true,
            accelerometer: gyro,
            touchpads: 0,
            rumble: false,
            lightbar: false,
            buttons: vec![],
            open_error: None,
        }
    }
    #[test]
    fn gyro_chooses_the_capable_interface_even_if_its_name_is_unknown() {
        let devices = [
            device(5, "XInput Controller #1", false),
            device(6, "Unknown", true),
        ];
        assert_eq!(
            select_device(&devices, None, true).unwrap(),
            Selection::One(6)
        );
        assert_eq!(
            select_device(&devices, None, false).unwrap(),
            Selection::Choose(vec![5, 6])
        );
    }
    #[test]
    fn a_single_xinput_interface_is_not_a_gyro_fallback() {
        let devices = [device(5, "XInput Controller #1", false)];
        assert!(select_device(&devices, None, true).is_err());
        assert!(select_device(&devices, Some(5), true).is_err());
        assert_eq!(
            select_device(&devices, None, false).unwrap(),
            Selection::One(5)
        );
    }
    #[test]
    fn virtual_and_unverified_devices_are_excluded_from_automatic_selection() {
        let mut virtual_pad = device(1, "DualShock 4", true);
        virtual_pad.origin = InputOrigin::Virtual;
        let mut unknown = device(2, "DualShock 4", true);
        unknown.origin = InputOrigin::Unknown;
        let physical = device(3, "DualShock 4", true);
        let devices = [virtual_pad, unknown, physical];
        assert_eq!(
            select_device(&devices, None, true).unwrap(),
            Selection::One(3)
        );
        assert!(select_device(&devices[..2], None, true).is_err());
        assert!(select_device(&devices, Some(1), true).is_err());
        assert_eq!(
            select_device(&devices, Some(2), true).unwrap(),
            Selection::One(2)
        );
    }
    #[test]
    fn multiple_gyro_controllers_require_a_choice_even_with_identical_ids() {
        let devices = [device(5, "Same model", true), device(6, "Same model", true)];
        assert_eq!(
            select_device(&devices, None, true).unwrap(),
            Selection::Choose(vec![5, 6])
        );
        assert_eq!(
            select_device(&devices, Some(6), true).unwrap(),
            Selection::One(6)
        );
    }
    #[test]
    fn raw_and_inaccessible_devices_stay_visible_but_are_not_selected() {
        let mut raw = device(5, "Unknown raw joystick", false);
        raw.gamepad = false;
        let mut blocked = device(6, "Unavailable pad", true);
        blocked.open_error = Some("open failed".into());
        let devices = [raw, blocked, device(7, "Gyro pad", true)];
        assert_eq!(
            select_device(&devices, None, true).unwrap(),
            Selection::One(7)
        );
        assert!(select_device(&devices, Some(5), false).is_err());
        assert!(select_device(&devices, Some(6), true).is_err());
    }
}
