mod portable;

use crate::{background, text, AppState};
use motion_core::protocol::Command;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{async_runtime, ipc::Channel, Manager, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASES: &str = "https://github.com/hotuns/MotionStudio/releases";
const BUSY: &str = "更新操作正在进行，请稍候。";
const STALE: &str = "更新已失效，请重新检查。";
const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;

pub type PortableRestart = Arc<Mutex<Option<PathBuf>>>;

#[derive(Default)]
struct Session {
    candidate: Option<Update>,
    downloaded: Option<Arc<[u8]>>,
}

pub struct Updates {
    session: async_runtime::Mutex<Session>,
    restart: PortableRestart,
}

impl Updates {
    pub fn new(restart: PortableRestart) -> Self {
        Self {
            session: Default::default(),
            restart,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Installed,
    Portable,
    Development,
}

impl Mode {
    fn current() -> Self {
        use tauri::utils::{config::BundleType, platform::bundle_type};
        if cfg!(debug_assertions) {
            Self::Development
        } else if bundle_type() == Some(BundleType::Nsis) {
            Self::Installed
        } else {
            Self::Portable
        }
    }

    fn target(self) -> &'static str {
        match self {
            Self::Installed => "windows-x86_64-nsis",
            Self::Portable => "windows-x86_64-portable",
            Self::Development => unreachable!(),
        }
    }

    fn asset_url(self, version: &str) -> String {
        let name = match self {
            Self::Installed => format!("MotionStudio-{version}-x64-setup.exe"),
            Self::Portable => format!("MotionStudio-{version}-windows-x64.zip"),
            Self::Development => unreachable!(),
        };
        format!("{RELEASES}/download/v{version}/{name}")
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
enum Availability {
    Idle,
    Current,
    Unpublished,
    Available { version: String, notes: String },
}

#[derive(Serialize)]
pub struct UpdateCheck {
    mode: Mode,
    #[serde(flatten)]
    availability: Availability,
}

#[derive(Clone, Serialize)]
pub struct Transfer {
    received: u64,
    total: Option<u64>,
}

fn only_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("软件更新只能在主窗口操作。".into());
    }
    Ok(())
}

fn failure(state: &AppState, message: &str, detail: impl std::fmt::Display) -> String {
    state.client.log(format!("Studio updater: {detail}"));
    message.into()
}

#[tauri::command]
pub async fn check_studio_update(
    window: WebviewWindow,
    updates: State<'_, Updates>,
    app_state: State<'_, AppState>,
) -> Result<UpdateCheck, String> {
    only_main(&window)?;
    let mode = Mode::current();
    let mut session = updates.session.try_lock().map_err(|_| BUSY)?;
    let check_error = |error| failure(&app_state, "无法检查更新，请稍后重试。", error);
    let availability = if mode == Mode::Development {
        Availability::Idle
    } else {
        let updater = window
            .app_handle()
            .updater_builder()
            .target(mode.target())
            .timeout(Duration::from_secs(12))
            .configure_client(|client| client.https_only(true))
            // The default hook hides every window before ShellExecute. Keep the
            // UI available if Windows rejects the installer. The engine is
            // stopped explicitly at install time, and success exits the process.
            .on_before_exit(|| {})
            .version_comparator(|current, release| {
                release.version.pre.is_empty()
                    && release.version.build.is_empty()
                    && release.version > current
            })
            .build()
            .map_err(check_error)?;
        match updater.check().await {
            Ok(Some(candidate))
                if candidate.download_url.as_str() == mode.asset_url(&candidate.version) =>
            {
                let availability = Availability::Available {
                    version: candidate.version.clone(),
                    notes: candidate
                        .body
                        .as_deref()
                        .unwrap_or_default()
                        .chars()
                        .take(6000)
                        .collect(),
                };
                *session = Session {
                    candidate: Some(candidate),
                    downloaded: None,
                };
                return Ok(UpdateCheck { mode, availability });
            }
            Ok(Some(_)) => return Err("更新清单中的下载地址无效。".into()),
            Ok(None) => Availability::Current,
            Err(tauri_plugin_updater::Error::ReleaseNotFound) => Availability::Unpublished,
            Err(error) => return Err(check_error(error)),
        }
    };
    *session = Session::default();
    Ok(UpdateCheck { mode, availability })
}

#[tauri::command]
pub async fn download_studio_update(
    window: WebviewWindow,
    updates: State<'_, Updates>,
    app_state: State<'_, AppState>,
    version: String,
    progress: Channel<Transfer>,
) -> Result<(), String> {
    only_main(&window)?;
    let mut session = updates.session.try_lock().map_err(|_| BUSY)?;
    let mut candidate = session
        .candidate
        .as_ref()
        .filter(|candidate| candidate.version == version)
        .cloned()
        .ok_or(STALE)?;
    candidate.timeout = Some(Duration::from_secs(180));
    let mut received = 0;
    let interval = Duration::from_millis(120);
    let mut last = Instant::now() - interval;
    let bytes = candidate
        .download(
            |size, total| {
                received += size as u64;
                if last.elapsed() >= interval || total == Some(received) {
                    let _ = progress.send(Transfer { received, total });
                    last = Instant::now();
                }
            },
            || {},
        )
        .await
        .map_err(|error| failure(&app_state, "下载更新失败或签名验证未通过，请重试。", error))?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err("更新包超出大小限制。".into());
    }
    session.downloaded = Some(bytes.into());
    Ok(())
}

fn stop_mapping_for_update(state: &AppState) -> Result<(), String> {
    let needs_stop = {
        let snapshot = state.client.snapshot.lock().map_err(text)?;
        let status = &snapshot.status;
        if status.binding_capture
            || status.motion.measuring.is_some()
            || matches!(
                status.state.as_str(),
                "calibrating" | "preparing_output" | "capture_release"
            )
        {
            return Err("请先结束录入或校准，再安装更新。".into());
        }
        snapshot.engine_ready
            || status.emitting
            || status.visibility.active
            || status.visibility.requested
    };
    if needs_stop {
        state
            .client
            .request(Command::Stop)
            .map_err(|error| failure(state, "无法停止映射，请恢复引擎后重试更新。", error))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn install_studio_update(
    window: WebviewWindow,
    updates: State<'_, Updates>,
    app_state: State<'_, AppState>,
    version: String,
) -> Result<(), String> {
    only_main(&window)?;
    let session = updates.session.try_lock().map_err(|_| BUSY)?;
    let candidate = session
        .candidate
        .as_ref()
        .filter(|candidate| candidate.version == version)
        .cloned()
        .ok_or(STALE)?;
    let bytes = session.downloaded.clone().ok_or(STALE)?;
    let state = app_state.inner().clone();
    let app = window.app_handle().clone();
    let restart = updates.restart.clone();
    let result = background(move || {
        let _operation = state.operations.try_lock().map_err(|_| BUSY)?;
        match Mode::current() {
            Mode::Installed => {
                stop_mapping_for_update(&state)?;
                state.client.with_worker_stopped_for_update(|| {
                    candidate
                        .install(&bytes)
                        .map_err(|error| failure(&state, "更新安装程序未能启动，请重试。", error))
                })
            }
            Mode::Portable => {
                let stage = portable::stage_next_to_app(&bytes).map_err(|error| {
                    failure(
                        &state,
                        "无法解压免安装更新，请确认磁盘空间和目录写入权限。",
                        error,
                    )
                })?;
                stop_mapping_for_update(&state)?;
                let next = portable::publish(stage, &candidate.version).map_err(|error| {
                    failure(
                        &state,
                        "无法解压免安装更新，请确认磁盘空间和目录写入权限。",
                        error,
                    )
                })?;
                *restart.lock().map_err(text)? = Some(next);
                app.exit(0);
                Ok(())
            }
            Mode::Development => Err(STALE.into()),
        }
    })
    .await;
    drop(session);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;

    #[test]
    #[ignore = "requires artifacts produced by npm run package:update"]
    fn signed_release_packages() {
        let version = env!("CARGO_PKG_VERSION");
        let release = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../artifacts")
            .join(format!("release-v{version}"));
        let manifest: Value =
            serde_json::from_slice(&fs::read(release.join("latest.json")).unwrap()).unwrap();
        assert_eq!(manifest["version"], version);
        let platforms = manifest["platforms"].as_object().unwrap();
        assert_eq!(platforms.len(), 1);
        let config: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins = config.plugins;
        context.package_info_mut().version = "0.15.2".parse().unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let mode = Mode::Installed;
        let updater = app.updater_builder().target(mode.target()).build().unwrap();
        let candidate = updater.restore_update(manifest.clone()).unwrap().unwrap();
        assert_eq!(candidate.download_url.as_str(), mode.asset_url(version));
        let name = candidate
            .download_url
            .path_segments()
            .unwrap()
            .next_back()
            .unwrap();
        let mut bytes = fs::read(release.join(name)).unwrap();
        candidate.verify(&bytes).unwrap();

        let mut mislabeled = manifest.clone();
        mislabeled["version"] = json!("999.0.0");
        let wrong_version = updater.restore_update(mislabeled).unwrap().unwrap();
        assert!(matches!(
            wrong_version.verify(&bytes),
            Err(tauri_plugin_updater::Error::SignedVersionMismatch { .. })
        ));
        let mut older = manifest;
        older["version"] = json!("0.15.1");
        assert!(updater.restore_update(older).unwrap().is_none());

        bytes[0] ^= 1;
        assert!(candidate.verify(&bytes).is_err());
    }
}
