//! Mode definitions: `modes/<mode>.toml`. A mode is a game type (bedwars, spleef, ...).
//! It has one or more formats (team layouts) and the rules, kits, schedule, sidebar and
//! rewards that apply to its rounds. A mode can run on config alone; a mode plugin
//! adds the logic the config can't express.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ids, rules::Rules};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamMode {
    /// Players are put into the arena's teams.
    Teams,
    /// Every player is their own team (free-for-all).
    Solo,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct FormatFile {
    #[serde(default = "teams_mode")]
    pub team_mode: TeamMode,
    #[serde(default = "one")]
    pub team_size: u32,
    /// Number of teams (for `solo`: the most players).
    pub teams: u32,
    /// Players needed to start the countdown.
    #[serde(default = "two")]
    pub min_players: u32,
    /// Players may choose a team with `/mg team`.
    #[serde(default)]
    pub team_choice: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub rules: toml::Table,
}

fn teams_mode() -> TeamMode {
    TeamMode::Teams
}
fn one() -> u32 {
    1
}
fn two() -> u32 {
    2
}

/// A format with its rules resolved.
#[derive(Debug, Clone)]
pub struct FormatDef {
    pub name: String,
    pub display_name: String,
    pub team_mode: TeamMode,
    pub team_size: u32,
    pub teams: u32,
    pub min_players: u32,
    pub team_choice: bool,
    pub rules: Rules,
}

impl FormatDef {
    pub fn max_players(&self) -> u32 {
        match self.team_mode {
            TeamMode::Teams => self.teams * self.team_size,
            TeamMode::Solo => self.teams,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDef {
    /// Seconds after the round starts.
    pub at: u32,
    pub id: String,
    /// Shown as the sidebar's next event. Empty = hidden.
    #[serde(default)]
    pub label: String,
    /// Built-in action: `spawner_interval:<group>:<ticks>`, `spawner_amount:<group>:<n>`,
    /// `spawner_on:<group>`, `spawner_off:<group>`, `rule:<key>:<json>`, `no_respawn`,
    /// `end_draw`, `broadcast:<text>`. Empty = only the `timer` event is sent.
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SidebarDef {
    pub title: String,
    pub waiting: Vec<String>,
    pub running: Vec<String>,
    pub ending: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RewardsDef {
    /// Console commands per winning player. Placeholders: {player} {uuid} {mode} {format}
    /// {arena} {kills} {deaths} and {counter:<key>}.
    pub win: Vec<String>,
    pub loss: Vec<String>,
    /// Every player at round end.
    pub played: Vec<String>,
    /// Per kill, for the killer; also {victim}.
    pub kill: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ModeFile {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    default_format: Option<String>,
    #[serde(default)]
    countdown_seconds: Option<u32>,
    #[serde(default)]
    full_countdown_seconds: Option<u32>,
    #[serde(default)]
    ending_seconds: Option<u32>,
    #[serde(default)]
    rules: toml::Table,
    formats: BTreeMap<String, FormatFile>,
    /// Kits players may pick with `/mg kit`.
    #[serde(default)]
    kits: Vec<String>,
    #[serde(default)]
    schedule: Vec<ScheduleDef>,
    #[serde(default)]
    sidebar: Option<SidebarDef>,
    #[serde(default)]
    rewards: RewardsDef,
    /// Extra entity types removed from the arena at reset (e.g. `zombie`).
    #[serde(default)]
    cleanup_entities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModeDef {
    pub id: String,
    pub display_name: String,
    pub default_format: String,
    pub countdown_seconds: Option<u32>,
    pub full_countdown_seconds: Option<u32>,
    pub ending_seconds: Option<u32>,
    pub formats: BTreeMap<String, FormatDef>,
    pub kits: Vec<String>,
    pub schedule: Vec<ScheduleDef>,
    pub sidebar: Option<SidebarDef>,
    pub rewards: RewardsDef,
    pub cleanup_entities: Vec<String>,
}

impl ModeDef {
    pub fn parse(id: &str, text: &str) -> Result<Self, String> {
        ids::mode(id)?;
        let file: ModeFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if file.formats.is_empty() {
            return Err("a mode needs at least one [formats.<name>]".into());
        }
        let mut formats = BTreeMap::new();
        for (name, f) in file.formats {
            ids::name(&name, "format name")?;
            if f.teams == 0 || f.team_size == 0 {
                return Err(format!("format '{name}': teams and team_size must be at least 1"));
            }
            let rules = Rules::from_layers(&[&file.rules, &f.rules])
                .map_err(|e| format!("format '{name}' rules: {e}"))?;
            let def = FormatDef {
                display_name: f.display_name.unwrap_or_else(|| name.clone()),
                name: name.clone(),
                team_mode: f.team_mode,
                team_size: if f.team_mode == TeamMode::Solo { 1 } else { f.team_size },
                teams: f.teams,
                min_players: f.min_players.max(1),
                team_choice: f.team_choice && f.team_mode == TeamMode::Teams,
                rules,
            };
            if def.min_players > def.max_players() {
                return Err(format!(
                    "format '{name}': min_players ({}) is more than the format holds ({})",
                    def.min_players,
                    def.max_players()
                ));
            }
            formats.insert(name, def);
        }
        let default_format = match file.default_format {
            Some(f) if formats.contains_key(&f) => f,
            Some(f) => return Err(format!("default_format '{f}' is not a format")),
            None => formats.keys().next().cloned().unwrap_or_default(),
        };
        let mut seen = std::collections::BTreeSet::new();
        for s in &file.schedule {
            ids::name(&s.id, "schedule id")?;
            if !seen.insert(&s.id) {
                return Err(format!("schedule id '{}' is listed twice", s.id));
            }
        }
        for k in &file.kits {
            ids::kit(k)?;
        }
        Ok(Self {
            id: id.into(),
            display_name: if file.display_name.is_empty() {
                id.into()
            } else {
                file.display_name
            },
            default_format,
            countdown_seconds: file.countdown_seconds,
            full_countdown_seconds: file.full_countdown_seconds,
            ending_seconds: file.ending_seconds,
            formats,
            kits: file.kits,
            schedule: file.schedule,
            sidebar: file.sidebar,
            rewards: file.rewards,
            cleanup_entities: file.cleanup_entities,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::rules::BreakRule;

    pub const SPLEEF: &str = include_str!("../../config/modes/spleef.toml");

    #[test]
    fn parses_bundled_example() {
        let m = ModeDef::parse("spleef", SPLEEF).unwrap();
        assert_eq!(m.default_format, "solo");
        let f = &m.formats["solo"];
        assert_eq!(f.team_mode, TeamMode::Solo);
        assert_eq!(f.rules.break_rule, BreakRule::None);
        assert!(f.rules.is_breakable("minecraft:snow_block"));
    }

    #[test]
    fn format_overrides_and_errors() {
        let text = r#"
[rules]
respawn = true
[formats.solo]
teams = 8
[formats.duo]
team_size = 2
teams = 4
min_players = 4
rules = { respawn = false }
"#;
        let m = ModeDef::parse("bedwars", text).unwrap();
        assert!(m.formats["solo"].rules.respawn);
        assert!(!m.formats["duo"].rules.respawn);
        assert_eq!(m.formats["duo"].max_players(), 8);
        assert_eq!(m.default_format, "duo");
        assert!(ModeDef::parse("bedwars", "[formats.x]\nteams = 1\nmin_players = 3").is_err());
        assert!(ModeDef::parse("bedwars", "display_name = \"x\"").is_err());
    }
}
