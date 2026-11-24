//! Local game membership and switching preferences. Profile files remain portable.
use crate::{
    shortcut::{validate_shortcut_rules, ShortcutRule},
    storage::{validate_profile_id, Settings, SwitchTarget},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub profile_ids: Vec<String>,
    pub default_profile: Option<String>,
    pub last_profile: Option<String>,
    #[serde(default)]
    pub use_game_rules: bool,
    pub switching: Option<SwitchPolicy>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchPolicy {
    pub cursor_enabled: bool,
    pub cursor_debounce_ms: u32,
    pub cursor_shown: SwitchTarget,
    pub cursor_hidden: SwitchTarget,
    pub shortcut_rules: Vec<ShortcutRule>,
}

impl SwitchPolicy {
    pub fn retain_targets(&mut self, profiles: &[String]) {
        self.shortcut_rules.retain(|r| profiles.contains(&r.target));
        for target in [&mut self.cursor_shown, &mut self.cursor_hidden] {
            if matches!(target, SwitchTarget::Profile(id) if !profiles.contains(id)) {
                *target = SwitchTarget::None;
            }
        }
    }

    fn validate(&self, game: &Game) -> Result<()> {
        ensure!(
            self.cursor_debounce_ms <= 5000,
            "光标状态确认时间应在 0 到 5000 毫秒之间"
        );
        validate_shortcut_rules(&self.shortcut_rules)?;
        let shortcuts = self.shortcut_rules.iter().map(|r| &r.target);
        let cursor = [&self.cursor_shown, &self.cursor_hidden]
            .into_iter()
            .filter_map(|t| match t {
                SwitchTarget::Profile(id) => Some(id),
                _ => None,
            });
        ensure!(
            shortcuts
                .chain(cursor)
                .all(|id| game.profile_ids.contains(id)),
            "切换目标必须属于当前游戏"
        );
        Ok(())
    }
}

impl Game {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            profile_ids: vec![],
            default_profile: None,
            last_profile: None,
            use_game_rules: false,
            switching: None,
        }
    }

    pub fn add(&mut self, id: &str) {
        if !self.contains(id) {
            self.profile_ids.push(id.to_owned());
            self.default_profile.get_or_insert_with(|| id.to_owned());
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.profile_ids.iter().any(|member| member == id)
    }

    pub fn remove(&mut self, id: &str) {
        self.profile_ids.retain(|member| member != id);
        if self.default_profile.as_deref() == Some(id) {
            self.default_profile = self.profile_ids.first().cloned();
        }
        if self.last_profile.as_deref() == Some(id) {
            self.last_profile = None;
        }
        if let Some(policy) = &mut self.switching {
            policy.retain_targets(&self.profile_ids);
        }
    }
}

impl Settings {
    pub fn global_switching(&self) -> SwitchPolicy {
        SwitchPolicy {
            cursor_enabled: self.cursor_enabled,
            cursor_debounce_ms: self.cursor_debounce_ms,
            cursor_shown: self.cursor_shown.clone(),
            cursor_hidden: self.cursor_hidden.clone(),
            shortcut_rules: self.effective_shortcut_rules(),
        }
    }

    pub fn switching_for(&self, profile: Option<&str>) -> (Option<String>, SwitchPolicy) {
        let id = profile.unwrap_or(&self.selected_profile);
        if let Some(game) = self
            .games
            .iter()
            .find(|g| g.use_game_rules && g.contains(id))
        {
            if let Some(policy) = &game.switching {
                return (Some(game.id.clone()), policy.clone());
            }
        }
        (None, self.global_switching())
    }

    pub fn switching_history_changed(&self, next: &Settings, profile: Option<&str>) -> bool {
        let (scope, before) = self.switching_for(profile);
        let (next_scope, after) = next.switching_for(profile);
        // Changing only the debounce clock re-establishes the cursor baseline.
        // It does not change where an outstanding return should go.
        scope != next_scope
            || before.cursor_enabled != after.cursor_enabled
            || before.cursor_shown != after.cursor_shown
            || before.cursor_hidden != after.cursor_hidden
            || before.shortcut_rules != after.shortcut_rules
    }

    pub fn remember_manual_profile(&mut self, id: &str) {
        if let Some(game) = self.games.iter_mut().find(|g| g.contains(id)) {
            game.last_profile = Some(id.to_owned());
        }
    }

    pub fn forget_profile(&mut self, id: &str) {
        let mut replacement = None;
        if let Some(game) = self.games.iter_mut().find(|game| game.contains(id)) {
            game.remove(id);
            replacement = game
                .last_profile
                .as_ref()
                .or(game.default_profile.as_ref())
                .cloned();
        }
        if self.selected_profile == id {
            self.selected_profile = replacement.unwrap_or_default();
        }
        for target in [&mut self.cursor_shown, &mut self.cursor_hidden] {
            if matches!(target, SwitchTarget::Profile(profile) if profile == id) {
                *target = SwitchTarget::None;
            }
        }
        if self.shortcut_profile.as_deref() == Some(id) {
            self.shortcut_profile = None;
        }
        if let Some(rules) = &mut self.shortcut_rules {
            rules.retain(|rule| rule.target != id);
        }
    }

    pub(crate) fn validate_games(&self) -> Result<()> {
        let mut ids = HashSet::new();
        let mut members = HashSet::new();
        let mut names = HashSet::new();
        for game in &self.games {
            validate_profile_id(&game.id)?;
            ensure!(ids.insert(&game.id), "游戏 ID 重复");
            ensure!(
                !game.name.trim().is_empty() && game.name.chars().count() <= 100,
                "游戏名称应为 1 到 100 个字符"
            );
            ensure!(
                names.insert(game.name.trim().to_lowercase()),
                "已有同名游戏"
            );
            for member in &game.profile_ids {
                validate_profile_id(member)?;
                ensure!(members.insert(member), "配置只能属于一个游戏");
            }
            ensure!(
                game.default_profile.is_none() == game.profile_ids.is_empty()
                    && game
                        .default_profile
                        .iter()
                        .chain(&game.last_profile)
                        .all(|id| game.contains(id)),
                "游戏引用的配置无效"
            );
            ensure!(
                !game.use_game_rules || game.switching.is_some(),
                "游戏切换规则缺失"
            );
            if let Some(policy) = &game.switching {
                policy.validate(game)?;
            }
        }
        Ok(())
    }
}
