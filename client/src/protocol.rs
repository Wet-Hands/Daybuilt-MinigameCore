//! Wire format between game-mode plugins and MinigameCore.
//!
//! Every message is UTF-8 JSON sent with Pumpkin's IPC to the plugin id [`PLUGIN_ID`].
//!
//! - Request: `{"v":1,"op":"<op>", ...fields}`. See [`Request`].
//! - Success: `{"ok":true,"reply":{"<variant>":...}}`. See [`Reply`].
//! - Failure: `{"ok":false,"error":{"kind":"<kind>","message":"..."}}`. See [`ErrorKind`].
//!
//! Changes are additive within a protocol version: new ops, new optional fields
//! (`#[serde(default)]`), new events and new error kinds. Unknown fields are ignored.
//! Clients should ignore events whose `type` they don't know ([`Event::Unknown`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The core's plugin id (its Cargo package name).
pub const PLUGIN_ID: &str = "daybuilt_minigamecore";

/// Current protocol version. Requests carry it as `"v"`.
pub const PROTOCOL_VERSION: u32 = 1;

fn default_version() -> u32 {
    PROTOCOL_VERSION
}

/// A whole-block position.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// A precise position with optional facing.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct Location {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
}

impl Location {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    pub fn block(&self) -> BlockPos {
        BlockPos::new(
            self.x.floor() as i32,
            self.y.floor() as i32,
            self.z.floor() as i32,
        )
    }
}

/// Envelope for a request: the version plus the operation.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WireRequest {
    #[serde(default = "default_version")]
    pub v: u32,
    #[serde(flatten)]
    pub request: Request,
}

/// Where a chat-style message is shown.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Chat,
    ActionBar,
    Title,
    Subtitle,
}

/// Operations. `player` fields accept an online player's name or a UUID string.
/// `game` is the arena id the game runs in.
///
/// Mutating operations on a game are only accepted from the plugin that registered
/// the game's mode with [`Request::RegisterMode`]. Queries and `join`/`leave` are open
/// to any plugin.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Returns [`Reply::Pong`].
    Ping,
    /// Claims a mode id for the calling plugin. Events of games in this mode are queued
    /// for it and returned by `poll`. Re-registering by the same plugin is fine.
    RegisterMode { mode: String },
    /// Returns and removes up to `max` queued events (default 256) for the calling plugin.
    Poll {
        #[serde(default)]
        max: Option<u32>,
    },
    /// All games, optionally of one mode.
    ListGames {
        #[serde(default)]
        mode: Option<String>,
    },
    /// One game.
    Game { game: String },
    /// Which game a player is in, if any.
    Player { player: String },
    /// An arena definition (spawns, regions, points, values).
    Arena { arena: String },
    /// Arena ids, optionally only those that can host `mode`.
    ListArenas {
        #[serde(default)]
        mode: Option<String>,
    },
    /// Matchmaking: put `players` (a party, kept on one team when possible) into a game of
    /// `mode`, optionally a specific `format` and/or `arena`.
    Join {
        players: Vec<String>,
        mode: String,
        #[serde(default)]
        format: Option<String>,
        #[serde(default)]
        arena: Option<String>,
    },
    /// Removes a player from their game (counts as a loss mid-round).
    Leave { player: String },
    /// Starts the countdown's end now, if there are enough players.
    Start { game: String },
    /// Ends the round. Empty `winners` is a draw.
    EndRound {
        game: String,
        #[serde(default)]
        winners: Vec<String>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Eliminates a player (no respawn).
    Eliminate { player: String },
    /// Allows or forbids respawning for a whole team or one player.
    SetRespawn {
        game: String,
        #[serde(default)]
        team: Option<String>,
        #[serde(default)]
        player: Option<String>,
        allowed: bool,
    },
    /// Changes one rule of a running game (same keys as `[rules]` in the mode file).
    SetRule {
        game: String,
        key: String,
        value: serde_json::Value,
    },
    /// Changes a spawner group at runtime. `None` fields keep their value.
    SpawnerGroup {
        game: String,
        group: String,
        #[serde(default)]
        interval_ticks: Option<u32>,
        #[serde(default)]
        amount: Option<u32>,
        #[serde(default)]
        enabled: Option<bool>,
    },
    /// Fires [`Event::Timer`] when the round has run `at_secs` seconds. `label` is shown as
    /// the sidebar's "next event". Replaces an entry with the same id.
    Schedule {
        game: String,
        id: String,
        at_secs: u32,
        #[serde(default)]
        label: Option<String>,
    },
    CancelSchedule { game: String, id: String },
    /// Sets a sidebar variable (`{var:<key>}` in sidebar lines). With `player`, only that
    /// player's sidebar sees it. `None` value removes it.
    SetVar {
        game: String,
        #[serde(default)]
        player: Option<String>,
        key: String,
        #[serde(default)]
        value: Option<String>,
    },
    /// Adds `delta` to a mode-specific counter for the player's current round
    /// (saved into stats at round end, e.g. `beds_broken`). Returns [`Reply::Value`].
    Counter {
        player: String,
        key: String,
        delta: i64,
    },
    /// Sets a block inside the game's arena, recording the original for reset.
    /// `block` is a block name (`minecraft:air`) or a numeric state id. Applied within a tick.
    SetBlock {
        game: String,
        pos: BlockPos,
        block: String,
    },
    /// Records that the plugin is about to change `pos` itself; `state` is the current
    /// (original) block state id. Use before changing blocks yourself.
    RecordBlock {
        game: String,
        pos: BlockPos,
        state: u16,
    },
    /// Sends text (`&` colour codes) to a game, a team or a player.
    Message {
        game: String,
        #[serde(default)]
        team: Option<String>,
        #[serde(default)]
        player: Option<String>,
        text: String,
        #[serde(default)]
        kind: MessageKind,
    },
    /// Applies a kit to a player now.
    GiveKit { player: String, kit: String },
    /// Selects the kit a player gets at round start (and on respawn).
    SelectKit { player: String, kit: String },
    /// Whether two players are on the same team of the same game. [`Reply::Bool`].
    SameTeam { a: String, b: String },
    /// Names of the arena regions containing `pos`. [`Reply::Regions`].
    RegionsAt { game: String, pos: Location },
}

/// Successful results.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Pong {
        protocol: u32,
        version: String,
    },
    Done,
    Events {
        events: Vec<EventEnvelope>,
        /// Events dropped because the queue was full since the last poll.
        dropped: u64,
    },
    Games(Vec<GameInfo>),
    Game(GameInfo),
    Player(Option<PlayerInfo>),
    Arena(ArenaInfo),
    Arenas(Vec<String>),
    Joined {
        game: String,
    },
    Bool(bool),
    Value(i64),
    Regions(Vec<String>),
}

/// Why a request was refused.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    BadRequest,
    UnsupportedVersion,
    UnknownGame,
    UnknownPlayer,
    UnknownArena,
    UnknownMode,
    UnknownKit,
    UnknownTeam,
    /// The calling plugin does not own this game's mode.
    NotOwner,
    /// Another plugin registered this mode.
    ModeTaken,
    /// No arena can take these players right now.
    NoArena,
    /// The player is already in a game.
    AlreadyInGame,
    /// The game is not in a state that allows this.
    WrongState,
    #[serde(other)]
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ErrorReply {
    pub kind: ErrorKind,
    pub message: String,
}

/// Envelope for a response.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WireResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Reply>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorReply>,
}

impl WireResponse {
    pub fn from_result(result: Result<Reply, ErrorReply>) -> Self {
        match result {
            Ok(reply) => Self {
                ok: true,
                reply: Some(reply),
                error: None,
            },
            Err(error) => Self {
                ok: false,
                reply: None,
                error: Some(error),
            },
        }
    }

    pub fn into_result(self) -> Result<Reply, ErrorReply> {
        match (self.ok, self.reply, self.error) {
            (true, Some(reply), _) => Ok(reply),
            (_, _, Some(error)) => Err(error),
            _ => Err(ErrorReply {
                kind: ErrorKind::Other,
                message: "malformed response".into(),
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Waiting,
    Countdown,
    Running,
    Ending,
    Resetting,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerStatus {
    /// In the game before the round starts.
    Waiting,
    Alive,
    /// Dead, will respawn.
    Respawning,
    /// Eliminated and watching.
    Spectating,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PlayerRef {
    pub uuid: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlayerInfo {
    pub uuid: String,
    pub name: String,
    pub game: String,
    pub mode: String,
    #[serde(default)]
    pub team: Option<String>,
    pub status: PlayerStatus,
    pub kills: u32,
    pub deaths: u32,
    #[serde(default)]
    pub counters: BTreeMap<String, i64>,
    /// Arena regions the player was in at the last position scan.
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub kit: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub players: Vec<PlayerRef>,
    /// Members still in the round (alive or respawning).
    pub alive: u32,
    pub respawn: bool,
    pub eliminated: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GameInfo {
    pub game: String,
    pub mode: String,
    pub format: String,
    pub phase: Phase,
    pub round: u64,
    /// Seconds since the round started (0 before).
    pub elapsed: u32,
    pub max_players: u32,
    pub min_players: u32,
    pub players: Vec<PlayerInfo>,
    pub teams: Vec<TeamInfo>,
    #[serde(default)]
    pub winners: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ArenaTeamInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub spawns: Vec<Location>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RegionInfo {
    pub name: String,
    pub min: BlockPos,
    pub max: BlockPos,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ArenaInfo {
    pub id: String,
    pub world: String,
    pub min: BlockPos,
    pub max: BlockPos,
    pub modes: Vec<String>,
    pub teams: Vec<ArenaTeamInfo>,
    /// Shared spawn points used by solo formats.
    pub spawns: Vec<Location>,
    #[serde(default)]
    pub spectator: Option<Location>,
    #[serde(default)]
    pub waiting: Option<Location>,
    pub regions: Vec<RegionInfo>,
    /// Named point lists captured with `/mga arena <id> point <name>`.
    pub points: BTreeMap<String, Vec<Location>>,
    /// Free-form values for the mode plugin (`[values]` in the arena file).
    pub values: BTreeMap<String, String>,
}

/// One queued event with its sequence number.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    pub seq: u64,
    #[serde(flatten)]
    pub event: Event,
}

/// Events queued for the plugin that owns a game's mode.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A game was opened in an arena (matchmaking picked it).
    GameCreated {
        game: String,
        mode: String,
        format: String,
    },
    PlayerJoined {
        game: String,
        player: PlayerRef,
        #[serde(default)]
        team: Option<String>,
    },
    /// `during_round` is true when leaving counted as a loss.
    PlayerLeft {
        game: String,
        player: PlayerRef,
        during_round: bool,
    },
    CountdownStarted {
        game: String,
        seconds: u32,
    },
    CountdownCancelled {
        game: String,
    },
    RoundStarted {
        game: String,
        round: u64,
        teams: Vec<TeamInfo>,
    },
    /// `respawns` is false for a final death (the player is eliminated).
    PlayerDied {
        game: String,
        player: PlayerRef,
        #[serde(default)]
        killer: Option<PlayerRef>,
        cause: String,
        respawns: bool,
    },
    PlayerRespawned {
        game: String,
        player: PlayerRef,
    },
    PlayerEliminated {
        game: String,
        player: PlayerRef,
    },
    TeamEliminated {
        game: String,
        team: String,
    },
    RegionEntered {
        game: String,
        player: PlayerRef,
        region: String,
    },
    RegionLeft {
        game: String,
        player: PlayerRef,
        region: String,
    },
    /// A `Schedule` entry or a `[[schedule]]` entry from the mode file came due.
    Timer {
        game: String,
        id: String,
    },
    /// The time limit passed and the mode's `time_up` rule is `"event"`.
    TimeUp {
        game: String,
    },
    RoundEnded {
        game: String,
        winners: Vec<String>,
        reason: String,
    },
    /// The arena was restored and is free again.
    GameReset {
        game: String,
    },
    /// An event type this client version does not know.
    #[serde(other)]
    Unknown,
}

impl Event {
    /// The game the event belongs to (empty for [`Event::Unknown`]).
    pub fn game(&self) -> &str {
        match self {
            Event::GameCreated { game, .. }
            | Event::PlayerJoined { game, .. }
            | Event::PlayerLeft { game, .. }
            | Event::CountdownStarted { game, .. }
            | Event::CountdownCancelled { game }
            | Event::RoundStarted { game, .. }
            | Event::PlayerDied { game, .. }
            | Event::PlayerRespawned { game, .. }
            | Event::PlayerEliminated { game, .. }
            | Event::TeamEliminated { game, .. }
            | Event::RegionEntered { game, .. }
            | Event::RegionLeft { game, .. }
            | Event::Timer { game, .. }
            | Event::TimeUp { game }
            | Event::RoundEnded { game, .. }
            | Event::GameReset { game } => game,
            Event::Unknown => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let json = r#"{"v":1,"op":"join","players":["Steve"],"mode":"bedwars"}"#;
        let req: WireRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.request,
            Request::Join {
                players: vec!["Steve".into()],
                mode: "bedwars".into(),
                format: None,
                arena: None
            }
        );
        let ping: WireRequest = serde_json::from_str(r#"{"op":"ping"}"#).unwrap();
        assert_eq!(ping.v, PROTOCOL_VERSION);
        assert_eq!(ping.request, Request::Ping);
    }

    #[test]
    fn response_shapes() {
        let ok = WireResponse::from_result(Ok(Reply::Bool(true)));
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"ok":true,"reply":{"bool":true}}"#
        );
        let done = WireResponse::from_result(Ok(Reply::Done));
        assert_eq!(
            serde_json::to_string(&done).unwrap(),
            r#"{"ok":true,"reply":"done"}"#
        );
        let err = WireResponse::from_result(Err(ErrorReply {
            kind: ErrorKind::NotOwner,
            message: "x".into(),
        }));
        let text = serde_json::to_string(&err).unwrap();
        assert_eq!(
            text,
            r#"{"ok":false,"error":{"kind":"not_owner","message":"x"}}"#
        );
        let back: WireResponse = serde_json::from_str(&text).unwrap();
        assert_eq!(back.into_result().unwrap_err().kind, ErrorKind::NotOwner);
    }

    #[test]
    fn events_flatten_and_unknown() {
        let env = EventEnvelope {
            seq: 3,
            event: Event::Timer {
                game: "a1".into(),
                id: "diamond_2".into(),
            },
        };
        let text = serde_json::to_string(&env).unwrap();
        assert_eq!(text, r#"{"seq":3,"type":"timer","game":"a1","id":"diamond_2"}"#);
        let unknown: EventEnvelope =
            serde_json::from_str(r#"{"seq":4,"type":"from_the_future","game":"x"}"#).unwrap();
        assert_eq!(unknown.event, Event::Unknown);
        let kind: ErrorKind = serde_json::from_str(r#""new_kind""#).unwrap();
        assert_eq!(kind, ErrorKind::Other);
    }
}
