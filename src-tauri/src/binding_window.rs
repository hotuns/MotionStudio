use crate::{background, text, update_shortcut_capture, AppState};
use motion_core::{
    bindings::{input_chord, output_chord},
    profile::Action,
    protocol::Command,
    storage::Language,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

pub const LABEL: &str = "binding-window";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchScope {
    profile_id: String,
    profile_name: String,
    layer_id: String,
    layer_name: Option<String>,
    actions: Vec<Action>,
    max_actions: usize,
}

#[derive(Clone, Deserialize, Serialize)]
struct RecordedAction {
    name: String,
    output: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Progress {
    cursor: usize,
    values: Vec<Option<String>>,
    #[serde(default)]
    added: Vec<RecordedAction>,
}

impl Progress {
    fn validate(&self, existing: usize, limit: usize) -> Result<(), String> {
        let count = existing + self.added.len();
        if limit > 128
            || count > limit
            || self.cursor >= count.max(1)
            || self.values.len() != count
            || self
                .values
                .iter()
                .flatten()
                .any(|value| output_chord(value).is_none())
            || self.added.iter().any(|action| {
                action.name.chars().count() > 100 || output_chord(&action.output).is_none()
            })
        {
            return Err("批量填键数据无效，请重新打开动作列表。".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Return,
    Keep,
}

#[derive(Clone, Serialize)]
pub struct WindowView {
    id: String,
    scope: BatchScope,
    progress: Progress,
    split: bool,
    language: Language,
    theme: String,
    outcome: Option<Outcome>,
}

struct Session {
    view: WindowView,
    closing: Option<Outcome>,
}

#[derive(Clone)]
struct Lease {
    id: String,
    window: String,
    restart: u32,
    expires: Instant,
}

#[derive(Clone, Default)]
pub struct BindingWindows {
    session: Arc<Mutex<Option<Session>>>,
    lease: Arc<Mutex<Option<Lease>>>,
}

#[tauri::command]
pub async fn open_binding_window(
    window: WebviewWindow,
    state: State<'_, AppState>,
    windows: State<'_, BindingWindows>,
    scope: BatchScope,
    progress: Progress,
    split: bool,
    theme: String,
) -> Result<String, String> {
    if window.label() != "main" {
        return Err("请在 Studio 打开录入悬浮窗。".into());
    }
    progress.validate(scope.actions.len(), scope.max_actions)?;
    if scope
        .actions
        .iter()
        .any(|action| !action.input.is_empty() && input_chord(&action.input).is_none())
    {
        return Err("批量填键数据无效，请重新打开动作列表。".into());
    }
    let app = window.app_handle().clone();
    let language = state.settings.lock().map_err(text)?.language;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut session = windows.session.lock().map_err(text)?;
        if session
            .as_ref()
            .is_some_and(|session| session.view.outcome.is_none())
            || app.get_webview_window(LABEL).is_some()
        {
            return Err("录入悬浮窗已打开。".into());
        }
        *session = Some(Session {
            closing: None,
            view: WindowView {
                id: id.clone(),
                scope,
                progress,
                split,
                language,
                theme: if theme == "dark" {
                    theme
                } else {
                    "light".into()
                },
                outcome: None,
            },
        });
    }

    let title = if language == Language::En {
        "Motion Studio · Game bindings"
    } else {
        "Motion Studio · 动作录入"
    };
    let floating =
        match WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("index.html".into()))
            .title(title)
            .initialization_script("window.__MOTION_BINDING_WINDOW__ = true;")
            .always_on_top(true)
            .skip_taskbar(true)
            .minimizable(false)
            .maximizable(false)
            .inner_size(525.0, 580.0)
            .min_inner_size(395.0, 360.0)
            .visible(false)
            .build()
        {
            Ok(floating) => floating,
            Err(error) => {
                *windows.session.lock().map_err(text)? = None;
                return Err(text(error));
            }
        };
    let view_id = id.clone();
    let app_state = state.inner().clone();
    let bindings = windows.inner().clone();
    floating.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => release_on_blur(&app_state, &bindings),
        WindowEvent::Destroyed => {
            release_on_blur(&app_state, &bindings);
            if let Ok(mut current) = bindings.session.lock() {
                if let Some(session) = current
                    .as_mut()
                    .filter(|session| session.view.id == view_id)
                {
                    session.view.outcome = Some(session.closing.unwrap_or(Outcome::Return));
                }
            }
        }
        _ => {}
    });
    if let Some(position) = floating
        .outer_size()
        .ok()
        .and_then(|size| floating_position(&window, size))
    {
        let _ = floating.set_position(position);
    }
    if let Err(error) = floating.show() {
        let _ = floating.destroy();
        return Err(text(error));
    }
    let _ = floating.set_focus();
    Ok(id)
}

pub(crate) fn floating_position(
    window: &WebviewWindow,
    size: PhysicalSize<u32>,
) -> Option<PhysicalPosition<i32>> {
    let monitor = window.current_monitor().ok()??;
    let work = monitor.work_area();
    let margin = (16.0 * monitor.scale_factor()).round() as i32;
    let x = work.position.x + (work.size.width as i32 - size.width as i32 - margin).max(0);
    let y = work.position.y + margin;
    Some(PhysicalPosition::new(x, y))
}

#[tauri::command]
pub fn binding_window_state(
    window: WebviewWindow,
    windows: State<'_, BindingWindows>,
    id: Option<String>,
) -> Result<WindowView, String> {
    let current = windows.session.lock().map_err(text)?;
    let session = current.as_ref().filter(|session| {
        window.label() == LABEL && session.view.outcome.is_none()
            || window.label() == "main" && id.as_deref() == Some(session.view.id.as_str())
    });
    session
        .map(|session| session.view.clone())
        .ok_or_else(|| "录入悬浮窗已关闭。".into())
}

#[tauri::command]
pub async fn update_binding_window(
    window: WebviewWindow,
    windows: State<'_, BindingWindows>,
    id: String,
    progress: Option<Progress>,
    finish: Option<Outcome>,
) -> Result<(), String> {
    let from_floating = window.label() == LABEL;
    if !(from_floating
        || (window.label() == "main" && progress.is_none() && finish == Some(Outcome::Return)))
    {
        return Err("请在录入悬浮窗中操作。".into());
    }
    {
        let mut current = windows.session.lock().map_err(text)?;
        let session = current
            .as_mut()
            .filter(|session| {
                session.view.id == id && session.view.outcome.is_none() && session.closing.is_none()
            })
            .ok_or("录入悬浮窗已关闭。")?;
        if let Some(progress) = progress {
            progress.validate(
                session.view.scope.actions.len(),
                session.view.scope.max_actions,
            )?;
            session.view.progress = progress;
        }
        session.closing = finish;
    }
    if finish.is_some() {
        if let Some(floating) = window.get_webview_window(LABEL) {
            if let Err(error) = floating.destroy() {
                if let Some(session) = windows
                    .session
                    .lock()
                    .map_err(text)?
                    .as_mut()
                    .filter(|session| session.view.id == id)
                {
                    session.closing = None;
                }
                return Err(text(error));
            }
        } else if let Some(session) = windows
            .session
            .lock()
            .map_err(text)?
            .as_mut()
            .filter(|session| session.view.id == id)
        {
            session.view.outcome = finish;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn show_binding_window(
    window: WebviewWindow,
    windows: State<'_, BindingWindows>,
    id: String,
) -> Result<(), String> {
    binding_window_state(window.clone(), windows, Some(id))?;
    let floating = window
        .get_webview_window(LABEL)
        .ok_or("录入悬浮窗已关闭。")?;
    floating.show().map_err(text)?;
    floating.set_focus().map_err(text)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capture {
    Start,
    Renew,
    Stop,
}

#[tauri::command]
pub async fn batch_capture(
    window: WebviewWindow,
    state: State<'_, AppState>,
    windows: State<'_, BindingWindows>,
    id: String,
    action: Capture,
) -> Result<(), String> {
    let state = state.inner().clone();
    let windows = windows.inner().clone();
    background(move || {
        let label = window.label();
        if !matches!(label, "main" | LABEL) {
            return Err("请在录入悬浮窗中操作。".into());
        }
        if matches!(action, Capture::Stop) {
            return release(&state, &windows, label, &id);
        }
        let _operation = state.operations.lock().map_err(text)?;
        if !window.is_focused().unwrap_or(false) {
            return Err("请先点击录入窗口。".into());
        }
        let snapshot = state.client.snapshot.lock().map_err(text)?;
        let restart = snapshot.restart_count;
        let capturing = snapshot.status.binding_capture;
        let renewing = matches!(action, Capture::Renew);
        if renewing {
            let lease = windows.lease.lock().map_err(text)?;
            if !lease.as_ref().is_some_and(|lease| {
                lease.id == id && lease.window == label && lease.restart == restart
            }) || !capturing
                || snapshot.status.state != "capturing"
            {
                return Err("录入已结束，请重新点击录入。".into());
            }
        } else if capturing
            || snapshot.status.motion.measuring.is_some()
            || matches!(
                snapshot.status.state.as_str(),
                "calibrating" | "preparing_output"
            )
            || windows
                .lease
                .lock()
                .map_err(text)?
                .as_ref()
                .is_some_and(|lease| Instant::now() < lease.expires)
            || state
                .shortcut_capture_until
                .lock()
                .map_err(text)?
                .is_some_and(|until| Instant::now() < until)
        {
            return Err("请先结束当前录入、校准或输出准备。".into());
        }
        drop(snapshot);
        update_shortcut_capture(&state, true)?;
        if let Err(error) = state
            .client
            .request(Command::SetBindingCapture { active: true })
        {
            let _ = update_shortcut_capture(&state, false);
            return Err(error);
        }
        *windows.lease.lock().map_err(text)? = Some(Lease {
            id,
            window: label.into(),
            restart,
            expires: Instant::now() + Duration::from_secs(5),
        });
        // A window can close while the engine is acknowledging the pause.
        if !window.is_focused().unwrap_or(false) {
            let lease = windows.lease.lock().map_err(text)?.take();
            if let Some(lease) = lease {
                release_engine(&state, lease.restart)?;
            }
            return Err("请先点击录入窗口。".into());
        }
        Ok(())
    })
    .await
}

fn release_engine(state: &AppState, restart: u32) -> Result<(), String> {
    update_shortcut_capture(state, false)?;
    let snapshot = state.client.snapshot.lock().map_err(text)?;
    let same = snapshot.restart_count == restart
        && snapshot.status.binding_capture
        && snapshot.status.state == "capturing";
    drop(snapshot);
    if same {
        state
            .client
            .request(Command::SetBindingCapture { active: false })?;
    }
    Ok(())
}

fn release(
    state: &AppState,
    windows: &BindingWindows,
    label: &str,
    id: &str,
) -> Result<(), String> {
    let _operation = state.operations.lock().map_err(text)?;
    let mut current = windows.lease.lock().map_err(text)?;
    let lease = if current
        .as_ref()
        .is_some_and(|lease| lease.id == id && lease.window == label)
    {
        current.take()
    } else {
        None
    };
    drop(current);
    if let Some(lease) = lease {
        release_engine(state, lease.restart)?;
    }
    Ok(())
}

fn release_on_blur(state: &AppState, windows: &BindingWindows) {
    let lease = windows.lease.lock().ok().and_then(|lease| {
        lease
            .as_ref()
            .filter(|lease| lease.window == LABEL)
            .cloned()
    });
    if let Some(lease) = lease {
        let state = state.clone();
        let windows = windows.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = release(&state, &windows, LABEL, &lease.id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_output_strings_and_matching_row_counts_can_cross_windows() {
        let mut progress = Progress {
            cursor: 0,
            values: vec![Some("key:CTRL+wheel:up".into()), None],
            added: vec![],
        };
        assert!(progress.validate(2, 128).is_ok());
        assert!(progress.validate(1, 128).is_err());
        progress.values[0] = Some("key:F12".into());
        assert!(progress.validate(2, 128).is_err());
        progress.values[0] = Some("mouse:x2".into());
        progress.cursor = 2;
        assert!(progress.validate(2, 128).is_err());
    }

    #[test]
    fn blank_profiles_can_stage_new_outputs_up_to_the_remaining_layer_capacity() {
        let mut progress = Progress {
            cursor: 0,
            values: vec![],
            added: vec![],
        };
        assert!(progress.validate(0, 1).is_ok());
        progress.added.push(RecordedAction {
            name: String::new(),
            output: "key:R".into(),
        });
        progress.values.push(None);
        assert!(progress.validate(0, 1).is_ok());
        assert!(progress.validate(0, 0).is_err());
        progress.added[0].output = "key:F12".into();
        assert!(progress.validate(0, 1).is_err());
    }

    #[test]
    fn both_guided_templates_are_valid_studio_profiles() {
        use motion_core::storage::parse_profile;
        let shooter = parse_profile(include_str!("../../examples/shooter.toml")).unwrap();
        let adventure = parse_profile(include_str!("../../examples/adventure.toml")).unwrap();
        assert!(shooter.gyro.enabled && !shooter.stick_mouse.enabled);
        assert!(!adventure.gyro.enabled && adventure.stick_mouse.enabled);
        assert_eq!(shooter.actions.len(), 13);
        assert_eq!(adventure.actions.len(), 13);
    }
}
