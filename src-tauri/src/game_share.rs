use crate::{
    background,
    profile_share::{prepare, GAME_CODE},
    text, AppState,
};
use anyhow::{anyhow, ensure, Context, Result};
use clipboard_win::set_clipboard_string;
use motion_core::{
    game_library::{Game, SwitchPolicy},
    profile::GamepadTarget,
    storage::{
        decode_profile, game_import_profile_ids, read_small, ProfileEntry, Settings, Store,
        SwitchTarget,
    },
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

// The array order identifies profiles inside the code. Targets in a switching
// policy use the same zero-based positions instead of the sender's local IDs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GameBundle {
    name: String,
    profiles: Vec<String>,
    default_profile: usize,
    use_game_rules: bool,
    switching: Option<SwitchPolicy>,
}

#[derive(Serialize)]
pub(crate) struct GamePreview {
    name: String,
    profiles: Vec<MemberPreview>,
    default_profile: usize,
    use_game_rules: bool,
    custom_filters: usize,
}

#[derive(Serialize)]
struct MemberPreview {
    name: String,
    output: GamepadTarget,
    layers: usize,
}

fn remap_switching(
    policy: &mut Option<SwitchPolicy>,
    resolve: impl Fn(&str) -> Option<String>,
) -> Result<()> {
    if let Some(policy) = policy {
        for target in [&mut policy.cursor_shown, &mut policy.cursor_hidden] {
            if let SwitchTarget::Profile(id) = target {
                *id = resolve(id).context("切换目标必须属于当前游戏")?;
            }
        }
        for shortcut in &mut policy.shortcut_rules {
            shortcut.target = resolve(&shortcut.target).context("切换目标必须属于当前游戏")?;
        }
    }
    Ok(())
}

impl GameBundle {
    fn from_library(store: &Store, game: &Game, draft: Option<&ProfileEntry>) -> Result<Self> {
        let position = |id: &str| game.profile_ids.iter().position(|member| member == id);
        let default_profile = game
            .default_profile
            .as_deref()
            .and_then(position)
            .context("游戏引用的配置无效")?;
        let mut switching = game.switching.clone();
        remap_switching(&mut switching, |id| {
            position(id).map(|index| index.to_string())
        })?;
        let mut profiles = Vec::with_capacity(game.profile_ids.len());
        let mut size = 0;
        for id in &game.profile_ids {
            let document = match draft.filter(|entry| &entry.id == id) {
                Some(entry) => toml::to_string_pretty(&entry.profile)?,
                None => read_small(&store.profile_path(id)?)?,
            };
            size += document.len();
            ensure!(size <= GAME_CODE.limit, GAME_CODE.size_error);
            profiles.push(document);
        }
        Ok(Self {
            name: game.name.clone(),
            profiles,
            default_profile,
            use_game_rules: game.use_game_rules,
            switching,
        })
    }

    fn prepare_profiles(&mut self) -> Result<GamePreview> {
        ensure!(
            self.default_profile < self.profiles.len(),
            "游戏引用的配置无效"
        );
        let mut profiles = Vec::new();
        let mut custom_filters = 0;
        let mut size = 0;
        for (index, document) in self.profiles.iter_mut().enumerate() {
            let (member, sanitized) = decode_profile(document)
                .and_then(prepare)
                .map_err(|error| anyhow!("第 {} 份配置无效：{error:#}", index + 1))?;
            size += sanitized.len();
            ensure!(size <= GAME_CODE.limit, GAME_CODE.size_error);
            *document = sanitized;
            custom_filters += member.custom_filters;
            profiles.push(MemberPreview {
                name: member.profile.name,
                output: member.profile.virtual_gamepad,
                layers: member.profile.layers.len() + 1,
            });
        }
        Ok(GamePreview {
            name: self.name.clone(),
            profiles,
            default_profile: self.default_profile,
            use_game_rules: self.use_game_rules,
            custom_filters,
        })
    }

    fn local_game(&self) -> Result<Game> {
        let id = Uuid::new_v4();
        let profile_ids: Vec<_> = game_import_profile_ids(id, self.profiles.len()).collect();
        let default_profile = profile_ids
            .get(self.default_profile)
            .context("游戏引用的配置无效")?
            .clone();
        let mut switching = self.switching.clone();
        remap_switching(&mut switching, |target| {
            profile_ids.get(target.parse::<usize>().ok()?).cloned()
        })?;
        Ok(Game {
            id: id.to_string(),
            name: self.name.trim().into(),
            profile_ids,
            default_profile: Some(default_profile),
            last_profile: None,
            use_game_rules: self.use_game_rules,
            switching,
        })
    }
}

fn decode_game(code: &str) -> Result<(Game, Vec<String>, GamePreview)> {
    let mut bundle: GameBundle =
        serde_json::from_str(&GAME_CODE.unpack(code)?).context("整组配置码格式无效")?;
    let preview = bundle.prepare_profiles()?;
    let game = bundle.local_game()?;
    Settings {
        games: vec![game.clone()],
        ..Settings::default()
    }
    .validate()?;
    Ok((game, bundle.profiles, preview))
}

#[tauri::command]
pub(crate) async fn copy_game_code(
    state: State<'_, AppState>,
    game_id: String,
    draft: Option<ProfileEntry>,
) -> Result<(), String> {
    let state = state.inner().clone();
    background(move || {
        let mut bundle = {
            let _operation = state.operations.lock().map_err(text)?;
            let settings = state.settings.lock().map_err(text)?;
            let game = settings
                .games
                .iter()
                .find(|g| g.id == game_id)
                .ok_or("游戏不存在")?;
            GameBundle::from_library(&state.store, game, draft.as_ref()).map_err(text)?
        };
        bundle.prepare_profiles().map_err(text)?;
        let code = GAME_CODE
            .pack(&serde_json::to_string(&bundle).map_err(text)?)
            .map_err(text)?;
        set_clipboard_string(&code).map_err(|_| "无法写入剪贴板".into())
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_shared_game(code: String) -> Result<GamePreview, String> {
    background(move || {
        decode_game(&code)
            .map(|(_, _, preview)| preview)
            .map_err(text)
    })
    .await
}

#[tauri::command]
pub(crate) async fn import_shared_game(
    state: State<'_, AppState>,
    code: String,
    name: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    background(move || {
        let (mut game, documents, _) = decode_game(&code).map_err(text)?;
        game.name = name.trim().into();
        let id = game.id.clone();
        let _operation = state.operations.lock().map_err(text)?;
        let mut settings = state.settings.lock().map_err(text)?;
        state
            .store
            .import_game(&mut settings, game, &documents)
            .map_err(text)?;
        Ok(id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use motion_core::{
        custom_filters::FilterConfig,
        shortcut::{ShortcutRule, ShortcutTrigger},
    };

    #[test]
    fn shares_the_whole_game_and_rebinds_rules_inside_each_imported_copy() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path().to_owned()).unwrap();
        let mut original = Game::new("game-a".into(), "Game".into());
        original.add("gyro");
        original.add("gamepad");
        original.default_profile = Some("gamepad".into());
        original.last_profile = Some("gyro".into());
        original.use_game_rules = true;
        original.switching = Some(SwitchPolicy {
            cursor_enabled: true,
            cursor_debounce_ms: 97,
            cursor_shown: SwitchTarget::Profile("gyro".into()),
            cursor_hidden: SwitchTarget::Restore,
            shortcut_rules: vec![ShortcutRule {
                trigger: ShortcutTrigger::Gamepad {
                    input: "south".into(),
                },
                target: "gamepad".into(),
                return_on_repeat: true,
            }],
        });
        let mut profile = decode_profile(include_str!("../../examples/hold-layer.toml")).unwrap();
        profile.name = "Unsaved draft".into();
        profile.gyro.motion.custom_filters.push(FilterConfig {
            name: "Shared script".into(),
            id: "shared".into(),
            source: "invalid script !".into(),
            ..Default::default()
        });
        let draft = ProfileEntry {
            id: "gyro".into(),
            profile,
        };
        let mut bundle = GameBundle::from_library(&store, &original, Some(&draft)).unwrap();
        // Neither an older sender nor a hand-edited code can enable an imported script.
        let code = GAME_CODE
            .pack(&serde_json::to_string(&bundle).unwrap())
            .unwrap();
        assert_eq!(bundle.prepare_profiles().unwrap().custom_filters, 1);
        assert!(
            !decode_profile(&bundle.profiles[0])
                .unwrap()
                .gyro
                .motion
                .custom_filters[0]
                .enabled
        );

        let (game, docs, preview) = decode_game(&code).unwrap();
        assert_eq!(preview.profiles.len(), 2);
        assert_eq!(preview.profiles[0].name, "Unsaved draft");
        assert_eq!(preview.profiles[0].layers, 2);
        assert_eq!(preview.custom_filters, 1);
        assert_ne!(game.id, original.id);
        assert_eq!(game.default_profile.as_ref(), Some(&game.profile_ids[1]));
        assert_eq!(game.last_profile, None);
        let rules = game.switching.as_ref().unwrap();
        assert_eq!(
            rules.cursor_shown,
            SwitchTarget::Profile(game.profile_ids[0].clone())
        );
        assert_eq!(rules.cursor_hidden, SwitchTarget::Restore);
        assert_eq!(rules.cursor_debounce_ms, 97);
        assert_eq!(rules.shortcut_rules[0].target, game.profile_ids[1]);
        let (second, _, _) = decode_game(&code).unwrap();
        assert!(second.profile_ids.iter().all(|id| !game.contains(id)));

        let mut settings = Settings {
            games: vec![original.clone()],
            ..Settings::default()
        };
        let files_before = store.profile_ids().unwrap().len();
        assert!(store
            .import_game(&mut settings, game.clone(), &docs)
            .is_err());
        assert_eq!(store.profile_ids().unwrap().len(), files_before);
        let mut game = game;
        game.name = "Game (2)".into();
        store
            .import_game(&mut settings, game.clone(), &docs)
            .unwrap();
        assert_eq!(store.settings().unwrap().games, [original, game.clone()]);
        let imported = store.load(&game.profile_ids[0]).unwrap().profile;
        assert_eq!(imported.layers[0].id, draft.profile.layers[0].id);
        assert!(!imported.gyro.motion.custom_filters[0].enabled);
        assert_ne!(store.load("gyro").unwrap().profile.name, imported.name);
    }

    #[test]
    fn rejects_external_targets_missing_defaults_and_invalid_members() {
        let mut bundle = GameBundle {
            name: "Game".into(),
            profiles: vec![include_str!("../../examples/blank.toml").into()],
            default_profile: 0,
            use_game_rules: true,
            switching: Some(SwitchPolicy {
                cursor_shown: SwitchTarget::Profile("external".into()),
                ..Settings::default().global_switching()
            }),
        };
        let code = |bundle: &GameBundle| {
            GAME_CODE
                .pack(&serde_json::to_string(bundle).unwrap())
                .unwrap()
        };
        assert!(decode_game(&code(&bundle)).is_err());
        bundle.use_game_rules = false;
        bundle.switching = None;
        bundle.default_profile = 1;
        assert!(decode_game(&code(&bundle)).is_err());
        bundle.default_profile = 0;
        bundle.profiles.push("broken toml = [".into());
        assert!(decode_game(&code(&bundle)).is_err());
    }
}
