use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub fn runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("MOTION_STUDIO_DATA_DIR") {
        return Ok(path.into());
    }
    app.path().app_data_dir().map_err(|e| e.to_string())
}
pub fn resources(app: &AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    }
    Ok(app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("resources"))
}
pub fn engine_path(app: &AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/studio-dev-runtime")
            .join(crate::installers::expected_engine_hash()?)
            .join("motion-engine.exe"));
    }
    Ok(resources(app)?.join("motion-engine.exe"))
}
