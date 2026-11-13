use crate::engine::{Frame, PadState};
use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::time::Instant;
pub const OUTPUT_MUTEX: &str = "Local\\MotionStudioOutput";
mod preview;
mod split;
pub(crate) use preview::MouseTrace;
use preview::PadTrace;
pub use preview::{MouseTracePoint, OutputPreview};
pub use split::SplitSink;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputBackend {
    #[default]
    HidMaestro,
    Legacy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamepadBackend {
    #[default]
    Vigem,
    HidMaestro,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GamepadStatus {
    pub target: crate::profile::GamepadTarget,
    pub backend: GamepadBackend,
    pub connected: bool,
    /// Zero-based native XInput player index, when the backend exposes it.
    pub player_index: Option<u32>,
    pub protected_devices: Vec<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OutputStats {
    pub queued: usize,
    pub coalesced_motion: u64,
    pub direct_motion_reports: u64,
    pub mouse_reports: u64,
    pub keyboard_reports: u64,
    pub wait_total_us: u64,
    pub wait_max_us: u64,
    pub queue_max_us: u64,
}

pub trait Sink {
    fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
        None
    }
    fn gamepad_status(&self) -> Option<GamepadStatus> {
        None
    }
    /// Exact PnP roots created by this output session, including companions.
    fn protected_devices(&self) -> Vec<String> {
        Vec::new()
    }
    fn poll(&mut self) -> Result<()> {
        Ok(())
    }
    /// Cancel delayed output before releasing held state on stop/switch/reset.
    fn cancel_pending(&mut self) -> Result<()> {
        Ok(())
    }
    fn stats(&self) -> OutputStats {
        OutputStats::default()
    }
    /// Backends with their own delivery path report motion when written.
    fn mouse_trace(&self) -> Option<Vec<MouseTracePoint>> {
        None
    }
    fn button(&mut self, name: &str, down: bool) -> Result<()>;
    fn mouse(&mut self, dx: i32, dy: i32) -> Result<()>;
    fn idle_mouse(&mut self) -> Result<()> {
        Ok(())
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()>;
    fn idle_gamepad(&mut self) -> Result<()> {
        Ok(())
    }
    fn scroll(&mut self, _horizontal: i32, _vertical: i32) -> Result<()> {
        bail!("This output sink does not support scrolling")
    }
}
impl<T: Sink + ?Sized> Sink for Box<T> {
    fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
        (**self).feedback()
    }
    fn gamepad_status(&self) -> Option<GamepadStatus> {
        (**self).gamepad_status()
    }
    fn protected_devices(&self) -> Vec<String> {
        (**self).protected_devices()
    }
    fn poll(&mut self) -> Result<()> {
        (**self).poll()
    }
    fn cancel_pending(&mut self) -> Result<()> {
        (**self).cancel_pending()
    }
    fn stats(&self) -> OutputStats {
        (**self).stats()
    }
    fn mouse_trace(&self) -> Option<Vec<MouseTracePoint>> {
        (**self).mouse_trace()
    }
    fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<()> {
        (**self).scroll(horizontal, vertical)
    }
    fn button(&mut self, name: &str, down: bool) -> Result<()> {
        (**self).button(name, down)
    }
    fn mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        (**self).mouse(dx, dy)
    }
    fn idle_mouse(&mut self) -> Result<()> {
        (**self).idle_mouse()
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()> {
        (**self).gamepad(pad)
    }
    fn idle_gamepad(&mut self) -> Result<()> {
        (**self).idle_gamepad()
    }
}

#[derive(Default)]
pub struct ObserveSink;
impl Sink for ObserveSink {
    fn scroll(&mut self, _: i32, _: i32) -> Result<()> {
        Ok(())
    }
    fn button(&mut self, _: &str, _: bool) -> Result<()> {
        Ok(())
    }
    fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
        Ok(())
    }
    fn gamepad(&mut self, _: PadState) -> Result<()> {
        Ok(())
    }
}

/// Aggregate desired key state avoids releasing a key while another binding
/// still holds it. Mouse residuals preserve sub-count motion in both directions.
pub struct Dispatcher<S: Sink> {
    sink: S,
    held: BTreeSet<String>,
    mouse_pending: [f64; 2],
    scheduled: bool,
    pending_pad: Option<PadState>,
    pad: Option<PadState>,
    pad_uncertain: bool,
    mouse_trace: Option<MouseTrace>,
    preview_generation: u64,
    pad_trace: PadTrace,
    pub mouse_counts: [i64; 2],
    pub button_edges: u64,
    pub pad_reports: u64,
}
impl<S: Sink> Dispatcher<S> {
    pub fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
        self.sink.feedback()
    }
    pub fn gamepad_status(&self) -> Option<GamepadStatus> {
        self.sink.gamepad_status()
    }
    pub fn protected_devices(&self) -> Vec<String> {
        self.sink.protected_devices()
    }
    pub fn new(sink: S) -> Self {
        let mouse_trace = sink.mouse_trace().is_none().then(MouseTrace::default);
        Self {
            sink,
            held: BTreeSet::new(),
            mouse_pending: [0.0; 2],
            scheduled: false,
            pending_pad: None,
            pad: None,
            pad_uncertain: false,
            mouse_trace,
            preview_generation: 0,
            pad_trace: PadTrace::default(),
            mouse_counts: [0; 2],
            button_edges: 0,
            pad_reports: 0,
        }
    }
    pub fn with_scheduled_output(mut self) -> Self {
        self.scheduled = true;
        self
    }
    pub fn apply(&mut self, frame: &Frame) -> Result<()> {
        self.sink.poll()?;
        if frame.scroll.iter().any(|v| v.unsigned_abs() > 1024) {
            bail!("Oversized scroll output");
        }
        // Drain motion from before the edge while the old modifiers/buttons
        // still apply. Discrete inputs do not wait for the next motion tick.
        if self.scheduled && (self.held != frame.held || frame.scroll != [0; 2]) {
            self.mouse_pending = self.send_mouse(self.mouse_pending)?;
        }
        let released = self.held.difference(&frame.held).cloned().collect();
        for key in crate::bindings::button_order(&released, false) {
            self.sink.button(&key, false)?;
            self.held.remove(&key);
            self.button_edges += 1;
        }
        let pressed = frame.held.difference(&self.held).cloned().collect();
        for key in crate::bindings::button_order(&pressed, true) {
            // Track before calling the OS so an error still triggers a cleanup attempt.
            self.held.insert(key.clone());
            self.sink.button(&key, true)?;
            self.button_edges += 1;
        }
        if frame.scroll != [0; 2] {
            self.sink.scroll(frame.scroll[0], frame.scroll[1])?;
        }
        let mut pending = self.mouse_pending;
        for (value, delta) in pending.iter_mut().zip(frame.mouse_delta) {
            *value += delta;
            if !value.is_finite() || value.abs() > 1_000_000.0 {
                bail!("Invalid/oversized mouse displacement; stopping output");
            }
        }
        self.mouse_pending = if self.scheduled {
            pending
        } else {
            self.send_mouse(pending)?
        };
        self.pending_pad = frame.pad;
        // Keep short button taps and trigger edges even if they begin and end
        // between output slots. Only continuous analog state can be replaced.
        if !self.scheduled || has_pad_edge(self.pad, frame.pad) {
            self.flush_pad()?;
        }
        Ok(())
    }
    fn flush_pad(&mut self) -> Result<bool> {
        if self.pending_pad != self.pad || self.pad_uncertain {
            // A failed first report may already have reached the device.
            self.pad_uncertain = true;
            self.sink.gamepad(self.pending_pad.unwrap_or_default())?;
            self.pad_uncertain = false;
            self.pad_reports += 1;
            self.pad = self.pending_pad;
            if let Some(pad) = self.pad {
                self.pad_trace.record(pad);
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub fn flush_output(&mut self) -> Result<()> {
        if self.mouse_pending.iter().all(|value| value.abs() < 1.0) {
            self.sink.idle_mouse()?;
        } else {
            self.mouse_pending = self.send_mouse(self.mouse_pending)?;
        }
        if !self.flush_pad()? {
            self.sink.idle_gamepad()?;
            if let Some(pad) = self.pad {
                self.pad_trace.record(pad);
            }
        }
        Ok(())
    }
    fn send_mouse(&mut self, mut pending: [f64; 2]) -> Result<[f64; 2]> {
        let count = pending.map(|value| value.trunc() as i32);
        if count != [0; 2] {
            self.sink.mouse(count[0], count[1])?;
            if let Some(trace) = &mut self.mouse_trace {
                trace.record(count, Instant::now());
            }
            for (total, step) in self.mouse_counts.iter_mut().zip(count) {
                *total += i64::from(step);
            }
            for (value, step) in pending.iter_mut().zip(count) {
                *value -= f64::from(step);
            }
        }
        Ok(pending)
    }
    /// A profile handoff keeps held key state but discards unsent relative motion.
    pub fn discard_pending_motion(&mut self) {
        self.mouse_pending = [0.0; 2];
    }
    pub fn reset(&mut self) -> Result<()> {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.pad_trace.clear();
        self.pending_pad = self.pad.map(|_| PadState::default());
        if let Some(trace) = &mut self.mouse_trace {
            trace.clear();
        }
        let mut errors = vec![];
        if let Err(error) = self.sink.cancel_pending() {
            errors.push(error.to_string());
        }
        for key in crate::bindings::button_order(&self.held, false) {
            match self.sink.button(&key, false) {
                Ok(()) => {
                    self.held.remove(&key);
                    self.button_edges += 1;
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        if self.pad_uncertain || self.pad.is_some_and(|pad| pad != PadState::default()) {
            match self.sink.gamepad(PadState::default()) {
                Ok(()) => {
                    self.pad = Some(PadState::default());
                    self.pending_pad = self.pad;
                    self.pad_uncertain = false;
                    self.pad_reports += 1;
                }
                Err(error) => {
                    self.pad_uncertain = true;
                    errors.push(error.to_string());
                }
            }
        }
        self.mouse_pending = [0.0; 2];
        if !errors.is_empty() {
            bail!("Output release failed: {}", errors.join("; "));
        }
        Ok(())
    }
    pub fn stats(&self) -> OutputStats {
        self.sink.stats()
    }
    pub fn preview(&self) -> OutputPreview {
        let mouse = self
            .mouse_trace
            .as_ref()
            .map(MouseTrace::points)
            .or_else(|| self.sink.mouse_trace())
            .unwrap_or_default();
        OutputPreview {
            generation: self.preview_generation,
            mouse,
            pad: self.pad,
            pad_trace: self.pad_trace.points(),
            pad_time_ms: self.pad_trace.time_ms(),
            sample_ms: preview::SAMPLE_MS,
        }
    }
}
fn has_pad_edge(previous: Option<PadState>, next: Option<PadState>) -> bool {
    let Some(next) = next else {
        return previous.is_some();
    };
    let previous = previous.unwrap_or_default();
    previous.buttons != next.buttons
        || previous.touchpad_click != next.touchpad_click
        || previous.left_trigger_pressed != next.left_trigger_pressed
        || previous.right_trigger_pressed != next.right_trigger_pressed
        || (previous.left_trigger == 0) != (next.left_trigger == 0)
        || (previous.right_trigger == 0) != (next.right_trigger == 0)
}
impl<S: Sink> Drop for Dispatcher<S> {
    fn drop(&mut self) {
        let _ = self.reset();
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::profile::GamepadTarget;
    use anyhow::Context;
    use flexinput_virtual::{
        native_gamepads::{VirtualDS4, VirtualXInput},
        VirtualDevice,
    };
    use vigem_client::Client;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

    enum VirtualTarget {
        Xbox360(VirtualXInput),
        DualShock4(VirtualDS4),
    }
    impl VirtualTarget {
        fn device(&mut self) -> &mut dyn VirtualDevice {
            match self {
                Self::Xbox360(p) => p,
                Self::DualShock4(p) => p,
            }
        }
        fn flush_checked(&mut self) -> Result<()> {
            match self {
                Self::Xbox360(p) => p.flush_checked(),
                Self::DualShock4(p) => p.flush_checked(),
            }
            .map_err(anyhow::Error::msg)
        }
    }
    pub struct WindowsSink {
        target: Option<VirtualTarget>,
        gamepad: Option<GamepadStatus>,
        last_probe: std::time::Instant,
    }
    pub fn probe_virtual_driver() -> Result<()> {
        Client::connect().context("ViGEmBus unavailable")?;
        Ok(())
    }
    pub fn emergency_stop() -> bool {
        unsafe { GetAsyncKeyState(VK_F12 as i32) < 0 }
    }
    impl WindowsSink {
        pub fn new(target: GamepadTarget) -> Result<Self> {
            if target == GamepadTarget::None {
                return Ok(Self {
                    target: None,
                    gamepad: None,
                    last_probe: std::time::Instant::now(),
                });
            }
            probe_virtual_driver()?;
            let baseline = crate::visibility::virtual_output_ids();
            let mut device = match target {
                GamepadTarget::None => unreachable!(),
                GamepadTarget::Xbox360 => VirtualTarget::Xbox360(VirtualXInput::new(0)),
                GamepadTarget::DualShock4 => VirtualTarget::DualShock4(VirtualDS4::new(0)),
            };
            anyhow::ensure!(
                device.device().is_connected(),
                "创建 ViGEm 虚拟手柄失败，请检查 ViGEmBus 状态"
            );
            device.device().reset_outputs();
            device.flush_checked()?;
            let player_index = match &mut device {
                VirtualTarget::Xbox360(p) => Some(p.player_index().context(
                    "虚拟 Xbox 未获得 XInput 玩家槽位；请检查是否已有 4 个 XInput 手柄",
                )?),
                VirtualTarget::DualShock4(_) => None,
            };
            let protected_devices = crate::visibility::virtual_output_ids()
                .into_iter()
                .filter(|id| !baseline.iter().any(|old| old.eq_ignore_ascii_case(id)))
                .collect();
            Ok(Self {
                target: Some(device),
                last_probe: std::time::Instant::now(),
                gamepad: Some(GamepadStatus {
                    target,
                    backend: GamepadBackend::Vigem,
                    connected: true,
                    player_index,
                    protected_devices,
                }),
            })
        }
    }

    fn keyboard_input(name: &str, down: bool) -> Result<INPUT> {
        let key = crate::keyboard::key(name).with_context(|| format!("Unsupported key: {name}"))?;
        // Pause's E1 sequence and Print Screen's special sequence are produced
        // by Windows from their VK. Ordinary keys retain physical scan codes.
        let virtual_key = matches!(name, "PAUSE" | "PRINTSCREEN");
        let scan = if virtual_key {
            0
        } else {
            (unsafe { MapVirtualKeyW(u32::from(key.vk), MAPVK_VK_TO_VSC) }) as u16
        };
        if !virtual_key && scan == 0 {
            bail!("No scan code for {name}");
        }
        Ok(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: if virtual_key { key.vk } else { 0 },
                    wScan: scan,
                    dwFlags: (if virtual_key { 0 } else { KEYEVENTF_SCANCODE })
                        | if down { 0 } else { KEYEVENTF_KEYUP }
                        | if key.extended {
                            KEYEVENTF_EXTENDEDKEY
                        } else {
                            0
                        },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }
    fn send(input: INPUT) -> Result<()> {
        let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
        if sent != 1 {
            bail!(
                "SendInput returned {sent}/1 (Windows may block input to an elevated app): {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
    fn mouse_input(dx: i32, dy: i32, flags: u32) -> INPUT {
        mouse_input_data(dx, dy, flags, 0)
    }
    fn mouse_input_data(dx: i32, dy: i32, flags: u32, data: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    fn mouse_button(name: &str, down: bool) -> Result<INPUT> {
        let (flags, data) = match (name, down) {
            ("mouse:left", true) => (MOUSEEVENTF_LEFTDOWN, 0),
            ("mouse:left", false) => (MOUSEEVENTF_LEFTUP, 0),
            ("mouse:right", true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            ("mouse:right", false) => (MOUSEEVENTF_RIGHTUP, 0),
            ("mouse:middle", true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            ("mouse:middle", false) => (MOUSEEVENTF_MIDDLEUP, 0),
            ("mouse:x1", true) => (MOUSEEVENTF_XDOWN, 1),
            ("mouse:x1", false) => (MOUSEEVENTF_XUP, 1),
            ("mouse:x2", true) => (MOUSEEVENTF_XDOWN, 2),
            ("mouse:x2", false) => (MOUSEEVENTF_XUP, 2),
            _ => bail!("Unsupported mouse button: {name}"),
        };
        Ok(mouse_input_data(0, 0, flags, data))
    }
    fn wheel_input(value: i32, horizontal: bool) -> INPUT {
        mouse_input_data(
            0,
            0,
            if horizontal {
                MOUSEEVENTF_HWHEEL
            } else {
                MOUSEEVENTF_WHEEL
            },
            (value * 120) as u32,
        )
    }
    impl Sink for WindowsSink {
        fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
            self.target
                .as_mut()
                .map(|target| crate::feedback::FeedbackFrame::read(target.device()))
        }
        fn button(&mut self, name: &str, down: bool) -> Result<()> {
            if let Some(key) = name.strip_prefix("key:") {
                send(keyboard_input(key, down)?)
            } else {
                send(mouse_button(name, down)?)
            }
        }
        fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<()> {
            if horizontal.unsigned_abs() > 1024 || vertical.unsigned_abs() > 1024 {
                bail!("Oversized wheel output");
            }
            if vertical != 0 {
                send(wheel_input(vertical, false))?;
            }
            if horizontal != 0 {
                send(wheel_input(horizontal, true))?;
            }
            Ok(())
        }
        fn mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
            send(mouse_input(
                dx,
                dy,
                MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE,
            ))
        }
        fn gamepad(&mut self, p: PadState) -> Result<()> {
            let Some(target) = self.target.as_mut() else {
                // Dispatcher sends neutral on every stop, including keyboard-only sessions.
                anyhow::ensure!(p == PadState::default(), "Virtual gamepad was not enabled");
                return Ok(());
            };
            crate::flex_output::write_pad(target.device(), p);
            target.flush_checked()
        }
        fn gamepad_status(&self) -> Option<GamepadStatus> {
            self.gamepad.clone()
        }
        fn protected_devices(&self) -> Vec<String> {
            self.gamepad
                .as_ref()
                .map(|p| p.protected_devices.clone())
                .unwrap_or_default()
        }
        fn poll(&mut self) -> Result<()> {
            if let Some(target) = &mut self.target {
                if self.last_probe.elapsed() >= std::time::Duration::from_secs(1) {
                    // Changed reports are submitted by gamepad(). Probe idle
                    // targets here without a synchronous DS4 IOCTL per gyro sample.
                    target.flush_checked()?;
                    if let VirtualTarget::Xbox360(pad) = target {
                        let index = pad.player_index().context(
                            "虚拟 Xbox 未获得 XInput 玩家槽位；请检查是否已有 4 个 XInput 手柄",
                        )?;
                        if let Some(status) = &mut self.gamepad {
                            status.player_index = Some(index);
                        }
                    }
                    self.last_probe = std::time::Instant::now();
                }
            }
            Ok(())
        }
    }
    impl Drop for WindowsSink {
        fn drop(&mut self) {
            if let Some(target) = &mut self.target {
                target.device().reset_outputs();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn all_catalogue_keys_encode_paired_events_without_sending_input() {
            for key in crate::keyboard::keys() {
                let down = keyboard_input(&key.name, true).unwrap();
                let up = keyboard_input(&key.name, false).unwrap();
                assert_eq!(down.r#type, INPUT_KEYBOARD);
                let down = unsafe { down.Anonymous.ki };
                let up = unsafe { up.Anonymous.ki };
                assert_eq!(down.wVk, up.wVk);
                assert_eq!(down.wScan, up.wScan);
                assert_eq!(down.dwFlags | KEYEVENTF_KEYUP, up.dwFlags);
            }
            let input = |name| unsafe { keyboard_input(name, true).unwrap().Anonymous.ki };
            assert_eq!(input("CAPSLOCK").wScan, 0x3a);
            assert_eq!(input("ESC").wScan, 1);
            assert_eq!(input("NUMPADENTER").wScan, input("ENTER").wScan);
            assert_eq!(input("ENTER").dwFlags & KEYEVENTF_EXTENDEDKEY, 0);
            assert_ne!(input("NUMPADENTER").dwFlags & KEYEVENTF_EXTENDEDKEY, 0);
            assert_eq!(input("PAUSE").wVk, VK_PAUSE);
            assert_eq!(input("PRINTSCREEN").wVk, VK_SNAPSHOT);
            assert!(keyboard_input("F12", true).is_err());
        }
        #[test]
        fn side_buttons_and_wheel_reports_use_the_correct_flags_and_signed_data() {
            for (name, data) in [("mouse:x1", 1), ("mouse:x2", 2)] {
                for down in [false, true] {
                    let event = mouse_button(name, down).unwrap();
                    let mouse = unsafe { event.Anonymous.mi };
                    assert_eq!(mouse.mouseData, data);
                    assert_eq!(
                        mouse.dwFlags,
                        if down {
                            MOUSEEVENTF_XDOWN
                        } else {
                            MOUSEEVENTF_XUP
                        }
                    );
                }
            }
            for horizontal in [false, true] {
                for value in [-2, 1] {
                    let event = wheel_input(value, horizontal);
                    let mouse = unsafe { event.Anonymous.mi };
                    assert_eq!(mouse.mouseData as i32, value * 120);
                    assert_eq!(
                        mouse.dwFlags,
                        if horizontal {
                            MOUSEEVENTF_HWHEEL
                        } else {
                            MOUSEEVENTF_WHEEL
                        }
                    );
                }
            }
        }
    }
}
#[cfg(windows)]
pub use windows::{emergency_stop, probe_virtual_driver, WindowsSink};
