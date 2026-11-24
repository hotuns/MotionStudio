use super::{atomic_replace, atomic_write, read_small, Settings, Store, MAX_PROFILE_BYTES};
use crate::game_library::Game;
use anyhow::{anyhow, ensure, Context, Result};
use std::{fs, io::ErrorKind};
use uuid::Uuid;

const PENDING_SETTINGS: &str = "pending-game-import.json";

pub fn game_import_profile_ids(game_id: Uuid, count: usize) -> impl Iterator<Item = String> {
    (0..count).map(move |index| format!("{game_id}-{index}"))
}

fn validate_new_ids(game: &Game) -> Result<()> {
    let batch = Uuid::parse_str(&game.id)?;
    ensure!(
        !game.profile_ids.is_empty()
            && game_import_profile_ids(batch, game.profile_ids.len())
                .eq(game.profile_ids.iter().cloned()),
        "Invalid game import manifest"
    );
    Ok(())
}

impl Store {
    pub(super) fn pending_game_import(&self) -> Result<Option<Game>> {
        let pending = self.root.join(PENDING_SETTINGS);
        if !pending.try_exists()? {
            return Ok(None);
        }
        let mut planned: Settings = serde_json::from_str(&read_small(&pending)?)?;
        let game = planned.games.pop().context("Missing game import manifest")?;
        validate_new_ids(&game)?;
        Ok(Some(game))
    }

    /// The pending file is the next settings file. Its atomic rename both
    /// commits the batch and removes the rollback record, even across a restart.
    pub fn recover_game_import(&self) -> Result<()> {
        let Some(game) = self.pending_game_import()? else {
            return Ok(());
        };
        for id in &game.profile_ids {
            match fs::remove_file(self.profile_path(id)?) {
                Ok(()) => (),
                Err(error) if error.kind() == ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        fs::remove_file(self.root.join(PENDING_SETTINGS))?;
        Ok(())
    }

    /// Callers hold the workspace operation and settings locks and have already
    /// validated the profile documents in the same order as the game members.
    pub fn import_game(
        &self,
        current: &mut Settings,
        game: Game,
        documents: &[String],
    ) -> Result<()> {
        self.recover_game_import()?;
        validate_new_ids(&game)?;
        ensure!(
            game.profile_ids.len() == documents.len(),
            "Missing game profiles"
        );
        let mut paths = Vec::with_capacity(documents.len());
        for (id, document) in game.profile_ids.iter().zip(documents) {
            let path = self.profile_path(id)?;
            ensure!(!path.try_exists()?, "Imported profile ID already exists");
            ensure!(document.len() <= MAX_PROFILE_BYTES, "配置文件超过 512 KB");
            paths.push(path);
        }
        let mut next = current.clone();
        next.games.push(game);
        let pending = self.root.join(PENDING_SETTINGS);
        atomic_write(&pending, &Self::settings_bytes(&next)?)?;
        let publish = || -> Result<()> {
            for (path, document) in paths.iter().zip(documents) {
                atomic_write(path, document.as_bytes())?;
            }
            self.backup_settings()?;
            atomic_replace(&pending, &self.root.join("settings.json"))
        };
        if let Err(error) = publish() {
            self.recover_game_import()
                .map_err(|rollback| anyhow!("{error:#} ({rollback:#})"))?;
            return Err(error);
        }
        *current = next;
        Ok(())
    }
}
