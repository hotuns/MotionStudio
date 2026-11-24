use crate::{background, text, AppState};
use motion_core::{
    game_library::{Game, SwitchPolicy},
    profile::Profile,
    storage::{ProfileEntry, Settings},
};
use serde::Deserialize;
use tauri::State;

fn game_mut<'a>(settings: &'a mut Settings, id: &str) -> Result<&'a mut Game, String> {
    settings
        .games
        .iter_mut()
        .find(|g| g.id == id)
        .ok_or_else(|| "游戏不存在".into())
}

fn destination(
    settings: &mut Settings,
    game_id: Option<String>,
    new_game_name: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(name) = new_game_name {
        if game_id.is_some() {
            return Err("请选择一个目标游戏".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        settings
            .games
            .push(Game::new(id.clone(), name.trim().into()));
        Ok(Some(id))
    } else {
        if let Some(id) = &game_id {
            game_mut(settings, id)?;
        }
        Ok(game_id)
    }
}

pub(crate) fn check_destination(
    state: &AppState,
    game_id: Option<String>,
    new_game_name: Option<String>,
) -> Result<(), String> {
    if game_id.is_some() || new_game_name.is_some() {
        let mut candidate = state.settings.lock().map_err(text)?.clone();
        destination(&mut candidate, game_id, new_game_name)?;
        candidate.validate().map_err(text)?;
    }
    Ok(())
}

// Callers hold the workspace operation lock. The profile keeps its standalone
// TOML format. Only newly allocated files can be rolled back on an index failure.
pub(crate) fn insert_profile(
    state: &AppState,
    profile: Profile,
    game_id: Option<String>,
    new_game_name: Option<String>,
) -> Result<ProfileEntry, String> {
    let entry = ProfileEntry {
        id: uuid::Uuid::new_v4().to_string(),
        profile,
    };
    if game_id.is_none() && new_game_name.is_none() {
        state.store.save(&entry).map_err(text)?;
        return Ok(entry);
    }
    let mut current = state.settings.lock().map_err(text)?;
    let mut next = current.clone();
    if let Some(id) = destination(&mut next, game_id, new_game_name)? {
        game_mut(&mut next, &id)?.add(&entry.id);
    }
    next.validate().map_err(text)?;
    state.store.save(&entry).map_err(text)?;
    if let Err(error) = state.store.save_settings(&next) {
        state
            .store
            .delete(&entry.id)
            .map_err(|rollback| format!("{error} ({rollback})"))?;
        return Err(text(error));
    }
    *current = next;
    Ok(entry)
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LibraryEdit {
    Rename {
        id: String,
        name: String,
    },
    Move {
        profile_ids: Vec<String>,
        game_id: Option<String>,
        new_game_name: Option<String>,
    },
    SetDefault {
        id: String,
        profile_id: String,
    },
    EnableRules {
        id: String,
        enabled: bool,
    },
    SetRules {
        id: String,
        switching: SwitchPolicy,
    },
    Ungroup {
        id: String,
    },
}

#[tauri::command]
pub(crate) async fn edit_game_library(
    state: State<'_, AppState>,
    edit: LibraryEdit,
) -> Result<Settings, String> {
    let state = state.inner().clone();
    background(move || {
        let _operation = state.operations.lock().map_err(text)?;
        let mut current = state.settings.lock().map_err(text)?;
        let mut next = current.clone();
        match edit {
            LibraryEdit::Rename { id, name } => game_mut(&mut next, &id)?.name = name.trim().into(),
            LibraryEdit::Move {
                profile_ids,
                game_id,
                new_game_name,
            } => {
                if profile_ids.is_empty() {
                    return Err("请先选择配置".into());
                }
                let target = destination(&mut next, game_id, new_game_name)?;
                for id in profile_ids {
                    // Moving a profile does not require compiling its filters.
                    std::fs::File::open(state.store.profile_path(&id).map_err(text)?)
                        .map_err(text)?;
                    for game in &mut next.games {
                        if Some(&game.id) == target.as_ref() {
                            game.add(&id);
                        } else if game.contains(&id) {
                            game.remove(&id);
                        }
                    }
                    if id == next.selected_profile {
                        next.remember_manual_profile(&id);
                    }
                }
            }
            LibraryEdit::SetDefault { id, profile_id } => {
                game_mut(&mut next, &id)?.default_profile = Some(profile_id)
            }
            LibraryEdit::EnableRules { id, enabled } => {
                let mut inherited = next.global_switching();
                let game = game_mut(&mut next, &id)?;
                if enabled && game.switching.is_none() {
                    inherited.retain_targets(&game.profile_ids);
                    game.switching = Some(inherited);
                }
                game.use_game_rules = enabled;
            }
            LibraryEdit::SetRules { id, switching } => {
                let game = game_mut(&mut next, &id)?;
                if !game.use_game_rules {
                    return Err("请先启用游戏专用规则".into());
                }
                game.switching = Some(switching);
            }
            LibraryEdit::Ungroup { id } => {
                game_mut(&mut next, &id)?;
                next.games.retain(|g| g.id != id);
            }
        }
        state.store.save_settings(&next).map_err(text)?;
        let running = state
            .client
            .snapshot
            .lock()
            .map_err(text)?
            .status
            .profile_id
            .clone();
        if current.switching_history_changed(&next, running.as_deref()) {
            state.history.lock().map_err(text)?.clear();
        }
        *current = next.clone();
        Ok(next)
    })
    .await
}

#[tauri::command]
pub(crate) async fn duplicate_profile(
    state: State<'_, AppState>,
    id: String,
    name: String,
    draft: Option<Profile>,
) -> Result<ProfileEntry, String> {
    let state = state.inner().clone();
    background(move || {
        let _operation = state.operations.lock().map_err(text)?;
        let saved = state.store.load(&id).map_err(text)?;
        let mut profile = draft.unwrap_or(saved.profile);
        let game_id = state
            .settings
            .lock()
            .map_err(text)?
            .games
            .iter()
            .find(|game| game.contains(&id))
            .map(|game| game.id.clone());
        profile.name = name.trim().into();
        insert_profile(&state, profile, game_id, None)
    })
    .await
}
