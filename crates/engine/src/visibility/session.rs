//! Ordinary-user recovery companion using the verified private-pipe transport.
//! Driver access follows the official HidHide CLI; this helper never elevates,
//! installs a driver, accepts an executable path or generates input.
use super::{policy, topology, VisibilityStatus};
use crate::hidmaestro::transport::{Pipe, Process};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[path = "helper.rs"]
mod helper;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Acquire {
        device_path: String,
    },
    Preparing {},
    Verify {
        protected: Vec<String>,
        expect_gamepad: bool,
    },
    Heartbeat {},
    Release {},
    Recover {},
    Shutdown {},
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    State { status: VisibilityStatus },
    Error { message: String },
}

struct Session {
    pipe: Pipe,
    peer: Process,
    _child: Child,
    status: VisibilityStatus,
    device: String,
    desired: bool,
    last: Instant,
    pending: Option<Instant>,
    broken: bool,
}
fn manager() -> &'static Mutex<Option<Session>> {
    static MANAGER: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    MANAGER.get_or_init(|| Mutex::new(None))
}
fn root() -> Result<PathBuf> {
    let path = std::env::var_os("MOTION_STUDIO_DATA_DIR")
        .map(PathBuf::from)
        .context("自动隐藏需要 Motion Studio 提供恢复目录")?;
    ensure!(
        path.is_absolute() && path.is_dir(),
        "自动隐藏恢复目录不可用"
    );
    Ok(path)
}
fn pending_record() -> Result<bool> {
    Ok(root()?.join(helper::JOURNAL_FILE).try_exists()?)
}
impl Session {
    fn start() -> Result<Self> {
        root()?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut pipe = Pipe::server(&token)?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--hidhide-lease", &std::process::id().to_string(), &token])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let child = command.spawn().context("启动手柄隐藏恢复进程")?;
        let peer = Process::open(child.id())?;
        pipe.accept(&peer, Instant::now() + Duration::from_secs(5))?;
        let mut session = Self {
            pipe,
            peer,
            _child: child,
            status: VisibilityStatus::default(),
            device: String::new(),
            desired: false,
            last: Instant::now(),
            pending: None,
            broken: false,
        };
        session.receive(Duration::from_secs(8), false)?;
        Ok(session)
    }
    fn receive(&mut self, timeout: Duration, initializing_output: bool) -> Result<()> {
        let end = Instant::now() + timeout;
        loop {
            if let Some(reply) = self.pipe.poll::<Reply>()? {
                return self.accept(reply);
            }
            ensure!(self.peer.alive(), "手柄隐藏恢复进程已退出");
            ensure!(Instant::now() < end, "手柄隐藏恢复进程响应超时");
            if initializing_output {
                crate::hidmaestro::heartbeat()?;
                ensure!(!crate::output::emergency_stop(), "F12 已取消手柄隐藏准备");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn accept(&mut self, reply: Reply) -> Result<()> {
        match reply {
            Reply::State { status } => {
                self.status = status;
                Ok(())
            }
            Reply::Error { message } => {
                self.status.error = Some(message.clone());
                bail!("{message}")
            }
        }
    }
    fn call(&mut self, request: &Control, initializing_output: bool) -> Result<()> {
        let result = (|| {
            ensure!(!self.broken && self.peer.alive(), "手柄隐藏恢复进程已退出");
            if self.pending.take().is_some() {
                self.receive(Duration::from_secs(3), initializing_output)?;
            }
            self.pipe.send(request)?;
            self.receive(Duration::from_secs(8), initializing_output)?;
            self.last = Instant::now();
            Ok(())
        })();
        if result.is_err() {
            self.broken = true;
        }
        result
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pipe.send(&Control::Shutdown {});
    }
}
fn connected(slot: &mut Option<Session>) -> Result<&mut Session> {
    if slot.as_ref().is_some_and(|s| s.broken || !s.peer.alive()) {
        slot.take();
    }
    if slot.is_none() {
        *slot = Some(Session::start()?);
    }
    Ok(slot.as_mut().unwrap())
}

pub fn acquire(device_path: &str) -> Result<Option<VisibilityStatus>> {
    // Do not spawn a helper or change driver state while the chosen pad is absent.
    let Some(targets) = topology::selected(device_path)? else {
        return Ok(None);
    };
    let mut slot = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("隐藏会话锁不可用"))?;
    let session = connected(&mut slot)?;
    if !session.desired || session.device != device_path || targets != session.status.hidden_paths {
        session.call(
            &Control::Acquire {
                device_path: device_path.into(),
            },
            false,
        )?;
        ensure!(
            session.status.active && session.status.error.is_none(),
            "{}",
            session
                .status
                .error
                .as_deref()
                .unwrap_or("实体手柄隐藏尚未确认")
        );
        session.device = device_path.into();
        session.desired = true;
    }
    Ok(Some(session.status.clone()))
}
pub fn preparing() -> Result<()> {
    let mut slot = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("隐藏会话锁不可用"))?;
    if let Some(session) = slot.as_mut().filter(|s| s.desired) {
        session.call(&Control::Preparing {}, false)?;
    }
    Ok(())
}
pub fn verify_outputs(protected: Vec<String>, expect_gamepad: bool) -> Result<()> {
    let mut slot = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("隐藏会话锁不可用"))?;
    if let Some(session) = slot.as_mut().filter(|s| s.desired) {
        session.call(
            &Control::Verify {
                protected,
                expect_gamepad,
            },
            true,
        )?;
        ensure!(
            session.status.active && session.status.error.is_none(),
            "隐藏状态未确认，已暂停输出"
        );
    }
    Ok(())
}
pub fn release() -> Result<VisibilityStatus> {
    let mut slot = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("隐藏会话锁不可用"))?;
    if slot.is_none()
        && (std::env::var_os("MOTION_STUDIO_DATA_DIR").is_none() || !pending_record()?)
    {
        return Ok(VisibilityStatus::default());
    }
    let session = connected(&mut slot)?;
    session.desired = false;
    session.call(&Control::Release {}, false)?;
    ensure!(
        !session.status.active && session.status.error.is_none(),
        "{}",
        session
            .status
            .error
            .as_deref()
            .unwrap_or("实体手柄恢复尚未确认")
    );
    let status = session.status.clone();
    slot.take();
    Ok(status)
}
pub fn recover() -> Result<VisibilityStatus> {
    if std::env::var_os("MOTION_STUDIO_DATA_DIR").is_none() {
        return Ok(VisibilityStatus::default());
    }
    let mut slot = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("隐藏会话锁不可用"))?;
    if slot.is_none() && !pending_record()? {
        return Ok(VisibilityStatus::default());
    }
    let session = connected(&mut slot)?;
    ensure!(!session.desired, "请先停止映射，再恢复自动隐藏规则");
    session.call(&Control::Recover {}, false)?;
    ensure!(
        !session.status.active && session.status.error.is_none(),
        "{}",
        session
            .status
            .error
            .as_deref()
            .unwrap_or("隐藏恢复尚未完成")
    );
    let status = session.status.clone();
    slot.take();
    Ok(status)
}
pub fn heartbeat() -> Result<Option<VisibilityStatus>> {
    let mut slot = manager()
        .lock()
        .map_err(|_| anyhow::anyhow!("隐藏会话锁不可用"))?;
    let Some(session) = slot.as_mut() else {
        return Ok(None);
    };
    if session.broken || !session.peer.alive() {
        if session.desired {
            bail!("手柄隐藏恢复进程已退出，已停止映射")
        }
        slot.take();
        return Ok(None);
    }
    if let Some(pending) = session.pending {
        let result = (|| {
            if let Some(reply) = session.pipe.poll::<Reply>()? {
                session.pending = None;
                session.accept(reply)?;
            } else {
                ensure!(
                    pending.elapsed() < Duration::from_secs(2),
                    "手柄隐藏恢复进程未响应"
                );
            }
            Ok(())
        })();
        if result.is_err() {
            session.broken = true;
        }
        result?;
    }
    if session.desired {
        ensure!(
            session.status.active && session.status.error.is_none(),
            "{}",
            session
                .status
                .error
                .as_deref()
                .unwrap_or("实体手柄隐藏会话已结束")
        );
    }
    if session.pending.is_none() && session.last.elapsed() >= Duration::from_millis(400) {
        session.pipe.send(&Control::Heartbeat {})?;
        session.pending = Some(Instant::now());
        session.last = Instant::now();
    }
    Ok(Some(session.status.clone()))
}
pub fn run_helper(parent: u32, token: &str) -> Result<()> {
    helper::run(parent, token)
}
