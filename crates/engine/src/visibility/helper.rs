use super::*;
use crate::hidmaestro::transport::PIPE_MESSAGE_BYTES;
use crate::visibility::control::{Gate, RealDriver};
use policy::{Driver, Journal, Record};
use std::{fs, io::Read};
use windows_sys::Win32::{Foundation::*, System::Threading::*};

use crate::visibility::SESSION_JOURNAL;
struct FileJournal(PathBuf);
impl Journal for FileJournal {
    fn load(&self) -> Result<Option<Record>> {
        let file = match fs::File::open(&self.0) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 2 * 1024 * 1024, "自动隐藏恢复记录过大");
        let record: Record =
            serde_json::from_slice(&bytes).context("自动隐藏恢复记录损坏；未改动其他隐藏规则")?;
        record.validate()?;
        Ok(Some(record))
    }
    fn save(&mut self, record: &Record) -> Result<()> {
        crate::storage::atomic_write(&self.0, &serde_json::to_vec_pretty(record)?)
    }
    fn clear(&mut self) -> Result<()> {
        match fs::remove_file(&self.0) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
fn image(process: HANDLE) -> Result<String> {
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    ensure!(
        unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_NATIVE,
                buffer.as_mut_ptr(),
                &mut length,
            )
        } != 0,
        "无法确认隐藏会话的引擎身份"
    );
    Ok(String::from_utf16(&buffer[..length as usize])?)
}

struct Lease {
    journal: FileJournal,
    gate: Option<Gate>,
    record: Option<Record>,
    status: VisibilityStatus,
    baseline: Vec<String>,
    protected: Vec<String>,
    expect_gamepad: bool,
    preparing: bool,
    releasing: bool,
    last: Instant,
    last_audit: Instant,
    engine: String,
}
impl Lease {
    fn restore(&mut self) -> Result<()> {
        self.releasing = true;
        self.status.requested = false;
        if self.journal.load()?.is_some() {
            if self.gate.is_none() {
                self.gate = Some(Gate::acquire()?);
            }
            let result =
                RealDriver::open().and_then(|mut d| policy::restore(&mut d, &mut self.journal));
            if let Err(error) = result {
                self.status.error = Some(format!("实体手柄恢复未完成：{error:#}"));
                return Err(error);
            }
        }
        self.record = None;
        self.gate.take();
        self.protected.clear();
        self.baseline.clear();
        self.status = VisibilityStatus::default();
        self.releasing = false;
        Ok(())
    }
    fn acquire(&mut self, path: &str, gameinput_container: Option<&str>) -> Result<()> {
        if self.record.is_some() || self.journal.load()?.is_some() {
            self.restore()?;
        }
        let targets = topology::selected(path, gameinput_container)?
            .context("所选实体手柄已断开，请重新开始映射")?;
        let status = VisibilityStatus {
            requested: true,
            active: true,
            hidden_paths: targets.clone(),
            error: None,
        };
        let reply = Reply::State { status: status.clone() };
        ensure!(
            serde_json::to_vec(&reply)?.len() < PIPE_MESSAGE_BYTES,
            "所选接收器接口过多，无法自动隐藏。请停止后手动管理隐藏"
        );
        self.gate = Some(Gate::acquire()?);
        self.baseline = topology::virtual_ids(&topology::inventory());
        let mut driver = RealDriver::open()?;
        let record = policy::acquire(&mut driver, &mut self.journal, targets, self.engine.clone())?;
        self.status = status;
        self.record = Some(record);
        self.preparing = true;
        self.releasing = false;
        self.last = Instant::now();
        Ok(())
    }
    fn audit(&mut self, strict: bool) -> Result<()> {
        let record = self.record.as_ref().context("自动隐藏会话尚未建立")?;
        let mut driver = RealDriver::open()?;
        policy::check(&mut driver, record)?;
        let state = driver.read()?;
        drop(driver);
        topology::verify(
            &topology::inventory(),
            &state.hidden,
            &self.protected,
            &self.baseline,
            self.expect_gamepad,
            strict,
        )
    }
    fn expired(&self) -> bool {
        self.record.is_some()
            && self.last.elapsed() > Duration::from_secs(if self.preparing { 120 } else { 2 })
    }
}

pub(super) fn run(parent_pid: u32, token: &str) -> Result<()> {
    let parent = Process::open(parent_pid)?;
    let engine = image(unsafe { GetCurrentProcess() })?;
    ensure!(
        image(parent.0)?.eq_ignore_ascii_case(&engine),
        "隐藏恢复进程只接受同一引擎的父进程"
    );
    let mut pipe = Pipe::connect(token, parent_pid)?;
    let mut lease = Lease {
        journal: FileJournal(root()?.join(SESSION_JOURNAL)),
        gate: None,
        record: None,
        status: VisibilityStatus::default(),
        baseline: vec![],
        protected: vec![],
        expect_gamepad: false,
        preparing: false,
        releasing: false,
        last: Instant::now(),
        last_audit: Instant::now(),
        engine,
    };
    if lease.journal.load()?.is_some() {
        if let Err(error) = lease.restore() {
            let _ = pipe.send(&Reply::Error {
                message: format!("上次隐藏尚未恢复：{error:#}"),
            });
            return Err(error);
        }
    }
    pipe.send(&Reply::State {
        status: lease.status.clone(),
    })?;
    let result = (|| -> Result<()> {
        loop {
            if !parent.alive() || lease.expired() {
                break;
            }
            if lease.releasing && lease.last_audit.elapsed() > Duration::from_secs(1) {
                let _ = lease.restore();
                lease.last_audit = Instant::now();
            }
            let control = match pipe.poll::<Control>() {
                Ok(value) => value,
                Err(_) => break,
            };
            if let Some(control) = control {
                let mut shutdown = false;
                let result = (|| -> Result<()> {
                    match control {
                        Control::Acquire {
                            device_path,
                            gameinput_container,
                        } => lease.acquire(&device_path, gameinput_container.as_deref()),
                        Control::Preparing {} => {
                            ensure!(
                                lease.record.is_some() && !lease.releasing,
                                "隐藏会话尚未就绪"
                            );
                            lease.preparing = true;
                            lease.last = Instant::now();
                            Ok(())
                        }
                        Control::Verify {
                            protected,
                            expect_gamepad,
                        } => {
                            ensure!(
                                protected.len() <= 8
                                    && protected
                                        .iter()
                                        .all(|id| id.len() <= 256 && !id.contains('\0')),
                                "无效的虚拟输出标识"
                            );
                            lease.protected = protected;
                            lease.expect_gamepad = expect_gamepad;
                            let deadline = Instant::now() + Duration::from_secs(2);
                            loop {
                                match lease.audit(true) {
                                    Ok(()) => break,
                                    Err(error) if Instant::now() >= deadline || !parent.alive() => {
                                        return Err(error)
                                    }
                                    Err(_) => std::thread::sleep(Duration::from_millis(20)),
                                }
                            }
                            lease.preparing = false;
                            lease.last = Instant::now();
                            Ok(())
                        }
                        Control::Heartbeat {} => {
                            lease.last = Instant::now();
                            if lease.record.is_some()
                                && !lease.preparing
                                && !lease.releasing
                                && lease.last_audit.elapsed() >= Duration::from_secs(1)
                            {
                                lease.audit(false)?;
                                lease.last_audit = Instant::now();
                            }
                            Ok(())
                        }
                        Control::Release {} | Control::Recover {} => lease.restore(),
                        Control::Shutdown {} => {
                            shutdown = true;
                            Ok(())
                        }
                    }
                })();
                if let Err(error) = result {
                    lease.status.error = Some(format!("{error:#}"));
                    pipe.send(&Reply::Error {
                        message: format!("{error:#}").chars().take(1000).collect(),
                    })?;
                    // Parent closes outputs before releasing. If it cannot do
                    // so, pipe EOF / the lease timeout performs recovery below.
                    break;
                } else {
                    pipe.send(&Reply::State {
                        status: lease.status.clone(),
                    })?;
                }
                if shutdown {
                    break;
                }
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    })();
    // Give the independent output owner time to publish its crash-neutral
    // reports before exposing the physical pad again.
    if lease.record.is_some() {
        std::thread::sleep(Duration::from_millis(500));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match lease.restore() {
            Ok(()) => break,
            Err(error) if Instant::now() >= deadline => {
                return Err(error.context("恢复记录已保留，下次启动会再次恢复"))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_survives_companion_exit_and_corruption_never_becomes_empty_policy() {
        let directory =
            std::env::temp_dir().join(format!("motion-visibility-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join(SESSION_JOURNAL);
        let record = Record {
            version: 1,
            targets: vec![r"HID\VID_1234&PID_5678\PAD".into()],
            added: vec![r"HID\VID_1234&PID_5678\PAD".into()],
            engine: r"\Device\Volume\motion-engine.exe".into(),
            before_active: false,
            before_hidden_digest: "0".repeat(64),
            expected_apps_digest: "1".repeat(64),
        };
        FileJournal(path.clone()).save(&record).unwrap();
        let mut restarted = FileJournal(path.clone());
        assert_eq!(restarted.load().unwrap().unwrap().added, record.added);
        fs::write(&path, b"{broken record}").unwrap();
        assert!(restarted.load().is_err());
        assert!(path.exists());
        restarted.clear().unwrap();
        assert!(restarted.load().unwrap().is_none());
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn companion_protocol_has_no_arbitrary_driver_or_process_operation() {
        assert!(serde_json::from_str::<Control>(r#"{"type":"set_blacklist","paths":[]}"#).is_err());
        assert!(
            serde_json::from_str::<Control>(r#"{"type":"release","path":"C:\\Windows"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<Control>(r#"{"type":"heartbeat","run":"bad.exe"}"#).is_err()
        );
    }
}
