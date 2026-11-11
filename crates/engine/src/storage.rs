use crate::{engine::Engine, profile::Profile};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: String,
    pub profile: Profile,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "profile_id", rename_all = "snake_case")]
pub enum SwitchTarget {
    #[default]
    None,
    Restore,
    Profile(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub selected_profile: String,
    pub device_path: String,
    pub output_backend: crate::output::OutputBackend,
    pub gamepad_backend: crate::output::GamepadBackend,
    pub mouse_display_sync: bool,
    pub auto_hide: bool,
    pub overlay_enabled: bool,
    pub overlay_duration_ms: u32,
    // Retained for compatibility with settings saved by older versions.
    pub overlay_position: OverlayPosition,
    pub language: Language,
    pub cursor_enabled: bool,
    pub cursor_debounce_ms: u32,
    pub cursor_shown: SwitchTarget,
    pub cursor_hidden: SwitchTarget,
    pub shortcut_profile: Option<String>,
    pub shortcut_key: Option<crate::shortcut::Shortcut>,
    pub shortcut_pad: Option<String>,
    pub calibrations: BTreeMap<String, [f32; 3]>,
    pub motion_calibrations: BTreeMap<String, crate::motion_config::SensorCalibration>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            selected_profile: "gyro".into(),
            device_path: String::new(),
            output_backend: Default::default(),
            gamepad_backend: Default::default(),
            mouse_display_sync: false,
            auto_hide: true,
            overlay_enabled: true,
            overlay_duration_ms: 2500,
            overlay_position: OverlayPosition::default(),
            language: Language::ZhCn,
            cursor_enabled: false,
            cursor_debounce_ms: 250,
            cursor_shown: SwitchTarget::None,
            cursor_hidden: SwitchTarget::None,
            shortcut_profile: None,
            shortcut_key: Some(Default::default()),
            shortcut_pad: None,
            calibrations: BTreeMap::new(),
            motion_calibrations: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}
#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("profiles"))?;
        let store = Self { root };
        // Seed only the first workspace. Removing all profiles later is intentional.
        if !store.root.join("initialized").exists() {
            for (id, text) in [
                ("gyro", include_str!("../../../examples/gyro-mouse.toml")),
                (
                    "keyboard",
                    include_str!("../../../examples/keyboard-mouse.toml"),
                ),
                (
                    "gamepad",
                    include_str!("../../../examples/native-gamepad.toml"),
                ),
                ("cursor", include_str!("../../../examples/cursor.toml")),
                ("blank", include_str!("../../../examples/blank.toml")),
            ] {
                if !store.profile_path(id)?.exists() {
                    store.save(&ProfileEntry {
                        id: id.into(),
                        profile: toml::from_str(text)?,
                    })?;
                }
            }
            atomic_write(&store.root.join("initialized"), b"1")?;
        }
        Ok(store)
    }
    pub fn profile_path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 80
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            bail!("Invalid profile ID");
        }
        Ok(self.root.join("profiles").join(format!("{id}.toml")))
    }
    pub fn list(&self) -> Result<(Vec<ProfileEntry>, Vec<String>)> {
        let mut entries = vec![];
        let mut warnings = vec![];
        for item in std::fs::read_dir(self.root.join("profiles"))? {
            let path = item?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("Invalid profile filename")?;
            match self.load(id) {
                Ok(entry) => entries.push(entry),
                Err(e) => warnings.push(format!("配置 {id} 未加载：{e:#}。原文件已保留。")),
            }
        }
        entries.sort_by(|a, b| a.profile.name.cmp(&b.profile.name));
        Ok((entries, warnings))
    }
    pub fn load(&self, id: &str) -> Result<ProfileEntry> {
        let path = self.profile_path(id)?;
        let text = read_small(&path)?;
        let profile = parse_profile(&text)?;
        Ok(ProfileEntry {
            id: id.to_owned(),
            profile,
        })
    }
    pub fn save(&self, entry: &ProfileEntry) -> Result<()> {
        Engine::new(entry.profile.clone())?;
        let path = self.profile_path(&entry.id)?;
        if let Ok(old) = read_small(&path) {
            if parse_profile(&old).is_ok() {
                atomic_write(&path.with_extension("toml.bak"), old.as_bytes())?;
            }
        }
        atomic_write(&path, toml::to_string_pretty(&entry.profile)?.as_bytes())
    }
    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.profile_path(id)?;
        std::fs::remove_file(&path)?;
        let backup = path.with_extension("toml.bak");
        if backup.exists() {
            std::fs::remove_file(backup)?;
        }
        Ok(())
    }
    pub fn restore_backup(&self, id: &str) -> Result<ProfileEntry> {
        let path = self.profile_path(id)?;
        let text = read_small(&path.with_extension("toml.bak"))?;
        let profile = parse_profile(&text)?;
        // Preserve the rejected file for inspection instead of overwriting it.
        if path.exists() {
            std::fs::copy(&path, path.with_extension("toml.rejected"))?;
        }
        atomic_write(&path, text.as_bytes())?;
        Ok(ProfileEntry {
            id: id.into(),
            profile,
        })
    }
    pub fn settings(&self) -> Result<Settings> {
        let path = self.root.join("settings.json");
        if !path.exists() {
            return Ok(Settings::default());
        }
        let result = serde_json::from_str::<Settings>(&read_small(&path)?);
        let settings = match result {
            Ok(s) => s,
            Err(error) => {
                bail!("设置文件损坏，原文件已保留。请在运行与恢复中恢复设置备份：{error}")
            }
        };
        settings.validate()?;
        Ok(settings)
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        settings.validate()?;
        let path = self.root.join("settings.json");
        if let Ok(old) = read_small(&path) {
            if serde_json::from_str::<Settings>(&old).is_ok() {
                atomic_write(&self.root.join("settings.json.bak"), old.as_bytes())?;
            }
        }
        atomic_write(&path, &serde_json::to_vec_pretty(settings)?)
    }
    pub fn restore_settings(&self) -> Result<Settings> {
        let text = read_small(&self.root.join("settings.json.bak"))?;
        let settings: Settings = serde_json::from_str(&text)?;
        settings.validate()?;
        atomic_write(&self.root.join("settings.json"), text.as_bytes())?;
        Ok(settings)
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported settings version");
        }
        if !(1000..=8000).contains(&self.overlay_duration_ms) {
            bail!("配置提示显示时长应在 1 到 8 秒之间");
        }
        if self.cursor_debounce_ms > 5000 {
            bail!("光标状态确认时间应在 0 到 5000 毫秒之间");
        }
        if let Some(key) = &self.shortcut_key {
            key.validate()?;
        }
        if let Some(input) = &self.shortcut_pad {
            crate::shortcut::validate_pad_input(input)?;
        }
        for calibration in self.motion_calibrations.values() {
            calibration.validate()?;
        }
        if self
            .calibrations
            .values()
            .flatten()
            .any(|x| !x.is_finite() || x.abs() > 0.15)
        {
            bail!("Invalid device calibration");
        }
        Ok(())
    }
}
pub const MAX_PROFILE_BYTES: usize = 512 * 1024;

/// Decode the stored format before initializing an engine. Sharing uses this
/// boundary to disable custom scripts in profiles received from other users.
pub fn decode_profile(text: &str) -> Result<Profile> {
    if text.len() > MAX_PROFILE_BYTES {
        bail!("配置文件超过 512 KB");
    }
    match toml::from_str(text) {
        Ok(profile) => Ok(profile),
        Err(error) => {
            if toml::from_str::<toml::Table>(text).is_ok_and(|table| table.contains_key("jsm")) {
                bail!("旧版 JSM 配置需要转换。请在「导入 JSM 配置」中选择此 TOML 文件");
            }
            Err(error).context("配置格式错误")
        }
    }
}

pub fn parse_profile(text: &str) -> Result<Profile> {
    let profile = decode_profile(text)?;
    Engine::new(profile.clone())?;
    Ok(profile)
}
pub fn read_small(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_PROFILE_BYTES as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > MAX_PROFILE_BYTES {
        bail!("File exceeds 512 KB");
    }
    Ok(text)
}
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("Missing parent directory")?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(not(windows))]
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
