use crate::runtime;
use motion_core::{
    motion::GameAngleMeasurement,
    protocol::{Command as EngineCommand, Event, Request, Status, PROTOCOL_VERSION},
    sdl::Device,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

type EngineReply = Result<Option<GameAngleMeasurement>, String>;

#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    pub status: Status,
    pub devices: Vec<Device>,
    pub restart_count: u32,
    pub engine_ready: bool,
    pub policy_error: Option<String>,
    pub logs: Vec<String>,
}
pub struct Client {
    app: AppHandle,
    lifecycle: Mutex<()>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<EngineReply>>>,
    serial: AtomicU64,
    generation: AtomicU64,
    pub stopping: AtomicBool,
    pub snapshot: Mutex<Snapshot>,
    last_seen: Mutex<Instant>,
}
impl Client {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            lifecycle: Mutex::new(()),
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(1),
            generation: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            snapshot: Mutex::new(Snapshot::default()),
            last_seen: Mutex::new(Instant::now()),
        })
    }
    pub fn log(&self, text: String) {
        if let Ok(mut state) = self.snapshot.lock() {
            state.logs.push(text);
            if state.logs.len() > 60 {
                state.logs.remove(0);
            }
        }
    }
    pub fn spawn(self: &Arc<Self>) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "Engine lifecycle lock failed")?;
        self.spawn_worker()
    }
    fn spawn_worker(self: &Arc<Self>) -> Result<(), String> {
        if self.stopping.load(Ordering::Relaxed) {
            return Err("应用正在退出".into());
        }
        let path = runtime::engine_path(&self.app)?;
        let dll = runtime::resources(&self.app)?.join("SDL3.dll");
        if !path.is_file() || !dll.is_file() {
            return Err("引擎或 SDL3.dll 缺失，请重新构建或安装".into());
        }
        let _verified_engine = crate::installers::verify_engine(&path)?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut command = Command::new(&path);
        command
            .arg("--sdl")
            .arg(dll)
            .env("MOTION_STUDIO_DATA_DIR", runtime::runtime_dir(&self.app)?)
            .current_dir(path.parent().unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|e| format!("引擎启动失败：{e}"))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        *self.stdin.lock().map_err(|_| "Engine input lock failed")? = child.stdin.take();
        *self
            .child
            .lock()
            .map_err(|_| "Engine process lock failed")? = Some(child);
        *self.last_seen.lock().unwrap() = Instant::now();
        let this = self.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match motion_core::protocol::read_packet::<Event>(&mut reader) {
                    Ok(Some(event)) if this.generation.load(Ordering::SeqCst) == generation => {
                        this.receive(event)
                    }
                    Ok(None) => break,
                    Ok(_) => break,
                    Err(error) => {
                        this.log(format!("引擎通信中断：{error:#}"));
                        break;
                    }
                }
            }
        });
        let this = self.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if this.generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                this.log(line.chars().take(2000).collect());
            }
        });
        Ok(())
    }
    fn receive(&self, event: Event) {
        *self.last_seen.lock().unwrap() = Instant::now();
        match event {
            Event::Hello { protocol, .. } => {
                if protocol != PROTOCOL_VERSION {
                    self.log("引擎协议版本不匹配".into());
                    self.kill();
                } else {
                    self.snapshot.lock().unwrap().engine_ready = true;
                }
            }
            Event::Reply {
                id,
                ok,
                error,
                measurement,
            } => {
                if let Some(sender) = self.pending.lock().unwrap().remove(&id) {
                    let _ = sender.send(if ok {
                        Ok(measurement)
                    } else {
                        Err(error.unwrap_or_else(|| "引擎操作失败".into()))
                    });
                }
            }
            Event::Status { status } => {
                self.snapshot.lock().unwrap().status = *status.clone();
                self.publish_calibration();
                let _ = self.app.emit("engine-status", status);
            }
            Event::Devices { devices } => {
                self.snapshot.lock().unwrap().devices = devices.clone();
                let _ = self.app.emit("engine-devices", devices);
            }
            Event::Calibration {
                device_path,
                bias,
                sensor,
            } => {
                let _ = self.app.emit(
                    "device-calibration",
                    serde_json::json!({"device_path": device_path, "bias": bias, "sensor":sensor}),
                );
                // Persist on the Rust side even if the webview is hidden or unavailable.
                if let Some(state) = self.app.try_state::<crate::AppState>() {
                    if let Ok(mut settings) = state.settings.lock() {
                        settings.calibrations.insert(device_path.clone(), bias);
                        settings.motion_calibrations.insert(device_path, sensor);
                        if let Err(error) = state.store.save_settings(&settings) {
                            self.log(format!("校准保存失败：{error:#}"));
                        }
                    }
                }
            }
            Event::Log { message } => self.log(message),
        }
    }
    fn publish_calibration(&self) {
        if let Some(state) = self.app.try_state::<crate::AppState>() {
            state.game_calibration.refresh();
        }
    }
    pub fn request(&self, command: EngineCommand) -> Result<(), String> {
        self.request_reply(command).map(|_| ())
    }
    pub fn finish_angle_measurement(
        &self,
        revision: u64,
    ) -> Result<Option<GameAngleMeasurement>, String> {
        self.request_reply(EngineCommand::EndMeasure {
            expected_revision: Some(revision),
        })
    }
    fn request_reply(
        &self,
        command: EngineCommand,
    ) -> Result<Option<GameAngleMeasurement>, String> {
        let timeout = if matches!(
            &command,
            EngineCommand::Start { .. } | EngineCommand::Apply { .. } | EngineCommand::ApplyCursor { .. }
        ) {
            120
        } else {
            8
        };
        let id = self.serial.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "Request lock failed")?
            .insert(id, tx);
        let result = (|| {
            let mut input = self.stdin.lock().map_err(|_| "Engine input lock failed")?;
            let input = input.as_mut().ok_or("引擎未启动，请点击恢复引擎")?;
            serde_json::to_writer(&mut *input, &Request { id, command })
                .map_err(|e| e.to_string())?;
            input.write_all(b"\n").map_err(|e| e.to_string())?;
            input.flush().map_err(|e| e.to_string())?;
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.pending.lock().unwrap().remove(&id);
            return Err(error);
        }
        let result = rx
            .recv_timeout(Duration::from_secs(timeout))
            .map_err(|_| "引擎未及时响应；输出保护会在失去心跳后释放按键".to_string());
        self.pending.lock().unwrap().remove(&id);
        result?
    }
    fn kill(&self) {
        self.stdin.lock().unwrap().take();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.pending.lock().unwrap().clear();
        self.snapshot.lock().unwrap().engine_ready = false;
        self.publish_calibration();
    }
    pub fn restart(self: &Arc<Self>) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "Engine lifecycle lock failed")?;
        self.restart_worker()
    }
    fn restart_worker(self: &Arc<Self>) -> Result<(), String> {
        if self.stopping.load(Ordering::Relaxed) {
            return Err("应用正在退出".into());
        }
        self.kill();
        // The guardian owns a separate output lease, so a new worker cannot emit
        // until the previous worker's releases have completed.
        {
            let mut state = self.snapshot.lock().unwrap();
            state.restart_count += 1;
            state.status = Status::default();
            state.status.message = "引擎已恢复，请手动开始映射".into();
        }
        self.spawn_worker()
    }
    pub fn monitor(self: &Arc<Self>) {
        let this = self.clone();
        std::thread::spawn(move || {
            let mut restarts = Vec::<Instant>::new();
            while !this.stopping.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                let Ok(_lifecycle) = this.lifecycle.try_lock() else {
                    continue;
                };
                if this.stopping.load(Ordering::Relaxed) {
                    break;
                }
                let dead = this
                    .child
                    .lock()
                    .unwrap()
                    .as_mut()
                    .is_some_and(|c| c.try_wait().ok().flatten().is_some());
                let preparing = this.snapshot.lock().unwrap().status.state == "preparing_output";
                let stale = this.child.lock().unwrap().is_some()
                    && this.last_seen.lock().unwrap().elapsed()
                        > Duration::from_secs(if preparing { 120 } else { 6 });
                if dead || stale {
                    this.log("引擎意外退出或无响应；清理输出后恢复待机，不自动恢复映射。".into());
                    restarts.retain(|t| t.elapsed() < Duration::from_secs(60));
                    if restarts.len() >= 3 {
                        this.kill();
                        let mut state = this.snapshot.lock().unwrap();
                        state.status.state = "faulted".into();
                        state.status.emitting = false;
                        state.status.message =
                            "引擎连续故障，自动重试已停止。可查看诊断后手动恢复。".into();
                    } else {
                        restarts.push(Instant::now());
                        if let Err(error) = this.restart_worker() {
                            this.log(error.clone());
                            this.snapshot.lock().unwrap().status.message = error;
                        }
                    }
                }
            }
        });
    }
    pub fn shutdown(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _lifecycle = self.lifecycle.lock().unwrap();
        self.stop_worker();
    }
    pub fn with_worker_stopped_for_update(
        self: &Arc<Self>,
        install: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| "Engine lifecycle lock failed")?;
        if self.stopping.load(Ordering::Relaxed) {
            return Err("应用正在退出".into());
        }
        self.stop_worker();
        // On Windows the updater exits this process if the installer starts.
        // A launch failure leaves this version open, with the worker back in standby.
        let result = install();
        if let Err(error) = self.spawn_worker() {
            self.log(error);
        }
        result
    }
    fn stop_worker(&self) {
        self.stdin.lock().unwrap().take(); // Worker observes EOF and releases normally.
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            let ended = self
                .child
                .lock()
                .unwrap()
                .as_mut()
                .is_none_or(|c| c.try_wait().ok().flatten().is_some());
            if ended {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.kill();
    }
}
