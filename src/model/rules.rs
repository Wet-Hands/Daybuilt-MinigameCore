//! Per-round rules. Set in a mode file's `[rules]`, overridden per format
//! (`[formats.<name>.rules]`), and changeable at runtime by the mode plugin (`set_rule`).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameModeName {
    Survival,
    Adventure,
    Creative,
    Spectator,
}

/// Which blocks may be broken (by players, or by explosions).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BreakRule {
    /// Nothing (except `breakable` blocks, for players).
    None,
    /// Only blocks a player placed during this round (plus `breakable`).
    Placed,
    /// Anything inside the arena.
    Any,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeathDrops {
    /// Vanilla: items fall on the ground.
    Drop,
    /// Inventory is kept through death.
    Keep,
    /// Inventory is deleted.
    Clear,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutOfBounds {
    /// Counts as a death (void).
    Kill,
    /// Teleported back to their team spawn.
    Teleport,
    Ignore,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeUp {
    /// The round ends as a draw.
    Draw,
    /// The teams with the most players left win (ties share the win).
    MostAlive,
    /// Only a `time_up` event is sent; the mode plugin ends the round.
    Event,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WinRule {
    /// The round ends when one team (or none) has players left.
    LastTeam,
    /// The mode plugin ends the round. The core still ends it if everyone is out.
    Manual,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    /// Gamemode for players in the round.
    pub gamemode: GameModeName,
    /// May players place blocks.
    pub build: bool,
    /// Which blocks players may break.
    #[serde(rename = "break")]
    pub break_rule: BreakRule,
    /// Blocks that may always be broken, e.g. `["minecraft:snow_block", "*_bed"]`.
    /// `*` matches any text; names without a namespace get `minecraft:`.
    pub breakable: Vec<String>,
    /// Highest Y a block may be placed at.
    pub max_build_y: Option<i32>,
    pub hunger: bool,
    pub item_drop: bool,
    pub item_pickup: bool,
    /// Any damage at all.
    pub damage: bool,
    pub fall_damage: bool,
    pub pvp: bool,
    pub friendly_fire: bool,
    pub death_drops: DeathDrops,
    /// May players respawn after dying (teams and players can be switched off at runtime).
    pub respawn: bool,
    pub respawn_seconds: u32,
    /// Eliminated players watch in spectator mode; otherwise they go back to the lobby.
    pub spectate_on_elimination: bool,
    /// Players below this Y die. Unset: the bottom of the arena box.
    pub void_y: Option<f64>,
    pub out_of_bounds: OutOfBounds,
    /// Seconds; 0 = no limit.
    pub time_limit: u32,
    pub time_up: TimeUp,
    pub win: WinRule,
    /// Which blocks explosions may destroy.
    pub explosions: BreakRule,
    /// Water and lava may flow.
    pub fluid_flow: bool,
    /// Fire spread and burning blocks.
    pub fire_spread: bool,
    /// Grass/mycelium spread, leaves decay, ice/snow forming and melting, and so on.
    pub nature: bool,
    /// Seconds players are held in place at round start.
    pub start_freeze: u32,
    /// Kit given at round start when the player chose none.
    pub kit: Option<String>,
    pub kit_on_respawn: bool,
    /// The core announces deaths to the players of the game.
    pub death_messages: bool,
    /// Daybuilt PvP Manager classic (1.8) combat during the round. Unset = player's own setting.
    pub classic_combat: Option<bool>,
    /// Players in this game only chat with each other (and lobby players with the lobby).
    pub isolate_chat: bool,
    /// Seconds after a hit during which a death is credited to the attacker.
    pub kill_credit_seconds: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            gamemode: GameModeName::Survival,
            build: true,
            break_rule: BreakRule::Placed,
            breakable: Vec::new(),
            max_build_y: None,
            hunger: false,
            item_drop: true,
            item_pickup: true,
            damage: true,
            fall_damage: true,
            pvp: true,
            friendly_fire: false,
            death_drops: DeathDrops::Drop,
            respawn: false,
            respawn_seconds: 5,
            spectate_on_elimination: true,
            void_y: None,
            out_of_bounds: OutOfBounds::Kill,
            time_limit: 0,
            time_up: TimeUp::Draw,
            win: WinRule::LastTeam,
            explosions: BreakRule::Placed,
            fluid_flow: true,
            fire_spread: false,
            nature: false,
            start_freeze: 0,
            kit: None,
            kit_on_respawn: true,
            death_messages: true,
            classic_combat: None,
            isolate_chat: true,
            kill_credit_seconds: 15,
        }
    }
}

impl Rules {
    /// Builds rules from layered TOML tables (later layers win).
    pub fn from_layers(layers: &[&toml::Table]) -> Result<Self, String> {
        let mut table = toml::Table::new();
        for layer in layers {
            crate::model::toml_util::merge(&mut table, (*layer).clone());
        }
        let rules: Rules = table.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        rules.validate()?;
        Ok(rules)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.respawn_seconds > 3600 {
            return Err("respawn_seconds must be at most 3600".into());
        }
        if self.start_freeze > 600 {
            return Err("start_freeze must be at most 600".into());
        }
        Ok(())
    }

    /// Changes one rule by key, with a JSON value (used by the IPC `set_rule` op).
    pub fn set(&mut self, key: &str, value: serde_json::Value) -> Result<(), String> {
        let mut json = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        let map = json.as_object_mut().ok_or("rules are not an object")?;
        if !map.contains_key(key) {
            return Err(format!("unknown rule '{key}'"));
        }
        map.insert(key.to_string(), value);
        let new: Rules = serde_json::from_value(json).map_err(|e| format!("rule '{key}': {e}"))?;
        new.validate()?;
        *self = new;
        Ok(())
    }

    /// True if `block` (e.g. `minecraft:red_bed`) matches a `breakable` pattern.
    pub fn is_breakable(&self, block: &str) -> bool {
        self.breakable.iter().any(|p| block_matches(p, block))
    }
}

/// Glob match with `*`. Names without a namespace get `minecraft:`. Block state
/// properties (`[...]`) on `block` are ignored.
pub fn block_matches(pattern: &str, block: &str) -> bool {
    let block = block.split('[').next().unwrap_or(block);
    let block = if block.contains(':') {
        block.to_string()
    } else {
        format!("minecraft:{block}")
    };
    let pattern = if pattern.contains(':') {
        pattern.to_string()
    } else {
        format!("minecraft:{pattern}")
    };
    glob(&pattern, &block)
}

fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(at) = rest.find(part) {
            rest = &rest[at + part.len()..];
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_and_set() {
        let mode: toml::Table = toml::from_str("respawn = true\nbreak = \"any\"").unwrap();
        let format: toml::Table = toml::from_str("respawn = false").unwrap();
        let mut rules = Rules::from_layers(&[&mode, &format]).unwrap();
        assert!(!rules.respawn);
        assert_eq!(rules.break_rule, BreakRule::Any);
        rules.set("pvp", serde_json::json!(false)).unwrap();
        assert!(!rules.pvp);
        rules.set("break", serde_json::json!("none")).unwrap();
        assert_eq!(rules.break_rule, BreakRule::None);
        assert!(rules.set("nope", serde_json::json!(1)).is_err());
        assert!(rules.set("pvp", serde_json::json!("yes")).is_err());
        assert!(rules.pvp == false);
        let bad: toml::Table = toml::from_str("typo_rule = 1").unwrap();
        assert!(Rules::from_layers(&[&bad]).is_err());
    }

    #[test]
    fn breakable_patterns() {
        let rules = Rules {
            breakable: vec!["*_bed".into(), "minecraft:snow_block".into()],
            ..Rules::default()
        };
        assert!(rules.is_breakable("minecraft:red_bed"));
        assert!(rules.is_breakable("snow_block"));
        assert!(rules.is_breakable("minecraft:red_bed[part=head]"));
        assert!(!rules.is_breakable("minecraft:bedrock"));
        assert!(glob("a*b*c", "axxbyyc"));
        assert!(!glob("a*b*c", "axxbyy"));
    }
}
