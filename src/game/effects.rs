//! Host actions the pure game logic asks for. The game code never calls the host;
//! it returns these and `host::apply` carries them out after the state lock is released.

use crate::model::{BlockPos, WorldLoc, rules::GameModeName};

#[derive(Debug, Clone, PartialEq)]
pub struct TeamSpec {
    /// Scoreboard team name (unique per arena).
    pub key: String,
    pub display: String,
    pub color: String,
    pub friendly_fire: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Teleport {
        player: String,
        to: WorldLoc,
    },
    /// Teleport to the lobby spawn and apply the lobby gamemode/inventory/commands.
    ToLobby {
        player: String,
    },
    /// Clear inventory and effects, refill health and food, put out fire, set gamemode.
    Reset {
        player: String,
        gamemode: GameModeName,
        clear_inventory: bool,
    },
    Gamemode {
        player: String,
        mode: GameModeName,
    },
    Kit {
        player: String,
        kit: String,
    },
    Freeze {
        player: String,
        frozen: bool,
    },
    Chat {
        players: Vec<String>,
        text: String,
    },
    ActionBar {
        players: Vec<String>,
        text: String,
    },
    Title {
        players: Vec<String>,
        title: String,
        subtitle: String,
        stay_ticks: i32,
    },
    Sound {
        players: Vec<String>,
        sound: String,
        pitch: f32,
    },
    Console {
        command: String,
    },
    /// Respawn a player who died for real (skips the death screen), a tick later.
    ForceRespawn {
        player: String,
    },
    CreateTeams {
        world: String,
        teams: Vec<TeamSpec>,
    },
    JoinTeam {
        world: String,
        key: String,
        player_name: String,
    },
    LeaveTeam {
        world: String,
        key: String,
        player_name: String,
    },
    RemoveTeams {
        world: String,
        keys: Vec<String>,
    },
    SidebarClear {
        player: String,
    },
    /// Read the original block, record it for `game`, then set the new block.
    SetBlock {
        game: String,
        world: String,
        pos: BlockPos,
        block: String,
    },
    /// Run the spawner drop command once.
    SpawnerDrop {
        world: String,
        x: f64,
        y: f64,
        z: f64,
        item: String,
        count: u32,
    },
    /// Put blocks back without recording them (explosion damage that was not allowed).
    PutBlocks {
        world: String,
        blocks: Vec<(BlockPos, u16)>,
    },
    /// Queue a player for the configured `lobby.auto_join` mode.
    AutoJoin {
        player: String,
    },
    /// Remove a game's spawner labels (holograms).
    RemoveHolograms {
        game: String,
    },
    /// Daybuilt PvP Manager: round begins for this player.
    PvpEnter {
        player: String,
        classic: Option<bool>,
    },
    /// Daybuilt PvP Manager: player leaves the round; restore their settings.
    PvpLeave {
        player: String,
    },
}

/// Collects effects plus results that the state layer handles (events, stats).
#[derive(Debug, Default)]
pub struct Out {
    pub effects: Vec<Effect>,
    pub events: Vec<minigamecore_client::Event>,
    pub results: Vec<RoundResult>,
    /// Games whose arena must now be restored.
    pub resets: Vec<String>,
    /// Players who left their game (for the player index).
    pub removed: Vec<String>,
}

impl Out {
    pub fn push(&mut self, e: Effect) {
        self.effects.push(e);
    }

    pub fn event(&mut self, e: minigamecore_client::Event) {
        self.events.push(e);
    }

    pub fn chat(&mut self, players: Vec<String>, text: String) {
        if !players.is_empty() && !text.is_empty() {
            self.effects.push(Effect::Chat { players, text });
        }
    }

    pub fn title(
        &mut self,
        players: Vec<String>,
        title: String,
        subtitle: String,
        stay_ticks: i32,
    ) {
        if !players.is_empty() && (!title.is_empty() || !subtitle.is_empty()) {
            self.effects.push(Effect::Title {
                players,
                title,
                subtitle,
                stay_ticks,
            });
        }
    }

    pub fn sound(&mut self, players: Vec<String>, sound: &str, pitch: f32) {
        if !players.is_empty() {
            self.effects.push(Effect::Sound {
                players,
                sound: sound.into(),
                pitch,
            });
        }
    }
}

/// One player's outcome, saved into stats.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerResult {
    pub uuid: String,
    pub name: String,
    pub won: bool,
    pub kills: u32,
    pub deaths: u32,
    pub counters: std::collections::BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoundResult {
    pub mode: String,
    /// True for a draw (nobody won, nobody lost).
    pub draw: bool,
    pub players: Vec<PlayerResult>,
}
