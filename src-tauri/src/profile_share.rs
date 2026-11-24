use crate::{background, text, AppState};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clipboard_win::{get_clipboard_string, set_clipboard_string};
use flate2::{write::ZlibEncoder, Compression, Decompress, FlushDecompress, Status};
use motion_core::{
    engine::Engine,
    profile::Profile,
    storage::{decode_profile, ProfileEntry, MAX_PROFILE_BYTES},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;
use tauri::State;

const GAME_COMMENT: &str = "# Motion Studio game: ";
pub(crate) struct CodeFormat {
    pub prefix: &'static str,
    pub limit: usize,
    pub size_error: &'static str,
}

const PROFILE_CODE: CodeFormat = CodeFormat {
    prefix: "MS1.",
    limit: MAX_PROFILE_BYTES,
    size_error: "配置文件超过 512 KB",
};
pub(crate) const GAME_CODE: CodeFormat = CodeFormat {
    prefix: "MSG1.",
    limit: 8 * 1024 * 1024,
    size_error: "整组配置超过 8 MiB",
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SharedSource {
    Code { code: String },
    Community { content: String, sha256: String },
}

#[derive(Serialize)]
pub(crate) struct SharedPreview {
    pub game_name: Option<String>,
    pub profile: Profile,
    pub custom_filters: usize,
}

// A single escaped TOML comment also works with older profile and MS1 readers.
pub(crate) fn with_game_name(content: String, game: Option<&str>) -> Result<String> {
    ensure!(content.len() <= MAX_PROFILE_BYTES, "配置文件超过 512 KB");
    if let Some(name) = game.filter(|s| !s.trim().is_empty() && s.chars().count() <= 100) {
        let mut header = format!("{GAME_COMMENT}{}\n", serde_json::to_string(name)?);
        if header.len() + content.len() <= MAX_PROFILE_BYTES {
            header.push_str(&content);
            return Ok(header);
        }
    }
    Ok(content)
}

fn game_hint(content: &str) -> Option<String> {
    let hint = content.trim_start_matches('\u{feff}').lines().next()?.strip_prefix(GAME_COMMENT)?;
    let name: String = serde_json::from_str(hint).ok()?;
    (!name.trim().is_empty() && name.chars().count() <= 100).then_some(name)
}

#[tauri::command]
pub(crate) async fn preview_profile_import(content: String) -> Result<serde_json::Value, String> {
    background(move || {
        let profile = motion_core::storage::parse_profile(&content).map_err(text)?;
        Ok(serde_json::json!({"name": profile.name, "game_name": game_hint(&content)}))
    }).await
}

fn short_code(code: &str) -> bool {
    code.get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("MS-"))
        && code.len() == 11
        && code[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl CodeFormat {
    fn packed_limit(&self) -> usize {
        self.limit + self.limit / 512
    }

    fn code_limit(&self) -> usize {
        self.prefix.len() + (self.packed_limit() * 4).div_ceil(3)
    }

    pub fn pack(&self, content: &str) -> Result<String> {
        ensure!(content.len() <= self.limit, self.size_error);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content.as_bytes())?;
        let bytes = encoder.finish()?;
        ensure!(bytes.len() <= self.packed_limit(), "配置码太长");
        Ok(format!("{}{}", self.prefix, URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn unpack(&self, code: &str) -> Result<String> {
        let code = code.trim();
        ensure!(code.len() <= self.code_limit(), "配置码太长");
        let payload = code.strip_prefix(self.prefix).context("配置码格式无效")?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).context("配置码格式无效")?;
        ensure!(bytes.len() <= self.packed_limit(), "配置码太长");
        let mut content = Vec::with_capacity(self.limit + 1);
        let mut decoder = Decompress::new(true);
        let status = decoder
            .decompress_vec(&bytes, &mut content, FlushDecompress::Finish)
            .context("配置码损坏或不完整")?;
        ensure!(content.len() <= self.limit, self.size_error);
        ensure!(
            status == Status::StreamEnd && decoder.total_in() == bytes.len() as u64,
            "配置码损坏或不完整"
        );
        String::from_utf8(content).context("配置码不是有效的 UTF-8 文本")
    }
}

pub(crate) fn prepare(mut profile: Profile) -> Result<(SharedPreview, String)> {
    let mut custom_filters = profile.gyro.motion.custom_filters.len();
    for filter in &mut profile.gyro.motion.custom_filters {
        filter.enabled = false;
    }
    // A hold layer can supply its own scripts. Disable them before the engine
    // validates any combination of layers or initializes the base profile.
    for layer in &mut profile.layers {
        let filters = layer
            .settings
            .get_mut("gyro")
            .and_then(|gyro| gyro.get_mut("motion"))
            .and_then(|motion| motion.get_mut("custom_filters"))
            .and_then(Value::as_array_mut);
        if let Some(filters) = filters {
            for filter in filters.iter_mut().filter_map(Value::as_object_mut) {
                if filter.get("enabled").is_none_or(Value::is_boolean) {
                    filter.insert("enabled".into(), Value::Bool(false));
                }
                custom_filters += 1;
            }
        }
    }
    let content = toml::to_string_pretty(&profile)?;
    ensure!(content.len() <= MAX_PROFILE_BYTES, "配置文件超过 512 KB");
    Engine::new(profile.clone())?;
    Ok((
        SharedPreview {
            game_name: None,
            profile,
            custom_filters,
        },
        content,
    ))
}

fn preview(source: SharedSource) -> Result<SharedPreview> {
    let content = match source {
        SharedSource::Code { code } => PROFILE_CODE.unpack(&code)?,
        SharedSource::Community { content, sha256 } => {
            ensure!(content.len() <= MAX_PROFILE_BYTES, "配置文件超过 512 KB");
            ensure!(
                format!("{:x}", Sha256::digest(content.as_bytes())) == sha256,
                "社区配置校验失败，请刷新目录后重试"
            );
            content
        }
    };
    let (mut preview, _) = prepare(decode_profile(&content)?)?;
    preview.game_name = game_hint(&content);
    Ok(preview)
}

#[tauri::command]
pub(crate) async fn copy_profile_code(profile: Profile, game_name: Option<String>) -> Result<(), String> {
    background(move || {
        let (_, content) = prepare(profile).map_err(text)?;
        let content = with_game_name(content, game_name.as_deref()).map_err(text)?;
        let code = PROFILE_CODE.pack(&content).map_err(text)?;
        set_clipboard_string(&code).map_err(|_| "无法写入剪贴板".into())
    })
    .await
}

#[tauri::command]
pub(crate) async fn copy_community_code(code: String) -> Result<(), String> {
    background(move || {
        if !short_code(&code) {
            return Err("配置码格式无效".into());
        }
        set_clipboard_string(&code).map_err(|_| "无法写入剪贴板".into())
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_profile_code() -> Result<String, String> {
    background(move || {
        let clipboard = get_clipboard_string().map_err(|_| "无法读取剪贴板")?;
        let code = clipboard.trim();
        if let Some(format) = [&PROFILE_CODE, &GAME_CODE].into_iter().find(|f| code.starts_with(f.prefix)) {
            if code.len() > format.code_limit() {
                return Err("配置码太长".into());
            }
        } else if !short_code(code) {
            return Err("剪贴板中没有可识别的配置码".into());
        }
        Ok(code.into())
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_shared_profile(source: SharedSource) -> Result<SharedPreview, String> {
    background(move || preview(source).map_err(text)).await
}

#[tauri::command]
pub(crate) async fn import_shared_profile(
    state: State<'_, AppState>,
    source: SharedSource,
    game_id: Option<String>,
    new_game_name: Option<String>,
) -> Result<ProfileEntry, String> {
    let state = state.inner().clone();
    background(move || {
        let SharedPreview { profile, .. } = preview(source).map_err(text)?;
        let _lock = state.operations.lock().map_err(text)?;
        crate::game_library::insert_profile(&state, profile, game_id, new_game_name)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_game_name_is_optional_and_accepted_by_the_original_toml_reader() {
        let content = include_str!("../../examples/hold-layer.toml");
        let name = "A game with \"quotes\"";
        let tagged = with_game_name(content.into(), Some(name)).unwrap();
        let original = decode_profile(content).unwrap();
        assert_eq!(decode_profile(&tagged).unwrap().layers[0].id, original.layers[0].id);
        let imported = preview(SharedSource::Code { code: PROFILE_CODE.pack(&tagged).unwrap() }).unwrap();
        assert_eq!(imported.game_name.as_deref(), Some(name));
        assert_eq!(imported.profile.name, original.name);
        assert!(game_hint(content).is_none());
    }

    #[test]
    fn code_round_trip_and_corruption_checks() {
        let content = include_str!("../../examples/hold-layer.toml");
        let code = PROFILE_CODE.pack(content).unwrap();
        assert_eq!(PROFILE_CODE.unpack(&format!(" {code}\n")).unwrap(), content);
        assert_eq!(
            preview(SharedSource::Code { code: code.clone() })
                .unwrap()
                .profile
                .layers
                .len(),
            1
        );
        let prefix = PROFILE_CODE.prefix;
        let mut bytes = URL_SAFE_NO_PAD.decode(&code[prefix.len()..]).unwrap();
        bytes.pop();
        assert!(PROFILE_CODE.unpack(&format!("{prefix}{}", URL_SAFE_NO_PAD.encode(&bytes))).is_err());
        bytes = URL_SAFE_NO_PAD.decode(&code[prefix.len()..]).unwrap();
        bytes.push(0);
        assert!(PROFILE_CODE.unpack(&format!("{prefix}{}", URL_SAFE_NO_PAD.encode(&bytes))).is_err());
        let mut oversized = ZlibEncoder::new(Vec::new(), Compression::default());
        oversized.write_all(&vec![b'x'; MAX_PROFILE_BYTES + 1]).unwrap();
        assert!(PROFILE_CODE.unpack(&format!("{prefix}{}", URL_SAFE_NO_PAD.encode(oversized.finish().unwrap()))).is_err());
    }

    #[test]
    fn shared_filters_are_disabled_in_base_and_hold_layers() {
        let mut profile = decode_profile(include_str!("../../examples/hold-layer.toml")).unwrap();
        let filter = motion_core::custom_filters::FilterConfig {
            id: "untrusted".into(),
            name: "Untrusted".into(),
            source: "this cannot compile as Rhai !".into(),
            ..Default::default()
        };
        profile.gyro.motion.custom_filters.push(filter.clone());
        let mut layer_filter = serde_json::to_value(filter).unwrap();
        layer_filter.as_object_mut().unwrap().remove("enabled");
        profile.layers[0].settings.insert(
            "gyro".into(),
            serde_json::json!({"motion": {"custom_filters": [layer_filter]}}),
        );
        let (prepared, content) = prepare(profile).unwrap();
        assert_eq!(prepared.custom_filters, 2);
        assert!(!prepared.profile.gyro.motion.custom_filters[0].enabled);
        let layer = motion_core::layers::resolve(&prepared.profile, &[0]).unwrap();
        assert!(!layer.gyro.motion.custom_filters[0].enabled);
        assert_eq!(
            preview(SharedSource::Code {
                code: PROFILE_CODE.pack(&content).unwrap()
            })
            .unwrap()
            .custom_filters,
            2
        );
    }

    #[test]
    fn published_catalog_profiles_match_their_codes_and_import() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../community-configs");
        let index: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("index.json")).unwrap())
                .unwrap();
        assert_eq!(index["version"], 1);
        let profiles = index["profiles"].as_array().unwrap();
        assert!(!profiles.is_empty());
        let mut codes = std::collections::HashSet::new();
        for entry in profiles {
            let sha256 = entry["sha256"].as_str().unwrap();
            let code = entry["code"].as_str().unwrap();
            assert!(sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert_eq!(code, format!("MS-{}", sha256[..8].to_ascii_uppercase()));
            assert!(codes.insert(code));
            let content =
                std::fs::read_to_string(root.join("profiles").join(format!("{sha256}.toml")))
                    .unwrap();
            let result = preview(SharedSource::Community {
                content,
                sha256: sha256.into(),
            })
            .unwrap();
            assert_eq!(result.profile.name, entry["name"].as_str().unwrap());
            assert_eq!(
                serde_json::to_value(result.profile.virtual_gamepad).unwrap(),
                entry["output"]
            );
        }
    }
}
