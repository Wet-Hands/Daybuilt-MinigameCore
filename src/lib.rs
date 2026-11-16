//! Daybuilt MinigameCore: arenas, teams, round lifecycle, kits, spawners and map reset
//! for round-based minigames on Pumpkin. Game-mode plugins talk to it over IPC
//! (see the `minigamecore-client` crate); simple modes run from config alone.
//!
//! Layout:
//! - `model/`     definitions loaded from files (arenas, modes, rules, kits) — pure
//! - `game/`      one game's state machine, matchmaking, sidebar text — pure
//! - `state.rs`   all games and indexes behind one lock — pure
//! - `stats.rs`   player statistics — pure
//! - `ipc.rs`     the protocol for mode plugins — pure (effects are deferred)
//! - `host/`      everything that calls Pumpkin: effects, files, kits, sidebar, spawners, reset
//! - `listeners/` Pumpkin event handlers
//! - `commands/`  `/mg` and `/dbmg`

mod commands;
mod config;
mod game;
mod host;
mod ipc;
mod listeners;
mod model;
mod plugin;
mod state;
mod stats;
mod ticker;

/// Plugin id from Cargo.toml. Also the permission namespace (`<ID>:...`).
pub const ID: &str = env!("CARGO_PKG_NAME");
