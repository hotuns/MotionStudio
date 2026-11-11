//! FlexInput's native HIDMaestro client, with Motion Studio's output lifecycle.
//! Only the data path writes input. The elevated helper creates fixed devices,
//! owns the driver release leases, and tears them down on worker death/stall.
mod helper;
mod keymouse;
mod reports;
pub(crate) mod transport;
use crate::{engine::PadState, output::Sink, profile::GamepadTarget};
use anyhow::{bail, Context, Result};
use flexinput_hidmaestro::{
    install::{self, DriverState},
    shm::DeliveryAck,
    InputSection,
};
use flexinput_virtual::{hidmaestro_device::HidMaestroDevice, VirtualDevice};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use transport::{Pipe, Process};

const HEARTBEAT: Duration = Duration::from_millis(400);
const LEASE_TIMEOUT: Duration = Duration::from_secs(2);
const START_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Devices {
    keyboard: u32,
    mouse: u32,
    pad: Option<u32>,
    protected: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Begin {},
    Configure { target: GamepadTarget },
    Heartbeat {},
    Shutdown {},
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Ready { devices: Devices },
    Ok,
    Error { message: String },
}

pub fn driver_status() -> (String, Option<String>) {
    match install::driver_state() {
        DriverState::Missing => ("missing".into(), None),
        DriverState::Partial { .. } => (
            "partial".into(),
            Some(
                "HIDMaestro 驱动包不完整，请先在原安装工具中修复；本应用不会覆盖其他软件的驱动"
                    .into(),
            ),
        ),
        DriverState::Complete => match check_versions() {
            Ok(()) => ("running".into(), None),
            Err(e) => ("incompatible".into(), Some(e.to_string())),
        },
    }
}
fn check_versions() -> Result<()> {
    let expected =
        install::inf_driver_version(flexinput_hidmaestro::deploy::payload::HIDMAESTRO_INF)
            .context("Invalid bundled driver version")?;
    for path in [
        install::installed_inf_path(),
        install::installed_xusb_inf_path(),
    ] {
        let path = path.context("HIDMaestro 驱动 INF 缺失")?;
        let bytes = std::fs::read(path)?;
        let actual = install::inf_driver_version(&bytes).context("无法读取 HIDMaestro 驱动版本")?;
        if actual != expected {
            bail!("HIDMaestro 驱动版本不匹配（需要 {expected}，检测到 {actual}）；请使用对应版本，避免影响其他应用");
        }
    }
    Ok(())
}
pub fn probe() -> Result<()> {
    let (state, error) = driver_status();
    if state != "running" {
        bail!(
            "{}",
            error.unwrap_or_else(|| "请先到「设置与组件」安装 HIDMaestro 输出驱动".into())
        );
    }
    Ok(())
}

struct Session {
    pipe: Pipe,
    child: Process,
    devices: Devices,
    target: GamepadTarget,
    last: Instant,
    heartbeat_pending: Option<Instant>,
    token: String,
}
fn manager() -> &'static Mutex<Option<Session>> {
    static S: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}
impl Session {
    fn start(exe: &Path, installing: bool) -> Result<Self> {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut pipe = Pipe::server(&token)?;
        let child = transport::elevate(
            exe,
            if installing {
                "--hidmaestro-install"
            } else {
                "--hidmaestro-output"
            },
            &token,
        )?;
        pipe.accept(&child, Instant::now() + START_TIMEOUT)?;
        pipe.send(&Control::Begin {})?;
        let reply: Reply = pipe.receive(
            &child,
            if installing {
                Duration::from_secs(180)
            } else {
                START_TIMEOUT
            },
        )?;
        let devices = match reply {
            Reply::Ready { devices } if !installing => devices,
            Reply::Ok if installing => Devices {
                keyboard: 0,
                mouse: 0,
                pad: None,
                protected: vec![],
            },
            Reply::Error { message } => bail!("{message}"),
            _ => bail!("Invalid HIDMaestro helper startup reply"),
        };
        Ok(Self {
            pipe,
            child,
            devices,
            target: GamepadTarget::None,
            last: Instant::now(),
            heartbeat_pending: None,
            token,
        })
    }
    fn call(&mut self, control: &Control, timeout: Duration) -> Result<Reply> {
        if !self.child.alive() {
            bail!("HIDMaestro 输出辅助进程已停止，请重新开始映射");
        }
        // Configuration may wait; per-frame output must not wait for the
        // helper's scheduler merely to renew a lease.
        if self.heartbeat_pending.take().is_some() {
            match self
                .pipe
                .receive::<Reply>(&self.child, Duration::from_millis(500))?
            {
                Reply::Ok => {}
                Reply::Error { message } => bail!("{message}"),
                _ => bail!("Invalid HIDMaestro heartbeat reply"),
            }
        }
        self.pipe.send(control)?;
        let reply: Reply = self.pipe.receive(&self.child, timeout)?;
        if let Reply::Error { message } = reply {
            bail!("{message}");
        }
        self.last = Instant::now();
        Ok(reply)
    }
    fn heartbeat(&mut self) -> Result<()> {
        if !self.child.alive() {
            bail!("HIDMaestro 输出已断开，映射已停止");
        }
        if let Some(sent) = self.heartbeat_pending {
            match self.pipe.poll::<Reply>()? {
                Some(Reply::Ok) => {
                    self.heartbeat_pending = None;
                    self.last = Instant::now();
                }
                Some(Reply::Error { message }) => bail!("{message}"),
                Some(_) => bail!("Invalid HIDMaestro heartbeat reply"),
                None if sent.elapsed() >= Duration::from_millis(500) => {
                    bail!("HIDMaestro 输出辅助进程心跳超时")
                }
                None => return Ok(()),
            }
        }
        if self.last.elapsed() >= HEARTBEAT {
            self.pipe.send(&Control::Heartbeat {})?;
            self.heartbeat_pending = Some(Instant::now());
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pipe.send(&Control::Shutdown {});
    }
}

/// Called by the main runtime even in standby. No independent heartbeat thread:
/// if that runtime hangs, the privileged helper's lease must actually expire.
pub fn heartbeat() -> Result<()> {
    let mut state = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("Output session lock poisoned"))?;
    if let Some(session) = state.as_mut() {
        if let Err(e) = session.heartbeat() {
            state.take();
            return Err(e);
        }
    }
    Ok(())
}
pub fn shutdown() -> Result<()> {
    let mut state = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("Output session lock poisoned"))?;
    if let Some(mut session) = state.take() {
        let _ = session.pipe.send(&Control::Shutdown {});
        let deadline = Instant::now() + Duration::from_secs(3);
        while session.child.alive() {
            if Instant::now() >= deadline {
                bail!("前一 HIDMaestro 输出会话正在清理设备，请稍后重新开始映射");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}
pub fn install_driver(exe: &Path) -> Result<()> {
    let _session = Session::start(exe, true)?;
    Ok(())
}
pub fn run_helper(parent_pid: u32, token: &str, installing: bool) -> Result<()> {
    helper::run(parent_pid, token, installing)
}

struct Writer {
    input: InputSection,
    ack: DeliveryAck,
    owner: Process,
    pending: bool,
}
impl Writer {
    fn open(index: u32, owner: &Process) -> Result<Self> {
        Ok(Self {
            input: InputSection::open(index)?,
            ack: DeliveryAck::open(index)?,
            owner: owner.try_clone()?,
            pending: false,
        })
    }
}
impl keymouse::ReportWriter for Writer {
    fn write(&mut self, data: &[u8], delivery: keymouse::Delivery) -> Result<()> {
        if !self.owner.alive() {
            bail!("HIDMaestro 输出辅助进程已停止");
        }
        // FlexInput's continuous mouse path publishes raw frames at its I/O
        // cadence; it does not synchronously wait for every motion ACK.
        if delivery == keymouse::Delivery::Confirmed {
            self.finish()?;
        }
        self.input.write_frame(data, None);
        self.pending = true;
        if delivery != keymouse::Delivery::Stream {
            self.finish()?;
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if !self.owner.alive() {
            bail!("HIDMaestro 输出辅助进程已停止");
        }
        if self.pending && !self.ack.wait_for(&self.input, Duration::from_millis(50)) {
            bail!("HIDMaestro 未确认键鼠报告，已暂停输出；请检查驱动和设备状态");
        }
        self.pending = false;
        Ok(())
    }
}
pub struct HidMaestroSink {
    target: GamepadTarget,
    protected: Vec<String>,
    keymouse: keymouse::KeyMouse,
    pad: Option<HidMaestroDevice>,
    held: BTreeSet<String>,
    token: String,
}
impl HidMaestroSink {
    pub fn new(target: GamepadTarget) -> Result<Self> {
        probe()?;
        let mut state = manager()
            .lock()
            .map_err(|_| anyhow::anyhow!("Output session lock poisoned"))?;
        if state.as_ref().is_some_and(|s| !s.child.alive()) {
            state.take();
        }
        if state.is_none() {
            *state = Some(Session::start(&std::env::current_exe()?, false)?);
        }
        let result = (|| {
            let session = state.as_mut().unwrap();
            if session.target != target {
                match session.call(&Control::Configure { target }, START_TIMEOUT)? {
                    Reply::Ready { devices } => {
                        session.devices = devices;
                        session.target = target;
                    }
                    _ => bail!("Invalid HIDMaestro configure reply"),
                }
            }
            let devices = session.devices.clone();
            let token = session.token.clone();
            let pad = if target == GamepadTarget::None {
                None
            } else {
                let index = devices.pad.context("原生手柄输出未返回设备索引")?;
                let mut device = HidMaestroDevice::open(
                    "motion-studio.gamepad",
                    "Motion Studio gamepad",
                    reports::profile_json(target).context("手柄输出类型无效")?,
                    index,
                )
                .context("无法打开原生手柄输出配置")?;
                if !device.is_connected() {
                    bail!("原生手柄输出共享通道未就绪");
                }
                // The upstream device primes a fresh neutral sequence after its
                // driver is started. Do the same when opening our owned section.
                device.reset_outputs();
                Some(device)
            };
            Ok(Self {
                target,
                protected: devices.protected,
                keymouse: keymouse::KeyMouse::start(
                    Writer::open(devices.keyboard, &session.child)?,
                    Writer::open(devices.mouse, &session.child)?,
                )?,
                pad,
                held: BTreeSet::new(),
                token,
            })
        })();
        if result.is_err() {
            state.take();
        }
        result
    }
    fn check(&self) -> Result<()> {
        let mut state = manager()
            .lock()
            .map_err(|_| anyhow::anyhow!("Output session lock poisoned"))?;
        let s = state.as_mut().context("HIDMaestro 输出会话已关闭")?;
        if s.token != self.token {
            bail!("HIDMaestro output session replaced");
        }
        s.heartbeat()
    }
    fn mouse_buttons(&self) -> u8 {
        reports::mouse(&self.held, 0, 0, 0, 0)[0]
    }
}
impl Sink for HidMaestroSink {
    fn feedback(&mut self) -> Option<crate::feedback::FeedbackFrame> {
        self.pad
            .as_mut()
            .map(|pad| crate::feedback::FeedbackFrame::read(pad))
    }
    fn gamepad_status(&self) -> Option<crate::output::GamepadStatus> {
        self.pad.as_ref().map(|pad| crate::output::GamepadStatus {
            target: self.target,
            backend: crate::output::GamepadBackend::HidMaestro,
            connected: pad.is_connected(),
            player_index: None,
            protected_devices: self.protected.clone(),
        })
    }
    fn protected_devices(&self) -> Vec<String> {
        self.protected.clone()
    }
    fn poll(&mut self) -> Result<()> {
        self.check()?;
        self.keymouse.poll()
    }
    fn idle_gamepad(&mut self) -> Result<()> {
        if let Some(device) = &mut self.pad {
            device.flush();
        }
        Ok(())
    }
    fn stats(&self) -> crate::output::OutputStats {
        self.keymouse.stats()
    }
    fn mouse_trace(&self) -> Option<Vec<crate::output::MouseTracePoint>> {
        Some(self.keymouse.mouse_trace())
    }
    fn cancel_pending(&mut self) -> Result<()> {
        // Priority reset discards old motion/presses and waits for neutral.
        // Virtual-pad neutral is still attempted by Dispatcher even on error.
        let result = self.keymouse.reset();
        self.held.clear();
        result
    }
    fn button(&mut self, name: &str, down: bool) -> Result<()> {
        self.check()?;
        if !crate::bindings::valid_button(name) {
            bail!("Unsupported HID binding: {name}");
        }
        if self.held.contains(name) == down {
            return Ok(());
        }
        let mut next = self.held.clone();
        if down {
            next.insert(name.into());
        } else {
            next.remove(name);
        }
        let report = reports::keyboard(&next)?; // validate overflow before changing output
        if name.starts_with("key:") {
            self.keymouse.submit(keymouse::Report::Keyboard(report))?;
        } else {
            let buttons = reports::mouse(&next, 0, 0, 0, 0)[0];
            self.keymouse.submit(keymouse::Report::Buttons(buttons))?;
        }
        self.held = next;
        Ok(())
    }
    fn mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.check()?;
        self.keymouse.motion(self.mouse_buttons(), dx, dy)
    }
    fn idle_mouse(&mut self) -> Result<()> {
        self.keymouse.idle_motion(self.mouse_buttons())
    }
    fn scroll(&mut self, h: i32, v: i32) -> Result<()> {
        self.check()?;
        self.keymouse.submit(keymouse::Report::Scroll {
            buttons: self.mouse_buttons(),
            horizontal: h,
            vertical: v,
        })
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()> {
        self.check()?;
        let Some(device) = self.pad.as_mut() else {
            anyhow::ensure!(pad == PadState::default(), "当前输出没有已连接的虚拟手柄");
            return Ok(());
        };
        crate::flex_output::write_pad(device, pad);
        device.flush();
        Ok(())
    }
}
impl Drop for HidMaestroSink {
    fn drop(&mut self) {
        // Join before any new session can reuse this device's section/index.
        let released = self.keymouse.stop();
        if self.check().is_err() {
            return;
        }
        // The driver also holds these neutral release frames. These local
        // writes cover normal stop; the helper/driver cover process termination.
        self.held.clear();
        if let Some(mut device) = self.pad.take() {
            device.reset_outputs();
            drop(device);
        }
        if released.is_err() {
            let _ = shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privileged_protocol_accepts_only_fixed_targets_and_control_messages() {
        assert!(
            serde_json::from_str::<Control>(r#"{"type":"configure","target":"dualshock_4"}"#)
                .is_ok()
        );
        for text in [
            r#"{"type":"install","path":"C:/arbitrary.inf"}"#,
            r#"{"type":"configure","target":"custom"}"#,
            r#"{"type":"configure","target":"none","profile_json":"arbitrary"}"#,
            r#"{"type":"heartbeat","driver":"foreign"}"#,
        ] {
            assert!(serde_json::from_str::<Control>(text).is_err(), "{text}");
        }
    }
    #[test]
    fn helper_error_is_reported_without_creating_a_fallback_output() {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut server = Pipe::server(&token).unwrap();
        let pid = std::process::id();
        let child = Process::open(pid).unwrap();
        let other_token = token.clone();
        let peer = std::thread::spawn(move || {
            let mut client = Pipe::connect(&other_token, pid).unwrap();
            let process = Process::open(pid).unwrap();
            let _: Control = client.receive(&process, Duration::from_secs(2)).unwrap();
            client
                .send(&Reply::Error {
                    message: "driver disconnected".into(),
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        server
            .accept(&child, Instant::now() + Duration::from_secs(2))
            .unwrap();
        let mut session = Session {
            pipe: server,
            child,
            devices: Devices {
                keyboard: 0,
                mouse: 1,
                pad: None,
                protected: vec![],
            },
            target: GamepadTarget::None,
            last: Instant::now(),
            heartbeat_pending: None,
            token,
        };
        assert!(session
            .call(&Control::Heartbeat {}, Duration::from_secs(2))
            .unwrap_err()
            .to_string()
            .contains("driver disconnected"));
        peer.join().unwrap();
    }
    #[test]
    fn heartbeat_does_not_wait_in_the_input_path_and_an_unanswered_lease_fails_closed() {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut pipe = Pipe::server(&token).unwrap();
        let pid = std::process::id();
        let process = Process::open(pid).unwrap();
        let other_token = token.clone();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (sent_tx, sent_rx) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut client = Pipe::connect(&other_token, pid).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let process = Process::open(pid).unwrap();
            assert!(matches!(
                client
                    .receive::<Control>(&process, Duration::from_secs(2))
                    .unwrap(),
                Control::Heartbeat {}
            ));
            client.send(&Reply::Ok).unwrap();
            sent_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        pipe.accept(&process, Instant::now() + Duration::from_secs(2))
            .unwrap();
        let mut session = Session {
            pipe,
            child: process,
            devices: Devices {
                keyboard: 0,
                mouse: 1,
                pad: None,
                protected: vec![],
            },
            target: GamepadTarget::None,
            last: Instant::now() - HEARTBEAT,
            heartbeat_pending: None,
            token,
        };
        // The peer intentionally cannot reply until this call returns.
        session.heartbeat().unwrap();
        assert!(session.heartbeat_pending.is_some());
        release_tx.send(()).unwrap();
        sent_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        session.heartbeat().unwrap();
        assert!(session.heartbeat_pending.is_none());
        session.heartbeat_pending = Some(Instant::now() - Duration::from_millis(501));
        assert!(session
            .heartbeat()
            .unwrap_err()
            .to_string()
            .contains("心跳超时"));
        release_tx.send(()).unwrap();
        peer.join().unwrap();
    }
}
