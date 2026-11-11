use crate::{
    background, binding_window::floating_position, client::Snapshot, save_profile_locked, text,
    AppState,
};
use motion_core::{
    motion::GameAngleMeasurement,
    motion_config::{MotionMode, Stick},
    profile::{Gyro, GyroOutput},
    protocol::Command,
    storage::Language,
};
use std::sync::{Arc, Mutex};
use tauri::{
    window::WindowBuilder, AppHandle, Emitter, Manager, State, Theme, WebviewWindow, WindowEvent,
};
use uuid::Uuid;

mod windows;

pub const LABEL: &str = "game-calibration";
const MAPPING_CHANGED: &str = "校准所用的映射已改变，请重新打开校准悬浮窗。";

#[derive(Clone, Default)]
pub struct CalibrationWindow {
    current: Arc<Mutex<Option<Session>>>,
    updates: Arc<windows::Updates>,
}

impl CalibrationWindow {
    pub fn is_open(&self) -> bool {
        self.current.lock().is_ok_and(|session| session.is_some())
    }

    fn session(&self) -> Result<Session, String> {
        self.current
            .lock()
            .map_err(text)?
            .clone()
            .ok_or_else(|| "校准悬浮窗已关闭。".into())
    }

    fn view(&self, snapshot: &Snapshot) -> Option<CalibrationView> {
        let current = self.current.lock().ok()?;
        Some(current.as_ref()?.view(snapshot))
    }

    pub fn refresh(&self) {
        self.updates.request();
    }
}

#[derive(Clone, Copy, Default)]
struct Hotkeys {
    measure: bool,
    apply: bool,
}

#[derive(Clone, Copy)]
enum Hotkey {
    Measure,
    Apply,
}

#[derive(Clone)]
struct Saved {
    applied: bool,
    warning: Option<String>,
}

#[derive(Clone)]
struct Session {
    id: Uuid,
    profile_id: String,
    profile_name: String,
    revision: u64,
    restart_count: u32,
    output: GyroOutput,
    stick: Stick,
    language: Language,
    owns_measurement: bool,
    result: Option<GameAngleMeasurement>,
    saved: Option<Saved>,
    hotkeys: Hotkeys,
    action_error: Option<String>,
}

impl Session {
    fn same_mapping(&self, snapshot: &Snapshot) -> bool {
        snapshot.restart_count == self.restart_count
            && snapshot.status.revision == self.revision
            && snapshot.status.profile_id.as_deref() == Some(self.profile_id.as_str())
    }

    fn ready(&self, snapshot: &Snapshot) -> Result<(), &'static str> {
        let status = &snapshot.status;
        if !self.same_mapping(snapshot)
            || !snapshot.engine_ready
            || !status.emitting
            || status.state != "running"
            || status.binding_capture
            || !status.device.as_ref().is_some_and(|device| device.gyro)
        {
            return Err(MAPPING_CHANGED);
        }
        if !status.active_layers.is_empty() {
            return Err("请松开配置层按键，切回基础层后再校准。");
        }
        Ok(())
    }

    fn view(&self, snapshot: &Snapshot) -> CalibrationView {
        let motion = &snapshot.status.motion;
        let blocked = self.ready(snapshot).err();
        let measuring = if self.owns_measurement && self.same_mapping(snapshot) {
            motion.measuring.clone()
        } else {
            None
        };
        let measurement_error = if measuring.is_some() {
            motion.measurement_error.clone()
        } else if self.owns_measurement && blocked.is_none() {
            Some("转角测量已中断，请重新开始。".into())
        } else {
            None
        };
        CalibrationView {
            id: self.id,
            blocked,
            profile_name: self.profile_name.clone(),
            output: self.output,
            stick: self.stick,
            language: self.language,
            measuring,
            degrees: motion.measured_degrees,
            measurement_error,
            result: self.result.clone(),
            saved: self.saved.clone(),
            hotkeys: self.hotkeys,
            action_error: self.action_error.clone(),
        }
    }

    fn hotkey_action(&self, snapshot: &Snapshot, key: Hotkey) -> Option<CalibrationAction> {
        let registered = match key {
            Hotkey::Measure => self.hotkeys.measure,
            Hotkey::Apply => self.hotkeys.apply,
        };
        if !registered {
            return None;
        }
        if self.saved.is_some() {
            return matches!(key, Hotkey::Apply).then_some(CalibrationAction::Close);
        }
        self.ready(snapshot).ok()?;
        match (key, snapshot.status.motion.measuring.is_some()) {
            (Hotkey::Measure, true) if self.owns_measurement => Some(CalibrationAction::Finish),
            (Hotkey::Measure, false) => Some(CalibrationAction::Start { axis: "yaw" }),
            (Hotkey::Apply, false) if self.result.is_some() => Some(CalibrationAction::Apply),
            _ => None,
        }
    }
}

struct CalibrationView {
    id: Uuid,
    profile_name: String,
    output: GyroOutput,
    stick: Stick,
    language: Language,
    blocked: Option<&'static str>,
    measuring: Option<String>,
    degrees: f32,
    measurement_error: Option<String>,
    result: Option<GameAngleMeasurement>,
    saved: Option<Saved>,
    hotkeys: Hotkeys,
    action_error: Option<String>,
}

#[tauri::command]
pub async fn open_calibration_window(
    window: WebviewWindow,
    state: State<'_, AppState>,
    profile_id: String,
    theme: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请在 Studio 打开校准悬浮窗。".into());
    }
    if let Some(floating) = window.get_window(LABEL) {
        if state.game_calibration.session()?.profile_id != profile_id {
            return Err("请先关闭另一份配置的校准悬浮窗。".into());
        }
        return floating.show().map_err(text);
    }

    let state = state.inner().clone();
    let owner = state.clone();
    let (id, language) = background(move || {
        let _lock = owner.operations.lock().map_err(text)?;
        if owner.game_calibration.is_open() {
            return Err("校准悬浮窗正在打开或关闭，请稍后再试。".into());
        }
        let entry = owner.store.load(&profile_id).map_err(text)?;
        let gyro = &entry.profile.gyro;
        if !gyro.enabled || gyro.motion.mode != MotionMode::Pointer {
            return Err("转角测量需要启用角速度瞄准模式，并选择水平或垂直轴".into());
        }
        let snapshot = owner.client.snapshot.lock().map_err(text)?;
        let language = owner.settings.lock().map_err(text)?.language;
        let session = Session {
            id: Uuid::new_v4(),
            profile_id,
            profile_name: entry.profile.name,
            revision: snapshot.status.revision,
            restart_count: snapshot.restart_count,
            output: gyro.output,
            stick: gyro.motion.flick.stick,
            language,
            owns_measurement: false,
            result: None,
            saved: None,
            hotkeys: Hotkeys::default(),
            action_error: None,
        };
        session.ready(&snapshot)?;
        if snapshot.status.motion.measuring.is_some() {
            return Err("请先结束或取消当前转角测量".into());
        }
        let id = session.id;
        *owner.game_calibration.current.lock().map_err(text)? = Some(session);
        Ok((id, language))
    })
    .await?;

    let title = if language == Language::En {
        "Motion Studio · Game angle calibration"
    } else {
        "Motion Studio · 游戏内转角校准"
    };
    let theme = if theme == "dark" {
        Theme::Dark
    } else {
        Theme::Light
    };
    let floating = match WindowBuilder::new(window.app_handle(), LABEL)
        .title(title)
        .theme(Some(theme))
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .minimizable(false)
        .maximizable(false)
        .inner_size(445.0, 398.0)
        .min_inner_size(420.0, 380.0)
        .visible(false)
        .build()
    {
        Ok(floating) => floating,
        Err(error) => {
            close_session(&state, id);
            return Err(text(error));
        }
    };
    let owner = state.clone();
    floating.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let owner = owner.clone();
            tauri::async_runtime::spawn_blocking(move || close_session(&owner, id));
        }
    });
    if let Some(position) = floating
        .outer_size()
        .ok()
        .and_then(|size| floating_position(&window, size))
    {
        let _ = floating.set_position(position);
    }
    if let Err(error) = windows::install(&floating, id, theme).await {
        let _ = floating.destroy();
        return Err(error);
    }
    if let Err(error) = floating.show() {
        let _ = floating.destroy();
        return Err(text(error));
    }
    Ok(())
}

fn close_session(state: &AppState, id: Uuid) {
    let Ok(_lock) = state.operations.lock() else {
        return;
    };
    let session = state
        .game_calibration
        .session()
        .ok()
        .filter(|session| session.id == id);
    let Some(session) = session else { return };
    if session.owns_measurement {
        let same = state
            .client
            .snapshot
            .lock()
            .is_ok_and(|snapshot| session.same_mapping(&snapshot));
        if same {
            if let Err(error) = state.client.finish_angle_measurement(session.revision) {
                state.client.log(error);
            }
        }
    }
    if let Ok(mut current) = state.game_calibration.current.lock() {
        *current = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalibrationAction {
    Start { axis: &'static str },
    Finish,
    Cancel,
    Apply,
    Close,
}

enum Input {
    Button(CalibrationAction),
    Hotkey(Hotkey),
}

fn run_input(app: &AppHandle, id: Uuid, input: Input) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let lock = match &input {
        Input::Button(_) => state.operations.lock().map_err(text)?,
        Input::Hotkey(_) => match state.operations.try_lock() {
            Ok(lock) => lock,
            Err(_) => return Ok(()),
        },
    };
    let Ok(session) = state.game_calibration.session() else {
        return Ok(());
    };
    if session.id != id {
        return Ok(());
    }
    let snapshot = state.client.snapshot.lock().map_err(text)?.clone();
    let action = match input {
        Input::Button(action) => Some(action),
        Input::Hotkey(key) => session.hotkey_action(&snapshot, key),
    };
    let Some(action) = action else {
        return Ok(());
    };
    if matches!(action, CalibrationAction::Close) {
        let window = app.get_window(LABEL);
        drop(lock);
        return window.map_or(Ok(()), |window| window.destroy().map_err(text));
    }
    perform_action(&state, app, session, &snapshot, action)
}

fn perform_action(
    state: &AppState,
    app: &AppHandle,
    mut session: Session,
    snapshot: &Snapshot,
    action: CalibrationAction,
) -> Result<(), String> {
    let result = (|| {
        match action {
            CalibrationAction::Start { axis } => {
                session.ready(snapshot)?;
                if snapshot.status.motion.measuring.is_some() || session.saved.is_some() {
                    return Err("请先结束或取消当前转角测量".into());
                }
                if let Err(error) = state.client.request(Command::BeginMeasure {
                    axis: axis.into(),
                    expected_revision: Some(session.revision),
                }) {
                    // An IPC timeout can arrive after the engine began the sweep.
                    session.owns_measurement = state
                        .client
                        .finish_angle_measurement(session.revision)
                        .is_err();
                    return Err(error);
                }
                session.owns_measurement = true;
                session.result = None;
            }
            CalibrationAction::Finish | CalibrationAction::Cancel => {
                let result = if session.owns_measurement && session.same_mapping(snapshot) {
                    state.client.finish_angle_measurement(session.revision)?
                } else {
                    None
                };
                session.owns_measurement = false;
                session.result = if matches!(action, CalibrationAction::Finish) {
                    result
                } else {
                    None
                };
                if matches!(action, CalibrationAction::Finish) && session.result.is_none() {
                    return Err("转角测量已中断，请重新开始。".into());
                }
            }
            CalibrationAction::Apply => {
                session.ready(snapshot)?;
                let measurement = session.result.as_ref().ok_or("请先完成转角测量。")?;
                let mut entry = state.store.load(&session.profile_id).map_err(text)?;
                apply_measurement(&mut entry.profile.gyro, measurement)?;
                let result = save_profile_locked(state, entry)?;
                session.saved = Some(Saved {
                    applied: result.applied,
                    warning: result.warning.clone(),
                });
                session.result = None;
                if let Err(error) = app.emit_to("main", "game-calibration-saved", &result) {
                    state.client.log(text(error));
                }
            }
            CalibrationAction::Close => unreachable!(),
        }
        Ok(())
    })();
    let mut guard = state.game_calibration.current.lock().map_err(text)?;
    if let Some(current) = guard.as_mut().filter(|current| current.id == session.id) {
        session.hotkeys = current.hotkeys;
        session.action_error = result.as_ref().err().cloned();
        *current = session;
    }
    result
}

fn measured_angles(measurement: &GameAngleMeasurement) -> Result<(f32, f32), &str> {
    if let Some(error) = &measurement.error {
        return Err(error);
    }
    let known = match measurement.axis.as_str() {
        "yaw" => 360.,
        "pitch" => 180.,
        _ => return Err("请先完成转角测量。"),
    };
    let degrees = measurement.degrees.abs();
    if !degrees.is_finite() || degrees < 1. || !measurement.peak.is_finite() {
        return Err("测得的手柄转角不足，请重新测量。");
    }
    if measurement.output == GyroOutput::RightStick && measurement.peak > 1. {
        return Err("测量过程中已超过满杆速度，请更慢地转动后重新测量。");
    }
    Ok((degrees, known))
}

fn apply_measurement(gyro: &mut Gyro, measurement: &GameAngleMeasurement) -> Result<(), String> {
    let (degrees, known) = measured_angles(measurement)?;
    if gyro.output != measurement.output {
        return Err(MAPPING_CHANGED.into());
    }
    let (scale, ratio) = match gyro.output {
        GyroOutput::Mouse => (&mut gyro.counts_per_degree, degrees / known),
        GyroOutput::RightStick => (&mut gyro.stick_full_scale_dps, known / degrees),
    };
    if *scale != measurement.scale {
        return Err(MAPPING_CHANGED.into());
    }
    *scale *= ratio;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_views_follow_engine_state_and_hotkeys_keep_save_separate_from_measurement() {
        use motion_core::sdl::{Device, InputOrigin};

        let mut session = Session {
            id: Uuid::new_v4(),
            profile_id: "game".into(),
            profile_name: "Test".into(),
            revision: 4,
            restart_count: 2,
            output: GyroOutput::Mouse,
            stick: Stick::Right,
            language: Language::En,
            owns_measurement: false,
            result: None,
            saved: None,
            hotkeys: Hotkeys {
                measure: true,
                apply: true,
            },
            action_error: None,
        };
        let mut snapshot = Snapshot {
            engine_ready: true,
            restart_count: 2,
            ..Snapshot::default()
        };
        let status = &mut snapshot.status;
        status.state = "running".into();
        status.emitting = true;
        status.profile_id = Some("game".into());
        status.revision = 4;
        status.device = Some(Device {
            id: 1,
            name: "Test".into(),
            path: "test://calibration".into(),
            vendor: 0,
            product: 0,
            origin: InputOrigin::Physical,
            gyro: true,
            gamepad: true,
            accelerometer: true,
            touchpads: 0,
            rumble: false,
            lightbar: false,
            buttons: vec![],
            open_error: None,
        });
        assert!(
            matches!(session.hotkey_action(&snapshot, Hotkey::Measure), Some(CalibrationAction::Start { axis }) if axis == "yaw")
        );
        assert!(session.hotkey_action(&snapshot, Hotkey::Apply).is_none());

        session.owns_measurement = true;
        snapshot.status.motion.measuring = Some("yaw".into());
        assert!(matches!(
            session.hotkey_action(&snapshot, Hotkey::Measure),
            Some(CalibrationAction::Finish)
        ));
        assert!(session.hotkey_action(&snapshot, Hotkey::Apply).is_none());

        let window = CalibrationWindow::default();
        *window.current.lock().unwrap() = Some(session.clone());
        let previous = window.view(&snapshot).unwrap();
        snapshot.status.motion.measured_degrees = 42.5;
        let latest = window.view(&snapshot).unwrap();
        assert_eq!(latest.degrees, 42.5);
        assert_eq!(previous.degrees, 0.);

        snapshot.status.motion.measuring = None;
        session.owns_measurement = false;
        session.result = Some(GameAngleMeasurement {
            axis: "yaw".into(),
            degrees: 90.,
            peak: 0.,
            error: None,
            output: GyroOutput::Mouse,
            scale: 12.,
        });
        assert!(matches!(
            session.hotkey_action(&snapshot, Hotkey::Measure),
            Some(CalibrationAction::Start { .. })
        ));
        assert!(matches!(
            session.hotkey_action(&snapshot, Hotkey::Apply),
            Some(CalibrationAction::Apply)
        ));
        snapshot.restart_count += 1;
        assert!(session.hotkey_action(&snapshot, Hotkey::Apply).is_none());
        session.saved = Some(Saved {
            applied: true,
            warning: None,
        });
        assert!(matches!(
            session.hotkey_action(&snapshot, Hotkey::Apply),
            Some(CalibrationAction::Close)
        ));
    }

    #[test]
    fn applying_a_measurement_updates_only_the_matching_output_scale() {
        let mut gyro = Gyro {
            counts_per_degree: 12.,
            stick_full_scale_dps: 720.,
            ..Default::default()
        };
        let mut measurement = GameAngleMeasurement {
            axis: "yaw".into(),
            degrees: -90.,
            peak: 0.,
            error: None,
            output: GyroOutput::Mouse,
            scale: 12.,
        };
        apply_measurement(&mut gyro, &measurement).unwrap();
        assert_eq!(gyro.counts_per_degree, 3.);
        assert_eq!(gyro.stick_full_scale_dps, 720.);
        assert!(apply_measurement(&mut gyro, &measurement).is_err());
        assert_eq!(gyro.counts_per_degree, 3.);

        gyro.output = GyroOutput::RightStick;
        measurement.output = gyro.output;
        measurement.scale = 720.;
        measurement.axis = "pitch".into();
        measurement.peak = 1.01;
        assert!(apply_measurement(&mut gyro, &measurement).is_err());
        measurement.peak = 0.95;
        apply_measurement(&mut gyro, &measurement).unwrap();
        assert_eq!(gyro.stick_full_scale_dps, 1440.);
        assert_eq!(gyro.counts_per_degree, 3.);
    }
}
