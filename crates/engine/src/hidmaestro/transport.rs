//! Private, per-launch pipe. The elevated peer only accepts a fixed operation;
//! there is no arbitrary driver, descriptor, executable or registry-path RPC.
use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Pipes::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
};

pub struct Process(pub HANDLE);
/// The device owner retains the output lock until every device is cleaned up.
pub(super) struct OutputLease(HANDLE);
impl OutputLease {
    pub(super) fn acquire() -> Result<Self> {
        let name = wide(crate::output::OUTPUT_MUTEX);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("取得输出控制权");
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            bail!("另一个引擎或前一输出会话仍在清理，请稍后重新开始映射");
        }
        Ok(Self(handle))
    }
}
impl Drop for OutputLease {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
// Process handles are not thread-affine; this wrapper closes the handle once.
unsafe impl Send for Process {}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Process {
    pub fn try_clone(&self) -> Result<Self> {
        let current = unsafe { GetCurrentProcess() };
        let mut handle = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                current,
                self.0,
                current,
                &mut handle,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(handle))
    }
    pub fn open(pid: u32) -> Result<Self> {
        let h = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(h))
    }
    pub fn alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn pipe_name(token: &str) -> Result<String> {
    if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("Invalid output session token");
    }
    Ok(format!(r"\\.\pipe\MotionStudio.HidMaestro.{token}"))
}
pub struct Pipe {
    file: File,
    pending: Vec<u8>,
}
impl Pipe {
    fn handle(&self) -> HANDLE {
        self.file.as_raw_handle()
    }
    pub fn server(token: &str) -> Result<Self> {
        let name = wide(&pipe_name(token)?);
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            file: unsafe { File::from_raw_handle(handle) },
            pending: vec![],
        })
    }
    pub fn accept(&mut self, child: &Process, deadline: Instant) -> Result<()> {
        loop {
            let ok = unsafe { ConnectNamedPipe(self.handle(), std::ptr::null_mut()) };
            let code = unsafe { GetLastError() };
            if ok != 0 || code == ERROR_PIPE_CONNECTED {
                break;
            }
            if code != ERROR_PIPE_LISTENING && code != ERROR_NO_DATA {
                bail!("Output pipe connect: {code}");
            }
            if !child.alive() || Instant::now() >= deadline {
                bail!("输出辅助进程未能就绪");
            }
            if crate::output::emergency_stop() {
                bail!("F12 已取消输出初始化");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut pid = 0;
        if unsafe { GetNamedPipeClientProcessId(self.handle(), &mut pid) } == 0
            || pid != unsafe { GetProcessId(child.0) }
        {
            bail!("Output helper identity mismatch");
        }
        Ok(())
    }
    pub fn connect(token: &str, parent_pid: u32) -> Result<Self> {
        let path = pipe_name(token)?;
        // Identification prevents a malicious pipe server from impersonating
        // this elevated client. The parent PID is verified before any mutation.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
            .open(path)?;
        let mut pid = 0;
        if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle(), &mut pid) } == 0
            || pid != parent_pid
        {
            bail!("Output parent identity mismatch");
        }
        Ok(Self {
            file,
            pending: vec![],
        })
    }
    pub fn send<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let mut bytes = serde_json::to_vec(message)?;
        if bytes.len() > 4095 {
            bail!("Output message too large");
        }
        bytes.push(b'\n');
        self.file.write_all(&bytes)?;
        self.file.flush()?;
        Ok(())
    }
    pub fn poll<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            return Ok(Some(serde_json::from_slice(&line)?));
        }
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                self.handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        if available == 0 {
            return Ok(None);
        }
        if self.pending.len() + available as usize > 4096 {
            bail!("Output message too large");
        }
        let mut bytes = vec![0; available as usize];
        let size = self.file.read(&mut bytes)?;
        if size == 0 {
            bail!("Output pipe closed");
        }
        self.pending.extend_from_slice(&bytes[..size]);
        // At most one bounded read per poll; incomplete JSON waits for a newline.
        if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            Ok(Some(serde_json::from_slice(&line)?))
        } else {
            Ok(None)
        }
    }
    pub fn receive<T: DeserializeOwned>(&mut self, peer: &Process, timeout: Duration) -> Result<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(message) = self.poll()? {
                return Ok(message);
            }
            if !peer.alive() {
                bail!("HIDMaestro 输出辅助进程已结束");
            }
            if Instant::now() >= deadline {
                bail!("HIDMaestro 输出辅助进程响应超时");
            }
            if crate::output::emergency_stop() {
                bail!("F12 已取消输出初始化");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
pub fn elevate(exe: &Path, mode: &str, token: &str) -> Result<Process> {
    pipe_name(token)?;
    if !["--hidmaestro-output", "--hidmaestro-install"].contains(&mode) {
        bail!("Invalid helper mode");
    }
    let file = wide(&exe.display().to_string());
    let verb = wide("runas");
    let args = wide(&format!("{mode} {} {token}", std::process::id()));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = SW_HIDE;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            bail!("已取消 Windows 管理员授权");
        }
        return Err(error).context("启动 HIDMaestro 输出辅助进程");
    }
    if info.hProcess.is_null() {
        bail!("Missing output helper process handle");
    }
    Ok(Process(info.hProcess))
}

/// Driver owner-watch ends: the driver never receives data on this pipe.
/// Closing them (including process termination) triggers its release reports.
pub struct OwnerPipe(HANDLE);
impl Drop for OwnerPipe {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn owner_pipes(index: u32) -> Result<Vec<OwnerPipe>> {
    use windows_sys::Win32::Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES,
    };
    let sddl = wide("O:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;LS)");
    let mut sd = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let name = wide(&format!(r"\\.\pipe\HIDMaestroOwner{index}"));
    let mut pipes = vec![];
    for i in 0..2 {
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_OUTBOUND
                    | if i == 0 {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                0,
                0,
                0,
                &sa,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            unsafe {
                LocalFree(sd);
            };
            return Err(std::io::Error::last_os_error().into());
        }
        pipes.push(OwnerPipe(handle));
    }
    unsafe {
        LocalFree(sd);
    }
    Ok(pipes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_names_are_bounded_and_do_not_accept_paths() {
        assert!(pipe_name("0123456789abcdef0123456789abcdef")
            .unwrap()
            .starts_with(r"\\.\pipe\MotionStudio.HidMaestro."));
        for value in [
            "",
            "../device",
            "0123456789abcdef0123456789abcde/",
            "G123456789abcdef0123456789abcdef",
        ] {
            assert!(pipe_name(value).is_err());
        }
    }
    #[test]
    fn private_pipe_round_trips_and_rejects_the_wrong_parent_without_devices() {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut server = Pipe::server(&token).unwrap();
        let pid = std::process::id();
        let original = Process::open(pid).unwrap();
        let process = original.try_clone().unwrap();
        drop(original);
        assert!(process.alive());
        let worker = std::thread::spawn(move || {
            let mut client = Pipe::connect(&token, pid).unwrap();
            client.send(&super::super::Control::Heartbeat {}).unwrap();
            let peer = Process::open(pid).unwrap();
            assert!(matches!(
                client
                    .receive::<super::super::Reply>(&peer, Duration::from_secs(2))
                    .unwrap(),
                super::super::Reply::Ok
            ));
        });
        server
            .accept(&process, Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(matches!(
            server
                .receive::<super::super::Control>(&process, Duration::from_secs(2))
                .unwrap(),
            super::super::Control::Heartbeat {}
        ));
        server.send(&super::super::Reply::Ok).unwrap();
        worker.join().unwrap();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let _server = Pipe::server(&token).unwrap();
        assert!(Pipe::connect(&token, pid.wrapping_add(1)).is_err());
    }
}
