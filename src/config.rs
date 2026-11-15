//! Main configuration (`config.toml`). The embedded default is merged under the
//! owner's file, so missing keys get defaults.

use serde::Deserialize;

use crate::model::{WorldLoc, rules::GameModeName, toml_util};

pub const DEFAULT_TOML: &str = include_str!("../config/config.toml");

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub lobby: Lobby,
    pub timing: Timing,
    pub spawners: Spawners,
    pub reset: Reset,
    pub protection: Protection,
    pub pvp_manager: PvpManager,
    pub placeholders: Placeholders,
    pub sidebar: Sidebar,
    pub messages: Messages,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Lobby {
    #[serde(default)]
    pub spawn: Option<WorldLoc>,
    pub teleport_on_join: bool,
    pub gamemode: GameModeName,
    pub clear_inventory: bool,
    pub commands: Vec<String>,
    pub auto_join: String,
    pub protect: bool,
    pub no_hunger: bool,
    pub no_damage: bool,
    pub no_item_drop: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub countdown_seconds: u32,
    pub full_countdown_seconds: u32,
    pub ending_seconds: u32,
    pub reset_blocks_per_tick: u32,
    pub position_check_ticks: u32,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpawnerMode {
    Stockpile,
    Command,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Spawners {
    pub mode: SpawnerMode,
    pub pickup_radius: f64,
    pub command: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Reset {
    pub crash_safe: bool,
    pub cleanup_entities: Vec<String>,
    pub explosion_radius: u32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Protection {
    pub idle_arenas: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PvpManager {
    pub enabled: bool,
    pub force_pvp: bool,
    pub untag_on_return: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Placeholders {
    pub enabled: bool,
    pub namespace: String,
    pub cache_ms: u64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Sidebar {
    pub enabled: bool,
    pub title: String,
    pub waiting: Vec<String>,
    pub running: Vec<String>,
    pub ending: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Messages {
    pub prefix: String,
    pub joined: String,
    pub left: String,
    pub waiting_status: String,
    pub countdown_status: String,
    pub countdown: String,
    pub countdown_title: String,
    pub countdown_cancelled: String,
    pub round_start_title: String,
    pub round_start_subtitle: String,
    pub death: String,
    pub killed: String,
    pub final_death: String,
    pub respawn_title: String,
    pub respawn_subtitle: String,
    pub eliminated: String,
    pub eliminated_title: String,
    pub team_eliminated: String,
    pub win_title: String,
    pub lose_title: String,
    pub draw_title: String,
    pub winners: String,
    pub draw: String,
    pub time_left: String,
    pub not_in_game: String,
    pub already_in_game: String,
    pub no_arena: String,
    pub unknown_mode: String,
    pub joined_game: String,
    pub left_game: String,
    pub kit_selected: String,
    pub kit_unknown: String,
    pub kit_denied: String,
    pub team_selected: String,
    pub team_full: String,
}

impl Config {
    /// Parses the owner's file (if any) over the embedded default.
    pub fn parse(user: Option<&str>) -> Result<Self, String> {
        let table = toml_util::merged(DEFAULT_TOML, user)?;
        let cfg: Config = table.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        let ns = &self.placeholders.namespace;
        if ns.is_empty()
            || !ns.starts_with(|c: char| c.is_ascii_lowercase())
            || !ns.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            || ["player", "server", "pie"].contains(&ns.as_str())
        {
            return Err(format!(
                "placeholders.namespace '{ns}' must be lowercase letters/digits, start with a letter, and not be player/server/pie"
            ));
        }
        if self.timing.position_check_ticks == 0 || self.timing.reset_blocks_per_tick == 0 {
            return Err("timing.position_check_ticks and reset_blocks_per_tick must be at least 1".into());
        }
        if !self.lobby.auto_join.is_empty() {
            let mode = self.lobby.auto_join.split(':').next().unwrap_or("");
            crate::model::ids::mode(mode).map_err(|e| format!("lobby.auto_join: {e}"))?;
        }
        Ok(())
    }

    /// The embedded default. Parsing it is covered by a test, so this cannot fail.
    pub fn default_config() -> Self {
        #[allow(clippy::expect_used)]
        Self::parse(None).expect("embedded default config parses")
    }

    /// Text for an auto-join entry: (mode, optional format).
    pub fn auto_join(&self) -> Option<(String, Option<String>)> {
        let s = self.lobby.auto_join.trim();
        if s.is_empty() {
            return None;
        }
        let mut it = s.splitn(2, ':');
        let mode = it.next()?.to_string();
        let format = it.next().map(str::to_string);
        Some((mode, format))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses() {
        let cfg = Config::parse(None).unwrap();
        assert_eq!(cfg.placeholders.namespace, "mg");
        assert!(cfg.lobby.spawn.is_none());
        assert_eq!(cfg.spawners.mode, SpawnerMode::Stockpile);
    }

    #[test]
    fn user_overrides_and_validation() {
        let cfg = Config::parse(Some(
            "[lobby]\nspawn = { world = \"world\", x = 1.0, y = 2.0, z = 3.0 }\nauto_join = \"bedwars:solo\"",
        ))
        .unwrap();
        assert_eq!(cfg.lobby.spawn.as_ref().unwrap().y, 2.0);
        assert_eq!(
            cfg.auto_join(),
            Some(("bedwars".into(), Some("solo".into())))
        );
        assert!(Config::parse(Some("[placeholders]\nnamespace = \"m_g\"")).is_err());
        assert!(Config::parse(Some("[lobby]\ntypo = 1")).is_err());
    }
}
