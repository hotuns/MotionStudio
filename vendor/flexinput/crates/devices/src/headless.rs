//! FlexInput device providers, headless Studio port of c69f53ad (MIT).
//! Native parsers are retained; the live Studio path selects the SDL provider.
#![allow(dead_code, unused_imports, unused_variables)]
pub mod click_gate;
pub mod gamepad;
pub mod gilrs_backend;
pub mod gyro;
pub mod identification;
pub mod layouts;
pub mod raw;
pub mod raw_hid;
pub mod sdl_backend;
pub mod spike;
pub mod steam_controller;
pub mod steam_haptics;
pub use sdl3::sys as sdl_sys;
#[cfg(windows)]
pub mod devnode;
use flexinput_core::raw_pad::RawPadProfiles;
use flexinput_core::{Signal, SignalType};
pub use identification::ControllerKind;
#[derive(Clone)]
pub struct DevicePin {
    pub id: String,
    pub display_name: String,
    pub signal_type: SignalType,
}

#[derive(Clone)]
pub struct PhysicalDevice {
    pub id: String,
    pub display_name: String,
    pub kind: ControllerKind,
    pub outputs: Vec<DevicePin>,
    pub inputs: Vec<DevicePin>,
    /// Windows device instance path (e.g. `HID\VID_054C&PID_09CC\5&...`),
    /// used for HidHide blacklist operations. None if unavailable.
    pub instance_path: Option<String>,
    /// USB vendor/product id for a gamepad (gilrs pads). `None` for devices
    /// without one (MIDI). Used to resolve the HID instance id off-thread for
    /// HidHide masking, so a slow SetupAPI lookup never runs on the I/O loop.
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

pub trait DeviceBackend: Send {
    /// Short label for diagnostics (the I/O loop's per-backend timings).
    fn name(&self) -> &'static str {
        "backend"
    }
    fn enumerate(&mut self) -> Vec<PhysicalDevice>;
    fn poll(&mut self) -> Vec<(String, String, Signal)>;
    /// Route a signal to a physical output pin (e.g. MIDI CC send).
    /// Backends that don't support output can ignore this.
    fn send(&mut self, _device_id: &str, _pin_id: &str, _signal: Signal) {}
    /// The I/O loop's output pass has sent everything it will: a backend that
    /// gathers its sends hands them over here (see `own_thread::OwnThread`).
    fn end_output_pass(&mut self) {}
    /// Drain accumulated raw-event counts per device id. Used by the I/O
    /// thread to compute live per-device polling rates. Default: empty
    /// (backends that don't track events are reported as 0 Hz).
    fn take_event_counts(&mut self) -> Vec<(String, u32)> {
        Vec::new()
    }
    /// Configure the outlier-spike filter for a specific physical device.
    /// Called from the I/O loop each tick with the current UI settings
    /// (cheap no-op if unchanged). `window` is the judging window width in
    /// samples — see `crate::spike`.
    ///
    /// Backends without a raw IMU stream should ignore this. Any backend
    /// that DOES emit `gyro_*` / `accel_*` pins must implement it, or the
    /// filter's UI controls silently do nothing for that device's prefix.
    fn set_spike_filter(
        &mut self,
        _device_id: &str,
        _enabled: bool,
        _sensitivity_pct: f32,
        _window: u8,
    ) {
    }
    /// Global "route every pad through SDL" switch, pushed from the I/O loop
    /// each tick (cheap no-op when unchanged). When on, the SDL backend claims
    /// ALL gamepads (not just the ones kind-detect calls `Generic`) and the
    /// gilrs backend emits + enumerates nothing, so a controller's inputs come
    /// entirely from SDL. Primarily a verification switch — it lets a pad with a
    /// native parser (DualSense / Switch Pro) be read through SDL and compared
    /// against its canonical-correct native path. Changes device IDs
    /// (`gilrs:…` → `sdl:…`), so existing wiring does not follow the switch.
    fn set_sdl_all_pads(&mut self, _on: bool) {}
    /// Hand a device its measured resting gyro drift, in **degrees per second
    /// on the device's own rate axes**, or `None` to fall back to whatever the
    /// backend compiled in.
    ///
    /// ⭐ Separate from the `gyro_offset` calibration the engine applies to the
    /// pins, and NOT a duplicate of it. Some devices integrate an orientation
    /// internally from the same rate; for those, a correction applied to the
    /// output pins arrives too late to stop the integrated estimate drifting,
    /// and only the device layer can subtract it early enough to fix both.
    /// Backends without an internal integrator can ignore this.
    fn set_gyro_drift(&mut self, _device_id: &str, _drift: Option<[f32; 3]>) {}
    /// Hand the backend every learned raw-pad layout (see
    /// `flexinput_core::raw_pad`), pushed from the I/O loop each iteration.
    /// Cheap to call: the snapshot is only replaced when the user changes a
    /// layout, so a backend compares the `Arc` by pointer and returns. Only the
    /// backend that surfaces `sdl:raw:` devices uses it.
    fn set_raw_pad_profiles(&mut self, _profiles: &std::sync::Arc<RawPadProfiles>) {}
}
