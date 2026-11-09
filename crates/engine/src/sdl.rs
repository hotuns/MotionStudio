//! Thin Studio DTO/lifecycle adapter over the pinned FlexInput SDL provider.
//! The one verified DLL is preloaded absolutely before delayed Rust SDL calls.
use crate::engine::InputEvent;
use crate::profile::BUTTONS;
use crate::timing::SampleTime;
use anyhow::{bail, Context, Result};
use flexinput_devices::{
    raw::{RawDevice, RawEvent},
    sdl_backend::SdlBackend,
};
use libloading::Library;
use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_void, CStr};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[repr(C)]
struct VirtualSensor {
    kind: i32,
    rate: f32,
}
#[repr(C)]
struct VirtualDesc {
    version: u32,
    kind: u16,
    padding: u16,
    vendor: u16,
    product: u16,
    axes: u16,
    buttons: u16,
    balls: u16,
    hats: u16,
    touchpads_count: u16,
    sensors_count: u16,
    padding2: [u16; 2],
    button_mask: u32,
    axis_mask: u32,
    name: *const c_char,
    touchpads: *const c_void,
    sensors: *const VirtualSensor,
    userdata: *mut c_void,
    callbacks: [*const c_void; 8],
}
struct VirtualGuard {
    id: u32,
    joystick: Pad,
    close: unsafe extern "C" fn(Pad),
    detach: unsafe extern "C" fn(u32) -> bool,
}
impl Drop for VirtualGuard {
    fn drop(&mut self) {
        if !self.joystick.is_null() {
            unsafe {
                (self.close)(self.joystick);
            }
        }
        unsafe {
            (self.detach)(self.id);
        }
    }
}

type Pad = *mut c_void;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputOrigin {
    Physical,
    Virtual,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub path: String,
    #[serde(default)]
    pub origin: InputOrigin,
    pub gyro: bool,
    pub gamepad: bool,
    pub accelerometer: bool,
    #[serde(default)]
    pub touchpads: u16,
    #[serde(default)]
    pub rumble: bool,
    #[serde(default)]
    pub lightbar: bool,
    pub buttons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_error: Option<String>,
}

pub struct Sdl {
    // Drop SDL handles before releasing the wrapper's DLL reference.
    backend: SdlBackend,
    _library: &'static Library,
    pub path: PathBuf,
    selected: Option<u32>,
    _main_thread: PhantomData<Rc<()>>,
}

// Delayed import thunks cache function addresses. Keep the selected module
// pinned until process exit, including across stop/reconnect and SDL_Quit.
static LOADED_DLL: std::sync::Mutex<Option<(PathBuf, &'static Library)>> =
    std::sync::Mutex::new(None);
fn load_dll(path: &Path) -> Result<(PathBuf, &'static Library)> {
    let path = path
        .canonicalize()
        .with_context(|| format!("SDL DLL not found: {}", path.display()))?;
    let mut loaded = LOADED_DLL
        .lock()
        .map_err(|_| anyhow::anyhow!("SDL loader lock poisoned"))?;
    if let Some((current, library)) = loaded.as_ref() {
        anyhow::ensure!(
            current == &path,
            "The process already uses a different SDL library"
        );
        return Ok((current.clone(), *library));
    }
    let library = unsafe { Library::new(&path) }.context("Load Studio SDL3.dll")?;
    let version = unsafe { library.get::<unsafe extern "C" fn() -> i32>(b"SDL_GetVersion\0")?() };
    anyhow::ensure!(
        version == 3_004_018,
        "The FlexInput provider requires the verified Studio SDL 3.4.18, found {version}"
    );
    let library: &'static Library = Box::leak(Box::new(library));
    *loaded = Some((path.clone(), library));
    Ok((path, library))
}

fn string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}
/// Use the device tree for auto-selection. The virtual DS4 shares a VID/PID
/// with a real controller and SDL can expose both as gyro-capable.
#[cfg(windows)]
fn windows_input_origin(path: &str) -> InputOrigin {
    use windows_sys::Win32::Devices::{
        DeviceAndDriverInstallation::{
            CM_Get_Device_IDW, CM_Get_Device_Interface_PropertyW, CM_Get_Parent,
            CM_Locate_DevNodeW, CR_SUCCESS, MAX_DEVICE_ID_LEN,
        },
        Properties::{DEVPKEY_Device_InstanceId, DEVPROP_TYPE_STRING},
    };
    if !path.starts_with(r"\\?\") {
        return InputOrigin::Unknown;
    }
    let path: Vec<_> = path.encode_utf16().chain([0]).collect();
    let mut id = [0u16; MAX_DEVICE_ID_LEN as usize];
    let mut size = std::mem::size_of_val(&id) as u32;
    let mut kind = 0;
    let mut node = 0;
    // Read-only lookup from the interface link to its PnP node.
    unsafe {
        if CM_Get_Device_Interface_PropertyW(
            path.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut kind,
            id.as_mut_ptr().cast(),
            &mut size,
            0,
        ) != CR_SUCCESS
            || kind != DEVPROP_TYPE_STRING
            || CM_Locate_DevNodeW(&mut node, id.as_ptr(), 0) != CR_SUCCESS
        {
            return InputOrigin::Unknown;
        }
        let mut usb = false;
        let mut hardware_bus = false;
        for _ in 0..16 {
            if CM_Get_Device_IDW(node, id.as_mut_ptr(), id.len() as u32, 0) != CR_SUCCESS {
                break;
            }
            let end = id.iter().position(|&c| c == 0).unwrap_or(id.len());
            let name = String::from_utf16_lossy(&id[..end]).to_ascii_uppercase();
            if virtual_device_ancestor(&name) {
                return InputOrigin::Virtual;
            }
            usb |= name.starts_with(r"USB\ROOT_HUB");
            hardware_bus |= name.starts_with(r"PCI\") || name.starts_with(r"ACPI\");
            let mut parent = 0;
            if CM_Get_Parent(&mut parent, node, 0) != CR_SUCCESS {
                break;
            }
            node = parent;
        }
        if usb && hardware_bus {
            InputOrigin::Physical
        } else {
            InputOrigin::Unknown
        }
    }
}

fn virtual_device_ancestor(name: &str) -> bool {
    [
        r"ROOT\VIGEMBUS\",
        r"ROOT\HIDMAESTRO\",
        r"ROOT\HIDCLASS\",
        r"SWD\HIDMAESTRO\",
        r"SWD\HIDMAESTRO_KBM\",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

#[cfg(not(windows))]
fn windows_input_origin(_: &str) -> InputOrigin {
    InputOrigin::Unknown
}

impl Sdl {
    pub fn load(path: &Path) -> Result<Self> {
        let (path, library) = load_dll(path)?;
        let backend = SdlBackend::for_events().map_err(anyhow::Error::msg)?;
        let mut this = Self {
            backend,
            _library: library,
            path,
            selected: None,
            _main_thread: PhantomData,
        };
        // Preserve the Flydigi asynchronous takeover discovery window.
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(500) {
            this.backend.update_events().map_err(anyhow::Error::msg)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Ok(this)
    }
    fn device(raw: RawDevice) -> Device {
        Device {
            id: raw.id,
            name: raw.name,
            vendor: raw.vendor,
            product: raw.product,
            origin: if raw.virtual_device {
                InputOrigin::Virtual
            } else {
                windows_input_origin(&raw.path)
            },
            path: raw.path,
            gyro: raw.gyro,
            gamepad: raw.gamepad,
            accelerometer: raw.accelerometer,
            touchpads: raw.touchpads,
            rumble: raw.rumble,
            lightbar: raw.lightbar,
            buttons: raw
                .buttons
                .into_iter()
                .filter_map(|i| BUTTONS.get(i as usize).map(|s| s.to_string()))
                .collect(),
            open_error: raw.open_error,
        }
    }
    pub fn devices(&mut self) -> Result<Vec<Device>> {
        Ok(self
            .backend
            .event_devices()
            .map_err(anyhow::Error::msg)?
            .into_iter()
            .map(Self::device)
            .collect())
    }
    pub fn select(&mut self, id: u32, require_gyro: bool, sensors: bool) -> Result<Device> {
        let device = self
            .devices()?
            .into_iter()
            .find(|d| d.id == id)
            .context("Selected input is unavailable")?;
        self.backend
            .select_events(id, sensors, require_gyro)
            .map_err(anyhow::Error::msg)?;
        self.selected = Some(id);
        Ok(device)
    }
    pub fn set_sensors(&mut self, enabled: bool, require_gyro: bool) -> Result<()> {
        self.backend
            .set_event_sensors(enabled, require_gyro)
            .map_err(anyhow::Error::msg)
    }
    pub fn feedback(&mut self, plan: &crate::feedback::FeedbackPlan) {
        if plan.clear_lightbar {
            self.backend.event_cancel_lightbar();
        }
        self.backend.event_feedback(&plan.pins());
    }
    pub fn stop_feedback(&mut self) {
        self.backend.event_stop_feedback();
    }
    pub fn snapshot(&self) -> Vec<InputEvent> {
        self.backend
            .event_snapshot()
            .into_iter()
            .map(input_event)
            .collect()
    }
    pub fn next_event(&mut self) -> Result<Option<Option<InputEvent>>> {
        Ok(self
            .backend
            .next_raw_event()
            .map_err(anyhow::Error::msg)?
            .map(|e| Some(input_event(e))))
    }
    pub fn is_connected(&self) -> bool {
        self.backend.event_connected()
    }
    pub fn version(&self) -> i32 {
        flexinput_devices::sdl_sys::version::SDL_GetVersion()
    }
    pub fn now_ns(&self) -> u64 {
        SdlBackend::event_now_ns()
    }
    pub fn idle(&self) {
        SdlBackend::event_idle();
    }
    fn error(&self) -> String {
        string(flexinput_devices::sdl_sys::error::SDL_GetError())
    }
    /// SDL's virtual input exists ONLY inside this process. It is not an OS
    /// controller. Exercises the actual DLL/event ABI without moving the mouse.
    pub fn synthetic_trace(&mut self) -> Result<Vec<InputEvent>> {
        if self.selected.is_some() {
            bail!("Synthetic test requires an unselected SDL session");
        }
        let attach = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(*const VirtualDesc) -> u32>(
                    b"SDL_AttachVirtualJoystick\0",
                )?
        };
        let detach = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(u32) -> bool>(b"SDL_DetachVirtualJoystick\0")?
        };
        let open = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(u32) -> Pad>(b"SDL_OpenJoystick\0")?
        };
        let close = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(Pad)>(b"SDL_CloseJoystick\0")?
        };
        let button = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(Pad, i32, bool) -> bool>(
                    b"SDL_SetJoystickVirtualButton\0",
                )?
        };
        let axis = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(Pad, i32, i16) -> bool>(
                    b"SDL_SetJoystickVirtualAxis\0",
                )?
        };
        let sensor = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(Pad, i32, u64, *const f32, i32) -> bool>(
                    b"SDL_SendJoystickVirtualSensorData\0",
                )?
        };
        let touch = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn(Pad, i32, i32, bool, f32, f32, f32) -> bool>(
                    b"SDL_SetJoystickVirtualTouchpad\0",
                )?
        };
        let update = unsafe {
            *self
                ._library
                .get::<unsafe extern "C" fn()>(b"SDL_UpdateJoysticks\0")?
        };
        let sensors = [
            VirtualSensor {
                kind: 2,
                rate: 1000.0,
            },
            VirtualSensor {
                kind: 1,
                rate: 1000.0,
            },
        ];
        let touchpads = [
            flexinput_devices::sdl_sys::joystick::SDL_VirtualJoystickTouchpadDesc {
                nfingers: 2,
                ..Default::default()
            },
        ];
        let desc = VirtualDesc {
            version: std::mem::size_of::<VirtualDesc>() as u32,
            kind: 1,
            padding: 0,
            vendor: 0,
            product: 0,
            axes: 6,
            buttons: 15,
            balls: 0,
            hats: 0,
            touchpads_count: 1,
            sensors_count: 2,
            padding2: [0; 2],
            button_mask: 0x7fff,
            axis_mask: 0x3f,
            name: c"Studio internal SDL input test".as_ptr(),
            touchpads: touchpads.as_ptr().cast(),
            sensors: sensors.as_ptr(),
            userdata: std::ptr::null_mut(),
            callbacks: [std::ptr::null(); 8],
        };
        let id = unsafe { attach(&desc) };
        if id == 0 {
            bail!("Attach synthetic SDL input: {}", self.error());
        }
        let mut guard = VirtualGuard {
            id,
            joystick: std::ptr::null_mut(),
            close,
            detach,
        };
        let device = self.select(id, true, true)?;
        self.set_sensors(false, false)?;
        anyhow::ensure!(
            self.backend.event_sensors_enabled() == [false; 2],
            "UI navigation must disable sensors"
        );
        self.set_sensors(true, true)?;
        anyhow::ensure!(
            self.backend.event_sensors_enabled() == [true; 2],
            "Gyro selection must enable both virtual sensors"
        );
        anyhow::ensure!(
            device.origin == InputOrigin::Virtual,
            "SDL virtual device not identified"
        );
        guard.joystick = unsafe { open(id) };
        if guard.joystick.is_null() {
            bail!("Open synthetic joystick: {}", self.error());
        }
        for _ in 0..10000 {
            if self.next_event()?.is_none() {
                break;
            }
        }
        // All three sensor reports arrive before a poll, including identical
        // first/second values. Latest-state polling would lose two of them.
        for (i, value) in [
            -std::f32::consts::FRAC_PI_2,
            -std::f32::consts::FRAC_PI_2,
            -std::f32::consts::FRAC_PI_4,
        ]
        .iter()
        .enumerate()
        {
            let acceleration = [0., 9.80665, 0.];
            if !unsafe {
                sensor(
                    guard.joystick,
                    1,
                    (i as u64 + 1) * 1_000_000,
                    acceleration.as_ptr(),
                    3,
                )
            } {
                bail!("Inject accelerometer: {}", self.error());
            }
            let data = [0.0, *value, 0.0];
            if !unsafe {
                sensor(
                    guard.joystick,
                    2,
                    (i as u64 + 1) * 1_000_000,
                    data.as_ptr(),
                    3,
                )
            } {
                bail!("Inject sensor: {}", self.error());
            }
        }
        if !unsafe { button(guard.joystick, 0, true) } {
            bail!("Inject button down");
        }
        unsafe {
            update();
        }
        if !unsafe { button(guard.joystick, 0, false) } {
            bail!("Inject button up");
        }
        if !unsafe { axis(guard.joystick, 1, -32768) } {
            bail!("Inject stick");
        }
        unsafe {
            update();
        }
        for (down, x, y) in [(true, 0.5, 0.5), (true, 0.6, 0.5), (false, 0.6, 0.5)] {
            anyhow::ensure!(
                unsafe { touch(guard.joystick, 0, 0, down, x, y, if down { 1. } else { 0. }) },
                "Synthetic SDL touch injection failed"
            );
            unsafe {
                update();
            }
        }
        let mut events = vec![];
        for _ in 0..10000 {
            match self.next_event()? {
                Some(Some(event)) => events.push(event),
                Some(None) => {}
                None => break,
            }
        }
        let gyro: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let InputEvent::Gyro { time, value } = event {
                    Some((time, value))
                } else {
                    None
                }
            })
            .collect();
        if gyro.len() != 3 {
            bail!("SDL delivered {} of 3 synthetic gyro samples", gyro.len());
        }
        let acceleration: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let InputEvent::Accel { time, value } = event {
                    Some((time, value))
                } else {
                    None
                }
            })
            .collect();
        anyhow::ensure!(acceleration.len() == 3, "SDL lost accelerometer samples");
        for (i, (time, value)) in acceleration.iter().enumerate() {
            anyhow::ensure!(
                time.sensor_ns == (i as u64 + 1) * 1_000_000 && (value[1] - 9.80665).abs() < 1e-5,
                "SDL acceleration value or timestamp changed"
            );
        }
        for (i, (time, _)) in gyro.iter().enumerate() {
            if time.sensor_ns != (i as u64 + 1) * 1_000_000 || time.host_ns == 0 {
                bail!("SDL sensor timestamp was not preserved");
            }
        }
        let edges: Vec<bool> = events
            .iter()
            .filter_map(|event| {
                if let InputEvent::Button { index: 0, down, .. } = event {
                    Some(*down)
                } else {
                    None
                }
            })
            .collect();
        if edges != [true, false] {
            bail!("SDL lost a button edge: {edges:?}");
        }
        if !events.iter().any(|event| {
            matches!(
                event,
                InputEvent::Axis {
                    index: 1,
                    value: -32768,
                    ..
                }
            )
        }) {
            bail!("SDL lost the stick event");
        }
        let contacts: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let InputEvent::Touch {
                    pad,
                    finger,
                    active,
                    x,
                    y,
                    time,
                } = event
                {
                    Some((*pad, *finger, *active, *x, *y, time.host_ns))
                } else {
                    None
                }
            })
            .collect();
        anyhow::ensure!(
            contacts.len() == 3
                && contacts[0].2
                && contacts[1].2
                && !contacts[2].2
                && contacts.iter().all(|c| c.0 == 0 && c.1 == 0 && c.5 > 0),
            "SDL lost touch FIFO transitions: {contacts:?}"
        );
        anyhow::ensure!(
            (contacts[1].3 - 0.6).abs() < 0.00001,
            "SDL touch coordinates were not preserved"
        );
        Ok(events)
    }
}

fn input_event(event: RawEvent) -> InputEvent {
    let convert = |time: flexinput_devices::raw::SampleTime| SampleTime {
        host_ns: time.host_ns,
        sensor_ns: time.sensor_ns,
    };
    match event {
        RawEvent::Button { time, index, down } => InputEvent::Button {
            time: convert(time),
            index,
            down,
        },
        RawEvent::Axis { time, index, value } => InputEvent::Axis {
            time: convert(time),
            index,
            value,
        },
        RawEvent::Gyro { time, value } => InputEvent::Gyro {
            time: convert(time),
            value,
        },
        RawEvent::Touch {
            time,
            pad,
            finger,
            active,
            x,
            y,
        } => InputEvent::Touch {
            time: convert(time),
            pad,
            finger,
            active,
            x,
            y,
        },
        RawEvent::Accel { time, value } => InputEvent::Accel {
            time: convert(time),
            value,
        },
        RawEvent::Disconnected { time } => InputEvent::Disconnected {
            time: convert(time),
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use flexinput_devices::sdl_sys::events::{
        SDL_Event as Event, SDL_GamepadAxisEvent as AxisEvent,
        SDL_GamepadButtonEvent as ButtonEvent, SDL_GamepadSensorEvent as SensorEvent,
    };
    #[test]
    fn hidmaestro_root_and_software_nodes_are_never_physical_inputs() {
        for name in [
            r"ROOT\HIDCLASS\MS0123",
            r"SWD\HIDMAESTRO\MS0123",
            r"SWD\HIDMAESTRO_KBM\MS0123",
            r"ROOT\VIGEMBUS\0000",
        ] {
            assert!(virtual_device_ancestor(name), "{name}");
        }
        assert!(!virtual_device_ancestor(r"USB\VID_054C&PID_09CC\REAL_DS4"));
    }
    #[test]
    fn abi_matches_sdl3_headers() {
        assert_eq!(std::mem::size_of::<Event>(), 128);
        assert_eq!(std::mem::align_of::<Event>(), 8);
        assert_eq!(std::mem::size_of::<SensorEvent>(), 48);
        assert_eq!(std::mem::offset_of!(SensorEvent, sensor_timestamp), 40);
        assert_eq!(std::mem::offset_of!(AxisEvent, value), 24);
        assert_eq!(std::mem::offset_of!(ButtonEvent, down), 21);
        assert_eq!(std::mem::size_of::<VirtualDesc>(), 136);
        assert_eq!(std::mem::offset_of!(VirtualDesc, callbacks), 72);
    }
}
