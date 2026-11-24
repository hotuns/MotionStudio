use crate::{
    engine::Engine,
    profile::Profile,
    shortcut::{ShortcutRule, ShortcutTrigger},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};
mod game_import;
pub use game_import::game_import_profile_ids;

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub games: Vec<crate::game_library::Game>,
    pub device_path: String,
    pub output_backend: crate::output::OutputBackend,
    pub gamepad_backend: crate::output::GamepadBackend,
    pub mouse_display_sync: bool,
    pub io_rates: crate::timing::IoRates,
    pub auto_hide: bool,
    pub overlay_enabled: bool,
    pub overlay_duration_ms: u32,
    // Retained for compatibility with settings saved by older versions.
    pub overlay_position: OverlayPosition,
    pub controller_overlay: ControllerOverlaySettings,
    pub language: Language,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_source: Option<UpdateSource>,
    pub cursor_enabled: bool,
    pub cursor_debounce_ms: u32,
    pub cursor_shown: SwitchTarget,
    pub cursor_hidden: SwitchTarget,
    pub shortcut_profile: Option<String>,
    pub shortcut_key: Option<crate::shortcut::Shortcut>,
    pub shortcut_pad: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut_rules: Option<Vec<ShortcutRule>>,
    pub calibrations: BTreeMap<String, [f32; 3]>,
    pub motion_calibrations: BTreeMap<String, crate::motion_config::SensorCalibration>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            selected_profile: "gyro".into(),
            games: Vec::new(),
            device_path: String::new(),
            output_backend: Default::default(),
            gamepad_backend: Default::default(),
            mouse_display_sync: false,
            io_rates: Default::default(),
            auto_hide: true,
            overlay_enabled: true,
            overlay_duration_ms: 2500,
            overlay_position: OverlayPosition::default(),
            controller_overlay: ControllerOverlaySettings::default(),
            language: Language::ZhCn,
            update_source: None,
            cursor_enabled: false,
            cursor_debounce_ms: 250,
            cursor_shown: SwitchTarget::None,
            cursor_hidden: SwitchTarget::None,
            shortcut_profile: None,
            shortcut_key: Some(Default::default()),
            shortcut_pad: None,
            shortcut_rules: None,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControllerOverlaySettings {
    /// The desktop WebView and the recording source have independent lifetimes.
    pub enabled: bool,
    pub obs_enabled: bool,
    #[serde(default)] // Previously saved overlay settings always displayed the controller.
    pub content: OverlayContent,
    pub width: u32,
    pub opacity: u32,
    pub position: OverlayPosition,
    pub monitor: Option<String>,
    pub follow_motion: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayContent {
    #[default]
    Controller,
    KeyMouse,
    Both,
}
impl Default for ControllerOverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            obs_enabled: false,
            content: OverlayContent::KeyMouse,
            width: 320,
            opacity: 100,
            position: OverlayPosition::BottomLeft,
            monitor: None,
            follow_motion: true,
        }
    }
}
impl ControllerOverlaySettings {
    pub fn validate(&self) -> Result<()> {
        if !(160..=640).contains(&self.width) || !(20..=100).contains(&self.opacity) {
            bail!("悬浮手柄宽度应为 160–640，透明度应为 20–100%");
        }
        if self.monitor.as_ref().is_some_and(|name| {
            name.len() > 256 || name.chars().any(char::is_control)
        }) {
            bail!("悬浮手柄显示器设置无效");
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateSource {
    GitHub,
    Sealos,
}

impl Settings {
    pub fn effective_update_source(&self) -> UpdateSource {
        self.update_source.unwrap_or(match self.language {
            Language::ZhCn => UpdateSource::Sealos,
            Language::En => UpdateSource::GitHub,
        })
    }
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
        validate_profile_id(id)?;
        Ok(self.root.join("profiles").join(format!("{id}.toml")))
    }
    pub fn profile_ids(&self) -> Result<Vec<String>> {
        let mut ids = vec![];
        for item in std::fs::read_dir(self.root.join("profiles"))? {
            let path = item?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("Invalid profile filename")?;
            ids.push(id.to_owned());
        }
        // A locked file can survive an interrupted rollback until the next startup.
        if let Ok(Some(pending)) = self.pending_game_import() {
            ids.retain(|id| !pending.contains(id));
        }
        Ok(ids)
    }
    pub fn load_profiles(&self, ids: Vec<String>) -> (Vec<ProfileEntry>, Vec<String>) {
        let mut entries = vec![];
        let mut warnings = vec![];
        for id in ids {
            match self.load(&id) {
                Ok(entry) => entries.push(entry),
                Err(e) => warnings.push(format!("配置 {id} 未加载：{e:#}。原文件已保留。")),
            }
        }
        entries.sort_by(|a, b| a.profile.name.cmp(&b.profile.name));
        (entries, warnings)
    }
    pub fn list(&self) -> Result<(Vec<ProfileEntry>, Vec<String>)> {
        Ok(self.load_profiles(self.profile_ids()?))
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
        let mut profile = entry.profile.clone();
        profile.materialize_gamepad_bindings();
        Engine::new(profile.clone())?;
        let path = self.profile_path(&entry.id)?;
        if let Ok(old) = read_small(&path) {
            if parse_profile(&old).is_ok() {
                atomic_write(&path.with_extension("toml.bak"), old.as_bytes())?;
            }
        }
        atomic_write(&path, toml::to_string_pretty(&profile)?.as_bytes())
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
        let mut settings = match result {
            Ok(s) => s,
            Err(error) => {
                bail!("设置文件损坏，原文件已保留。请在运行与恢复中恢复设置备份：{error}")
            }
        };
        settings.validate()?;
        self.reconcile_game_files(&mut settings)?;
        Ok(settings)
    }
    // A user can remove a TOML on disk, or restore settings from before a
    // deletion. Keep membership for existing files even if they need repair.
    fn reconcile_game_files(&self, settings: &mut Settings) -> Result<bool> {
        let mut missing = Vec::new();
        for game in &settings.games {
            for id in &game.profile_ids {
                if matches!(self.profile_path(id)?.try_exists(), Ok(false)) {
                    missing.push(id.clone());
                }
            }
        }
        for id in &missing { settings.forget_profile(id); }
        if !missing.is_empty() && settings.selected_profile.is_empty() {
            settings.selected_profile = self.list()?.0.first().map(|entry| entry.id.clone()).unwrap_or_default();
        }
        Ok(!missing.is_empty())
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let bytes = Self::settings_bytes(settings)?;
        self.backup_settings()?;
        atomic_write(&self.root.join("settings.json"), &bytes)
    }
    fn settings_bytes(settings: &Settings) -> Result<Vec<u8>> {
        settings.validate()?;
        let bytes = serde_json::to_vec_pretty(settings)?;
        if bytes.len() > MAX_PROFILE_BYTES { bail!("File exceeds 512 KB"); }
        Ok(bytes)
    }
    fn backup_settings(&self) -> Result<()> {
        let path = self.root.join("settings.json");
        if let Ok(old) = read_small(&path) {
            if serde_json::from_str::<Settings>(&old).is_ok() {
                atomic_write(&self.root.join("settings.json.bak"), old.as_bytes())?;
            }
        }
        Ok(())
    }
    pub fn restore_settings(&self) -> Result<Settings> {
        let text = read_small(&self.root.join("settings.json.bak"))?;
        let mut settings: Settings = serde_json::from_str(&text)?;
        settings.validate()?;
        let bytes = if self.reconcile_game_files(&mut settings)? {
            serde_json::to_vec_pretty(&settings)?
        } else {
            text.into_bytes()
        };
        atomic_write(&self.root.join("settings.json"), &bytes)?;
        Ok(settings)
    }
}
impl Settings {
    /// An explicit list, including an empty list, replaces the legacy shortcut fields.
    pub fn effective_shortcut_rules(&self) -> Vec<ShortcutRule> {
        if let Some(rules) = &self.shortcut_rules {
            return rules.clone();
        }
        let Some(target) = &self.shortcut_profile else {
            return Vec::new();
        };
        self.shortcut_key
            .iter()
            .cloned()
            .map(|key| ShortcutTrigger::Keyboard { key })
            .chain(
                self.shortcut_pad
                    .iter()
                    .cloned()
                    .map(|input| ShortcutTrigger::Gamepad { input }),
            )
            .map(|trigger| ShortcutRule {
                trigger,
                target: target.clone(),
                return_on_repeat: true,
            })
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported settings version");
        }
        self.io_rates.validate()?;
        self.validate_games()?;
        self.controller_overlay.validate()?;
        if !(1000..=8000).contains(&self.overlay_duration_ms) {
            bail!("配置提示显示时长应在 1 到 8 秒之间");
        }
        if self.cursor_debounce_ms > 5000 {
            bail!("光标状态确认时间应在 0 到 5000 毫秒之间");
        }
        if let Some(rules) = &self.shortcut_rules {
            crate::shortcut::validate_shortcut_rules(rules)?;
        } else {
            if let Some(key) = &self.shortcut_key {
                key.validate()?;
            }
            if let Some(input) = &self.shortcut_pad {
                crate::shortcut::validate_pad_input(input)?;
            }
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

pub(crate) fn validate_profile_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 80
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("Invalid profile ID");
    }
    Ok(())
}

pub const MAX_PROFILE_BYTES: usize = 512 * 1024;

/// Decode the stored format before initializing an engine. Sharing uses this
/// boundary to disable custom scripts in profiles received from other users.
pub fn decode_profile(text: &str) -> Result<Profile> {
    if text.len() > MAX_PROFILE_BYTES {
        bail!("配置文件超过 512 KB");
    }
    match toml::from_str::<Profile>(text) {
        Ok(mut profile) => {
            profile.materialize_gamepad_bindings();
            Ok(profile)
        }
        Err(error) => {
            if toml::from_str::<toml::Table>(text).is_ok_and(|table| table.contains_key("jsm")) {
                bail!("这份旧版配置需要转换。请在「导入文本配置」中选择此 TOML 文件。");
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
        atomic_replace(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

fn atomic_replace(from: &Path, to: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(from, to)?;
    Ok(())
}
