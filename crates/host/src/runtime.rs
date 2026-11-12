use crate::{calibration::Calibration, guardian::Guard, io_clock::IoClock};
use anyhow::{bail, Context, Result};
use motion_core::{
    engine::{Engine, Frame, InputEvent},
    output::{
        emergency_stop, probe_virtual_driver, Dispatcher, GamepadBackend, ObserveSink,
        OutputBackend, Sink, SplitSink, WindowsSink,
    },
    profile::{pressed_inputs, GamepadTarget, Profile},
    protocol::{Command, Event, PadShortcutEvent, Request, RunConfig, Status, PROTOCOL_VERSION},
    sdl::{Device, InputOrigin, Sdl},
    shortcut::{validate_shortcut_rules, PadShortcut, PadShortcutEdges, ShortcutRule, ShortcutTrigger},
    storage::{parse_profile, read_small},
    timing::{
        io_schedule::{self, IoPass},
        Histogram, MAX_EVENT_AGE_NS,
    },
    visibility::{self, VisibilityStatus},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

const PAD_SHORTCUT_EVENT_LIMIT: usize = 64;

fn probe_output(
    backend: OutputBackend,
    pad_backend: GamepadBackend,
    target: GamepadTarget,
) -> Result<()> {
    if backend == OutputBackend::HidMaestro {
        motion_core::hidmaestro::probe()?;
    }
    if target != GamepadTarget::None
        && (backend == OutputBackend::Legacy || pad_backend == GamepadBackend::Vigem)
    {
        probe_virtual_driver()?;
    }
    Ok(())
}

trait VisibilityLease {
    fn acquire(&mut self, path: &str) -> Result<Option<VisibilityStatus>>;
    fn preparing(&mut self) -> Result<()>;
    fn verify(&mut self, protected: Vec<String>, gamepad: bool) -> Result<()>;
    fn heartbeat(&mut self) -> Result<Option<VisibilityStatus>>;
    fn release(&mut self) -> Result<VisibilityStatus>;
    fn recover(&mut self) -> Result<VisibilityStatus>;
}
struct DriverVisibility;
impl VisibilityLease for DriverVisibility {
    fn acquire(&mut self, path: &str) -> Result<Option<VisibilityStatus>> {
        visibility::acquire(path)
    }
    fn preparing(&mut self) -> Result<()> {
        visibility::preparing()
    }
    fn verify(&mut self, protected: Vec<String>, gamepad: bool) -> Result<()> {
        visibility::verify_outputs(protected, gamepad)
    }
    fn heartbeat(&mut self) -> Result<Option<VisibilityStatus>> {
        visibility::heartbeat()
    }
    fn release(&mut self) -> Result<VisibilityStatus> {
        visibility::release()
    }
    fn recover(&mut self) -> Result<VisibilityStatus> {
        visibility::recover()
    }
}

pub enum Control {
    Request(Request),
    Reload {
        path: PathBuf,
        result: Box<Result<Profile, String>>,
    },
    Closed,
}
#[derive(Clone)]
pub struct Publisher(pub mpsc::SyncSender<Event>);
impl Publisher {
    pub fn reliable(&self, event: Event) -> Result<()> {
        self.0
            .try_send(event)
            .map_err(|_| anyhow::anyhow!("界面通信队列不可用，已停止输出"))
    }
    pub fn status(&self, status: &Status) {
        let _ = self.0.try_send(Event::Status {
            status: Box::new(status.clone()),
        });
    }
}
pub fn watch_profiles(
    target: Arc<Mutex<Option<PathBuf>>>,
    tx: mpsc::SyncSender<Control>,
    done: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut previous: Option<(PathBuf, Result<String, String>)> = None;
        while !done.load(Ordering::Relaxed) {
            let path = target.lock().ok().and_then(|p| p.clone());
            if let Some(path) = path {
                let text = read_small(&path).map_err(|e| e.to_string());
                let current = (path.clone(), text.clone());
                if previous.as_ref() != Some(&current) {
                    let result =
                        text.and_then(|text| parse_profile(&text).map_err(|e| format!("{e:#}")));
                    if tx
                        .try_send(Control::Reload {
                            path,
                            result: Box::new(result),
                        })
                        .is_ok()
                    {
                        previous = Some(current);
                    }
                }
            } else {
                previous = None;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

pub struct Runtime {
    dll: PathBuf,
    sdl: Option<Sdl>,
    config: Option<RunConfig>,
    engine: Option<Engine>,
    output: Option<Dispatcher<Box<dyn Sink>>>,
    guard: Option<Guard>,
    calibration: Option<Calibration>,
    pending_reload: Option<(PathBuf, Result<Profile, String>)>,
    pub status: Status,
    publisher: Publisher,
    watch: Arc<Mutex<Option<PathBuf>>>,
    next_discovery: Instant,
    last_status: Instant,
    last_advance: Instant,
    io_clock: IoClock,
    last_stats: Instant,
    last_samples: u64,
    navigation: motion_core::navigation::Navigation,
    processing: Histogram,
    queue: Histogram,
    visibility: Box<dyn VisibilityLease>,
    capture_until: Option<Instant>,
    capture_releasing: bool,
    active_layers: Vec<usize>,
    seeding: bool,
    resync_input: bool,
    measurement_input_interrupted: bool,
    pad_shortcut_edges: Vec<PadShortcutEdges>,
    feedback: motion_core::feedback::FeedbackRouter,
    last_feedback: Instant,
}
impl Runtime {
    fn gamepad_output_configured(&self) -> bool {
        self.config.as_ref().is_some_and(|config| {
            config.emit && config.profile.virtual_gamepad != GamepadTarget::None
        })
    }

    fn status_snapshot(&self) -> Status {
        let mut status = self.status.clone();
        status.mouse_display_sync = self.io_clock.status();
        status.feedback = self.feedback.status.clone();
        status.output_gamepad = self
            .output
            .as_ref()
            .filter(|_| self.gamepad_output_configured())
            .and_then(|output| output.gamepad_status());
        if (status.emitting || status.state == "observing")
            && !self.output_paused()
            && !status.ui_navigation
        {
            status.output_preview = self.output.as_ref().map(Dispatcher::preview);
        }
        status.motion.filters = self
            .engine
            .as_ref()
            .map(|engine| engine.motion.custom_filters.statuses())
            .unwrap_or_default();
        status
    }
    fn publish_status(&self) {
        self.publisher.status(&self.status_snapshot());
    }
    pub fn new(dll: PathBuf, publisher: Publisher, watch: Arc<Mutex<Option<PathBuf>>>) -> Self {
        let now = Instant::now();
        Self {
            dll,
            publisher,
            watch,
            sdl: None,
            config: None,
            engine: None,
            output: None,
            guard: None,
            calibration: None,
            pending_reload: None,
            status: Status::default(),
            next_discovery: Instant::now(),
            last_status: Instant::now(),
            last_advance: now,
            io_clock: IoClock::new(now),
            last_stats: Instant::now(),
            last_samples: 0,
            navigation: Default::default(),
            processing: Histogram::default(),
            queue: Histogram::default(),
            visibility: Box::new(DriverVisibility),
            capture_until: None,
            capture_releasing: false,
            active_layers: vec![],
            seeding: false,
            resync_input: false,
            measurement_input_interrupted: false,
            pad_shortcut_edges: vec![],
            feedback: Default::default(),
            last_feedback: Instant::now(),
        }
    }
    fn clear_outputs(&mut self) -> Result<()> {
        self.reset_feedback();
        if let Some(output) = &mut self.output {
            output.reset()?;
        }
        if let Some(guard) = &mut self.guard {
            guard.after(&BTreeSet::new())?;
        }
        Ok(())
    }
    fn clear_outputs_or_fault(&mut self) -> Result<()> {
        if let Err(error) = self.clear_outputs() {
            self.fault(format!("输出已暂停：{error:#}"));
            return Err(error);
        }
        Ok(())
    }
    fn reset_feedback(&mut self) {
        let owned = self.feedback.status.active
            || self.feedback.status.rumble != [0.; 2]
            || self.feedback.status.lightbar.is_some();
        let sequence = self
            .output
            .as_mut()
            .and_then(|output| output.feedback())
            .map_or(0, |frame| frame.sequence);
        let config = self
            .engine
            .as_ref()
            .map(|engine| engine.profile.feedback.clone())
            .unwrap_or_default();
        self.feedback.suspend(sequence, &config);
        if let Some(sdl) = self.sdl.as_mut().filter(|_| owned) {
            sdl.stop_feedback();
        }
    }
    fn output_paused(&self) -> bool {
        self.calibration.is_some()
            || self.status.binding_capture
            || self.resync_input
            || self.measurement_input_interrupted
    }
    fn update_feedback(&mut self) {
        let active = self.status.emitting
            && self.gamepad_output_configured()
            && !self.output_paused()
            && !self.status.ui_navigation;
        if let Some(frame) = self.output.as_mut().and_then(|output| output.feedback()) {
            let cfg = self
                .engine
                .as_ref()
                .map(|engine| engine.profile.feedback.clone())
                .unwrap_or_default();
            let now = self.sdl.as_ref().map_or(0, |sdl| sdl.now_ns() / 1_000_000);
            let plan = self.feedback.tick(&frame, &cfg, active, now);
            if let Some(sdl) = &mut self.sdl {
                sdl.feedback(&plan);
            }
        }
    }
    fn close_output(&mut self) -> Result<()> {
        self.io_clock.set_enabled(false);
        let result = self.clear_outputs();
        self.output.take(); // Dispatcher retries releases on Drop.
        self.guard.take(); // Guardian releases any outstanding intent on EOF.
        self.status.emitting = false;
        self.status.output_backend = None;
        result
    }
    fn connected_input(&self) -> Option<&Device> {
        self.status
            .device
            .as_ref()
            .filter(|_| self.sdl.as_ref().is_some_and(Sdl::is_connected))
    }
    fn close_input(&mut self) {
        self.sdl.take();
        self.resync_input = false;
        self.measurement_input_interrupted = false;
        self.status.device = None;
        self.status.buttons.fill(false);
        self.status.axes = [0.0; 6];
        self.status.touches = Default::default();
        self.status.gyro = [0.0; 3];
    }
    pub fn stop(&mut self, message: &str) -> Result<()> {
        self.finish_session(message, false)
    }
    fn stop_mapping(&mut self, message: &str) -> Result<()> {
        let keep_input = self.connected_input().is_some();
        let result = self.finish_session(message, keep_input);
        if let Some(sdl) = &mut self.sdl {
            // Keep the selected handle for button and stick monitoring. Sensors
            // are enabled again in place when observation or mapping starts.
            let _ = sdl.set_sensors(false, false);
            self.status.gyro = [0.0; 3];
        }
        result
    }
    fn finish_session(&mut self, message: &str, keep_input: bool) -> Result<()> {
        let mut result = self.close_output();
        if result.is_err() {
            let _ = motion_core::hidmaestro::shutdown();
        }
        self.engine.take();
        if !keep_input {
            self.close_input();
        }
        self.config.take();
        self.calibration.take();
        self.pending_reload = None;
        self.capture_until = None;
        self.capture_releasing = false;
        self.status.binding_capture = false;
        self.active_layers.clear();
        self.status.active_layers.clear();
        self.seeding = false;
        self.resync_input = false;
        self.measurement_input_interrupted = false;
        if self.status.visibility.requested
            || self.status.visibility.active
            || self.status.visibility.error.is_some()
        {
            self.status.visibility.requested = false;
            match self.visibility.release() {
                Ok(status) => self.status.visibility = status,
                Err(error) => {
                    let message = format!("实体手柄恢复未完成：{error:#}");
                    self.status.visibility.error = Some(message.clone());
                    result = Err(anyhow::anyhow!(message));
                }
            }
        }
        *self
            .watch
            .lock()
            .map_err(|_| anyhow::anyhow!("Profile watcher unavailable"))? = None;
        self.status.state = "idle".into();
        self.status.message = message.into();
        if let Err(error) = &result {
            self.status.message = format!("{message}；{error:#}");
        }
        self.status.input_chord.clear();
        self.status.ui_navigation = false;
        self.navigation.reset();
        self.status.navigation_events.clear();
        self.status.navigation_held.clear();
        self.reset_pad_shortcut_events();
        self.status.processed_axes = [0.0; 6];
        self.status.stick_regions = Default::default();
        self.status.pad_region_capture = Default::default();
        self.status.calibration_progress = 0.0;
        self.status.motion = Default::default();
        result
    }
    pub fn fault(&mut self, message: String) {
        let _ = self.stop(&message);
        let _ = motion_core::hidmaestro::shutdown();
        self.status.state = "faulted".into();
        self.publish_status();
    }
    fn dispatch(&mut self, frame: Frame) -> Result<()> {
        if let Some(guard) = &mut self.guard {
            guard.before(&frame.held)?;
        }
        if let Some(output) = &mut self.output {
            output.apply(&frame)?;
        }
        if let Some(guard) = &mut self.guard {
            guard.after(&frame.held)?;
        }
        Ok(())
    }
    pub fn scan(&mut self) -> Result<()> {
        if self.status.ui_navigation {
            self.stop("正在重新检测手柄")?;
        }
        if self.config.is_some() {
            bail!("请先停止映射或观察，再重新检测设备；自动重连仍在后台工作");
        }
        // Explicit retry performs a fresh takeover handshake. A background
        // list refresh alone cannot recover a missed device status response.
        self.close_input();
        self.refresh_devices()
    }
    fn refresh_devices(&mut self) -> Result<()> {
        self.next_discovery = Instant::now() + Duration::from_secs(2);
        if self.sdl.is_none() {
            self.sdl = Some(Sdl::load(&self.dll)?);
        }
        let devices = self.sdl.as_mut().context("SDL unavailable")?.devices()?;
        self.publisher.reliable(Event::Devices { devices })?;
        Ok(())
    }
    fn start(&mut self, config: RunConfig) -> Result<()> {
        config.io_rates.validate()?;
        Engine::new(config.profile.clone())?;
        if config.device_path.trim().is_empty() {
            bail!("请先选择一个手柄接口");
        }
        if config.bias.iter().any(|v| !v.is_finite() || v.abs() > 0.15) {
            bail!("设备校准值无效");
        }
        config.sensor_calibration.validate()?;
        if config.emit {
            probe_output(
                config.output_backend,
                config.gamepad_backend,
                config.profile.virtual_gamepad,
            )?;
        }
        let reuse = self
            .connected_input()
            .is_some_and(|device| device.path.eq_ignore_ascii_case(&config.device_path));
        let keep_input = reuse || (self.config.is_none() && self.status.device.is_none());
        self.finish_session("正在连接手柄", keep_input)?;
        if config.output_backend == OutputBackend::Legacy {
            motion_core::hidmaestro::shutdown()?;
        }
        *self
            .watch
            .lock()
            .map_err(|_| anyhow::anyhow!("Profile watcher unavailable"))? =
            Some(config.profile_file.clone());
        self.status.profile_id = Some(config.profile_id.clone());
        self.status.profile_name = Some(config.profile.name.clone());
        self.status.state = if reuse && config.emit {
            self.status.message = "正在准备输出".into();
            "preparing_output"
        } else {
            "waiting"
        }
        .into();
        self.status.reload_error = None;
        self.status.stats = Default::default();
        self.status.visibility.requested = config.emit && config.auto_hide;
        self.io_clock.set_rates(config.io_rates);
        self.config = Some(config);
        self.next_discovery = Instant::now();
        Ok(())
    }
    fn prepare_engine(&mut self) -> Result<()> {
        let config = self
            .config
            .as_ref()
            .context("Missing configuration")?
            .clone();
        let mut engine = Engine::new(motion_core::layers::resolve(&config.profile, &[])?)?;
        self.active_layers.clear();
        self.status.active_layers.clear();
        engine.bias_rad_s = config.bias;
        engine.sensor_calibration = config.sensor_calibration;
        engine.set_touchpad_count(
            self.status
                .device
                .as_ref()
                .map_or(0, |device| device.touchpads),
        );
        self.engine = Some(engine);
        self.last_samples = 0;
        self.processing = Histogram::default();
        self.queue = Histogram::default();
        self.last_stats = Instant::now();
        if config.emit {
            if config.auto_hide {
                self.visibility.preparing()?;
            }
            if self.guard.is_none() && config.output_backend == OutputBackend::Legacy {
                self.guard = Some(Guard::start()?);
            }
            if self.output.is_none() {
                self.status.state = "preparing_output".into();
                self.status.message =
                    "正在准备输出；HIDMaestro 首次启用可能请求 Windows 管理员授权".into();
                self.publish_status();
                let target = self.engine.as_ref().unwrap().profile.virtual_gamepad;
                let sink: Box<dyn Sink + Send> = match config.output_backend {
                    OutputBackend::HidMaestro
                        if target != GamepadTarget::None
                            && config.gamepad_backend == GamepadBackend::Vigem =>
                    {
                        Box::new(SplitSink::new(
                            motion_core::hidmaestro::HidMaestroSink::new(GamepadTarget::None)?,
                            WindowsSink::new(target)?,
                        ))
                    }
                    OutputBackend::HidMaestro => {
                        Box::new(motion_core::hidmaestro::HidMaestroSink::new(target)?)
                    }
                    OutputBackend::Legacy => Box::new(WindowsSink::new(target)?),
                };
                let sink: Box<dyn Sink> =
                    Box::new(motion_core::flex_output::FlexVirtualSink::new(sink));
                self.output = Some(Dispatcher::new(sink).with_scheduled_output());
            }
            if let Some(guard) = &mut self.guard {
                guard.heartbeat()?;
            }
        } else if self.output.is_none() {
            self.output = Some(
                Dispatcher::new(Box::new(ObserveSink) as Box<dyn Sink>).with_scheduled_output(),
            );
        }
        if config.emit && config.profile.virtual_gamepad != GamepadTarget::None {
            let pad = self
                .output
                .as_ref()
                .and_then(|output| output.gamepad_status())
                .context("虚拟手柄输出未建立，映射已暂停")?;
            anyhow::ensure!(
                pad.connected && pad.target == config.profile.virtual_gamepad,
                "虚拟手柄未就绪或类型不匹配，映射已暂停"
            );
        }
        if config.emit && config.auto_hide {
            self.visibility.verify(
                self.output
                    .as_ref()
                    .context("Missing output")?
                    .protected_devices(),
                self.engine.as_ref().unwrap().profile.virtual_gamepad != GamepadTarget::None,
            )?;
        }
        self.status.emitting = config.emit;
        self.reset_feedback();
        self.status.output_backend = config.emit.then_some(config.output_backend);
        self.status.state = if config.emit { "running" } else { "observing" }.into();
        self.status.message = if config.emit {
            "映射运行中 · F12 随时停止"
        } else {
            "正在观察输入，不发送键鼠或手柄信号"
        }
        .into();
        if self.status.ui_navigation {
            self.status.state = "navigating".into();
            self.status.message = "手柄操作界面 · A 确认 · B 返回 · LB/RB 切换区域".into();
        }
        self.status.revision += 1;
        self.status.buttons.fill(false);
        self.status.axes = [0.0; 6];
        self.status.processed_axes = [0.0; 6];
        self.status.stick_regions = Default::default();
        self.status.pad_region_capture = Default::default();
        self.status.touches = Default::default();
        if let Some(sdl) = &mut self.sdl {
            let sensors = sdl.set_sensors(
                !self.status.ui_navigation,
                self.engine
                    .as_ref()
                    .is_some_and(|engine| engine.profile.gyro.enabled),
            );
            if !sdl.is_connected() {
                return self.lost_device();
            }
            sensors?;
            sdl.poll_events()?;
            for _ in 0..10000 {
                match sdl.take_event()? {
                    None => break,
                    Some(InputEvent::Disconnected { .. }) => return self.lost_device(),
                    _ => {}
                }
            }
            if !sdl.is_connected() {
                return self.lost_device();
            }
        }
        let snapshot = self.sdl.as_ref().map(Sdl::snapshot).unwrap_or_default();
        self.seeding = true;
        for event in snapshot {
            self.input(event, true)?;
        }
        self.seeding = false;
        self.resync_input = false;
        self.measurement_input_interrupted = false;
        self.sync_layers()?;
        self.track_pad_shortcut(true);
        if let Some(engine) = &self.engine {
            self.dispatch(engine.frame.clone())?;
        }
        self.last_advance = Instant::now();
        self.io_clock.reset(self.last_advance);
        self.io_clock
            .set_enabled(config.emit && config.mouse_display_sync);
        Ok(())
    }
    fn discover(&mut self) -> Result<()> {
        let config = self.config.as_ref().context("Missing configuration")?;
        if config.emit && config.auto_hide {
            self.status.state = "preparing_output".into();
            self.status.message = "正在隐藏所选实体手柄并登记引擎读取权限".into();
            self.publish_status();
            match self.visibility.acquire(&config.device_path) {
                Ok(Some(status)) => self.status.visibility = status,
                Ok(None) => {
                    self.close_input();
                    self.status.state = "waiting".into();
                    self.status.message = "等待所选实体手柄连接；尚未新增隐藏规则".into();
                    self.next_discovery = Instant::now() + Duration::from_secs(2);
                    return Ok(());
                }
                Err(error) => {
                    self.fault(format!("自动隐藏未就绪，映射已停止：{error:#}"));
                    return Ok(());
                }
            }
        }
        let config = self.config.as_ref().context("Missing configuration")?;
        let path = config.device_path.clone();
        let require_gyro = config.profile.gyro.enabled;
        let require_accel = require_gyro && config.profile.gyro.motion.needs_accel();
        let selected = self
            .connected_input()
            .filter(|device| device.path.eq_ignore_ascii_case(&path))
            .cloned();
        let reuse = selected.is_some();
        let device = if reuse {
            selected
        } else {
            if self.status.device.is_some() {
                self.close_input();
            }
            if self.sdl.is_none() {
                self.sdl = Some(Sdl::load(&self.dll)?);
            }
            let devices = self.sdl.as_mut().unwrap().devices()?;
            let device = devices
                .iter()
                .find(|d| {
                    d.path.eq_ignore_ascii_case(&path)
                        && d.gamepad
                        && d.origin != InputOrigin::Virtual
                        && d.open_error.is_none()
                })
                .cloned();
            self.publisher.reliable(Event::Devices { devices })?;
            device
        };
        let Some(device) = device else {
            // Retry a failed takeover with a fresh SDL session.
            self.close_input();
            self.status.state = "waiting".into();
            self.status.message = "等待所选手柄重新连接；不会切换到其他接口".into();
            self.next_discovery = Instant::now() + Duration::from_secs(2);
            return Ok(());
        };
        if require_gyro && !device.gyro {
            self.close_input();
            self.status.state = "waiting".into();
            self.status.message = "所选接口没有体感，请选择第三方接管的体感接口".into();
            self.next_discovery = Instant::now() + Duration::from_secs(2);
            return Ok(());
        }
        if require_accel && !device.accelerometer {
            self.close_input();
            self.status.state = "waiting".into();
            self.status.message =
                "当前体感设置需要加速度计，请选择支持的接口或关闭依赖重力的功能".into();
            self.next_discovery = Instant::now() + Duration::from_secs(2);
            return Ok(());
        }
        if !reuse {
            self.status.device = Some(self.sdl.as_mut().unwrap().select(
                device.id,
                require_gyro,
                !self.status.ui_navigation,
            )?);
        }
        if let Err(error) = self.prepare_engine() {
            self.fault(format!("无法初始化输出，已暂停：{error:#}"));
        }
        Ok(())
    }
    fn apply(&mut self, id: String, profile: Profile, path: PathBuf) -> Result<()> {
        if self.status.binding_capture {
            bail!("请先结束映射录入并松开手柄，再应用配置");
        }
        Engine::new(profile.clone())?;
        let config = self
            .config
            .as_ref()
            .context("引擎未运行，请先开始映射或观察")?;
        if self.calibration.is_some() {
            bail!("请等待静止校准完成");
        }
        if self.engine.as_ref().is_some_and(Engine::is_measuring) {
            bail!("请先结束或取消转角测量，再应用配置");
        }
        if self
            .status
            .device
            .as_ref()
            .is_some_and(|d| profile.gyro.enabled && !d.gyro)
        {
            bail!("当前接口没有体感，请停止并选择支持体感的接口");
        }
        if self.status.device.as_ref().is_some_and(|d| {
            profile.gyro.enabled && profile.gyro.motion.needs_accel() && !d.accelerometer
        }) {
            bail!("当前接口没有加速度计，不能应用玩家／世界坐标或倾斜动作");
        }
        if config.profile_id == id
            && serde_json::to_string(&config.profile)? == serde_json::to_string(&profile)?
        {
            self.status.reload_error = None;
            return Ok(());
        }
        // The attached device may outlive a keyboard/mouse-only profile. Reuse
        // the neutral device when returning to its type within this session.
        let recreate = if config.emit {
            match self.output.as_ref().and_then(Dispatcher::gamepad_status) {
                Some(pad) => {
                    !pad.connected
                        || (profile.virtual_gamepad != GamepadTarget::None
                            && pad.target != profile.virtual_gamepad)
                }
                None => profile.virtual_gamepad != GamepadTarget::None,
            }
        } else {
            config.profile.virtual_gamepad != profile.virtual_gamepad
        };
        if config.emit && (recreate || self.output.is_none()) {
            probe_output(
                config.output_backend,
                config.gamepad_backend,
                profile.virtual_gamepad,
            )?;
        }
        if let Err(error) = self.clear_outputs() {
            let message = format!("切换时无法释放旧输出，已暂停：{error:#}");
            self.fault(message.clone());
            bail!(message);
        }
        if recreate {
            // Drop the old device before creating the replacement, with the
            // guardian lease held for the entire switch.
            self.output.take();
            self.status.emitting = false;
        }
        let config = self.config.as_mut().unwrap();
        config.profile_id = id.clone();
        config.profile = profile;
        config.profile_file = path.clone();
        *self
            .watch
            .lock()
            .map_err(|_| anyhow::anyhow!("Profile watcher unavailable"))? = Some(path);
        self.status.profile_id = Some(id);
        self.status.profile_name = Some(config.profile.name.clone());
        self.status.reload_error = None;
        if self.engine.is_some() {
            if let Err(error) = self.prepare_engine() {
                let message = format!("新配置输出初始化失败，已暂停：{error:#}");
                self.fault(message.clone());
                bail!(message);
            }
        }
        Ok(())
    }
    pub fn control(&mut self, control: Control) -> Result<bool> {
        match control {
            Control::Closed => {
                self.stop("界面已退出")?;
                return Ok(false);
            }
            Control::Reload { path, result } => {
                let result = *result;
                if self.config.as_ref().is_some_and(|c| c.profile_file == path) {
                    if self.calibration.is_some() || self.status.binding_capture {
                        self.pending_reload = Some((path, result));
                        return Ok(true);
                    }
                    let id = self.config.as_ref().unwrap().profile_id.clone();
                    let result = result
                        .map_err(anyhow::Error::msg)
                        .and_then(|p| self.apply(id, p, path));
                    if let Err(error) = result {
                        self.status.reload_error = Some(if self.status.state == "faulted" {
                            format!("热更新后输出已暂停：{error:#}")
                        } else {
                            format!("热更新未应用，保留当前配置：{error:#}")
                        });
                    }
                }
            }
            Control::Request(request) => {
                let shutdown = matches!(request.command, Command::Shutdown);
                let shortcut_update = matches!(&request.command, Command::SetPadShortcuts { .. });
                let status_before_reply = shortcut_update
                    || matches!(
                        &request.command,
                        Command::Start { .. }
                            | Command::Navigate { .. }
                            | Command::Stop
                            | Command::TrialTurn { .. }
                            | Command::BeginMeasure { .. }
                            | Command::EndMeasure { .. }
                    );
                let mut measurement = None;
                let result = (|| -> Result<()> {
                    match request.command {
                        Command::Start { config } => self.start(*config),
                        Command::Navigate { device_path } => {
                            if self.calibration.is_some()
                                || self.engine.as_ref().is_some_and(Engine::is_measuring)
                            {
                                bail!("请先结束校准或转角测量");
                            }
                            let profile =
                                parse_profile(include_str!("../../../examples/blank.toml"))?;
                            self.start(RunConfig {
                                profile_id: "__studio_ui__".into(),
                                profile,
                                profile_file: PathBuf::new(),
                                device_path,
                                emit: false,
                                auto_hide: false,
                                output_backend: Default::default(),
                                gamepad_backend: Default::default(),
                                mouse_display_sync: false,
                                io_rates: Default::default(),
                                bias: [0.; 3],
                                sensor_calibration: Default::default(),
                            })?;
                            *self
                                .watch
                                .lock()
                                .map_err(|_| anyhow::anyhow!("Profile watcher unavailable"))? =
                                None;
                            self.status.ui_navigation = true;
                            self.status.profile_id = None;
                            self.status.profile_name = None;
                            Ok(())
                        }
                        Command::Apply {
                            profile_id,
                            profile,
                            profile_file,
                        } => self.apply(profile_id, *profile, profile_file),
                        Command::Stop => self.stop_mapping("映射已停止，输出已释放"),
                        Command::Shutdown => self.stop("映射已停止，输出已释放"),
                        Command::RecoverVisibility => {
                            if self.config.as_ref().is_some_and(|c| c.emit) {
                                bail!("请先停止映射，再恢复隐藏规则");
                            }
                            self.status.visibility = self.visibility.recover()?;
                            self.publish_status();
                            Ok(())
                        }
                        Command::SetBindingCapture { active } => self.set_binding_capture(active),
                        Command::SetPadShortcuts { bindings } => self.set_pad_shortcuts(bindings),
                        Command::Scan => self.scan(),
                        Command::Calibrate => {
                            if self.status.binding_capture {
                                bail!("请先结束映射录入");
                            }
                            if self.engine.is_none()
                                || self.calibration.is_some()
                                || self.engine.as_ref().is_some_and(Engine::is_measuring)
                                || !self.status.device.as_ref().is_some_and(|d| d.gyro)
                            {
                                Err(anyhow::anyhow!("请先选择体感手柄并开始观察或映射"))
                            } else {
                                self.clear_outputs_or_fault()?;
                                self.engine.as_mut().unwrap().reset();
                                self.calibration = Some(Calibration::new());
                                self.status.state = "calibrating".into();
                                self.status.message =
                                    "请把手柄平放，2 秒后开始采样；此时输出已暂停".into();
                                self.status.emitting = false;
                                Ok(())
                            }
                        }
                        Command::CalibrateFace { axis, positive } => {
                            if self.status.binding_capture {
                                bail!("请先结束映射录入");
                            }
                            let c = Calibration::face(axis, positive)?;
                            if self.calibration.is_some()
                                || self.engine.as_ref().is_some_and(Engine::is_measuring)
                                || !self
                                    .status
                                    .device
                                    .as_ref()
                                    .is_some_and(|d| d.gyro && d.accelerometer)
                                || self.engine.is_none()
                            {
                                Err(anyhow::anyhow!(
                                    "请先连接带加速度计的体感接口，并等待当前校准完成"
                                ))
                            } else {
                                self.clear_outputs_or_fault()?;
                                self.engine.as_mut().unwrap().reset();
                                self.calibration = Some(c);
                                self.status.state = "calibrating".into();
                                self.status.emitting = false;
                                self.status.message = format!(
                                    "六面校准：将 {}{} 轴朝上并放稳，2 秒后开始采样",
                                    if positive { "+" } else { "−" },
                                    ["X（右握把）", "Y（手柄正面）", "Z（朝向身体）"][axis]
                                );
                                Ok(())
                            }
                        }
                        Command::SetSensorCalibration { calibration } => {
                            calibration.validate()?;
                            if self.status.binding_capture {
                                bail!("请先结束映射录入");
                            }
                            if self.calibration.is_some()
                                || self.engine.as_ref().is_some_and(Engine::is_measuring)
                            {
                                Err(anyhow::anyhow!("请先结束当前校准或转角测量"))
                            } else {
                                if self.config.is_none() {
                                    bail!("请先开始观察或映射");
                                }
                                self.clear_outputs_or_fault()?;
                                let config = self.config.as_mut().unwrap();
                                config.sensor_calibration = (*calibration).clone();
                                let event = Event::Calibration {
                                    device_path: config.device_path.clone(),
                                    bias: config.bias,
                                    sensor: *calibration,
                                };
                                let prepare = if self.engine.is_some() {
                                    self.prepare_engine()
                                } else {
                                    Ok(())
                                };
                                let result = prepare.and_then(|()| self.publisher.reliable(event));
                                if let Err(error) = &result {
                                    self.fault(format!("应用设备校准失败，输出已暂停：{error:#}"));
                                }
                                result
                            }
                        }
                        Command::TrialTurn {
                            scale,
                            expected_revision,
                        } => {
                            if expected_revision != self.status.revision {
                                bail!("校准所用的映射已改变，请重新打开校准悬浮窗。");
                            }
                            if self.calibration.is_some()
                                || !self.status.emitting
                                || self.status.binding_capture
                                || self.resync_input
                                || self.status.device.is_none()
                            {
                                bail!("试转需要先保存配置并启用映射输出");
                            }
                            if !self.active_layers.is_empty() {
                                bail!("请松开配置层按键，切回基础层后再校准。");
                            }
                            self.engine
                                .as_mut()
                                .context("引擎未运行")?
                                .begin_trial_turn(scale)?;
                            self.clear_outputs_or_fault()?;
                            self.status.motion =
                                self.engine.as_ref().unwrap().motion.telemetry.clone();
                            Ok(())
                        }
                        Command::BeginMeasure {
                            axis,
                            scale,
                            expected_revision,
                        } => {
                            if expected_revision.is_some_and(|rev| rev != self.status.revision) {
                                bail!("校准所用的映射已改变，请重新打开校准悬浮窗。");
                            }
                            if self.calibration.is_some()
                                || !self.status.emitting
                                || !self
                                    .status
                                    .device
                                    .as_ref()
                                    .is_some_and(|device| device.gyro)
                            {
                                bail!("转角测量需要先保存配置并启用映射输出");
                            }
                            if self.resync_input {
                                bail!("正在恢复手柄输入，请稍后重新开始转角测量");
                            }
                            if !self.active_layers.is_empty() {
                                bail!("请松开配置层按键，切回基础层后再校准。");
                            }
                            self.engine
                                .as_mut()
                                .context("引擎未运行")?
                                .begin_measurement_at_scale(&axis, scale)?;
                            self.clear_outputs_or_fault()?;
                            self.status.motion =
                                self.engine.as_ref().unwrap().motion.telemetry.clone();
                            self.publish_status();
                            Ok(())
                        }
                        Command::EndMeasure { expected_revision } => {
                            if expected_revision.is_some_and(|rev| rev != self.status.revision) {
                                bail!("校准所用的映射已改变，请重新打开校准悬浮窗。");
                            }
                            if let Some(engine) = &mut self.engine {
                                measurement = engine.end_measurement();
                                self.status.motion = engine.motion.telemetry.clone();
                            }
                            if measurement.is_some() {
                                self.clear_outputs_or_fault()?;
                                self.measurement_input_interrupted = false;
                                if !self.resync_input {
                                    self.sync_layers()?;
                                }
                            }
                            self.publish_status();
                            Ok(())
                        }
                        Command::ResetPose => {
                            self.clear_outputs_or_fault()?;
                            if let Some(engine) = &mut self.engine {
                                engine.reset_pose();
                            }
                            Ok(())
                        }
                        Command::ResetFilters => {
                            let engine = self.engine.as_mut().context("引擎未运行")?;
                            engine.motion.custom_filters.reset();
                            engine.frame.mouse_delta = [0.; 2];
                            Ok(())
                        }
                    }
                })();
                if !shortcut_update {
                    self.track_pad_shortcut(true);
                }
                if status_before_reply {
                    // The caller may use the selected device or rule indices
                    // from this status as soon as it receives the reply.
                    self.publisher.reliable(Event::Status {
                        status: Box::new(self.status_snapshot()),
                    })?;
                }
                self.publisher.reliable(Event::Reply {
                    id: request.id,
                    ok: result.is_ok(),
                    error: result.err().map(|e| format!("{e:#}")),
                    measurement,
                })?;
                if !status_before_reply {
                    self.publish_status();
                }
                if shutdown {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    fn lost_device(&mut self) -> Result<()> {
        if self.config.is_none() {
            self.stop("手柄已断开")?;
            self.next_discovery = Instant::now();
            self.publish_status();
            return Ok(());
        }
        self.active_layers.clear();
        self.status.active_layers.clear();
        self.capture_until = None;
        self.capture_releasing = false;
        self.status.binding_capture = false;
        // SDL's XInput fallback uses a slot name rather than a device path.
        if self.config.as_ref().is_some_and(|config| {
            config
                .device_path
                .to_ascii_lowercase()
                .starts_with("xinput#")
        }) {
            self.stop("XInput 接口已断开，请重新选择手柄")?;
            self.publish_status();
            return Ok(());
        }
        self.close_output()?;
        self.calibration.take();
        self.pending_reload = None;
        self.engine.take();
        self.close_input();
        self.status.state = "waiting".into();
        self.status.processed_axes = [0.0; 6];
        self.status.stick_regions = Default::default();
        self.status.pad_region_capture = Default::default();
        self.status.reconnects += 1;
        self.navigation.reset();
        self.status.navigation_events.clear();
        self.status.navigation_held.clear();
        self.reset_pad_shortcut_events();
        self.status.input_chord.clear();
        self.status.motion = Default::default();
        self.status.calibration_progress = 0.0;
        self.status.message = "手柄已断开，已释放输出，正在等待原接口".into();
        self.next_discovery = Instant::now() + Duration::from_millis(500);
        self.publish_status();
        Ok(())
    }
    fn input(&mut self, event: InputEvent, from_snapshot: bool) -> Result<()> {
        if matches!(event, InputEvent::Disconnected { .. }) {
            return self.lost_device();
        }
        let previous_serial = self.status.input_serial;
        let controls_event = matches!(
            &event,
            InputEvent::Button { .. } | InputEvent::Axis { .. } | InputEvent::Touch { .. }
        );
        match event {
            InputEvent::Button { index, down, .. } => {
                if let Some(button) = self.status.buttons.get_mut(index as usize) {
                    if down && !*button && !from_snapshot {
                        self.status.last_input = motion_core::profile::BUTTONS
                            .get(index as usize)
                            .map(|s| s.to_string());
                        self.status.input_serial += 1;
                    }
                    *button = down;
                }
            }
            InputEvent::Axis { index, value, .. } => {
                if let Some(axis) = self.status.axes.get_mut(index as usize) {
                    let next = value as f32 / if value < 0 { 32768.0 } else { 32767.0 };
                    if !from_snapshot
                        && (if index < 4 { next.abs() } else { next.max(0.) }) > 0.6
                        && (axis.abs() <= 0.6 || axis.signum() != next.signum())
                    {
                        let input = match (index, next > 0.0) {
                            (0, false) => "left_stick_left",
                            (0, true) => "left_stick_right",
                            (1, false) => "left_stick_up",
                            (1, true) => "left_stick_down",
                            (2, false) => "right_stick_left",
                            (2, true) => "right_stick_right",
                            (3, false) => "right_stick_up",
                            (3, true) => "right_stick_down",
                            (4, _) => "left_trigger",
                            _ => "right_trigger",
                        };
                        self.status.last_input = Some(input.into());
                        self.status.input_serial += 1;
                    }
                    *axis = next;
                }
            }
            InputEvent::Gyro { value, .. } => {
                if value.iter().all(|v| v.is_finite()) {
                    self.status.gyro = value.map(f32::to_degrees);
                }
                if let Some(calibration) = &mut self.calibration {
                    calibration.sample(value);
                }
            }
            InputEvent::Accel { value, .. } => {
                if let Some(c) = &mut self.calibration {
                    c.sample_accel(value);
                }
            }
            InputEvent::Touch {
                pad,
                finger,
                active,
                x,
                y,
                ..
            } if !active
                || (x.is_finite()
                    && y.is_finite()
                    && (0.0..=1.).contains(&x)
                    && (0.0..=1.).contains(&y)) =>
            {
                let split = self
                    .status
                    .device
                    .as_ref()
                    .is_some_and(|device| device.touchpads >= 2);
                let before = motion_core::touchpad::fields(&self.status.touches, split);
                if let Some(slot) = self
                    .status
                    .touches
                    .get_mut(usize::from(pad))
                    .and_then(|p| p.get_mut(usize::from(finger)))
                {
                    *slot = motion_core::touchpad::TouchContact {
                        active,
                        x: if active { x } else { 0. },
                        y: if active { y } else { 0. },
                    };
                    let after = motion_core::touchpad::fields(&self.status.touches, split);
                    for (i, (before, after)) in before.into_iter().zip(after).enumerate() {
                        if after.active && !before.active && !from_snapshot {
                            self.status.last_input = Some(format!("touch{}", i + 1));
                            self.status.input_serial += 1;
                        }
                    }
                }
            }
            _ => {}
        }
        if self.status.input_serial != previous_serial {
            self.status.input_chord =
                motion_core::profile::pressed_inputs(&self.status.buttons, &self.status.axes, 0.6);
            for (i, contact) in motion_core::touchpad::fields(
                &self.status.touches,
                self.status
                    .device
                    .as_ref()
                    .is_some_and(|d| d.touchpads >= 2),
            )
            .into_iter()
            .enumerate()
            {
                if contact.active {
                    self.status.input_chord.push(format!("touch{}", i + 1));
                }
            }
        }
        if controls_event {
            self.navigation
                .update(&self.status.buttons, &self.status.axes, from_snapshot);
            if self.status.navigation_serial != self.navigation.serial
                || self.status.navigation_held != self.navigation.held
            {
                self.status.navigation_serial = self.navigation.serial;
                self.status.navigation_events = self.navigation.events.clone();
                self.status.navigation_held = self.navigation.held.clone();
            }
        }
        if self.status.binding_capture {
            if controls_event {
                if let Some(engine) = &mut self.engine {
                    // Update native physical inputs while the output lease is paused.
                    engine.process_snapshot(&event);
                    self.status.processed_axes = engine.processed_axes();
                }
                let previous_regions = self.status.stick_regions;
                self.track_pad_shortcut(from_snapshot);
                use motion_core::profile::StickRegion::{Center, Inner, Outer};
                let entered = previous_regions
                    .into_iter()
                    .zip(self.status.stick_regions)
                    .any(|(before, after)| {
                        before != after && (after == Outer || (before == Center && after == Inner))
                    });
                let non_stick_press = self.status.input_serial != previous_serial
                    && !matches!(event, InputEvent::Axis { index: 0..=3, .. });
                if !from_snapshot && (entered || non_stick_press) {
                    let mut chord = motion_core::profile::pressed_inputs(
                        &self.status.buttons,
                        &self.status.axes,
                        0.6,
                    );
                    chord.retain(|input| {
                        !motion_core::profile::DIRECTIONS.contains(&input.as_str())
                    });
                    chord.extend(
                        self.shortcut_extra_inputs()
                            .into_iter()
                            .flatten()
                            .map(String::from),
                    );
                    if !chord.is_empty() {
                        self.status.pad_region_capture.serial += 1;
                        self.status.pad_region_capture.chord = chord;
                    }
                }
            }
            return Ok(());
        }
        if let Some(engine) = &mut self.engine {
            if from_snapshot {
                engine.process_snapshot(&event)
            } else {
                engine.process(&event)
            };
            self.status.motion = engine.motion.telemetry.clone();
            self.status.processed_axes = engine.processed_axes();
        }
        if controls_event {
            self.track_pad_shortcut(from_snapshot);
        }
        if controls_event && !from_snapshot {
            self.sync_layers()?;
            if !self.output_paused() {
                if let Some(engine) = &mut self.engine {
                    engine.advance_flick_input();
                }
            }
        }
        if !self.output_paused() && !self.seeding {
            if let Some(engine) = &self.engine {
                self.dispatch(engine.frame.clone())?;
            }
        }
        Ok(())
    }
    fn set_pad_shortcuts(&mut self, bindings: Vec<PadShortcut>) -> Result<()> {
        if self.status.pad_shortcuts != bindings {
            let rules: Vec<_> = bindings
                .iter()
                .map(|binding| ShortcutRule {
                    trigger: ShortcutTrigger::Gamepad {
                        input: binding.input.clone(),
                    },
                    target: binding.target.clone(),
                    return_on_repeat: true,
                })
                .collect();
            validate_shortcut_rules(&rules)?;
            self.pad_shortcut_edges.clear();
            self.pad_shortcut_edges
                .resize_with(bindings.len(), PadShortcutEdges::default);
            self.status.pad_shortcuts = bindings;
            self.track_pad_shortcut(true);
        }
        Ok(())
    }

    fn reset_pad_shortcut_events(&mut self) {
        self.status.pad_shortcut_baseline = self.status.pad_shortcut_serial;
        self.status.pad_shortcut_events.clear();
    }

    fn track_pad_shortcut(&mut self, baseline: bool) {
        self.status.stick_regions = self.engine.as_mut().zip(self.config.as_ref()).map_or(
            Default::default(),
            |(engine, config)| {
                engine.base_stick_regions(&config.profile.stick_zones, self.status.stick_regions)
            },
        );
        let [left, right, touch1, touch2] = self.shortcut_extra_inputs();
        let [lt_soft, lt_full, rt_soft, rt_full] = self
            .engine
            .as_ref()
            .map(Engine::base_trigger_inputs)
            .unwrap_or_default();
        let extra = [
            left, right, touch1, touch2, lt_soft, lt_full, rt_soft, rt_full,
        ];
        let enabled = matches!(self.status.state.as_str(), "running" | "observing")
            && !self.status.ui_navigation
            && !self.output_paused()
            && !self.seeding
            && !self.engine.as_ref().is_some_and(Engine::is_measuring);
        if baseline {
            self.reset_pad_shortcut_events();
        }
        if self.status.pad_shortcuts.is_empty() {
            return;
        }
        let pressed = pressed_inputs(&self.status.buttons, &self.status.processed_axes, 0.6);
        let retained = pressed_inputs(&self.status.buttons, &self.status.processed_axes, 0.5);
        let mut matched = None;
        for (index, (edge, binding)) in self
            .pad_shortcut_edges
            .iter_mut()
            .zip(&self.status.pad_shortcuts)
            .enumerate()
        {
            if edge.update_with_held(Some(binding), &pressed, &retained, &extra, enabled, baseline) {
                let specificity = binding.input.split('+').count();
                // Prefer the longer chord when one control completes multiple
                // bindings, and prefer the first saved rule on a tie.
                if matched.is_none_or(|(_, best)| specificity > best) {
                    matched = Some((index, specificity));
                }
            }
        }
        if let Some((index, _)) = matched {
            self.status.pad_shortcut_serial = self.status.pad_shortcut_serial.saturating_add(1);
            let events = &mut self.status.pad_shortcut_events;
            if events.len() == PAD_SHORTCUT_EVENT_LIMIT {
                events.remove(0);
            }
            events.push(PadShortcutEvent {
                serial: self.status.pad_shortcut_serial,
                index,
            });
        }
    }

    fn shortcut_extra_inputs(&self) -> [Option<&'static str>; 4] {
        let split = self
            .status
            .device
            .as_ref()
            .is_some_and(|device| device.touchpads >= 2);
        let contacts = motion_core::touchpad::fields(&self.status.touches, split);
        [
            self.status.stick_regions[0].input(0),
            self.status.stick_regions[1].input(1),
            contacts[0].active.then_some("touch1"),
            contacts[1].active.then_some("touch2"),
        ]
    }
    fn sync_layers(&mut self) -> Result<()> {
        let Some(config) = self.config.as_ref() else {
            return Ok(());
        };
        if self.engine.is_none() || self.engine.as_ref().is_some_and(Engine::is_measuring) {
            return Ok(());
        }
        let active: Vec<_> = config
            .profile
            .layers
            .iter()
            .enumerate()
            .filter(|(index, layer)| {
                let threshold = layer.activation_threshold
                    - if self.active_layers.contains(index) {
                        layer.activation_hysteresis
                    } else {
                        0.
                    };
                motion_core::bindings::input_chord(&layer.input).is_some_and(|parts| {
                    parts.iter().all(|p| {
                        self.engine.as_ref().unwrap().layer_input_value(
                            p,
                            &config.profile,
                            self.active_layers.contains(index),
                        ) > threshold
                    })
                })
            })
            .map(|(i, _)| i)
            .collect();
        if active == self.active_layers {
            return Ok(());
        }
        let profile = motion_core::layers::resolve(&config.profile, &active)?;
        let feedback_changed = self.engine.as_ref().unwrap().profile.feedback != profile.feedback;
        self.engine.as_mut().unwrap().apply_layer(profile)?;
        self.status.active_layers = active
            .iter()
            .map(|i| config.profile.layers[*i].name.clone())
            .collect();
        self.active_layers = active;
        if feedback_changed {
            self.reset_feedback();
        }
        // Dispatcher diffs desired state: a shared ADS key remains held.
        self.publish_status();
        Ok(())
    }
    fn set_binding_capture(&mut self, active: bool) -> Result<()> {
        if active {
            if self.calibration.is_some() || self.engine.as_ref().is_some_and(Engine::is_measuring)
            {
                bail!("请先结束校准或转角测量");
            }
            if !self.status.binding_capture {
                if let Err(error) = self.clear_outputs() {
                    let message = format!("录入前无法释放输出：{error:#}");
                    self.fault(message.clone());
                    bail!(message);
                }
                self.status.pad_region_capture = Default::default();
                if let Some(engine) = &mut self.engine {
                    engine.reset();
                    if let Some(sdl) = &self.sdl {
                        for event in sdl.snapshot() {
                            engine.process_snapshot(&event);
                        }
                    }
                }
            }
            self.capture_until = Some(Instant::now() + Duration::from_secs(5));
            self.capture_releasing = false;
            self.status.binding_capture = true;
            self.status.emitting = false;
            self.status.state = "capturing".into();
            self.status.message = "正在录入映射；游戏输出和自动切换已暂停，手柄保持连接".into();
        } else if self.status.binding_capture {
            self.capture_until = None;
            self.capture_releasing = true;
            self.status.state = "capture_release".into();
            self.status.message = "录入已结束；松开手柄按键并让摇杆回中后恢复映射".into();
        }
        self.publish_status();
        Ok(())
    }
    fn advance_capture(&mut self, now: Instant) -> Result<()> {
        if self.capture_until.is_some_and(|until| now >= until) {
            self.set_binding_capture(false)?;
        }
        if self.capture_releasing
            && !self.resync_input
            && !self.status.buttons.iter().any(|v| *v)
            && !self
                .status
                .touches
                .iter()
                .flatten()
                .any(|contact| contact.active)
            && self.status.axes[..4].iter().all(|v| v.abs() <= 0.25)
            && self.status.axes[4..].iter().all(|v| *v <= 0.1)
        {
            self.capture_releasing = false;
            self.status.binding_capture = false;
            if self.engine.is_some() {
                self.prepare_engine()?;
            } else if self.config.is_none() {
                self.status.state = "idle".into();
                self.status.message = "录入已结束".into();
            }
            if let Some((path, result)) = self.pending_reload.take() {
                self.control(Control::Reload {
                    path,
                    result: Box::new(result),
                })?;
            }
        }
        Ok(())
    }
    fn output_pass(&mut self, pass: IoPass) -> Result<()> {
        let advance_at = pass.display_tick.unwrap_or(pass.started).max(pass.started);
        let dt = advance_at
            .saturating_duration_since(self.last_advance)
            .as_secs_f32();
        self.last_advance = advance_at;
        if self.output_paused() {
            self.io_clock.begin_flush(pass);
            return Ok(());
        }
        let Some(engine) = &mut self.engine else {
            self.io_clock.begin_flush(pass);
            return Ok(());
        };
        let now_ns = self.sdl.as_ref().map(Sdl::now_ns).unwrap_or(0);
        engine.advance_output(dt, now_ns);
        self.status.motion = engine.motion.telemetry.clone();
        if let Some(engine) = &self.engine {
            self.dispatch(engine.frame.clone())?;
        }
        self.io_clock.begin_flush(pass);
        if let Some(output) = &mut self.output {
            output.flush_output()?;
        }
        Ok(())
    }
    fn begin_input_resync(&mut self) -> Result<()> {
        self.resync_input = true;
        self.reset_pad_shortcut_events();
        self.clear_outputs()?;
        if let Some(engine) = &mut self.engine {
            if engine.is_measuring() {
                self.measurement_input_interrupted = true;
                engine.reset_pose();
                engine.motion.telemetry.measurement_error =
                    Some("手柄输入积压，当前转角测量已中断，请重新测量".into());
            } else {
                engine.reset();
            }
            self.status.motion = engine.motion.telemetry.clone();
        }
        Ok(())
    }
    fn finish_input_resync(&mut self, snapshot: Vec<InputEvent>) -> Result<()> {
        for event in snapshot {
            self.input(event, true)?;
        }
        self.resync_input = false;
        if !self.status.binding_capture {
            self.sync_layers()?;
        }
        self.track_pad_shortcut(true);
        if !self.output_paused() {
            if let Some(engine) = &self.engine {
                self.dispatch(engine.frame.clone())?;
            }
        }
        Ok(())
    }
    fn poll_inputs(&mut self) -> Result<()> {
        let Some(sdl) = &mut self.sdl else {
            return Ok(());
        };
        // Decide timeouts only through input known to have been polled. A
        // faster output tick could otherwise overtake a queued release.
        let watermark = sdl.now_ns();
        sdl.poll_events()?;
        let mut drained = false;
        for _ in 0..256 {
            let Some(sdl) = &self.sdl else { break };
            let Some(event) = sdl.take_event()? else {
                drained = true;
                break;
            };
            if matches!(event, InputEvent::Disconnected { .. }) {
                self.lost_device()?;
                break;
            }
            let age = sdl.now_ns().saturating_sub(event.time().host_ns);
            if age > MAX_EVENT_AGE_NS {
                self.status.stats.stale_events += 1;
                if !self.resync_input {
                    self.begin_input_resync()?;
                }
            }
            // Once input continuity is lost, a newer tail event can still depend
            // on an abandoned press or release. Resume only from a clean baseline.
            if self.resync_input
                || (self.measurement_input_interrupted
                    && matches!(event, InputEvent::Gyro { .. } | InputEvent::Accel { .. }))
            {
                continue;
            }
            self.queue.add(age);
            let start = Instant::now();
            self.input(
                event,
                self.config.is_none() || self.measurement_input_interrupted,
            )?;
            self.processing.add(start.elapsed().as_nanos() as u64);
        }
        if self.status.device.is_some() && self.connected_input().is_none() {
            self.lost_device()?;
        }
        if self.resync_input {
            if let Some(sdl) = self.sdl.as_ref().filter(|_| drained) {
                let snapshot = sdl.snapshot();
                match sdl.take_event()? {
                    Some(InputEvent::Disconnected { .. }) => self.lost_device()?,
                    Some(_) => {} // More input arrived during the snapshot; keep draining.
                    None => self.finish_input_resync(snapshot)?,
                }
            }
            return Ok(());
        }
        if drained && self.config.is_some() && !self.output_paused() && !self.seeding {
            if let Some(engine) = &mut self.engine {
                let before = engine.base_trigger_inputs();
                let changed = engine.advance_input(watermark);
                let trigger_changed = before != engine.base_trigger_inputs();
                self.status.processed_axes = engine.processed_axes();
                self.track_pad_shortcut(false);
                if trigger_changed {
                    self.sync_layers()?;
                }
                if changed || trigger_changed {
                    if let Some(engine) = &self.engine {
                        self.dispatch(engine.frame.clone())?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        if self.config.is_some() && emergency_stop() {
            self.stop_mapping("F12 紧急停止：输出已释放")?;
            self.publish_status();
        }
        if let Some(status) = self.visibility.heartbeat()? {
            self.status.visibility = status;
        }
        motion_core::hidmaestro::heartbeat()?;
        self.advance_capture(Instant::now())?;
        if self.config.is_none()
            && !self.status.binding_capture
            && Instant::now() >= self.next_discovery
        {
            if let Err(error) = self.refresh_devices() {
                self.status.message = format!("设备发现暂不可用：{error:#}");
            }
        }
        if let Some(guard) = &mut self.guard {
            guard.heartbeat()?;
        }
        if self.config.is_some()
            && self.engine.is_none()
            && !self.status.binding_capture
            && Instant::now() >= self.next_discovery
        {
            if let Err(error) = self.discover() {
                // SDL discovery errors can retry while the original interface is absent.
                self.close_output()?;
                self.engine.take();
                self.close_input();
                self.status.state = "waiting".into();
                self.status.message = format!("连接尚未就绪，稍后重试：{error:#}");
                self.next_discovery = Instant::now() + Duration::from_secs(2);
            }
        }
        if let Some(calibration) = &self.calibration {
            self.status.calibration_progress = calibration.progress();
            if calibration.done() {
                let result = self.calibration.take().unwrap().result_full(
                    &self
                        .config
                        .as_ref()
                        .context("Calibration lost config")?
                        .sensor_calibration,
                );
                match result {
                    Ok((bias, sensor)) => {
                        let config = self.config.as_mut().context("Calibration lost config")?;
                        config.bias = bias;
                        config.sensor_calibration = sensor.clone();
                        self.publisher.reliable(Event::Calibration {
                            device_path: config.device_path.clone(),
                            bias,
                            sensor,
                        })?;
                    }
                    Err(error) => {
                        self.status.reload_error = Some(error.to_string());
                    }
                }
                self.status.calibration_progress = 0.0;
                self.prepare_engine()?;
                if let Some((path, result)) = self.pending_reload.take() {
                    self.control(Control::Reload {
                        path,
                        result: Box::new(result),
                    })?;
                }
            }
        }
        let pass = self.io_clock.begin(Instant::now());
        if !pass.input_due && !pass.output_due {
            self.io_clock.wait_until(pass.wake);
            return Ok(());
        }
        if pass.input_due {
            self.poll_inputs()?;
        }
        if pass.output_due {
            self.output_pass(pass)?;
        }
        if self.last_stats.elapsed() >= Duration::from_secs(1) {
            if let Some(engine) = &self.engine {
                let count = engine.gyro_events;
                self.status.stats.gyro_hz = count.saturating_sub(self.last_samples) as f64
                    / self.last_stats.elapsed().as_secs_f64();
                self.last_samples = count;
                self.status.stats.gyro_events = count;
                self.status.stats.invalid_timestamps = engine.clock.stats.invalid_timestamps;
                self.status.stats.gaps = engine.clock.stats.gaps;
            } else {
                self.status.stats.gyro_hz = 0.0;
            }
            self.status.stats.process_p99_us = self.processing.summary().p99_us;
            self.status.stats.queue_p99_us = self.queue.summary().p99_us;
            if let Some(output) = &self.output {
                self.status.stats.mouse_counts = output.mouse_counts;
                self.status.stats.output = output.stats();
            }
            self.last_stats = Instant::now();
            self.processing = Histogram::default();
            self.queue = Histogram::default();
        }
        if self.last_feedback.elapsed() >= Duration::from_millis(8) {
            self.update_feedback();
            self.last_feedback = Instant::now();
        }
        if self.last_status.elapsed()
            >= Duration::from_millis(if self.status.ui_navigation { 33 } else { 66 })
        {
            self.publish_status();
            self.last_status = Instant::now();
        }
        self.io_clock.finish(pass);
        Ok(())
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.stop("引擎退出");
    }
}

pub fn run(dll: &Path) -> Result<()> {
    use std::io::{BufWriter, Write};
    let (output, events) = mpsc::sync_channel(128);
    let done = Arc::new(AtomicBool::new(false));
    let out_done = done.clone();
    let writer_thread = std::thread::spawn(move || {
        let mut writer = BufWriter::new(std::io::stdout().lock());
        for event in events {
            if serde_json::to_writer(&mut writer, &event).is_err()
                || writer.write_all(b"\n").is_err()
                || writer.flush().is_err()
            {
                break;
            }
        }
        out_done.store(true, Ordering::Relaxed);
    });
    let publisher = Publisher(output);
    publisher.reliable(Event::Hello {
        protocol: PROTOCOL_VERSION,
        version: env!("CARGO_PKG_VERSION").into(),
    })?;
    let (tx, rx) = mpsc::sync_channel(32);
    let reader_tx = tx.clone();
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            match motion_core::protocol::read_packet::<Request>(&mut input) {
                Ok(Some(request)) => {
                    if reader_tx.send(Control::Request(request)).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = reader_tx.send(Control::Closed);
                    break;
                }
            }
        }
    });
    let watch = Arc::new(Mutex::new(None));
    watch_profiles(watch.clone(), tx, done.clone());
    let mut runtime = Runtime::new(dll.to_owned(), publisher, watch);
    match runtime.visibility.recover() {
        Ok(status) => runtime.status.visibility = status,
        Err(error) => {
            runtime.status.visibility.error = Some(format!("上次隐藏恢复未完成：{error:#}"));
            runtime.status.state = "faulted".into();
            runtime.publisher.status(&runtime.status);
        }
    }
    if let Err(error) = runtime.scan() {
        runtime.status.message = format!("设备发现暂不可用：{error:#}");
    }
    io_schedule::prepare_io_thread();
    'service: while !done.load(Ordering::Relaxed) {
        for _ in 0..16 {
            match rx.try_recv() {
                Ok(control) => {
                    if !runtime.control(control)? {
                        break 'service;
                    }
                }
                Err(_) => break,
            }
        }
        if let Err(error) = runtime.tick() {
            runtime.fault(format!("输出已暂停：{error:#}"));
        }
    }
    done.store(true, Ordering::Relaxed);
    drop(runtime);
    let _ = writer_thread.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use motion_core::{engine::PadState, output::GamepadStatus, timing::SampleTime};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn finishing_angle_measurement_returns_the_final_sample_and_checks_the_revision() {
        let (mut runtime, _, rx) = fixture();
        let engine = runtime.engine.as_mut().unwrap();
        engine.begin_measurement("yaw").unwrap();
        engine.motion.telemetry.measured_degrees = 123.45;
        runtime.status.revision = 5;

        for revision in [4, 5] {
            runtime
                .control(Control::Request(Request {
                    id: revision,
                    command: Command::EndMeasure {
                        expected_revision: Some(revision),
                    },
                }))
                .unwrap();
            let reply = rx
                .try_iter()
                .find(|event| matches!(event, Event::Reply { .. }));
            if revision == 4 {
                assert!(matches!(
                    reply,
                    Some(Event::Reply {
                        ok: false,
                        measurement: None,
                        ..
                    })
                ));
                assert!(runtime.engine.as_ref().unwrap().is_measuring());
            } else {
                let Some(Event::Reply {
                    ok: true,
                    measurement: Some(result),
                    ..
                }) = reply
                else {
                    panic!("Expected the final measurement")
                };
                assert_eq!(result.degrees, 123.45);
                assert!(runtime.status.motion.measuring.is_none());
            }
        }
    }

    #[test]
    fn input_resync_restores_held_controls_without_new_presses() {
        let (mut runtime, held, _) = fixture();
        runtime.input(down(), false).unwrap();
        let previous = held.borrow().clone();
        let serial = runtime.status.input_serial;
        assert!(!previous.is_empty());

        runtime.begin_input_resync().unwrap();
        assert!(runtime.output_paused());
        assert!(held.borrow().is_empty());

        runtime.finish_input_resync(vec![down()]).unwrap();
        assert!(!runtime.output_paused());
        assert_eq!(*held.borrow(), previous);
        assert_eq!(runtime.status.input_serial, serial);
    }

    #[test]
    fn input_resync_interrupts_angle_measurement_until_finish() {
        let (mut runtime, held, rx) = fixture();
        let engine = runtime.engine.as_mut().unwrap();
        engine.begin_measurement("yaw").unwrap();
        engine.motion.telemetry.measured_degrees = 180.0;

        runtime.begin_input_resync().unwrap();
        runtime.finish_input_resync(vec![down()]).unwrap();
        assert!(runtime.output_paused());
        assert!(held.borrow().is_empty());
        assert_eq!(runtime.status.motion.measuring.as_deref(), Some("yaw"));
        assert!(runtime.status.motion.measurement_error.is_some());

        runtime
            .control(Control::Request(Request {
                id: 1,
                command: Command::EndMeasure {
                    expected_revision: None,
                },
            }))
            .unwrap();
        assert!(rx.try_iter().any(|event| matches!(
            event,
            Event::Reply {
                id: 1,
                ok: true,
                measurement: Some(result),
                ..
            } if result.error.is_some() && result.degrees == 180.0
        )));
        assert!(!runtime.output_paused());
        assert!(runtime.status.motion.measuring.is_none());
    }

    #[test]
    fn sensor_calibration_release_failure_stops_mapping_without_persisting() {
        struct FailFirstRelease {
            sink: Recording,
            fail_release: bool,
        }
        impl Sink for FailFirstRelease {
            fn button(&mut self, name: &str, down: bool) -> Result<()> {
                if !down && std::mem::take(&mut self.fail_release) {
                    bail!("simulated release failure");
                }
                self.sink.button(name, down)
            }
            fn mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
                self.sink.mouse(dx, dy)
            }
            fn gamepad(&mut self, pad: PadState) -> Result<()> {
                self.sink.gamepad(pad)
            }
        }

        let (mut runtime, held, rx) = fixture();
        runtime.config.as_mut().unwrap().emit = true;
        runtime.status.emitting = true;
        runtime.output = Some(Dispatcher::new(Box::new(FailFirstRelease {
            sink: Recording(held.clone()),
            fail_release: true,
        })));
        runtime.input(down(), false).unwrap();
        assert!(!held.borrow().is_empty());
        runtime
            .control(Control::Request(Request {
                id: 1,
                command: Command::SetSensorCalibration {
                    calibration: Default::default(),
                },
            }))
            .unwrap();

        assert_eq!(runtime.status.state, "faulted");
        assert!(!runtime.status.emitting);
        assert!(runtime.output.is_none());
        assert!(held.borrow().is_empty());
        let events: Vec<_> = rx.try_iter().collect();
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::Calibration { .. })));
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Reply {
                id: 1, ok: false, ..
            }
        )));
    }

    #[test]
    fn sensor_calibration_saved_while_waiting_does_not_block_reconnect() {
        let (mut runtime, _, rx) = fixture();
        runtime.lost_device().unwrap();
        runtime
            .control(Control::Request(Request {
                id: 1,
                command: Command::SetSensorCalibration {
                    calibration: Box::new(motion_core::motion_config::SensorCalibration {
                        gyro_noise_dps: 0.25,
                        ..Default::default()
                    }),
                },
            }))
            .unwrap();

        assert_eq!(runtime.status.state, "waiting");
        assert!(runtime.engine.is_none());
        assert!(runtime.output.is_none());
        let config = runtime.config.as_ref().unwrap();
        assert_eq!(config.sensor_calibration.gyro_noise_dps, 0.25);
        assert!(rx.try_iter().any(|event| matches!(
            event,
            Event::Calibration { sensor, .. } if sensor.gyro_noise_dps == 0.25
        )));
    }

    #[test]
    fn held_stick_and_flick_emit_at_the_mouse_output_period() {
        for (hz, flick, expected_counts, max_step) in [
            (500, false, 120, 3),
            (500, true, 900, 19),
            (960, false, 120, 2),
            (960, true, 900, 11),
        ] {
            let (mut runtime, _, _rx) = fixture();
            let profile = &mut runtime.config.as_mut().unwrap().profile;
            profile.stick_mouse.enabled = !flick;
            profile.stick_mouse.speed = 1200.0;
            profile.gyro.enabled = flick;
            profile.gyro.counts_per_degree = 10.0;
            profile.gyro.motion.flick.enabled = flick;
            profile.gyro.motion.flick.smooth_ms = 100.0;
            // Keep the turn linear so this test isolates the output schedule.
            profile.gyro.motion.flick.release_ease = 0.0;
            runtime.engine = Some(Engine::new(profile.clone()).unwrap());
            runtime.output = Some(
                Dispatcher::new(Box::new(ObserveSink) as Box<dyn Sink>).with_scheduled_output(),
            );

            let start = Instant::now() - Duration::from_secs(1);
            runtime.last_advance = start;
            let poll_period = Duration::from_millis(2);
            let period = Duration::from_nanos(1_000_000_000 / u64::from(hz));
            let mut schedule = io_schedule::IoSchedule::new(start + period);
            let mut advance = |runtime: &mut Runtime, slot| {
                let deadline = start + period * slot;
                let tick = (hz == 960).then_some(deadline);
                let now = tick.map_or(deadline, |t| t - Duration::from_micros(100));
                let pass = schedule.begin(now, poll_period, period, tick, Duration::ZERO);
                assert!(pass.output_due);
                runtime.output_pass(pass).unwrap();
                schedule.flushed(deadline);
                schedule.finish(pass, deadline, tick.map(|t| t + period));
            };
            let axis = |host_ns, value| InputEvent::Axis {
                time: SampleTime {
                    host_ns,
                    sensor_ns: 0,
                },
                index: 2,
                value,
            };
            let slots = hz / 10;
            let slot_ns = period.as_nanos() as u64;
            advance(&mut runtime, 1);
            runtime.input(axis(slot_ns + 1, i16::MAX), false).unwrap();
            let mut previous = 0;
            for slot in 2..=slots + 1 {
                advance(&mut runtime, slot);
                let counts = runtime.output.as_ref().unwrap().mouse_counts[0];
                assert!(
                    (1..=max_step).contains(&(counts - previous)),
                    "flick={flick}, {hz} Hz slot {slot}, step={}",
                    counts - previous
                );
                previous = counts;
            }
            assert!(previous.abs_diff(expected_counts) <= 1);

            runtime
                .input(axis(slot_ns * u64::from(slots + 1) + 1, 0), false)
                .unwrap();
            advance(&mut runtime, slots + 2);
            // 96 truncated nanosecond slots can leave one fractional count
            // of the 100 ms Flick to finish in the next slot.
            let remaining = u64::from(flick && hz == 960);
            let counts = runtime.output.as_ref().unwrap().mouse_counts[0];
            assert!(counts.abs_diff(previous) <= remaining);
        }
    }

    #[test]
    fn capture_timeout_waits_for_neutral_then_resumes_the_original_session() {
        let (mut runtime, held, rx) = fixture();
        runtime
            .dispatch(Frame {
                mouse_delta: [3.75, -2.5],
                ..Frame::default()
            })
            .unwrap();
        runtime.publish_status();
        let Event::Status { status } = rx.try_recv().unwrap() else {
            panic!("expected observation status");
        };
        assert!(!status.emitting);
        assert_eq!(
            status
                .output_preview
                .unwrap()
                .mouse
                .last()
                .unwrap()
                .position,
            [3, -2]
        );
        runtime.input(down(), false).unwrap();
        let events = automatic(&mut runtime, held.clone(), false, false, false);
        runtime.set_binding_capture(true).unwrap();
        runtime.capture_until = Some(Instant::now() - Duration::from_secs(1));
        runtime.advance_capture(Instant::now()).unwrap();
        assert!(runtime.status.binding_capture);
        assert!(runtime.capture_releasing);
        assert!(held.borrow().is_empty());
        assert!(rx.try_iter().any(|event| matches!(
            event,
            Event::Status { status } if status.binding_capture && status.output_preview.is_none()
        )));
        runtime
            .input(
                InputEvent::Button {
                    time: SampleTime {
                        host_ns: 2,
                        sensor_ns: 2,
                    },
                    index: 0,
                    down: false,
                },
                false,
            )
            .unwrap();
        runtime.advance_capture(Instant::now()).unwrap();
        assert!(!runtime.status.binding_capture);
        assert!(runtime.status.emitting);
        assert!(runtime.status.visibility.active);
        assert_eq!(runtime.config.as_ref().unwrap().profile_id, "combat");
        assert_eq!(*events.borrow(), ["prepare", "verify virtual"]);
        runtime.input(down(), false).unwrap();
        assert!(!held.borrow().is_empty());
    }
    #[test]
    fn outer_ring_layer_uses_base_geometry_when_overriding_its_own_ring() {
        let (mut runtime, _held, _rx) = fixture();
        runtime
            .config
            .as_mut()
            .unwrap()
            .profile
            .layers
            .push(motion_core::layers::Layer {
                id: "outer".into(),
                name: "Outer layer".into(),
                input: "left_stick_outer".into(),
                activation_threshold: 0.5,
                activation_hysteresis: 0.1,
                settings: serde_json::from_value(
                    serde_json::json!({"stick_zones":{"left_outer":0.98}}),
                )
                .unwrap(),
                actions: vec![],
                disabled_inputs: vec![],
            });
        let axis = |value| InputEvent::Axis {
            time: SampleTime {
                host_ns: 1,
                sensor_ns: 1,
            },
            index: 0,
            value,
        };
        runtime.input(axis(30000), false).unwrap();
        assert_eq!(runtime.status.active_layers, ["Outer layer"]);
        assert_eq!(
            runtime
                .engine
                .as_ref()
                .unwrap()
                .profile
                .stick_zones
                .left_outer,
            0.98
        );
        assert_eq!(
            runtime
                .engine
                .as_ref()
                .unwrap()
                .input_value("left_stick_outer"),
            0.
        );
        runtime.input(down(), false).unwrap();
        assert_eq!(runtime.status.active_layers, ["Outer layer"]);
        // Input is now source-deadzoned before native ring geometry. This raw
        // travel remains above the base ring's processed hysteresis floor.
        runtime.input(axis(28000), false).unwrap();
        assert_eq!(runtime.status.active_layers, ["Outer layer"]);
        runtime.input(axis(25000), false).unwrap();
        assert!(runtime.status.active_layers.is_empty());
    }
    #[test]
    fn analog_layer_activation_has_release_hysteresis() {
        let (mut runtime, _held, _rx) = fixture();
        runtime
            .config
            .as_mut()
            .unwrap()
            .profile
            .layers
            .push(motion_core::layers::Layer {
                id: "ads".into(),
                name: "ADS".into(),
                input: "left_trigger".into(),
                activation_threshold: 0.5,
                activation_hysteresis: 0.1,
                settings: serde_json::from_value(serde_json::json!({"gyro":{"sensitivity":0.4}}))
                    .unwrap(),
                actions: vec![],
                disabled_inputs: vec![],
            });
        let axis = |value| InputEvent::Axis {
            time: SampleTime {
                host_ns: 1,
                sensor_ns: 1,
            },
            index: 4,
            value,
        };
        runtime.input(axis(16000), false).unwrap();
        assert!(runtime.status.active_layers.is_empty());
        runtime.input(axis(18000), false).unwrap();
        assert_eq!(runtime.status.active_layers, ["ADS"]);
        runtime.input(axis(14000), false).unwrap();
        assert_eq!(runtime.status.active_layers, ["ADS"]);
        runtime.input(axis(12000), false).unwrap();
        assert!(runtime.status.active_layers.is_empty());
    }
    #[test]
    fn binding_capture_keeps_input_and_visibility_and_does_not_emit_recorded_controls() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        assert!(!held.borrow().is_empty());
        let events = automatic(&mut runtime, held.clone(), false, false, false);
        runtime.set_binding_capture(true).unwrap();
        assert!(held.borrow().is_empty());
        runtime.input(down(), false).unwrap();
        assert!(held.borrow().is_empty());
        assert!(runtime.engine.is_some());
        assert!(runtime.config.as_ref().unwrap().emit);
        assert!(runtime.status.visibility.active);
        assert!(events.borrow().is_empty());
        runtime.set_binding_capture(false).unwrap();
        assert!(runtime.capture_releasing);
        assert!(runtime.status.binding_capture);
        runtime
            .input(
                InputEvent::Button {
                    time: SampleTime {
                        host_ns: 2,
                        sensor_ns: 2,
                    },
                    index: 0,
                    down: false,
                },
                false,
            )
            .unwrap();
        assert!(held.borrow().is_empty());
        runtime.stop("F12 during capture").unwrap();
        assert_eq!(*events.borrow(), ["restore"]);
    }
    #[test]
    fn held_layers_restore_settings_and_keep_shared_ads_output_without_reopening_devices() {
        let (mut runtime, held, _rx) = fixture();
        let mut base = runtime.config.as_ref().unwrap().profile.clone();
        base.actions = vec![motion_core::profile::Action {
            name: "ADS".into(),
            input: "left_shoulder".into(),
            output: "mouse:right".into(),
            threshold: 0.5,
            repeat_ms: None,
            trigger: Default::default(),
            trigger_ms: 250,
            sustain: true,
            turbo_ms: None,
            allow_simultaneous: false,
        }];
        let sensitivity = base.gyro.sensitivity;
        base.layers.push(motion_core::layers::Layer {
            activation_threshold: 0.5,
            activation_hysteresis: 0.1,
            id: "ads".into(),
            name: "ADS layer".into(),
            input: "left_shoulder".into(),
            settings: serde_json::from_value(serde_json::json!({"gyro":{"sensitivity":0.5}}))
                .unwrap(),
            actions: vec![],
            disabled_inputs: vec![],
        });
        runtime.config.as_mut().unwrap().profile = base.clone();
        runtime.engine = Some(Engine::new(base).unwrap());
        let events = automatic(&mut runtime, held.clone(), false, false, false);
        let shoulder = |down| InputEvent::Button {
            time: SampleTime {
                host_ns: 2,
                sensor_ns: 2,
            },
            index: 9,
            down,
        };
        runtime.input(shoulder(true), false).unwrap();
        assert_eq!(
            runtime.engine.as_ref().unwrap().profile.gyro.sensitivity,
            0.5
        );
        assert_eq!(runtime.status.active_layers, ["ADS layer"]);
        assert!(held.borrow().contains("mouse:right"));
        runtime.input(shoulder(false), false).unwrap();
        assert_eq!(
            runtime.engine.as_ref().unwrap().profile.gyro.sensitivity,
            sensitivity
        );
        assert!(held.borrow().is_empty());
        assert!(runtime.status.active_layers.is_empty());
        assert!(events.borrow().is_empty());
        assert!(runtime.status.visibility.active);
    }
    struct FakeVisibility {
        events: Rc<RefCell<Vec<&'static str>>>,
        held: Rc<RefCell<BTreeSet<String>>>,
        absent: bool,
        fail_verify: bool,
        fail_release: bool,
    }
    impl VisibilityLease for FakeVisibility {
        fn acquire(&mut self, _: &str) -> Result<Option<VisibilityStatus>> {
            self.events.borrow_mut().push("hide");
            Ok((!self.absent).then(|| VisibilityStatus {
                requested: true,
                active: true,
                hidden_paths: vec!["physical".into()],
                error: None,
            }))
        }
        fn preparing(&mut self) -> Result<()> {
            self.events.borrow_mut().push("prepare");
            Ok(())
        }
        fn verify(&mut self, _: Vec<String>, _: bool) -> Result<()> {
            self.events.borrow_mut().push("verify virtual");
            if self.fail_verify {
                bail!("virtual output masked")
            }
            Ok(())
        }
        fn heartbeat(&mut self) -> Result<Option<VisibilityStatus>> {
            Ok(None)
        }
        fn release(&mut self) -> Result<VisibilityStatus> {
            assert!(
                self.held.borrow().is_empty(),
                "physical controller exposed before keys were released"
            );
            self.events.borrow_mut().push("restore");
            if self.fail_release {
                bail!("driver busy")
            }
            Ok(VisibilityStatus::default())
        }
        fn recover(&mut self) -> Result<VisibilityStatus> {
            self.release()
        }
    }
    fn automatic(
        runtime: &mut Runtime,
        held: Rc<RefCell<BTreeSet<String>>>,
        absent: bool,
        fail_verify: bool,
        fail_release: bool,
    ) -> Rc<RefCell<Vec<&'static str>>> {
        let events = Rc::new(RefCell::new(vec![]));
        runtime.visibility = Box::new(FakeVisibility {
            events: events.clone(),
            held,
            absent,
            fail_verify,
            fail_release,
        });
        runtime.config.as_mut().unwrap().emit = true;
        runtime.config.as_mut().unwrap().auto_hide = true;
        runtime.status.visibility = VisibilityStatus {
            requested: true,
            active: !absent,
            hidden_paths: vec!["physical".into()],
            error: None,
        };
        events
    }
    #[test]
    fn disconnect_keeps_physical_hiding_until_full_stop_and_releases_outputs_first() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        assert!(!held.borrow().is_empty());
        let events = automatic(&mut runtime, held.clone(), false, false, false);
        runtime.lost_device().unwrap();
        assert!(held.borrow().is_empty());
        assert!(runtime.status.visibility.active);
        assert_eq!(runtime.status.state, "waiting");
        assert!(events.borrow().is_empty());
        runtime.stop("F12").unwrap();
        assert_eq!(*events.borrow(), ["restore"]);
        assert!(!runtime.status.visibility.active);
    }
    #[test]
    fn stopping_can_keep_input_connected_and_a_later_unplug_stays_idle() {
        let (mut runtime, held, rx) = fixture();
        runtime.status.device = Some(Device {
            id: 42,
            name: "Selected controller".into(),
            vendor: 0,
            product: 0,
            path: "physical#MI_02".into(),
            origin: InputOrigin::Physical,
            gyro: true,
            gamepad: true,
            accelerometer: true,
            touchpads: 0,
            rumble: false,
            lightbar: false,
            buttons: vec!["south".into()],
            open_error: None,
        });
        runtime.input(down(), false).unwrap();
        let events = automatic(&mut runtime, held.clone(), false, false, false);
        assert!(!held.borrow().is_empty());
        runtime.finish_session("Stopped", true).unwrap();
        assert_eq!(runtime.status.state, "idle");
        assert_eq!(runtime.status.device.as_ref().unwrap().id, 42);
        assert!(runtime.status.buttons[0]);
        assert!(!runtime.status.emitting);
        assert!(runtime.config.is_none() && runtime.engine.is_none() && runtime.output.is_none());
        assert_eq!(*events.borrow(), ["restore"]);

        runtime.lost_device().unwrap();
        assert_eq!(runtime.status.state, "idle");
        assert!(runtime.status.device.is_none());
        assert!(!runtime.status.buttons[0]);
        assert_eq!(*events.borrow(), ["restore"]);
        assert!(
            matches!(rx.try_recv().unwrap(), Event::Status { status } if status.device.is_none())
        );
    }
    #[test]
    fn start_publishes_only_the_new_session_state() {
        let (mut runtime, _, rx) = fixture();
        let config = runtime.config.as_ref().unwrap().clone();
        runtime
            .control(Control::Request(Request {
                id: 1,
                command: Command::Start {
                    config: Box::new(config),
                },
            }))
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::Status { status } if status.state == "waiting"
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::Reply { id: 1, ok: true, .. }
        ));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn output_is_verified_during_profile_reprepare_and_before_emission() {
        let (mut runtime, held, _rx) = fixture();
        let events = automatic(&mut runtime, held, false, false, false);
        runtime.prepare_engine().unwrap();
        runtime.config.as_mut().unwrap().profile.gyro.sensitivity = 2.;
        runtime.prepare_engine().unwrap();
        assert_eq!(
            *events.borrow(),
            ["prepare", "verify virtual", "prepare", "verify virtual"]
        );
        assert!(runtime.status.visibility.active);
    }
    #[test]
    fn masked_virtual_output_faults_and_restores_physical_after_releasing_keys() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        let events = automatic(&mut runtime, held.clone(), false, true, false);
        let error = runtime.prepare_engine().unwrap_err();
        runtime.fault(error.to_string());
        assert_eq!(*events.borrow(), ["prepare", "verify virtual", "restore"]);
        assert!(!runtime.status.emitting);
        assert!(held.borrow().is_empty());
        assert!(!runtime.status.visibility.active);
        assert_eq!(runtime.status.state, "faulted");
    }
    #[test]
    fn absent_controller_waits_without_sdl_or_new_hide_rules() {
        let (mut runtime, held, _rx) = fixture();
        let events = automatic(&mut runtime, held, true, false, false);
        runtime.close_output().unwrap();
        runtime.engine.take();
        runtime.discover().unwrap();
        assert_eq!(runtime.status.state, "waiting");
        assert!(!runtime.status.visibility.active);
        assert!(runtime.sdl.is_none());
        assert_eq!(*events.borrow(), ["hide"]);
    }
    #[test]
    fn eof_and_drop_restore_the_lease_and_failed_restore_stays_visible_for_retry() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        let events = automatic(&mut runtime, held.clone(), false, false, true);
        assert!(runtime.control(Control::Closed).is_err());
        assert!(runtime
            .status
            .visibility
            .error
            .as_deref()
            .unwrap()
            .contains("driver busy"));
        assert!(!runtime.status.visibility.requested);
        assert!(held.borrow().is_empty());
        runtime.visibility = Box::new(FakeVisibility {
            events: events.clone(),
            held,
            absent: false,
            fail_verify: false,
            fail_release: false,
        });
        drop(runtime);
        assert_eq!(*events.borrow(), ["restore", "restore"]);
    }
    #[test]
    fn ui_navigation_releases_mapping_and_uses_a_blank_observer_without_a_profile_watch() {
        let (mut runtime, held, _rx) = fixture();
        runtime
            .input(
                InputEvent::Button {
                    time: SampleTime {
                        host_ns: 1_000_000,
                        sensor_ns: 0,
                    },
                    index: 0,
                    down: true,
                },
                false,
            )
            .unwrap();
        assert!(!held.borrow().is_empty());
        runtime
            .control(Control::Request(Request {
                id: 42,
                command: Command::Navigate {
                    device_path: "test://absent".into(),
                },
            }))
            .unwrap();
        assert!(held.borrow().is_empty());
        assert!(runtime.status.ui_navigation);
        assert!(runtime.status.profile_id.is_none());
        let config = runtime.config.as_ref().unwrap();
        assert!(!config.emit);
        assert!(!config.profile.gyro.enabled);
        assert!(config.profile.actions.is_empty());
        assert!(runtime.watch.lock().unwrap().is_none());
        assert!(runtime.guard.is_none());
        assert!(runtime.output.is_none());
    }
    #[test]
    fn navigation_telemetry_keeps_fast_taps_and_clears_on_disconnect() {
        let (mut runtime, _held, _rx) = fixture();
        let event = |down| InputEvent::Button {
            time: SampleTime {
                host_ns: 1_000_000,
                sensor_ns: 0,
            },
            index: 0,
            down,
        };
        runtime.input(event(true), true).unwrap();
        assert_eq!(runtime.status.navigation_serial, 0);
        runtime.input(event(false), false).unwrap();
        runtime.input(event(true), false).unwrap();
        runtime.input(event(false), false).unwrap();
        assert_eq!(runtime.status.navigation_serial, 1);
        assert_eq!(runtime.status.navigation_events.len(), 1);
        assert!(runtime.status.navigation_held.is_empty());
        runtime.lost_device().unwrap();
        assert!(runtime.status.navigation_events.is_empty());
        assert!(runtime.status.navigation_held.is_empty());
    }
    struct Recording(Rc<RefCell<BTreeSet<String>>>);
    impl Sink for Recording {
        fn button(&mut self, name: &str, down: bool) -> Result<()> {
            if down {
                self.0.borrow_mut().insert(name.into());
            } else {
                self.0.borrow_mut().remove(name);
            }
            Ok(())
        }
        fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
            Ok(())
        }
        fn gamepad(&mut self, _: PadState) -> Result<()> {
            Ok(())
        }
    }
    #[test]
    fn pad_shortcut_keeps_every_raw_tap_without_status_polling_or_profile_engine_ownership() {
        let (mut runtime, _, _rx) = fixture();
        runtime
            .set_pad_shortcuts(vec![PadShortcut {
                input: "south".into(),
                target: "aim".into(),
            }])
            .unwrap();
        runtime.track_pad_shortcut(true);
        for _ in 0..2 {
            runtime.input(down(), false).unwrap();
            runtime
                .input(
                    InputEvent::Button {
                        time: SampleTime {
                            host_ns: 2,
                            sensor_ns: 2,
                        },
                        index: 0,
                        down: false,
                    },
                    false,
                )
                .unwrap();
        }
        assert_eq!(runtime.status.pad_shortcut_serial, 2);
        runtime.status.binding_capture = true;
        runtime.track_pad_shortcut(true);
        runtime.input(down(), false).unwrap();
        assert_eq!(runtime.status.pad_shortcut_serial, 2);
        runtime.status.binding_capture = false;
        runtime.track_pad_shortcut(true);
        runtime.input(down(), false).unwrap();
        assert_eq!(runtime.status.pad_shortcut_serial, 2);
        assert_eq!(runtime.status.pad_shortcut_baseline, 2);
    }

    #[test]
    fn controller_shortcuts_keep_rule_order_across_edges_and_rebase_on_replacement() {
        let (mut runtime, _, _rx) = fixture();
        let bindings = [("east", "menu"), ("south+east", "aim"), ("west", "map")]
            .map(|(input, target)| PadShortcut {
                input: input.into(),
                target: target.into(),
            });
        runtime.set_pad_shortcuts(bindings.to_vec()).unwrap();
        runtime.track_pad_shortcut(true);
        let button = |index, down| InputEvent::Button {
            time: SampleTime {
                host_ns: 10,
                sensor_ns: 0,
            },
            index,
            down,
        };
        for (index, pressed) in [(0, true), (1, true), (1, false), (0, false), (2, true)] {
            runtime.input(button(index, pressed), false).unwrap();
        }
        assert_eq!(
            runtime.status.pad_shortcut_events,
            [
                PadShortcutEvent {
                    serial: 1,
                    index: 1,
                },
                PadShortcutEvent {
                    serial: 2,
                    index: 2,
                },
            ]
        );
        let mut invalid = bindings.to_vec();
        invalid[1].input.clear();
        assert!(runtime.set_pad_shortcuts(invalid).is_err());
        assert_eq!(runtime.status.pad_shortcuts, bindings);
        assert_eq!(runtime.status.pad_shortcut_events.len(), 2);

        runtime.set_pad_shortcuts(vec![bindings[2].clone()]).unwrap();
        runtime.track_pad_shortcut(true);
        runtime.input(button(2, true), false).unwrap();
        assert!(runtime.status.pad_shortcut_events.is_empty());
        assert_eq!(runtime.status.pad_shortcut_baseline, 2);
        runtime.input(button(2, false), false).unwrap();
        runtime.input(button(2, true), false).unwrap();
        assert_eq!(
            runtime.status.pad_shortcut_events,
            [PadShortcutEvent {
                serial: 3,
                index: 0,
            }]
        );
    }

    #[test]
    fn outer_shortcut_uses_base_geometry_and_capture_keeps_the_outer_edge() {
        use motion_core::profile::StickRegion::{Inner, Outer};
        let (mut runtime, held, _rx) = fixture();
        let mut active = runtime.config.as_ref().unwrap().profile.clone();
        active.stick_zones.left_outer = 0.99;
        runtime
            .engine
            .as_mut()
            .unwrap()
            .apply_layer(active)
            .unwrap();
        runtime
            .set_pad_shortcuts(vec![PadShortcut {
                input: "left_stick_outer".into(),
                target: "aim".into(),
            }])
            .unwrap();
        let axis = |value: f32| InputEvent::Axis {
            time: SampleTime {
                host_ns: 10,
                sensor_ns: 0,
            },
            index: 0,
            value: (value * 32767.0) as i16,
        };
        runtime.track_pad_shortcut(true);
        runtime.input(axis(0.75), false).unwrap();
        assert_eq!(runtime.status.stick_regions[0], Inner);
        assert_eq!(runtime.status.pad_shortcut_serial, 0);
        runtime.input(axis(0.95), false).unwrap();
        assert_eq!(runtime.status.stick_regions[0], Outer);
        assert_eq!(
            runtime
                .engine
                .as_ref()
                .unwrap()
                .input_value("left_stick_outer"),
            0.
        );
        runtime.track_pad_shortcut(true);
        runtime.input(axis(0.86), false).unwrap();
        assert_eq!(runtime.status.stick_regions[0], Outer);
        assert_eq!(runtime.status.pad_shortcut_serial, 1);
        runtime.input(axis(0.75), false).unwrap();
        runtime.input(axis(0.95), false).unwrap();
        assert_eq!(runtime.status.pad_shortcut_serial, 2);

        runtime.input(axis(0.), false).unwrap();
        runtime.set_binding_capture(true).unwrap();
        for value in [0.75, 0.95, 0.75, 0.] {
            runtime.input(axis(value), false).unwrap();
        }
        assert_eq!(
            runtime.status.pad_region_capture.chord,
            ["left_stick_outer"]
        );
        assert_eq!(runtime.status.pad_shortcut_serial, 2);
        assert!(held.borrow().is_empty());
    }

    #[test]
    fn touch_contact_can_complete_a_switch_chord_and_release_rearms_it() {
        let (mut runtime, _, _rx) = fixture();
        runtime
            .set_pad_shortcuts(vec![PadShortcut {
                input: "south+touch1".into(),
                target: "aim".into(),
            }])
            .unwrap();
        runtime.track_pad_shortcut(true);
        runtime.input(down(), false).unwrap();
        assert_eq!(runtime.status.pad_shortcut_serial, 0);
        for active in [true, true, false, true] {
            runtime
                .input(
                    InputEvent::Touch {
                        time: SampleTime {
                            host_ns: 10,
                            sensor_ns: 0,
                        },
                        pad: 0,
                        finger: 0,
                        active,
                        x: 0.5,
                        y: 0.5,
                    },
                    false,
                )
                .unwrap();
        }
        assert_eq!(runtime.status.pad_shortcut_serial, 2);
    }
    fn fixture() -> (
        Runtime,
        Rc<RefCell<BTreeSet<String>>>,
        mpsc::Receiver<Event>,
    ) {
        let (tx, rx) = mpsc::sync_channel(128);
        let mut runtime = Runtime::new(PathBuf::new(), Publisher(tx), Arc::new(Mutex::new(None)));
        let profile = parse_profile(include_str!("../../../examples/keyboard-mouse.toml")).unwrap();
        let held = Rc::new(RefCell::new(BTreeSet::new()));
        runtime.config = Some(RunConfig {
            profile_id: "combat".into(),
            profile: profile.clone(),
            profile_file: "combat.toml".into(),
            device_path: "physical#MI_02".into(),
            emit: false,
            auto_hide: false,
            output_backend: Default::default(),
            gamepad_backend: Default::default(),
            mouse_display_sync: false,
            io_rates: Default::default(),
            bias: [0.0; 3],
            sensor_calibration: Default::default(),
        });
        runtime.engine = Some(Engine::new(profile).unwrap());
        runtime.output = Some(Dispatcher::new(Box::new(Recording(held.clone()))));
        runtime.status.state = "observing".into();
        runtime.status.profile_id = Some("combat".into());
        (runtime, held, rx)
    }
    fn mock_xbox_status() -> GamepadStatus {
        GamepadStatus {
            target: GamepadTarget::Xbox360,
            backend: GamepadBackend::Vigem,
            connected: true,
            player_index: Some(0),
            protected_devices: Vec::new(),
        }
    }

    #[test]
    fn feedback_capture_waits_for_touch_release_and_requires_a_fresh_game_command() {
        struct FeedbackSink(Arc<Mutex<motion_core::feedback::FeedbackFrame>>);
        impl Sink for FeedbackSink {
            fn gamepad_status(&self) -> Option<GamepadStatus> {
                Some(mock_xbox_status())
            }
            fn feedback(&mut self) -> Option<motion_core::feedback::FeedbackFrame> {
                Some(self.0.lock().unwrap().clone())
            }
            fn button(&mut self, _: &str, _: bool) -> Result<()> {
                Ok(())
            }
            fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
                Ok(())
            }
            fn gamepad(&mut self, _: motion_core::engine::PadState) -> Result<()> {
                Ok(())
            }
        }
        let (mut runtime, _held, _rx) = fixture();
        let frame = Arc::new(Mutex::new(motion_core::feedback::FeedbackFrame {
            sequence: 1,
            rumble: [0.5, 0.25],
            lightbar: None,
        }));
        runtime.output = Some(Dispatcher::new(Box::new(FeedbackSink(frame.clone()))));
        let config = runtime.config.as_mut().unwrap();
        config.emit = true;
        config.profile.virtual_gamepad = GamepadTarget::Xbox360;
        runtime.engine = Some(Engine::new(config.profile.clone()).unwrap());
        runtime.status.emitting = true;
        runtime.update_feedback();
        assert_eq!(runtime.feedback.status.rumble, [0.5, 0.25]);
        runtime.set_binding_capture(true).unwrap();
        assert_eq!(runtime.feedback.status.rumble, [0.; 2]);
        frame.lock().unwrap().sequence = 2;
        runtime.update_feedback();
        assert_eq!(runtime.feedback.status.rumble, [0.; 2]);
        let contact = |active| InputEvent::Touch {
            time: SampleTime {
                host_ns: 10,
                sensor_ns: 0,
            },
            pad: 0,
            finger: 0,
            active,
            x: 0.5,
            y: 0.5,
        };
        runtime.input(contact(true), false).unwrap();
        runtime.set_binding_capture(false).unwrap();
        runtime.advance_capture(Instant::now()).unwrap();
        assert!(runtime.status.binding_capture);
        runtime.input(contact(false), false).unwrap();
        runtime.advance_capture(Instant::now()).unwrap();
        assert!(!runtime.status.binding_capture);
        runtime.update_feedback();
        assert_eq!(runtime.feedback.status.rumble, [0.; 2]);
        frame.lock().unwrap().sequence = 3;
        runtime.update_feedback();
        assert_eq!(runtime.feedback.status.rumble, [0.5, 0.25]);
    }
    fn down() -> InputEvent {
        InputEvent::Button {
            time: SampleTime {
                host_ns: 1,
                sensor_ns: 1,
            },
            index: 0,
            down: true,
        }
    }
    #[test]
    fn fast_chords_are_retained_after_release_and_snapshot_buttons_are_not_new_presses() {
        let (mut runtime, _held, _rx) = fixture();
        let b = |index, down| InputEvent::Button {
            time: SampleTime {
                host_ns: 1,
                sensor_ns: 1,
            },
            index,
            down,
        };
        runtime.input(b(9, true), false).unwrap();
        runtime.input(b(0, true), false).unwrap();
        runtime.input(b(0, false), false).unwrap();
        runtime.input(b(9, false), false).unwrap();
        assert_eq!(runtime.status.input_serial, 2);
        assert_eq!(runtime.status.input_chord, vec!["south", "left_shoulder"]);
        runtime.input(b(1, true), true).unwrap();
        assert_eq!(runtime.status.input_serial, 2);
    }
    #[test]
    fn hot_reload_releases_old_keys_and_rejects_invalid_candidate_atomically() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        assert!(held.borrow().contains("key:SPACE"));
        let mut invalid = runtime.config.as_ref().unwrap().profile.clone();
        invalid.gyro.sensitivity = f32::NAN;
        assert!(runtime
            .apply("invalid".into(), invalid, "invalid.toml".into())
            .is_err());
        assert!(held.borrow().contains("key:SPACE"));
        assert_eq!(runtime.config.as_ref().unwrap().profile_id, "combat");
        let cursor = parse_profile(include_str!("../../../examples/cursor.toml")).unwrap();
        runtime
            .apply("cursor".into(), cursor, "cursor.toml".into())
            .unwrap();
        assert!(held.borrow().is_empty());
        assert_eq!(runtime.status.profile_id.as_deref(), Some("cursor"));
        assert_eq!(runtime.status.revision, 1);
    }
    #[test]
    fn disconnect_releases_outputs_and_keeps_exact_interface_for_reconnect() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        runtime.lost_device().unwrap();
        assert!(held.borrow().is_empty());
        assert_eq!(runtime.status.state, "waiting");
        assert_eq!(
            runtime.config.as_ref().unwrap().device_path,
            "physical#MI_02"
        );
        assert!(runtime.engine.is_none());
        assert!(!runtime.status.emitting);
    }
    #[test]
    fn bad_file_hot_reload_keeps_current_config_and_short_presses_can_be_learned() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        runtime
            .input(
                InputEvent::Button {
                    time: SampleTime {
                        host_ns: 2,
                        sensor_ns: 2,
                    },
                    index: 0,
                    down: false,
                },
                false,
            )
            .unwrap();
        assert!(!runtime.status.buttons[0]);
        assert_eq!(runtime.status.last_input.as_deref(), Some("south"));
        assert_eq!(runtime.status.input_serial, 1);
        runtime
            .control(Control::Reload {
                path: "combat.toml".into(),
                result: Box::new(Err("bad TOML".into())),
            })
            .unwrap();
        assert!(runtime
            .status
            .reload_error
            .as_ref()
            .unwrap()
            .contains("bad TOML"));
        assert_eq!(runtime.config.as_ref().unwrap().profile_id, "combat");
        assert!(held.borrow().is_empty());
    }
    #[test]
    fn parent_eof_and_output_fault_clear_runtime() {
        let (mut runtime, held, _rx) = fixture();
        runtime.input(down(), false).unwrap();
        assert!(!runtime.control(Control::Closed).unwrap());
        assert!(held.borrow().is_empty());
        assert!(runtime.config.is_none());
        runtime.fault("simulated output failure".into());
        assert_eq!(runtime.status.state, "faulted");
        assert!(!runtime.status.emitting);
    }

    #[test]
    fn switching_pad_type_neutralizes_and_drops_the_old_target() {
        struct RecordingPad(Rc<RefCell<Vec<Option<PadState>>>>);
        impl Sink for RecordingPad {
            fn button(&mut self, _: &str, _: bool) -> Result<()> {
                Ok(())
            }
            fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
                Ok(())
            }
            fn gamepad(&mut self, pad: PadState) -> Result<()> {
                self.0.borrow_mut().push(Some(pad));
                Ok(())
            }
        }
        impl Drop for RecordingPad {
            fn drop(&mut self) {
                self.0.borrow_mut().push(None);
            }
        }
        let (mut runtime, _, _rx) = fixture();
        let config = runtime.config.as_mut().unwrap();
        config.profile.virtual_gamepad = GamepadTarget::Xbox360;
        runtime.engine = Some(Engine::new(config.profile.clone()).unwrap());
        let mut next = config.profile.clone();
        next.virtual_gamepad = GamepadTarget::DualShock4;
        let reports = Rc::new(RefCell::new(Vec::new()));
        runtime.output = Some(Dispatcher::new(Box::new(RecordingPad(reports.clone()))));
        runtime.input(down(), false).unwrap();
        runtime
            .apply("ds4".into(), next, "ds4.toml".into())
            .unwrap();
        let reports = reports.borrow();
        assert_ne!(reports[0], Some(PadState::default()));
        assert_eq!(reports[1..], [Some(PadState::default()), None]);
        assert_eq!(
            runtime.engine.as_ref().unwrap().profile.virtual_gamepad,
            GamepadTarget::DualShock4
        );
    }

    #[test]
    fn keyboard_mouse_profile_keeps_neutral_pad_connected_until_reused() {
        struct RecordingPad {
            reports: Rc<RefCell<Vec<Option<PadState>>>>,
            feedback_sequence: u64,
        }
        impl Sink for RecordingPad {
            fn gamepad_status(&self) -> Option<GamepadStatus> {
                Some(mock_xbox_status())
            }
            fn feedback(&mut self) -> Option<motion_core::feedback::FeedbackFrame> {
                self.feedback_sequence += 1;
                Some(motion_core::feedback::FeedbackFrame {
                    sequence: self.feedback_sequence,
                    rumble: [0.5, 0.25],
                    lightbar: None,
                })
            }
            fn button(&mut self, _: &str, _: bool) -> Result<()> {
                Ok(())
            }
            fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
                Ok(())
            }
            fn gamepad(&mut self, pad: PadState) -> Result<()> {
                self.reports.borrow_mut().push(Some(pad));
                Ok(())
            }
        }
        impl Drop for RecordingPad {
            fn drop(&mut self) {
                self.reports.borrow_mut().push(None);
            }
        }

        let (mut runtime, _, rx) = fixture();
        let config = runtime.config.as_mut().unwrap();
        let keyboard_mouse = config.profile.clone();
        config.emit = true;
        config.profile.virtual_gamepad = GamepadTarget::Xbox360;
        let gamepad = config.profile.clone();
        runtime.engine = Some(Engine::new(gamepad.clone()).unwrap());
        let reports = Rc::new(RefCell::new(Vec::new()));
        runtime.output = Some(Dispatcher::new(Box::new(RecordingPad {
            reports: reports.clone(),
            feedback_sequence: 0,
        })));
        runtime.status.emitting = true;
        runtime.input(down(), false).unwrap();
        runtime.update_feedback();
        assert_eq!(runtime.feedback.status.rumble, [0.5, 0.25]);
        assert_ne!(reports.borrow()[0], Some(PadState::default()));

        runtime
            .apply("mouse".into(), keyboard_mouse, "mouse.toml".into())
            .unwrap();
        runtime.input(down(), false).unwrap();
        runtime.update_feedback();
        assert!(!runtime.feedback.status.active);
        assert_eq!(runtime.feedback.status.rumble, [0.; 2]);
        assert_eq!(reports.borrow()[1], Some(PadState::default()));
        assert!(reports.borrow()[1..]
            .iter()
            .all(|report| *report == Some(PadState::default())));
        runtime.publish_status();
        assert!(matches!(
            rx.try_iter().last(),
            Some(Event::Status { status }) if status.output_gamepad.is_none()
        ));

        runtime
            .apply("pad".into(), gamepad, "pad.toml".into())
            .unwrap();
        runtime.input(down(), false).unwrap();
        assert!(matches!(
            reports.borrow().last(),
            Some(Some(pad)) if *pad != PadState::default()
        ));
        assert!(reports.borrow().iter().all(Option::is_some));
        runtime.stop("F12").unwrap();
        assert!(reports
            .borrow()
            .ends_with(&[Some(PadState::default()), None]));
    }

    #[test]
    fn reconnect_requires_a_stable_path_instead_of_reusing_an_xinput_slot() {
        let (mut runtime, held, _rx) = fixture();
        runtime.config.as_mut().unwrap().device_path = "XInput#0".into();
        runtime.input(down(), false).unwrap();
        assert!(!held.borrow().is_empty());
        runtime.lost_device().unwrap();
        assert_eq!(runtime.status.state, "idle");
        assert!(runtime.config.is_none());
        assert!(held.borrow().is_empty());
    }
}
