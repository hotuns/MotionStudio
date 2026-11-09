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

const PREFIX: &str = "MS1.";
const MAX_PACKED_BYTES: usize = MAX_PROFILE_BYTES + 1024;
const MAX_CODE_LENGTH: usize = PREFIX.len() + (MAX_PACKED_BYTES * 4).div_ceil(3);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SharedSource {
    Code { code: String },
    Community { content: String, sha256: String },
}

#[derive(Serialize)]
pub(crate) struct SharedPreview {
    profile: Profile,
    custom_filters: usize,
}

fn short_code(code: &str) -> bool {
    code.get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("MS-"))
        && code.len() == 11
        && code[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn pack(content: &str) -> Result<String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(content.as_bytes())?;
    let bytes = encoder.finish()?;
    ensure!(bytes.len() <= MAX_PACKED_BYTES, "配置码太长");
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn unpack(code: &str) -> Result<String> {
    ensure!(code.len() <= MAX_CODE_LENGTH, "配置码太长");
    let payload = code.trim().strip_prefix(PREFIX).context("配置码格式无效")?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).context("配置码格式无效")?;
    ensure!(bytes.len() <= MAX_PACKED_BYTES, "配置码太长");
    let mut content = Vec::with_capacity(MAX_PROFILE_BYTES + 1);
    let mut decoder = Decompress::new(true);
    let status = decoder
        .decompress_vec(&bytes, &mut content, FlushDecompress::Finish)
        .context("配置码损坏或不完整")?;
    ensure!(content.len() <= MAX_PROFILE_BYTES, "配置文件超过 512 KB");
    ensure!(
        status == Status::StreamEnd && decoder.total_in() == bytes.len() as u64,
        "配置码损坏或不完整"
    );
    String::from_utf8(content).context("配置码不是有效的 UTF-8 文本")
}

fn prepare(mut profile: Profile) -> Result<(SharedPreview, String)> {
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
            profile,
            custom_filters,
        },
        content,
    ))
}

fn preview(source: SharedSource) -> Result<SharedPreview> {
    let content = match source {
        SharedSource::Code { code } => unpack(&code)?,
        SharedSource::Community { content, sha256 } => {
            ensure!(content.len() <= MAX_PROFILE_BYTES, "配置文件超过 512 KB");
            ensure!(
                format!("{:x}", Sha256::digest(content.as_bytes())) == sha256,
                "社区配置校验失败，请刷新目录后重试"
            );
            content
        }
    };
    let (preview, _) = prepare(decode_profile(&content)?)?;
    Ok(preview)
}

#[tauri::command]
pub(crate) async fn copy_profile_code(profile: Profile) -> Result<(), String> {
    background(move || {
        let (_, content) = prepare(profile).map_err(text)?;
        let code = pack(&content).map_err(text)?;
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
        if code.len() > MAX_CODE_LENGTH {
            return Err("配置码太长".into());
        }
        if !code.starts_with(PREFIX) && !short_code(code) {
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
) -> Result<ProfileEntry, String> {
    let state = state.inner().clone();
    background(move || {
        let SharedPreview { profile, .. } = preview(source).map_err(text)?;
        let _lock = state.operations.lock().map_err(text)?;
        let entry = ProfileEntry {
            id: uuid::Uuid::new_v4().to_string(),
            profile,
        };
        state.store.save(&entry).map_err(text)?;
        Ok(entry)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trip_and_corruption_checks() {
        let content = include_str!("../../examples/hold-layer.toml");
        let code = pack(content).unwrap();
        assert_eq!(unpack(&format!(" {code}\n")).unwrap(), content);
        assert_eq!(
            preview(SharedSource::Code { code: code.clone() })
                .unwrap()
                .profile
                .layers
                .len(),
            1
        );
        let mut bytes = URL_SAFE_NO_PAD.decode(&code[PREFIX.len()..]).unwrap();
        bytes.pop();
        assert!(unpack(&format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&bytes))).is_err());
        bytes = URL_SAFE_NO_PAD.decode(&code[PREFIX.len()..]).unwrap();
        bytes.push(0);
        assert!(unpack(&format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&bytes))).is_err());
        assert!(unpack(&pack(&"x".repeat(MAX_PROFILE_BYTES + 1)).unwrap()).is_err());
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
                code: pack(&content).unwrap()
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
