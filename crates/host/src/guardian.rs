use anyhow::{bail, Context, Result};
use motion_core::{
    output::{Sink, WindowsSink},
    profile::GamepadTarget,
    protocol::read_packet,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
struct LeaseMessage {
    held: BTreeSet<String>,
}

/// The independent guardian holds the output mutex until cleanup is complete.
/// A crashed/hung worker therefore cannot overlap its successor's key releases.
pub struct Guard {
    child: Child,
    input: Option<ChildStdin>,
    held: BTreeSet<String>,
    last_heartbeat: Instant,
}
impl Guard {
    pub fn start() -> Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--guardian")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("启动输出保护进程")?;
        let stdout = child.stdout.take().context("Missing guardian stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = tx.send(result);
        });
        let ready = rx.recv_timeout(Duration::from_secs(3));
        if !matches!(ready, Ok(Ok(ref text)) if text.trim() == "ready") {
            let _ = child.kill();
            let _ = child.wait();
            bail!("输出保护无法启动，可能已有另一 Motion Studio 正在映射");
        }
        Ok(Self {
            input: child.stdin.take(),
            child,
            held: BTreeSet::new(),
            last_heartbeat: Instant::now(),
        })
    }
    fn publish(&mut self, held: BTreeSet<String>) -> Result<()> {
        if self.child.try_wait()?.is_some() {
            bail!("输出保护已停止，映射已暂停");
        }
        let input = self.input.as_mut().context("输出保护管道已关闭")?;
        serde_json::to_writer(&mut *input, &LeaseMessage { held: held.clone() })?;
        input.write_all(b"\n")?;
        input.flush()?;
        self.held = held;
        self.last_heartbeat = Instant::now();
        Ok(())
    }
    pub fn before(&mut self, next: &BTreeSet<String>) -> Result<()> {
        if next != &self.held {
            self.publish(self.held.union(next).cloned().collect())?;
        }
        Ok(())
    }
    pub fn after(&mut self, next: &BTreeSet<String>) -> Result<()> {
        if next != &self.held {
            self.publish(next.clone())?;
        }
        Ok(())
    }
    pub fn heartbeat(&mut self) -> Result<()> {
        if self.last_heartbeat.elapsed() >= Duration::from_millis(400) {
            self.publish(self.held.clone())?;
        }
        Ok(())
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.input.take();
        // EOF releases the keys before the lease is surrendered.
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Leave a slow guardian alive to finish cleanup; never kill it before release.
    }
}

pub struct Instance(windows_sys::Win32::Foundation::HANDLE);
impl Instance {
    pub fn acquire(name: &str) -> Result<Self> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        };
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            bail!("另一个引擎仍持有输出控制权");
        }
        Ok(Self(handle))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub fn run() -> Result<()> {
    let _instance = Instance::acquire(motion_core::output::OUTPUT_MUTEX)?;
    let (tx, rx) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        while let Ok(Some(message)) = read_packet::<LeaseMessage>(&mut input) {
            if tx.send(message).is_err() {
                break;
            }
        }
    });
    println!("ready");
    std::io::stdout().flush()?;
    let mut held = BTreeSet::new();
    while let Ok(message) = rx.recv_timeout(Duration::from_secs(2)) {
        // This process can only release a supported key; it never presses one.
        if message
            .held
            .iter()
            .any(|k| !motion_core::bindings::valid_button(k))
        {
            break;
        }
        held = message.held;
    }
    let mut sink = WindowsSink::new(GamepadTarget::None)?;
    release(&mut sink, held, || {
        std::thread::sleep(Duration::from_millis(100))
    })
}

fn release(
    sink: &mut impl Sink,
    mut held: BTreeSet<String>,
    mut retry_delay: impl FnMut(),
) -> Result<()> {
    let mut failures = vec![];
    for attempt in 0..3 {
        failures.clear();
        for key in motion_core::bindings::button_order(&held, false) {
            match sink.button(&key, false) {
                Ok(()) => {
                    held.remove(&key);
                }
                Err(error) => failures.push(error.to_string()),
            }
        }
        if held.is_empty() {
            return Ok(());
        }
        if attempt < 2 {
            retry_delay();
        }
    }
    bail!("部分按键释放失败：{}", failures.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ReleaseSink {
        calls: Vec<(String, bool)>,
        fail: usize,
    }
    impl Sink for ReleaseSink {
        fn button(&mut self, key: &str, down: bool) -> Result<()> {
            self.calls.push((key.into(), down));
            if key == "key:A" && self.fail > 0 {
                self.fail -= 1;
                bail!("transient release failure");
            }
            Ok(())
        }
        fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
            panic!("guardian must not move mouse")
        }
        fn gamepad(&mut self, _: motion_core::engine::PadState) -> Result<()> {
            panic!("guardian must not create gamepad")
        }
    }
    #[test]
    fn cleanup_attempts_all_keys_and_retries_only_failures() {
        let mut sink = ReleaseSink {
            calls: vec![],
            fail: 1,
        };
        let held = ["key:A", "key:B", "mouse:left"]
            .map(String::from)
            .into_iter()
            .collect();
        release(&mut sink, held, || {}).unwrap();
        assert_eq!(sink.calls.len(), 4);
        assert!(sink.calls.iter().all(|(_, down)| !down));
        assert_eq!(
            sink.calls.iter().filter(|(key, _)| key == "key:B").count(),
            1
        );
        assert_eq!(sink.calls.last().unwrap().0, "key:A");
    }
    #[test]
    fn cleanup_reports_persistent_failure_after_bounded_retries() {
        let mut sink = ReleaseSink {
            calls: vec![],
            fail: 100,
        };
        assert!(release(&mut sink, ["key:A".into()].into_iter().collect(), || {}).is_err());
        assert_eq!(sink.calls.len(), 3);
    }
}
