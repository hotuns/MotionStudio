use crate::{
    background, binding_window::floating_position, client::Snapshot, save_profile_locked, text,
    AppState,
};
use motion_core::{
    motion::GameAngleMeasurement,
    motion_config::{MotionMode, Stick},
    profile::{GamepadTarget, Gyro, GyroOutput},
    protocol::{CalibrationButton, Command},
    storage::Language,
};
use std::sync::{Arc, Mutex};
use tauri::{
    window::WindowBuilder, AppHandle, Emitter, Manager, State, Theme, WebviewWindow, WindowEvent,
};
use uuid::Uuid;

mod windows;
mod curve;
use curve::{CurveDraft, CurvePhase, CurveView};

pub const LABEL: &str = "game-calibration";
const MAPPING_CHANGED: &str = "校准所用的映射已改变，请重新打开校准悬浮窗。";
const IMU_CALIBRATION_UNAVAILABLE: &str = "传感器直通由游戏处理，无需校准游戏转角。";
const PAD_BUTTONS: [CalibrationButton; 6] = [
    CalibrationButton::Measure, CalibrationButton::Apply, CalibrationButton::Less,
    CalibrationButton::More, CalibrationButton::Mode, CalibrationButton::Back,
];

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
        let mut current = self.current.lock().ok()?;
        let session = current.as_mut()?;
        session.observe_curve(snapshot);
        Some(session.view(snapshot))
    }

    fn session_for(&self, snapshot: &Snapshot) -> Result<Session, String> {
        let mut current = self.current.lock().map_err(text)?;
        let session = current.as_mut().ok_or("校准悬浮窗已关闭。")?;
        session.observe_curve(snapshot);
        Ok(session.clone())
    }

    pub fn refresh(&self) {
        self.updates.request();
    }

    pub fn pad_input(&self, session_id: Uuid, button: CalibrationButton) {
        self.updates.pad_input(session_id, button);
    }
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
    reconnects: u64,
    output: GyroOutput,
    stick: Stick,
    language: Language,
    owns_measurement: bool,
    owns_trial: bool,
    manual: bool,
    manual_available: bool,
    original_scale: f32,
    draft_scale: f32,
    curve: Option<CurveDraft>,
    result: Option<GameAngleMeasurement>,
    saved: Option<Saved>,
    action_error: Option<String>,
}

impl Session {
    fn observe_curve(&mut self, snapshot: &Snapshot) {
        if self.same_mapping(snapshot) {
            if let Some(curve) = &mut self.curve {
                curve.observe(self.id, snapshot.status.curve_probe.as_ref());
            }
        }
    }
    fn same_mapping(&self, snapshot: &Snapshot) -> bool {
        self.same_controller_session(snapshot) && snapshot.status.revision == self.revision
    }

    fn same_controller_session(&self, snapshot: &Snapshot) -> bool {
        snapshot.restart_count == self.restart_count && snapshot.status.reconnects == self.reconnects
            && snapshot.status.profile_id.as_deref() == Some(self.profile_id.as_str())
    }

    fn reserve_buttons(&self, state: &AppState, revision: u64, active: bool) -> Result<(), String> {
        state.client.request(Command::SetCalibrationControls {
            session_id: self.id, expected_revision: revision, active,
        })
    }

    fn ready(&self, snapshot: &Snapshot) -> Result<(), &'static str> {
        let status = &snapshot.status;
        if !self.same_mapping(snapshot)
            || !snapshot.engine_ready
            || !status.emitting
            || status.state != "running"
            || status.binding_capture
            || status.device.is_none()
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
        } else if self.owns_measurement && !self.owns_trial && blocked.is_none() {
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
            pad_actions: PAD_BUTTONS.map(|button| self.pad_action(snapshot, button)),
            action_error: self.action_error.clone(),
            manual: self.manual,
            manual_available: self.manual_available,
            draft_scale: self.draft_scale,
            curve: self.curve.as_ref().map(CurveDraft::view),
        }
    }

    fn pad_action(&self, snapshot: &Snapshot, button: CalibrationButton) -> Option<CalibrationAction> {
        use CalibrationAction as Action;
        use CalibrationButton as Pad;
        let status = &snapshot.status;
        if !self.same_mapping(snapshot) || !snapshot.engine_ready || !status.emitting
            || status.state != "running" || status.device.is_none() {
            return None;
        }
        if self.saved.is_some() {
            return matches!(button, Pad::Apply | Pad::Back).then_some(Action::Close);
        }
        if self.ready(snapshot).is_err() {
            return matches!(button, Pad::Back).then_some(Action::Close);
        }
        if status.motion.measuring.is_some() {
            return match button {
                Pad::Measure if self.owns_trial => Some(Action::Cancel),
                Pad::Measure if self.owns_measurement => Some(Action::Finish),
                Pad::Back if self.owns_measurement => Some(Action::Cancel),
                _ => None,
            };
        }
        if let Some(curve) = &self.curve {
            return curve.pad_action(button);
        }
        match button {
            Pad::Measure if self.manual => Some(Action::Start { axis: "yaw" }),
            Pad::Measure => Some(Action::Trial),
            Pad::Back => Some(if self.manual { Action::Back } else { Action::Close }),
            Pad::Mode if self.manual => Some(Action::Start { axis: "pitch" }),
            Pad::Mode if self.manual_available => Some(Action::Manual),
            Pad::Apply if !self.manual => Some(Action::Apply),
            Pad::Less if !self.manual => Some(Action::Less),
            Pad::More if !self.manual => Some(Action::More),
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
    pad_actions: [Option<CalibrationAction>; 6],
    action_error: Option<String>,
    manual: bool,
    manual_available: bool,
    draft_scale: f32,
    curve: Option<CurveView>,
}

#[tauri::command]
pub async fn open_calibration_window(
    window: WebviewWindow,
    state: State<'_, AppState>,
    profile_id: String,
    theme: String,
    curve_mode: Option<bool>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请在 Studio 打开校准悬浮窗。".into());
    }
    if let Some(floating) = window.get_window(LABEL) {
        let session = state.game_calibration.session()?;
        if session.profile_id != profile_id {
            return Err("请先关闭另一份配置的校准悬浮窗。".into());
        }
        if curve_mode.is_some_and(|curve| curve != session.curve.is_some()) {
            return Err("请先关闭当前校准悬浮窗，再切换测量方式。".into());
        }
        return floating.show().map_err(text);
    }

    let state = state.inner().clone();
    let owner = state.clone();
    let curve_mode = curve_mode.unwrap_or(false);
    let (id, language) = background(move || {
        let _lock = owner.operations.lock().map_err(text)?;
        if owner.game_calibration.is_open() {
            return Err("校准悬浮窗正在打开或关闭，请稍后再试。".into());
        }
        let entry = owner.store.load(&profile_id).map_err(text)?;
        let gyro = &entry.profile.gyro;
        let scale = if curve_mode { gyro.stick_full_scale_dps } else { scale(gyro)? };
        if !curve_mode && !gyro.enabled
            && !(gyro.motion.flick.standalone
                && (gyro.motion.flick.enabled || gyro.motion.flick.stick_aim))
        {
            return Err("请先启用体感或 Flick，再打开转角校准。".into());
        }
        let snapshot = owner.client.snapshot.lock().map_err(text)?;
        if curve_mode && (entry.profile.virtual_gamepad == GamepadTarget::None ||
            !snapshot.status.output_gamepad.as_ref().is_some_and(|pad| pad.connected && pad.target == entry.profile.virtual_gamepad)) {
            return Err("请先启用虚拟手柄输出并启动映射，再测量游戏曲线。".into());
        }
        let language = owner.settings.lock().map_err(text)?.language;
        let session = Session {
            id: Uuid::new_v4(),
            profile_id,
            profile_name: entry.profile.name,
            revision: snapshot.status.revision,
            restart_count: snapshot.restart_count,
            reconnects: snapshot.status.reconnects,
            output: if curve_mode { GyroOutput::RightStick } else { gyro.output },
            stick: gyro.motion.flick.stick,
            language,
            owns_measurement: false,
            owns_trial: false,
            manual: false,
            manual_available: gyro.enabled
                && gyro.motion.mode == MotionMode::Pointer
                && snapshot.status.device.as_ref().is_some_and(|d| d.gyro),
            original_scale: scale,
            draft_scale: scale,
            curve: curve_mode.then(|| CurveDraft::new(gyro.right_stick_curve.clone())),
            result: None,
            saved: None,
            action_error: None,
        };
        session.ready(&snapshot)?;
        if snapshot.status.motion.measuring.is_some() {
            return Err("请先结束或取消当前转角测量".into());
        }
        let id = session.id;
        let revision = session.revision;
        drop(snapshot);
        if let Err(error) = session.reserve_buttons(&owner, revision, true) {
            // A timed-out request may already have reserved the buttons.
            let _ = session.reserve_buttons(&owner, revision, false);
            return Err(error);
        }
        *owner.game_calibration.current.lock().map_err(text)? = Some(session);
        Ok((id, language))
    })
    .await?;

    let title = match (language, curve_mode) {
        (Language::En, true) => "Motion Studio · Game stick curve",
        (_, true) => "Motion Studio · 游戏摇杆曲线测量",
        (Language::En, false) => "Motion Studio · Game angle calibration",
        (_, false) => "Motion Studio · 游戏内转角校准",
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
        .focusable(true)
        .minimizable(false)
        .maximizable(false)
        .inner_size(490.0, 540.0)
        .min_inner_size(470.0, 520.0)
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
    if let Err(error) = session.reserve_buttons(state, session.revision, false) {
        state.client.log(error);
    }
    if let Ok(mut current) = state.game_calibration.current.lock() {
        *current = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalibrationAction {
    CurveStart,
    CurveMark,
    CurveCancel,
    CurveZero,
    CurveLaps,
    CurvePrev,
    CurveNext,
    Trial,
    Less,
    More,
    Manual,
    Back,
    Start { axis: &'static str },
    Finish,
    Cancel,
    Apply,
    Close,
}

enum Input {
    Button(CalibrationAction),
    Pad(CalibrationButton, CalibrationAction),
    Edited(String, Box<Input>),
}

fn run_input(app: &AppHandle, id: Uuid, input: Input) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let (value, input) = match input {
        Input::Edited(value, input) => (Some(value), *input),
        input => (None, input),
    };
    let lock = state.operations.lock().map_err(text)?;
    let snapshot = state.client.snapshot.lock().map_err(text)?.clone();
    let Ok(session) = state.game_calibration.session_for(&snapshot) else {
        return Ok(());
    };
    if session.id != id {
        return Ok(());
    }
    let action = match input {
        Input::Button(action) => action,
        Input::Pad(button, action) if session.pad_action(&snapshot, button) == Some(action) => action,
        Input::Pad(..) => return Ok(()),
        Input::Edited(..) => unreachable!(),
    };
    if matches!(action, CalibrationAction::Close) {
        let window = app.get_window(LABEL);
        drop(lock);
        return window.map_or(Ok(()), |window| window.destroy().map_err(text));
    }
    perform_action(&state, app, session, &snapshot, action, value)
}

fn perform_action(
    state: &AppState,
    app: &AppHandle,
    mut session: Session,
    snapshot: &Snapshot,
    action: CalibrationAction,
    value: Option<String>,
) -> Result<(), String> {
    let result = (|| {
        if session.curve.is_none() && matches!(
            action,
            CalibrationAction::Trial
                | CalibrationAction::Apply
                | CalibrationAction::Less
                | CalibrationAction::More
                | CalibrationAction::Manual
        ) {
            session.ready(snapshot)?;
            if snapshot.status.motion.measuring.is_some() || session.saved.is_some() {
                return Err("请先结束或取消当前转角测量".into());
            }
            if let Some(value) = value {
                let parsed = value
                    .trim()
                    .parse::<f32>()
                    .map_err(|_| "请输入有效的校准数值。")?;
                validate_scale(session.output, parsed)?;
                session.draft_scale = parsed;
            }
        }
        match action {
            CalibrationAction::CurveStart | CalibrationAction::CurveMark | CalibrationAction::CurveCancel
            | CalibrationAction::CurveZero | CalibrationAction::CurveLaps
            | CalibrationAction::CurvePrev | CalibrationAction::CurveNext => {
                session.ready(snapshot)?;
                if session.saved.is_some() { return Err(MAPPING_CHANGED.into()); }
                let curve = session.curve.as_mut().ok_or(MAPPING_CHANGED)?;
                curve.perform(action, &state.client, session.id, session.revision)?;
            }
            CalibrationAction::Trial => {
                if let Err(error) = state.client.request(Command::TrialTurn {
                    scale: session.draft_scale,
                    expected_revision: session.revision,
                }) {
                    session.owns_measurement = state
                        .client
                        .finish_angle_measurement(session.revision)
                        .is_err();
                    session.owns_trial = session.owns_measurement;
                    return Err(error);
                }
                session.owns_measurement = true;
                session.owns_trial = true;
                session.result = None;
            }
            CalibrationAction::Less | CalibrationAction::More => {
                let increase =
                    (action == CalibrationAction::Less) == (session.output == GyroOutput::Mouse);
                let next = session.draft_scale * if increase { 1.01 } else { 1. / 1.01 };
                validate_scale(session.output, next)?;
                session.draft_scale = next;
            }
            CalibrationAction::Manual => {
                if !session.manual_available {
                    return Err("手动测量需要启用角速度瞄准，并连接支持体感的手柄。".into());
                }
                session.manual = true;
                session.owns_measurement = false;
                session.owns_trial = false;
            }
            CalibrationAction::Back => {
                session.manual = false;
                session.result = None;
            }
            CalibrationAction::Start { axis } => {
                session.ready(snapshot)?;
                if snapshot.status.motion.measuring.is_some() || session.saved.is_some() {
                    return Err("请先结束或取消当前转角测量".into());
                }
                if let Err(error) = state.client.request(Command::BeginMeasure {
                    axis: axis.into(),
                    scale: Some(session.draft_scale),
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
                session.owns_trial = false;
                session.result = None;
            }
            CalibrationAction::Finish | CalibrationAction::Cancel => {
                let result = if session.owns_measurement && session.same_mapping(snapshot) {
                    state.client.finish_angle_measurement(session.revision)?
                } else {
                    None
                };
                session.owns_measurement = false;
                session.owns_trial = false;
                session.result = if matches!(action, CalibrationAction::Finish) {
                    result
                } else {
                    None
                };
                if matches!(action, CalibrationAction::Finish) && session.result.is_none() {
                    return Err("转角测量已中断，请重新开始。".into());
                }
                if let Some(measurement) = &session.result {
                    let mut gyro = state
                        .store
                        .load(&session.profile_id)
                        .map_err(text)?
                        .profile
                        .gyro;
                    match gyro.output {
                        GyroOutput::Mouse => gyro.counts_per_degree = measurement.scale,
                        GyroOutput::RightStick => gyro.stick_full_scale_dps = measurement.scale,
                        GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => return Err(IMU_CALIBRATION_UNAVAILABLE.into()),
                    }
                    apply_measurement(&mut gyro, measurement)?;
                    let measured_scale = scale(&gyro)?;
                    validate_scale(gyro.output, measured_scale)?;
                    session.draft_scale = measured_scale;
                    session.manual = false;
                    session.result = None;
                }
            }
            CalibrationAction::Apply => {
                session.ready(snapshot)?;
                let mut entry = state.store.load(&session.profile_id).map_err(text)?;
                if let Some(curve) = &session.curve {
                    if session.saved.is_some() || entry.profile.gyro.right_stick_curve.as_ref() != curve.original.as_deref() {
                        return Err(MAPPING_CHANGED.into());
                    }
                    entry.profile.gyro.right_stick_curve = Some(curve.fitted()?);
                } else {
                    if entry.profile.gyro.output != session.output
                        || scale(&entry.profile.gyro)? != session.original_scale
                    {
                        return Err(MAPPING_CHANGED.into());
                    }
                    validate_scale(session.output, session.draft_scale)?;
                    match session.output {
                        GyroOutput::Mouse => entry.profile.gyro.counts_per_degree = session.draft_scale,
                        GyroOutput::RightStick => entry.profile.gyro.stick_full_scale_dps = session.draft_scale,
                        GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => return Err(IMU_CALIBRATION_UNAVAILABLE.into()),
                    }
                }
                let result = save_profile_locked(state, entry)?;
                session.saved = Some(Saved {
                    applied: result.applied,
                    warning: result.warning.clone(),
                });
                session.owns_measurement = false;
                session.owns_trial = false;
                session.result = None;
                if let Err(error) = app.emit_to("main", "game-calibration-saved", &result) {
                    state.client.log(text(error));
                }
                // A changed mapping releases its reserved buttons. Reattach only for
                // this overlay's own successful save, so the player can close it in-game.
                if result.applied {
                    let latest = state.client.snapshot.lock().map_err(text)?;
                    let next = latest.status.revision;
                    let reattach = session.same_controller_session(&latest) && next != session.revision;
                    drop(latest);
                    if reattach {
                        session.reserve_buttons(state, next, true)?;
                        session.revision = next;
                    }
                }
            }
            CalibrationAction::Close => unreachable!(),
        }
        Ok(())
    })();
    let mut guard = state.game_calibration.current.lock().map_err(text)?;
    if let Some(current) = guard.as_mut().filter(|current| current.id == session.id) {
        session.action_error = result.as_ref().err().cloned();
        *current = session;
    }
    result
}

fn scale(gyro: &Gyro) -> Result<f32, &'static str> {
    match gyro.output {
        GyroOutput::Mouse => Ok(gyro.counts_per_degree),
        GyroOutput::RightStick => Ok(gyro.stick_full_scale_dps),
        GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => Err(IMU_CALIBRATION_UNAVAILABLE),
    }
}

fn validate_scale(output: GyroOutput, value: f32) -> Result<(), String> {
    match output {
        GyroOutput::Mouse if value.is_finite() && value > 0. && value <= 10000. => Ok(()),
        GyroOutput::RightStick if (30.0..=1440.0).contains(&value) => Ok(()),
        GyroOutput::Mouse => Err("鼠标转角系数需大于 0 且不超过 10000。".into()),
        GyroOutput::RightStick => Err("满杆角速度需为 30–1440 °/s".into()),
        GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => Err(IMU_CALIBRATION_UNAVAILABLE.into()),
    }
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
        GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => return Err(IMU_CALIBRATION_UNAVAILABLE.into()),
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
    fn live_views_follow_engine_state_and_pad_controls_keep_save_separate_from_measurement() {
        use motion_core::sdl::{Device, InputOrigin};

        let mut session = Session {
            id: Uuid::new_v4(),
            profile_id: "game".into(),
            profile_name: "Test".into(),
            revision: 4,
            restart_count: 2,
            reconnects: 0,
            output: GyroOutput::Mouse,
            stick: Stick::Right,
            language: Language::En,
            owns_measurement: false,
            owns_trial: false,
            manual: true,
            manual_available: true,
            original_scale: 12.,
            draft_scale: 12.,
            curve: None,
            result: None,
            saved: None,
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
            gameinput_container: None,
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
            matches!(session.pad_action(&snapshot, CalibrationButton::Measure), Some(CalibrationAction::Start { axis }) if axis == "yaw")
        );
        assert!(session.pad_action(&snapshot, CalibrationButton::Apply).is_none());

        session.owns_measurement = true;
        snapshot.status.motion.measuring = Some("yaw".into());
        assert!(matches!(
            session.pad_action(&snapshot, CalibrationButton::Measure),
            Some(CalibrationAction::Finish)
        ));
        assert!(session.pad_action(&snapshot, CalibrationButton::Apply).is_none());

        let window = CalibrationWindow::default();
        *window.current.lock().unwrap() = Some(session.clone());
        let previous = window.view(&snapshot).unwrap();
        snapshot.status.motion.measured_degrees = 42.5;
        let latest = window.view(&snapshot).unwrap();
        assert_eq!(latest.degrees, 42.5);
        assert_eq!(previous.degrees, 0.);

        snapshot.status.motion.measuring = None;
        session.owns_measurement = false;
        session.manual = false;
        session.result = Some(GameAngleMeasurement {
            axis: "yaw".into(),
            degrees: 90.,
            peak: 0.,
            error: None,
            output: GyroOutput::Mouse,
            scale: 12.,
        });
        assert!(matches!(
            session.pad_action(&snapshot, CalibrationButton::Measure),
            Some(CalibrationAction::Trial)
        ));
        assert!(matches!(
            session.pad_action(&snapshot, CalibrationButton::Apply),
            Some(CalibrationAction::Apply)
        ));
        snapshot.restart_count += 1;
        assert!(session.pad_action(&snapshot, CalibrationButton::Apply).is_none());
        snapshot.restart_count -= 1;
        snapshot.status.revision += 1;
        session.revision = snapshot.status.revision;
        session.saved = Some(Saved {
            applied: true,
            warning: None,
        });
        assert!(matches!(
            session.pad_action(&snapshot, CalibrationButton::Apply),
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
