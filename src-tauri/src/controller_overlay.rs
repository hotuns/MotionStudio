//! A display-only WebView over the existing status stream; never owns input/output.
use crate::{text, AppState};
use motion_core::{
    protocol::Status,
    sdl::Device,
    storage::{ControllerOverlaySettings, Language, OverlayContent, OverlayPosition},
};
use serde::Serialize;
use tauri::{
    window::Color, AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

pub const LABEL: &str = "controller-overlay";

fn logical_size(settings: &ControllerOverlaySettings) -> [f64; 2] {
    let width = f64::from(settings.width);
    let model = width * 0.75;
    let panel = (width * 0.48).clamp(128., 224.);
    let height = match settings.content {
        OverlayContent::Controller => model,
        OverlayContent::KeyMouse => panel,
        OverlayContent::Both => model + panel,
    };
    [width, height]
}

fn placement(
    origin: [i32; 2],
    area: [u32; 2],
    size: [u32; 2],
    margin: u32,
    corner: OverlayPosition,
) -> [i32; 2] {
    let available = [
        area[0].saturating_sub(size[0]),
        area[1].saturating_sub(size[1]),
    ];
    let right = matches!(
        corner,
        OverlayPosition::TopRight | OverlayPosition::BottomRight
    );
    let bottom = matches!(
        corner,
        OverlayPosition::BottomLeft | OverlayPosition::BottomRight
    );
    [right, bottom]
        .into_iter()
        .enumerate()
        .map(|(axis, far)| {
            let inset = margin.min(available[axis]);
            origin[axis].saturating_add(if far { available[axis] - inset } else { inset } as i32)
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

pub fn configure(app: &AppHandle, settings: &ControllerOverlaySettings) -> Result<(), String> {
    settings.validate().map_err(text)?;
    if !settings.enabled {
        if let Some(window) = app.get_webview_window(LABEL) {
            window.destroy().map_err(text)?;
        }
        return Ok(());
    }
    if app.get_webview_window("main").is_none() {
        return Err("应用正在退出".into());
    }
    let existing = app.get_webview_window(LABEL);
    let created = existing.is_none();
    let [width, height] = logical_size(settings);
    let window = match existing {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
            .title("Motion Studio · Desktop overlay")
            .initialization_script("window.__MOTION_CONTROLLER_OVERLAY__ = true;")
            .transparent(true)
            .background_color(Color(0, 0, 0, 0))
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .closable(false)
            .visible(false)
            .inner_size(width, height)
            .build()
            .map_err(text)?,
    };
    let result = (|| {
        window.set_ignore_cursor_events(true).map_err(text)?;
        let monitors = window.available_monitors().map_err(text)?;
        let monitor = settings
            .monitor
            .as_ref()
            .and_then(|name| monitors.iter().find(|m| m.name() == Some(name)).cloned())
            .or(window.primary_monitor().map_err(text)?)
            .or_else(|| monitors.first().cloned())
            .ok_or("无法确定悬浮手柄显示器")?;
        let work = monitor.work_area();
        let scale = monitor.scale_factor();
        let fit = scale.min(f64::from(work.size.width) / width)
            .min(f64::from(work.size.height) / height);
        let size = [width, height].map(|axis| (axis * fit).round().max(1.) as u32);
        let pos = placement(
            [work.position.x, work.position.y],
            [work.size.width, work.size.height],
            size,
            (16. * scale).round() as u32,
            settings.position,
        );
        window
            .set_position(PhysicalPosition::new(pos[0], pos[1]))
            .map_err(text)?;
        window
            .set_size(PhysicalSize::new(size[0], size[1]))
            .map_err(text)?;
        // The first frame is shown by the WebView after its transparent surface
        // is ready, avoiding a flash of the ordinary Studio background.
        if app.get_webview_window("main").is_none() {
            let _ = window.destroy();
            return Err("应用正在退出".into());
        }
        Ok(())
    })();
    if result.is_err() && created {
        let _ = window.destroy();
    }
    result
}

fn require_window(label: &str) -> Result<(), String> {
    if label == "main" || label == LABEL {
        Ok(())
    } else {
        Err("请在 Studio 查看悬浮手柄".into())
    }
}

fn live(status: &Status, ready: bool, fresh: bool) -> bool {
    ready
        && fresh
        && status.device.is_some()
        && matches!(
            status.state.as_str(),
            "running"
                | "observing"
                | "navigating"
                | "calibrating"
                | "capturing"
                | "capture_release"
        )
}

#[derive(Serialize)]
pub struct OverlayView {
    settings: ControllerOverlaySettings,
    language: Language,
    status: Option<Status>,
    status_age_ms: Option<u64>,
    device: Option<Device>,
    restart_count: u32,
}

#[tauri::command]
pub fn controller_overlay_state(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<OverlayView, String> {
    require_window(window.label())?;
    let settings = state.settings.lock().map_err(text)?.clone();
    let snapshot = state.client.snapshot.lock().map_err(text)?.clone();
    let device = snapshot.status.device.clone().or_else(|| {
        snapshot
            .devices
            .iter()
            .find(|device| device.path == settings.device_path)
            .cloned()
    });
    let status_age_ms = snapshot.status_seen.map(|at| at.elapsed().as_millis() as u64);
    let status = live(
        &snapshot.status,
        snapshot.engine_ready,
        status_age_ms.is_some_and(|age| age < 600),
    )
    .then_some(snapshot.status);
    Ok(OverlayView {
        settings: settings.controller_overlay,
        language: settings.language,
        status,
        status_age_ms,
        device,
        restart_count: snapshot.restart_count,
    })
}

#[derive(Serialize)]
pub struct Display {
    name: String,
    width: u32,
    height: u32,
}
#[tauri::command]
pub fn controller_overlay_displays(window: WebviewWindow) -> Result<Vec<Display>, String> {
    require_window(window.label())?;
    Ok(window
        .available_monitors()
        .map_err(text)?
        .iter()
        .filter_map(|monitor| {
            Some(Display {
                name: monitor.name()?.to_owned(),
                width: monitor.size().width,
                height: monitor.size().height,
            })
        })
        .collect())
}

#[tauri::command]
pub fn controller_overlay_ready(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("请在 Studio 查看悬浮手柄".into());
    }
    if state
        .settings
        .lock()
        .map_err(text)?
        .controller_overlay
        .enabled
    {
        window.show().map_err(text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_support_negative_monitor_coordinates_and_small_work_areas() {
        assert_eq!(
            placement(
                [-1920, -200],
                [1920, 1080],
                [320, 240],
                16,
                OverlayPosition::BottomLeft
            ),
            [-1904, 624]
        );
        assert_eq!(
            placement(
                [-1920, -200],
                [1920, 1080],
                [320, 240],
                16,
                OverlayPosition::TopRight
            ),
            [-336, -184]
        );
        assert_eq!(
            placement(
                [100, 200],
                [100, 50],
                [100, 50],
                24,
                OverlayPosition::BottomRight
            ),
            [100, 200]
        );
    }

    #[test]
    fn stopped_faulted_waiting_or_stale_status_cannot_latch_inputs() {
        let mut status = Status::default();
        for state in ["idle", "faulted", "waiting", "preparing_output", "running"] {
            status.state = state.into();
            assert!(!live(&status, true, true)); // No device yet.
        }
        status.device = Some(serde_json::from_value(serde_json::json!({
            "id": 1, "name": "Synthetic", "path": "test://controller", "vendor": 0, "product": 0,
            "origin": "physical", "gamepad": true, "gyro": false, "accelerometer": false,
            "buttons": [], "open_error": null
        })).unwrap());
        for state in ["idle", "faulted", "waiting", "preparing_output"] {
            status.state = state.into();
            assert!(!live(&status, true, true));
        }
        for state in [
            "running",
            "observing",
            "calibrating",
            "capturing",
            "capture_release",
        ] {
            status.state = state.into();
            assert!(live(&status, true, true));
            assert!(!live(&status, false, true));
            assert!(!live(&status, true, false));
        }
    }

    #[test]
    fn old_settings_keep_overlay_off_and_values_are_bounded() {
        let settings: motion_core::storage::Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.controller_overlay.enabled);
        assert_eq!(settings.controller_overlay.width, 320);
        assert_eq!(settings.controller_overlay.content, OverlayContent::KeyMouse);
        let old: ControllerOverlaySettings = serde_json::from_str(r#"{"enabled":true,"width":400}"#).unwrap();
        assert_eq!(old.content, OverlayContent::Controller);
        assert_eq!(logical_size(&old), [400., 300.]);
        assert_eq!(logical_size(&ControllerOverlaySettings {content: OverlayContent::Both, ..old}), [400., 492.]);
        for (width, opacity) in [(159, 100), (641, 100), (320, 19), (320, 101)] {
            let config = ControllerOverlaySettings {
                width,
                opacity,
                ..Default::default()
            };
            assert!(config.validate().is_err());
        }
        let config = ControllerOverlaySettings {
            enabled: true,
            width: 480,
            opacity: 80,
            position: OverlayPosition::TopLeft,
            ..Default::default()
        };
        let loaded: ControllerOverlaySettings =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(config, loaded);
        loaded.validate().unwrap();
        assert!(require_window("binding-window").is_err());
        assert!(require_window(LABEL).is_ok());
    }
}
