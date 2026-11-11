#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod binding_window;
mod client;
mod components;
mod game_calibration;
mod hid_inventory;
mod import_files;
mod installers;
mod overlay;
mod policy;
mod profile_share;
mod runtime;
mod updates;

use client::Client;
use motion_core::{
    jsm_import::{self, JsmFile, JsmImport},
    profile::Profile,
    protocol::{Command, RunConfig},
    storage::{ProfileEntry, Settings, Store, SwitchTarget},
    switching::SwitchHistory,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};

#[derive(Clone)]
pub struct AppState {
    pub client: Arc<Client>,
    pub store: Store,
    pub settings: Arc<Mutex<Settings>>,
    pub history: Arc<Mutex<SwitchHistory>>,
    pub operations: Arc<Mutex<()>>,
    pub shortcut_capture_until: Arc<Mutex<Option<std::time::Instant>>>,
    pub shortcut_capture_revision: Arc<std::sync::atomic::AtomicU64>,
    pub startup_warnings: Vec<String>,
    pub overlay: overlay::Overlay,
    pub game_calibration: game_calibration::CalibrationWindow,
}
fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
async fn background<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(text)?
}
fn active(state: &AppState) -> bool {
    let snapshot = state.client.snapshot.lock().unwrap();
    if snapshot.status.ui_navigation {
        return false;
    }
    matches!(
        snapshot.status.state.as_str(),
        "running"
            | "observing"
            | "waiting"
            | "preparing_output"
            | "calibrating"
            | "capturing"
            | "capture_release"
    )
}
pub fn apply_profile(state: &AppState, id: &str) -> Result<(), String> {
    let entry = state.store.load(id).map_err(text)?;
    state.client.request(Command::Apply {
        profile_id: id.into(),
        profile: Box::new(entry.profile),
        profile_file: state.store.profile_path(id).map_err(text)?,
    })
}
pub fn notify_profile(state: &AppState, id: &str) {
    if let Ok(entry) = state.store.load(id) {
        let settings = state.settings.lock().unwrap().clone();
        if let Err(error) = state.overlay.show(&settings, &entry.profile.name) {
            state.client.log(error);
        }
    }
}
#[tauri::command]
async fn preview_overlay(state: State<'_, AppState>) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let settings = state.settings.lock().map_err(text)?.clone();
        let name = state
            .store
            .load(&settings.selected_profile)
            .map(|e| e.profile.name)
            .unwrap_or_else(|_| "体感瞄准 · 示例配置".into());
        state.overlay.show(&settings, &name)
    })
    .await
}
#[tauri::command]
async fn workspace(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let state = state.inner().clone();
    background(move || {
        let (profiles, mut warnings) = state.store.list().map_err(text)?;
        warnings.extend(state.startup_warnings.clone());
        Ok(serde_json::json!({
            "profiles": profiles, "settings": state.settings.lock().unwrap().clone(),
            "snapshot": state.client.snapshot.lock().unwrap().clone(), "warnings": warnings,
            "data_dir": state.store.root,
        }))
    })
    .await
}
#[tauri::command]
fn snapshot(state: State<'_, AppState>) -> client::Snapshot {
    state.client.snapshot.lock().unwrap().clone()
}
#[derive(Serialize)]
struct SaveResult {
    entry: ProfileEntry,
    applied: bool,
    warning: Option<String>,
}
#[tauri::command]
async fn save_profile(
    state: State<'_, AppState>,
    entry: ProfileEntry,
) -> Result<SaveResult, String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        save_profile_locked(&state, entry)
    })
    .await
}
fn save_profile_locked(state: &AppState, entry: ProfileEntry) -> Result<SaveResult, String> {
    {
        let snapshot = state.client.snapshot.lock().map_err(text)?;
        if snapshot.status.binding_capture {
            return Err("请先结束映射录入，再保存配置".into());
        }
        if snapshot.status.motion.measuring.is_some() {
            return Err("请先结束或取消转角测量，再保存配置".into());
        }
    }
    state.store.save(&entry).map_err(text)?;
    let should_apply = active(state)
        && state
            .client
            .snapshot
            .lock()
            .map_err(text)?
            .status
            .profile_id
            .as_deref()
            == Some(&entry.id);
    let warning = if should_apply {
        apply_profile(state, &entry.id).err()
    } else {
        None
    };
    Ok(SaveResult {
        applied: should_apply && warning.is_none(),
        entry,
        warning,
    })
}
#[tauri::command]
async fn create_profile(
    state: State<'_, AppState>,
    name: String,
    template: String,
) -> Result<ProfileEntry, String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        let source = match template.as_str() {
            "gyro" => include_str!("../../examples/gyro-mouse.toml"),
            "keyboard" => include_str!("../../examples/keyboard-mouse.toml"),
            "shooter" => include_str!("../../examples/shooter.toml"),
            "adventure" => include_str!("../../examples/adventure.toml"),
            "gamepad" => include_str!("../../examples/native-gamepad.toml"),
            "gyro-gamepad" => include_str!("../../examples/gyro-gamepad.toml"),
            "cursor" => include_str!("../../examples/cursor.toml"),
            "blank" => include_str!("../../examples/blank.toml"),
            _ => return Err("未知配置模板".into()),
        };
        let mut profile = motion_core::storage::parse_profile(source).map_err(text)?;
        if matches!(template.as_str(), "shooter" | "adventure")
            && state.settings.lock().map_err(text)?.language == motion_core::storage::Language::En
        {
            let labels: serde_json::Value =
                serde_json::from_str(include_str!("../../src/locales/en-game-recording.json"))
                    .map_err(text)?;
            for action in &mut profile.actions {
                if let Some(name) = labels[&action.name].as_str() {
                    action.name = name.into();
                }
            }
        }
        profile.name = name.trim().into();
        let entry = ProfileEntry {
            id: uuid::Uuid::new_v4().to_string(),
            profile,
        };
        state.store.save(&entry).map_err(text)?;
        Ok(entry)
    })
    .await
}
#[tauri::command]
async fn import_profile(
    state: State<'_, AppState>,
    content: String,
) -> Result<ProfileEntry, String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        let profile = motion_core::storage::parse_profile(&content).map_err(text)?;
        let entry = ProfileEntry {
            id: uuid::Uuid::new_v4().to_string(),
            profile,
        };
        state.store.save(&entry).map_err(text)?;
        Ok(entry)
    })
    .await
}
#[tauri::command]
async fn preview_jsm(files: Vec<JsmFile>) -> Result<JsmImport, String> {
    background(move || jsm_import::import_files(&files).map_err(text)).await
}
#[tauri::command]
async fn import_jsm(
    state: State<'_, AppState>,
    files: Vec<JsmFile>,
    name: String,
) -> Result<ProfileEntry, String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        let report = jsm_import::import_files(&files).map_err(text)?;
        if report.converted + report.approximate == 0 {
            return Err("没有可迁移的设置，请查看逐行报告".into());
        }
        let mut profile = report.profile;
        profile.name = name.trim().into();
        save_imported_jsm(&state, profile)
    })
    .await
}

fn save_imported_jsm(state: &AppState, profile: Profile) -> Result<ProfileEntry, String> {
    // Validate the complete program before touching a live mapping session.
    motion_core::engine::Engine::new(profile.clone()).map_err(text)?;
    let emitting = {
        let snapshot = state.client.snapshot.lock().map_err(text)?;
        if snapshot.status.motion.measuring.is_some() {
            return Err("请先结束转角测量".into());
        }
        if snapshot.status.binding_capture {
            return Err("请先结束映射录入，再保存配置".into());
        }
        snapshot.status.emitting
    };
    if emitting {
        state.client.request(Command::Stop)?;
    }
    let entry = ProfileEntry {
        id: uuid::Uuid::new_v4().to_string(),
        profile,
    };
    state.store.save(&entry).map_err(text)?;
    Ok(entry)
}
#[tauri::command]
async fn set_ui_navigation(
    state: State<'_, AppState>,
    active: bool,
    device_path: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        if active {
            state.client.request(Command::Navigate {
                device_path: device_path.clone(),
            })?;
            let mut settings = state.settings.lock().map_err(text)?;
            settings.device_path = device_path;
            state.store.save_settings(&settings).map_err(text)?;
        } else if state
            .client
            .snapshot
            .lock()
            .map_err(text)?
            .status
            .ui_navigation
        {
            state.client.request(Command::Stop)?;
        }
        Ok(())
    })
    .await
}
#[tauri::command]
async fn export_profile(
    state: State<'_, AppState>,
    id: String,
    path: PathBuf,
) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let entry = state.store.load(&id).map_err(text)?;
        if path.extension().and_then(|x| x.to_str()) != Some("toml") {
            return Err("请保存为 .toml 文件".into());
        }
        motion_core::storage::atomic_write(
            &path,
            toml::to_string_pretty(&entry.profile)
                .map_err(text)?
                .as_bytes(),
        )
        .map_err(text)
    })
    .await
}
#[tauri::command]
async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        if active(&state)
            && state
                .client
                .snapshot
                .lock()
                .unwrap()
                .status
                .profile_id
                .as_deref()
                == Some(&id)
        {
            return Err("请先停止或切换配置，再删除正在使用的配置".into());
        }
        state.store.delete(&id).map_err(text)?;
        let mut settings = state.settings.lock().map_err(text)?;
        if settings.cursor_shown == SwitchTarget::Profile(id.clone()) {
            settings.cursor_shown = SwitchTarget::None;
        }
        if settings.cursor_hidden == SwitchTarget::Profile(id.clone()) {
            settings.cursor_hidden = SwitchTarget::None;
        }
        if settings.shortcut_profile.as_deref() == Some(&id) {
            settings.shortcut_profile = None;
        }
        if settings.selected_profile == id {
            settings.selected_profile = state
                .store
                .list()
                .map_err(text)?
                .0
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_default();
        }
        state.store.save_settings(&settings).map_err(text)
    })
    .await
}
#[tauri::command]
async fn save_settings(
    state: State<'_, AppState>,
    mut settings: Settings,
) -> Result<Settings, String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        for target in [&settings.cursor_shown, &settings.cursor_hidden] {
            if let SwitchTarget::Profile(id) = target {
                state.store.load(id).map_err(text)?;
            }
        }
        if let Some(id) = &settings.shortcut_profile {
            state.store.load(id).map_err(text)?;
        }
        let mut current = state.settings.lock().map_err(text)?;
        if settings.mouse_display_sync != current.mouse_display_sync && active(&state) {
            return Err("请先停止映射或观察，再调整输出显示同步".into());
        }
        if (settings.output_backend != current.output_backend
            || settings.gamepad_backend != current.gamepad_backend
            || settings.auto_hide != current.auto_hide)
            && active(&state)
        {
            return Err("请先停止映射，再切换输出后端或自动隐藏设置".into());
        }
        let switching_changed = settings.cursor_enabled != current.cursor_enabled
            || settings.cursor_shown != current.cursor_shown
            || settings.cursor_hidden != current.cursor_hidden
            || settings.shortcut_profile != current.shortcut_profile;
        settings.calibrations = current.calibrations.clone();
        settings.motion_calibrations = current.motion_calibrations.clone();
        settings.selected_profile = current.selected_profile.clone();
        settings.device_path = current.device_path.clone();
        state.store.save_settings(&settings).map_err(text)?;
        *current = settings.clone();
        if !settings.overlay_enabled {
            state.overlay.dismiss();
        }
        if switching_changed {
            state.history.lock().unwrap().clear();
        }
        Ok(settings)
    })
    .await
}
#[tauri::command]
async fn start_mapping(
    state: State<'_, AppState>,
    id: String,
    device_path: String,
    emit: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        let entry = state.store.load(&id).map_err(text)?;
        let bias = state
            .settings
            .lock()
            .unwrap()
            .calibrations
            .get(&device_path)
            .copied()
            .unwrap_or([0.0; 3]);
        let sensor_calibration = state
            .settings
            .lock()
            .unwrap()
            .motion_calibrations
            .get(&device_path)
            .cloned()
            .unwrap_or_default();
        let runtime_settings = state.settings.lock().unwrap().clone();
        state.client.request(Command::Start {
            config: Box::new(RunConfig {
                profile_id: id.clone(),
                profile: entry.profile,
                profile_file: state.store.profile_path(&id).map_err(text)?,
                device_path: device_path.clone(),
                emit,
                auto_hide: runtime_settings.auto_hide,
                output_backend: runtime_settings.output_backend,
                gamepad_backend: runtime_settings.gamepad_backend,
                mouse_display_sync: runtime_settings.mouse_display_sync,
                bias,
                sensor_calibration,
            }),
        })?;
        let mut settings = state.settings.lock().unwrap();
        settings.selected_profile = id;
        settings.device_path = device_path;
        state.store.save_settings(&settings).map_err(text)?;
        state.history.lock().unwrap().clear();
        Ok(())
    })
    .await
}
#[tauri::command]
async fn activate_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        state.store.load(&id).map_err(text)?;
        if active(&state) {
            let changed = state
                .client
                .snapshot
                .lock()
                .unwrap()
                .status
                .profile_id
                .as_deref()
                != Some(id.as_str());
            apply_profile(&state, &id)?;
            if changed {
                notify_profile(&state, &id);
            }
        }
        let mut settings = state.settings.lock().unwrap();
        settings.selected_profile = id;
        state.store.save_settings(&settings).map_err(text)?;
        state.history.lock().unwrap().clear();
        Ok(())
    })
    .await
}
#[tauri::command]
async fn set_binding_capture(state: State<'_, AppState>, active: bool) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        state.client.request(Command::SetBindingCapture { active })
    })
    .await
}
#[tauri::command]
async fn engine_action(state: State<'_, AppState>, action: String) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        match action.as_str() {
            "stop" => state.client.request(Command::Stop),
            "scan" => state.client.request(Command::Scan),
            "calibrate" => state.client.request(Command::Calibrate),
            "recover" => state.client.restart(),
            _ => Err("未知引擎操作".into()),
        }
    })
    .await
}
#[tauri::command]
fn set_shortcut_capture(state: State<'_, AppState>, active: bool) -> Result<(), String> {
    update_shortcut_capture(&state, active)
}
fn update_shortcut_capture(state: &AppState, active: bool) -> Result<(), String> {
    let mut until = state.shortcut_capture_until.lock().map_err(text)?;
    let now = std::time::Instant::now();
    let was_active = until.is_some_and(|time| now < time);
    *until = active.then(|| now + std::time::Duration::from_secs(5));
    if active != was_active {
        state
            .shortcut_capture_revision
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }
    Ok(())
}
#[tauri::command]
async fn motion_action(
    state: State<'_, AppState>,
    action: String,
    axis: Option<usize>,
    positive: Option<bool>,
) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        let command = match action.as_str() {
            "reset_pose" => Command::ResetPose,
            "reset_filters" => Command::ResetFilters,
            "calibrate_face" => Command::CalibrateFace {
                axis: axis.ok_or("缺少校准轴")?,
                positive: positive.ok_or("缺少校准方向")?,
            },
            _ => return Err("未知体感操作".into()),
        };
        state.client.request(command)
    })
    .await
}
#[tauri::command]
async fn preview_motion_filter(
    filter: motion_core::custom_filters::FilterConfig,
    signal: motion_core::custom_filters::PreviewSignal,
) -> Result<motion_core::custom_filters::PreviewReport, String> {
    background(move || motion_core::custom_filters::preview(filter, signal).map_err(text)).await
}
#[tauri::command]
async fn preview_motion_stability(
    gyro: motion_core::profile::Gyro,
    sensor_noise: Option<f32>,
) -> Result<motion_core::motion::stability_preview::StabilityPreview, String> {
    background(move || {
        motion_core::motion::stability_preview::preview(&gyro, sensor_noise).map_err(text)
    })
    .await
}
#[tauri::command]
async fn preview_flick_release(strength: f32) -> Result<Vec<[f32; 3]>, String> {
    background(move || {
        motion_core::motion::stability_preview::flick_release(strength).map_err(text)
    })
    .await
}
#[tauri::command]
async fn save_sensor_calibration(
    state: State<'_, AppState>,
    device_path: String,
    calibration: motion_core::motion_config::SensorCalibration,
) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        calibration.validate().map_err(text)?;
        if device_path.trim().is_empty() {
            return Err("请先选择设备".into());
        }
        if state
            .client
            .snapshot
            .lock()
            .map_err(text)?
            .status
            .device
            .as_ref()
            .is_some_and(|d| d.path == device_path)
        {
            state.client.request(Command::SetSensorCalibration {
                calibration: Box::new(calibration),
            })
        } else {
            let mut settings = state.settings.lock().map_err(text)?;
            settings
                .motion_calibrations
                .insert(device_path, calibration);
            state.store.save_settings(&settings).map_err(text)
        }
    })
    .await
}
#[tauri::command]
async fn restore_backup(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        if id == "settings" {
            if active(&state) {
                return Err("请先停止映射或观察，再恢复本机设置备份".into());
            }
            let settings = state.store.restore_settings().map_err(text)?;
            if !settings.overlay_enabled {
                state.overlay.dismiss();
            }
            *state.settings.lock().unwrap() = settings;
        } else {
            state.store.restore_backup(&id).map_err(text)?;
        }
        Ok(())
    })
    .await
}
#[tauri::command]
async fn component_status(app: tauri::AppHandle) -> Result<components::ComponentsStatus, String> {
    background(move || components::get_status(&app)).await
}
#[tauri::command]
async fn component_action(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    device_id: String,
    xinput: bool,
) -> Result<components::ComponentsStatus, String> {
    let state = state.inner().clone();
    background(move || {
        let _lock = state.operations.lock().map_err(text)?;
        let status = state.client.snapshot.lock().map_err(text)?.status.clone();
        if status.visibility.requested
            || (action != "restore" && status.visibility.active)
            || (action == "restore" && active(&state))
        {
            return Err("请先停止映射，再手动修改或恢复隐藏规则".into());
        }
        if action == "restore" {
            state.client.request(Command::RecoverVisibility)?;
        }
        components::manage(&app, &action, &device_id, xinput)
    })
    .await
}
#[tauri::command]
async fn install_component(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<installers::InstallResult, String> {
    if kind == "hidmaestro" && active(state.inner()) {
        return Err("请先停止映射或观察，再安装 HIDMaestro 驱动".into());
    }
    background(move || installers::install(&app, &kind)).await
}
#[tauri::command]
async fn open_location(state: State<'_, AppState>, kind: String) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || match kind.as_str() {
        "profiles" => open::that(state.store.root.join("profiles")).map_err(text),
        "data" => open::that(state.store.root).map_err(text),
        "community" => {
            open::that("https://github.com/hotuns/MotionStudio/tree/main/community-configs")
                .map_err(text)
        }
        "releases" => open::that("https://github.com/hotuns/MotionStudio/releases").map_err(text),
        "hidhide" | "gamepads" => components::open_tool(&kind),
        _ => Err("未知位置".into()),
    })
    .await
}
#[tauri::command]
async fn export_diagnostics(state: State<'_, AppState>, path: PathBuf) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        if path.extension().and_then(|s|s.to_str()) != Some("json") { return Err("请保存为 JSON".into()); }
        let report = serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"snapshot":state.client.snapshot.lock().unwrap().clone()});
        motion_core::storage::atomic_write(&path, &serde_json::to_vec_pretty(&report).map_err(text)?).map_err(text)
    }).await
}
fn main() {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };
    let name: Vec<u16> = "Local\\MotionStudioUI"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let instance = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if instance.is_null() {
        return;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(instance);
        }
        return;
    }
    let portable_restart = updates::PortableRestart::default();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Updates::new(portable_restart.clone()))
        .setup(|app| {
            let handle = app.handle().clone();
            let root = runtime::runtime_dir(&handle).map_err(anyhow::Error::msg)?;
            let store = Store::open(root)?;
            let mut warnings = vec![];
            let settings = match store.settings() {
                Ok(settings) => settings,
                Err(error) => {
                    warnings.push(error.to_string());
                    Settings::default()
                }
            };
            let client = Client::new(handle);
            let state = AppState {
                client: client.clone(),
                store,
                settings: Arc::new(Mutex::new(settings)),
                overlay: overlay::Overlay::start(),
                game_calibration: game_calibration::CalibrationWindow::default(),
                history: Arc::new(Mutex::new(SwitchHistory::default())),
                operations: Arc::new(Mutex::new(())),
                shortcut_capture_until: Arc::new(Mutex::new(None)),
                shortcut_capture_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                startup_warnings: warnings,
            };
            app.manage(state.clone());
            app.manage(binding_window::BindingWindows::default());
            if let Err(error) = client.spawn() {
                client.log(error.clone());
                client.snapshot.lock().unwrap().status.message = error;
            }
            client.monitor();
            policy::start(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                for label in [binding_window::LABEL, game_calibration::LABEL] {
                    if let Some(floating) = window.get_webview_window(label) {
                        let _ = floating.destroy();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_binding_capture,
            binding_window::open_binding_window,
            binding_window::binding_window_state,
            binding_window::update_binding_window,
            binding_window::show_binding_window,
            binding_window::batch_capture,
            game_calibration::open_calibration_window,
            game_calibration::calibration_window_state,
            game_calibration::calibration_window_action,
            updates::check_studio_update,
            updates::download_studio_update,
            updates::install_studio_update,
            preview_overlay,
            workspace,
            snapshot,
            save_profile,
            create_profile,
            import_profile,
            profile_share::copy_profile_code,
            profile_share::copy_community_code,
            profile_share::read_profile_code,
            profile_share::preview_shared_profile,
            profile_share::import_shared_profile,
            preview_jsm,
            import_jsm,
            set_ui_navigation,
            import_files::browse_import_files,
            import_files::read_import_file,
            export_profile,
            delete_profile,
            save_settings,
            start_mapping,
            activate_profile,
            engine_action,
            set_shortcut_capture,
            motion_action,
            preview_motion_filter,
            preview_motion_stability,
            preview_flick_release,
            save_sensor_calibration,
            restore_backup,
            component_status,
            component_action,
            install_component,
            open_location,
            export_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("Motion Studio startup failed");
    let exit_code = app.run_return(|app, event| {
        if let tauri::RunEvent::Exit = event {
            app.state::<AppState>().overlay.shutdown();
            app.state::<AppState>().client.shutdown();
        }
    });
    unsafe {
        CloseHandle(instance);
    }
    // Wait until both the engine and single-instance guard are released.
    if let Some(executable) = portable_restart.lock().unwrap().take() {
        let folder = executable.parent().unwrap_or_else(|| Path::new("."));
        // Preserve relative data-directory overrides across the handoff.
        if std::process::Command::new(&executable).spawn().is_err() {
            let _ = open::that(folder);
        }
    }
    std::process::exit(exit_code);
}
