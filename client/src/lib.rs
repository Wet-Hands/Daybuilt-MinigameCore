//! Client for Daybuilt MinigameCore.
//!
//! A game-mode plugin (Bedwars, Spleef, ...) depends on this crate, claims its mode with
//! [`register_mode`], and then calls [`poll`] from a repeating task to receive the
//! [`Event`]s of its games. Everything else is a request: read game state, end a round,
//! change rules, set blocks that the core restores at reset, and so on.
//!
//! ```rust,ignore
//! use minigamecore_client as mg;
//!
//! // In on_load: poll every tick. Registering again is harmless, so retry until it works.
//! pumpkin_plugin_api::scheduler::schedule_repeating_task(1, 1, |_server| {
//!     if !REGISTERED.load(Relaxed) && mg::register_mode("spleef").is_ok() {
//!         REGISTERED.store(true, Relaxed);
//!     }
//!     if let Ok(batch) = mg::poll(None) {
//!         for env in batch.events {
//!             match env.event {
//!                 mg::Event::RoundStarted { game, .. } => { /* ... */ }
//!                 _ => {}
//!             }
//!         }
//!     }
//! });
//! ```
//!
//! Rules for callers:
//! - Don't list the core in `metadata().dependencies` unless your plugin can't run without it;
//!   a missing dependency stops your plugin loading. Treat [`ClientError::Unavailable`] as
//!   "core not loaded".
//! - The core never calls your plugin, so calling it from any handler, command or task is safe.
//! - Host effects of requests (teleports, blocks, kits) are applied on the core's next tick.

pub mod protocol;

pub use protocol::*;

/// Why a call failed.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// MinigameCore is not loaded.
    Unavailable,
    /// The core answered and refused.
    Refused(ErrorReply),
    /// The reply could not be understood.
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Unavailable => write!(f, "{PLUGIN_ID} is not loaded"),
            ClientError::Refused(e) => write!(f, "{:?}: {}", e.kind, e.message),
            ClientError::Protocol(m) => write!(f, "protocol error: {m}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Encodes a request for the wire.
pub fn encode(request: Request) -> Vec<u8> {
    serde_json::to_vec(&WireRequest {
        v: PROTOCOL_VERSION,
        request,
    })
    .unwrap_or_default()
}

/// Decodes the core's reply bytes.
pub fn decode(bytes: &[u8]) -> Result<Reply, ClientError> {
    let wire: WireResponse =
        serde_json::from_slice(bytes).map_err(|e| ClientError::Protocol(e.to_string()))?;
    wire.into_result().map_err(ClientError::Refused)
}

#[cfg(feature = "ipc")]
mod calls {
    use super::*;

    /// Sends one request and returns the reply.
    pub fn call(request: Request) -> Result<Reply, ClientError> {
        let bytes = encode(request);
        match pumpkin_plugin_api::ipc::send_ipc_message(PLUGIN_ID, &bytes) {
            Ok(Ok(reply)) => decode(&reply),
            // The core always answers with Ok + JSON; an Err string is a host-level failure.
            Ok(Err(message)) => Err(ClientError::Protocol(message)),
            Err(()) => Err(ClientError::Unavailable),
        }
    }

    fn done(request: Request) -> Result<(), ClientError> {
        match call(request)? {
            Reply::Done => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    fn unexpected(reply: Reply) -> ClientError {
        ClientError::Protocol(format!("unexpected reply {reply:?}"))
    }

    /// True if the core is loaded and answers.
    pub fn is_available() -> bool {
        matches!(call(Request::Ping), Ok(Reply::Pong { .. }))
    }

    pub fn register_mode(mode: &str) -> Result<(), ClientError> {
        done(Request::RegisterMode { mode: mode.into() })
    }

    /// Queued events for this plugin's modes, oldest first.
    pub struct Batch {
        pub events: Vec<EventEnvelope>,
        pub dropped: u64,
    }

    pub fn poll(max: Option<u32>) -> Result<Batch, ClientError> {
        match call(Request::Poll { max })? {
            Reply::Events { events, dropped } => Ok(Batch { events, dropped }),
            other => Err(unexpected(other)),
        }
    }

    pub fn game(game: &str) -> Result<GameInfo, ClientError> {
        match call(Request::Game { game: game.into() })? {
            Reply::Game(info) => Ok(info),
            other => Err(unexpected(other)),
        }
    }

    pub fn list_games(mode: Option<&str>) -> Result<Vec<GameInfo>, ClientError> {
        match call(Request::ListGames {
            mode: mode.map(Into::into),
        })? {
            Reply::Games(games) => Ok(games),
            other => Err(unexpected(other)),
        }
    }

    /// The player's game, or `None` if they are not in one.
    pub fn player(player: &str) -> Result<Option<PlayerInfo>, ClientError> {
        match call(Request::Player {
            player: player.into(),
        })? {
            Reply::Player(info) => Ok(info),
            other => Err(unexpected(other)),
        }
    }

    pub fn arena(arena: &str) -> Result<ArenaInfo, ClientError> {
        match call(Request::Arena {
            arena: arena.into(),
        })? {
            Reply::Arena(info) => Ok(info),
            other => Err(unexpected(other)),
        }
    }

    /// Matchmaking. Returns the game id the party joined.
    pub fn join(
        players: &[&str],
        mode: &str,
        format: Option<&str>,
        arena: Option<&str>,
    ) -> Result<String, ClientError> {
        match call(Request::Join {
            players: players.iter().map(|p| (*p).to_string()).collect(),
            mode: mode.into(),
            format: format.map(Into::into),
            arena: arena.map(Into::into),
        })? {
            Reply::Joined { game } => Ok(game),
            other => Err(unexpected(other)),
        }
    }

    pub fn leave(player: &str) -> Result<(), ClientError> {
        done(Request::Leave {
            player: player.into(),
        })
    }

    pub fn end_round(game: &str, winners: &[&str], reason: &str) -> Result<(), ClientError> {
        done(Request::EndRound {
            game: game.into(),
            winners: winners.iter().map(|w| (*w).to_string()).collect(),
            reason: Some(reason.into()),
        })
    }

    pub fn eliminate(player: &str) -> Result<(), ClientError> {
        done(Request::Eliminate {
            player: player.into(),
        })
    }

    pub fn set_team_respawn(game: &str, team: &str, allowed: bool) -> Result<(), ClientError> {
        done(Request::SetRespawn {
            game: game.into(),
            team: Some(team.into()),
            player: None,
            allowed,
        })
    }

    pub fn set_player_respawn(game: &str, player: &str, allowed: bool) -> Result<(), ClientError> {
        done(Request::SetRespawn {
            game: game.into(),
            team: None,
            player: Some(player.into()),
            allowed,
        })
    }

    pub fn set_rule(game: &str, key: &str, value: serde_json::Value) -> Result<(), ClientError> {
        done(Request::SetRule {
            game: game.into(),
            key: key.into(),
            value,
        })
    }

    pub fn spawner_group(
        game: &str,
        group: &str,
        interval_ticks: Option<u32>,
        amount: Option<u32>,
        enabled: Option<bool>,
    ) -> Result<(), ClientError> {
        done(Request::SpawnerGroup {
            game: game.into(),
            group: group.into(),
            interval_ticks,
            amount,
            enabled,
        })
    }

    pub fn schedule(
        game: &str,
        id: &str,
        at_secs: u32,
        label: Option<&str>,
    ) -> Result<(), ClientError> {
        done(Request::Schedule {
            game: game.into(),
            id: id.into(),
            at_secs,
            label: label.map(Into::into),
        })
    }

    pub fn set_var(
        game: &str,
        player: Option<&str>,
        key: &str,
        value: Option<&str>,
    ) -> Result<(), ClientError> {
        done(Request::SetVar {
            game: game.into(),
            player: player.map(Into::into),
            key: key.into(),
            value: value.map(Into::into),
        })
    }

    /// Adds to a per-round counter; returns the new value.
    pub fn counter(player: &str, key: &str, delta: i64) -> Result<i64, ClientError> {
        match call(Request::Counter {
            player: player.into(),
            key: key.into(),
            delta,
        })? {
            Reply::Value(v) => Ok(v),
            other => Err(unexpected(other)),
        }
    }

    pub fn set_block(game: &str, pos: BlockPos, block: &str) -> Result<(), ClientError> {
        done(Request::SetBlock {
            game: game.into(),
            pos,
            block: block.into(),
        })
    }

    pub fn record_block(game: &str, pos: BlockPos, state: u16) -> Result<(), ClientError> {
        done(Request::RecordBlock {
            game: game.into(),
            pos,
            state,
        })
    }

    pub fn message(
        game: &str,
        team: Option<&str>,
        player: Option<&str>,
        text: &str,
        kind: MessageKind,
    ) -> Result<(), ClientError> {
        done(Request::Message {
            game: game.into(),
            team: team.map(Into::into),
            player: player.map(Into::into),
            text: text.into(),
            kind,
        })
    }

    pub fn give_kit(player: &str, kit: &str) -> Result<(), ClientError> {
        done(Request::GiveKit {
            player: player.into(),
            kit: kit.into(),
        })
    }

    pub fn select_kit(player: &str, kit: &str) -> Result<(), ClientError> {
        done(Request::SelectKit {
            player: player.into(),
            kit: kit.into(),
        })
    }

    pub fn same_team(a: &str, b: &str) -> Result<bool, ClientError> {
        match call(Request::SameTeam {
            a: a.into(),
            b: b.into(),
        })? {
            Reply::Bool(v) => Ok(v),
            other => Err(unexpected(other)),
        }
    }

    pub fn regions_at(game: &str, pos: Location) -> Result<Vec<String>, ClientError> {
        match call(Request::RegionsAt {
            game: game.into(),
            pos,
        })? {
            Reply::Regions(v) => Ok(v),
            other => Err(unexpected(other)),
        }
    }
}

#[cfg(feature = "ipc")]
pub use calls::*;
