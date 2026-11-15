//! Arena definitions: one TOML file per arena in `arenas/<id>.toml`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{BlockPos, Cuboid, Location, ids, text};
use minigamecore_client::{ArenaInfo, ArenaTeamInfo, RegionInfo};

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TeamDef {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "white")]
    pub color: String,
    #[serde(default)]
    pub spawns: Vec<Location>,
}

fn white() -> String {
    "white".into()
}

impl TeamDef {
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            let mut c = self.id.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        } else {
            self.name.clone()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpawnerDef {
    /// Group name; the mode plugin changes groups at runtime (`spawner_group`).
    pub group: String,
    /// Item id, e.g. `minecraft:iron_ingot`.
    pub item: String,
    pub at: Location,
    /// Ticks between drops.
    pub interval: u32,
    #[serde(default = "one")]
    pub amount: u32,
    /// Most items waiting at the spawner before it pauses.
    #[serde(default = "default_cap")]
    pub cap: u32,
    /// Shown on a floating label above the spawner (`&` codes). Empty = no label.
    #[serde(default)]
    pub label: String,
    /// Team this spawner belongs to, for mode plugins (the core does not use it).
    #[serde(default)]
    pub team: Option<String>,
}

fn one() -> u32 {
    1
}
fn default_cap() -> u32 {
    64
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegionDef {
    pub name: String,
    pub pos1: BlockPos,
    pub pos2: BlockPos,
    /// Overrides the round's `build` rule inside the region.
    #[serde(default)]
    pub build: Option<bool>,
    /// Overrides block breaking inside the region (`false` = nothing may be broken).
    #[serde(default, rename = "break")]
    pub break_blocks: Option<bool>,
    #[serde(default)]
    pub pvp: Option<bool>,
    #[serde(default)]
    pub damage: Option<bool>,
    /// Send `region_entered`/`region_left` events for this region.
    #[serde(default = "yes")]
    pub track: bool,
}

impl RegionDef {
    pub fn cuboid(&self) -> Cuboid {
        Cuboid::new(self.pos1, self.pos2)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArenaDef {
    #[serde(skip)]
    pub id: String,
    pub world: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Modes this arena can host. Empty = any mode whose formats fit.
    #[serde(default)]
    pub modes: Vec<String>,
    /// Optional limit to specific `mode:format` pairs.
    #[serde(default)]
    pub formats: Vec<String>,
    pub pos1: BlockPos,
    pub pos2: BlockPos,
    /// 0 = no cap beyond the format's.
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub spectator: Option<Location>,
    /// Where players wait before the round. Unset: they stay in the server lobby.
    #[serde(default)]
    pub waiting: Option<Location>,
    /// Spawn points for solo formats.
    #[serde(default)]
    pub spawns: Vec<Location>,
    #[serde(default)]
    pub teams: Vec<TeamDef>,
    #[serde(default)]
    pub spawners: Vec<SpawnerDef>,
    #[serde(default)]
    pub regions: Vec<RegionDef>,
    #[serde(default)]
    pub points: BTreeMap<String, Vec<Location>>,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

impl ArenaDef {
    pub fn new(id: &str, world: &str, at: BlockPos) -> Self {
        Self {
            id: id.into(),
            world: world.into(),
            enabled: false,
            modes: Vec::new(),
            formats: Vec::new(),
            pos1: at,
            pos2: at,
            max_players: 0,
            spectator: None,
            waiting: None,
            spawns: Vec::new(),
            teams: Vec::new(),
            spawners: Vec::new(),
            regions: Vec::new(),
            points: BTreeMap::new(),
            values: BTreeMap::new(),
        }
    }

    pub fn parse(id: &str, text: &str) -> Result<Self, String> {
        ids::arena(id)?;
        let mut def: ArenaDef = toml::from_str(text).map_err(|e| e.to_string())?;
        def.id = id.to_string();
        def.validate()?;
        Ok(def)
    }

    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        for m in &self.modes {
            ids::mode(m)?;
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &self.teams {
            ids::name(&t.id, "team id")?;
            if !seen.insert(&t.id) {
                return Err(format!("team '{}' is listed twice", t.id));
            }
            if !text::is_color(&t.color) {
                return Err(format!("team '{}': unknown colour '{}'", t.id, t.color));
            }
        }
        for s in &self.spawners {
            ids::name(&s.group, "spawner group")?;
            if s.interval == 0 {
                return Err(format!("spawner in group '{}' has interval 0", s.group));
            }
        }
        for r in &self.regions {
            ids::name(&r.name, "region name")?;
        }
        for p in self.points.keys() {
            ids::name(p, "point name")?;
        }
        Ok(())
    }

    pub fn bounds(&self) -> Cuboid {
        Cuboid::new(self.pos1, self.pos2)
    }

    pub fn team(&self, id: &str) -> Option<&TeamDef> {
        self.teams.iter().find(|t| t.id == id)
    }

    /// Teams that have at least one spawn point.
    pub fn usable_teams(&self) -> impl Iterator<Item = &TeamDef> {
        self.teams.iter().filter(|t| !t.spawns.is_empty())
    }

    /// Spawn points for solo formats: `spawns`, or else every team spawn.
    pub fn solo_spawns(&self) -> Vec<Location> {
        if !self.spawns.is_empty() {
            self.spawns.clone()
        } else {
            self.teams.iter().flat_map(|t| t.spawns.clone()).collect()
        }
    }

    pub fn allows_mode(&self, mode: &str) -> bool {
        self.modes.is_empty() || self.modes.iter().any(|m| m == mode)
    }

    pub fn allows_format(&self, mode: &str, format: &str) -> bool {
        self.allows_mode(mode)
            && (self.formats.is_empty()
                || self
                    .formats
                    .iter()
                    .any(|f| f == &format!("{mode}:{format}")))
    }

    /// Names of the regions containing `l`.
    pub fn regions_at(&self, l: &Location) -> Vec<String> {
        self.regions
            .iter()
            .filter(|r| r.cuboid().contains(l))
            .map(|r| r.name.clone())
            .collect()
    }

    pub fn info(&self) -> ArenaInfo {
        let b = self.bounds();
        ArenaInfo {
            id: self.id.clone(),
            world: self.world.clone(),
            min: b.min,
            max: b.max,
            modes: self.modes.clone(),
            teams: self
                .teams
                .iter()
                .map(|t| ArenaTeamInfo {
                    id: t.id.clone(),
                    name: t.display_name(),
                    color: t.color.clone(),
                    spawns: t.spawns.clone(),
                })
                .collect(),
            spawns: self.spawns.clone(),
            spectator: self.spectator,
            waiting: self.waiting,
            regions: self
                .regions
                .iter()
                .map(|r| {
                    let c = r.cuboid();
                    RegionInfo {
                        name: r.name.clone(),
                        min: c.min,
                        max: c.max,
                    }
                })
                .collect(),
            points: self.points.clone(),
            values: self.values.clone(),
        }
    }

    /// Problems that stop the arena from being used (shown by `/mga arena <id> info`).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.pos1 == self.pos2 {
            out.push("bounds not set (pos1/pos2)".into());
        }
        if self.teams.iter().all(|t| t.spawns.is_empty()) && self.spawns.is_empty() {
            out.push("no spawn points".into());
        }
        if self.spectator.is_none() {
            out.push("no spectator spawn (team spawns are used)".into());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"
world = "world"
modes = ["bedwars"]
pos1 = { x = -50, y = 0, z = -50 }
pos2 = { x = 50, y = 120, z = 50 }
spectator = { x = 0.5, y = 100, z = 0.5 }

[[teams]]
id = "red"
color = "red"
spawns = [{ x = 30.5, y = 65, z = 0.5, yaw = 90.0 }]

[[teams]]
id = "blue"
color = "blue"
spawns = [{ x = -30.5, y = 65, z = 0.5 }]

[[spawners]]
group = "iron"
item = "minecraft:iron_ingot"
at = { x = 32.5, y = 65, z = 0.5 }
interval = 20
team = "red"

[[regions]]
name = "red_base"
pos1 = { x = 25, y = 60, z = -5 }
pos2 = { x = 35, y = 80, z = 5 }

[points]
bed = [{ x = 28, y = 65, z = 0 }]

[values]
bed_red = "28 65 0"
"#;

    #[test]
    fn parse_and_round_trip() {
        let a = ArenaDef::parse("castle", SAMPLE).unwrap();
        assert_eq!(a.id, "castle");
        assert_eq!(a.teams[0].display_name(), "Red");
        assert_eq!(a.usable_teams().count(), 2);
        assert_eq!(a.solo_spawns().len(), 2);
        assert_eq!(
            a.regions_at(&Location::new(30.0, 65.0, 0.0)),
            vec!["red_base".to_string()]
        );
        let text = a.to_toml().unwrap();
        let b = ArenaDef::parse("castle", &text).unwrap();
        assert_eq!(a, b);
        assert!(a.allows_format("bedwars", "solo"));
        assert!(!a.allows_mode("spleef"));
        assert!(a.problems().is_empty());
    }

    #[test]
    fn rejects_bad_data() {
        let dup = SAMPLE.replace("id = \"blue\"", "id = \"red\"");
        assert!(ArenaDef::parse("castle", &dup).is_err());
        assert!(ArenaDef::parse("Castle", SAMPLE).is_err());
        let typo = format!("{SAMPLE}\nwrold = 1\n");
        assert!(ArenaDef::parse("castle", &typo).is_err());
    }
}
