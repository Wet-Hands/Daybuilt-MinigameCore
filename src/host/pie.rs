//! PumpkinPIE expansion (optional). Protocol: UTF-8 JSON over IPC to `PumpkinPIE`.
//!
//! - Registration is sent from the ticker only, retried with backoff until PIE answers,
//!   and again after `/mga reload`. Never from event or command handlers.
//! - `on_request` (PIE → us) is answered from memory only; it must not call the host or PIE.
//! - PumpkinPIE is never listed in `metadata().dependencies`.
//!
//! Placeholders (`mg` is the configurable namespace):
//! - `%mg_<stat>%` total over all modes; `%mg_<mode>_<stat>%` for one mode. Stats: wins,
//!   losses, draws, played, kills, deaths, win_streak, best_win_streak and mode counters.
//! - `%mg_game_<field>%` for the viewer's current game: arena, mode, mode_name, format,
//!   team, state, alive, players, max, kills, deaths, time, kit.
//! - `%mg_playing_<mode>%`, `%mg_waiting_<mode>%`, `%mg_games_<mode>%` server counts.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use pumpkin_plugin_api::ipc::send_ipc_message;
use serde_json::{Value, json};

use crate::model::text;
use crate::state::{self, State};

pub const PIE: &str = "PumpkinPIE";

/// Tick of the next registration attempt, and the current backoff in ticks.
static NEXT_TRY: AtomicU64 = AtomicU64::new(0);
static BACKOFF: AtomicU64 = AtomicU64::new(100);

const ADVERTISED: &[&str] = &[
    "wins",
    "losses",
    "kills",
    "deaths",
    "played",
    "win_streak",
    "best_win_streak",
    "game_arena",
    "game_mode",
    "game_team",
    "game_state",
    "game_alive",
    "game_players",
    "game_time",
    "game_kills",
];

/// Called from the ticker. Sends the registration when needed.
pub fn tick() {
    let (dirty, enabled, ns, cache_ms, now) = state::with(|s| {
        (
            s.pie_dirty,
            s.cfg.placeholders.enabled,
            s.cfg.placeholders.namespace.clone(),
            s.cfg.placeholders.cache_ms,
            s.now,
        )
    });
    if !dirty || now < NEXT_TRY.load(Relaxed) {
        return;
    }
    let request = if enabled {
        json!({
            "op": "register_expansion",
            "namespace": ns,
            "placeholders": ADVERTISED,
            "cache": if cache_ms == 0 { json!({"kind": "never"}) } else { json!({"kind": "ttl", "ms": cache_ms}) },
        })
    } else {
        json!({"op": "unregister_expansion"})
    };
    let bytes = serde_json::to_vec(&request).unwrap_or_default();
    match send_ipc_message(PIE, &bytes) {
        Ok(Ok(reply)) => {
            let ok = serde_json::from_slice::<Value>(&reply)
                .ok()
                .and_then(|v| v.get("ok").and_then(Value::as_bool))
                .unwrap_or(false);
            if ok {
                state::with(|s| s.pie_dirty = false);
                BACKOFF.store(100, Relaxed);
                if enabled {
                    tracing::info!(
                        "{}: placeholders registered with PumpkinPIE as %{ns}_...%",
                        crate::ID
                    );
                }
            } else {
                tracing::warn!(
                    "{}: PumpkinPIE refused the registration: {}",
                    crate::ID,
                    String::from_utf8_lossy(&reply)
                );
                state::with(|s| s.pie_dirty = false);
            }
        }
        Ok(Err(e)) => {
            tracing::warn!("{}: PumpkinPIE error: {e}", crate::ID);
            retry_later(now);
        }
        Err(()) => {
            if !enabled {
                state::with(|s| s.pie_dirty = false);
            } else {
                retry_later(now); // not loaded (yet)
            }
        }
    }
}

fn retry_later(now: u64) {
    let b = BACKOFF.load(Relaxed);
    NEXT_TRY.store(now + b, Relaxed);
    BACKOFF.store((b * 2).min(20 * 60), Relaxed);
}

/// Handles a message from PumpkinPIE. Memory only.
pub fn handle(bytes: &[u8]) -> Vec<u8> {
    let decline = || br#"{"ok":true,"on_request":{}}"#.to_vec();
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return decline();
    };
    if v.get("op").and_then(Value::as_str) != Some("on_request") {
        return decline();
    }
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let viewer = v.get("viewer").and_then(Value::as_str).map(str::to_string);
    let value = state::with(|s| resolve(s, &name, viewer.as_deref()));
    match value {
        Some(val) => serde_json::to_vec(&json!({"ok": true, "on_request": {"value": val}}))
            .unwrap_or_else(|_| decline()),
        None => decline(),
    }
}

/// Resolves a placeholder name (after the namespace). Pure.
pub fn resolve(s: &State, name: &str, viewer: Option<&str>) -> Option<String> {
    let uuid = viewer.and_then(|v| {
        s.resolve(v)
            .map(|r| r.0)
            .or_else(|| s.stats.uuid_by_name(v))
    });
    if let Some(field) = name.strip_prefix("game_") {
        return game_field(s, uuid.as_deref()?, field);
    }
    for (prefix, kind) in [("playing_", 0), ("waiting_", 1), ("games_", 2)] {
        if let Some(mode) = name.strip_prefix(prefix) {
            let games = s.games.values().filter(|g| g.mode.id == mode);
            let n: usize = match kind {
                0 => games
                    .filter(|g| matches!(g.phase, crate::game::GamePhase::Running { .. }))
                    .map(|g| g.players.len())
                    .sum(),
                1 => games
                    .filter(|g| g.phase.joinable())
                    .map(|g| g.players.len())
                    .sum(),
                _ => games.count(),
            };
            return Some(n.to_string());
        }
    }
    let uuid = uuid?;
    if let Some((mode, key)) = name.split_once('_')
        && s.modes.contains_key(mode)
    {
        return Some(s.stats.get(&uuid, Some(mode), key).to_string());
    }
    Some(s.stats.get(&uuid, None, name).to_string())
}

fn game_field(s: &State, uuid: &str, field: &str) -> Option<String> {
    let Some(g) = s.game_of(uuid) else {
        return Some(match field {
            "state" => "lobby".into(),
            "alive" | "players" | "max" | "kills" | "deaths" => "0".into(),
            _ => String::new(),
        });
    };
    let p = g.players.get(uuid);
    Some(match field {
        "arena" => g.arena.id.clone(),
        "mode" => g.mode.id.clone(),
        "mode_name" => text::legacy(&g.mode.display_name),
        "format" => g.format.display_name.clone(),
        "team" => p
            .and_then(|p| p.team.as_deref())
            .and_then(|t| g.team(t))
            .map(|t| text::legacy(&t.display()))
            .unwrap_or_default(),
        "state" => format!("{:?}", g.phase.public()).to_ascii_lowercase(),
        "alive" => g
            .players
            .values()
            .filter(|p| p.in_round())
            .count()
            .to_string(),
        "players" => g.players.len().to_string(),
        "max" => g.capacity().to_string(),
        "kills" => p.map(|p| p.kills).unwrap_or(0).to_string(),
        "deaths" => p.map(|p| p.deaths).unwrap_or(0).to_string(),
        "time" => text::clock(g.elapsed(s.now)),
        "kit" => p.and_then(|p| p.kit.clone()).unwrap_or_default(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::{arena, mode};

    #[test]
    fn resolves_stats_and_live_values() {
        let mut s = State::new();
        s.arenas.insert("castle".into(), arena());
        s.modes.insert("bedwars".into(), mode(""));
        s.player_online("u1", "Steve", 1);
        s.join(vec![("u1".into(), "Steve".into())], "bedwars", None, None)
            .unwrap();
        assert_eq!(
            resolve(&s, "game_arena", Some("Steve")).as_deref(),
            Some("castle")
        );
        assert_eq!(
            resolve(&s, "game_state", Some("steve")).as_deref(),
            Some("waiting")
        );
        assert_eq!(resolve(&s, "waiting_bedwars", None).as_deref(), Some("1"));
        assert_eq!(
            resolve(&s, "bedwars_wins", Some("Steve")).as_deref(),
            Some("0")
        );
        assert_eq!(
            resolve(&s, "best_win_streak", Some("Steve")).as_deref(),
            Some("0")
        );
        assert_eq!(resolve(&s, "wins", None), None);
        let reply = handle(br#"{"op":"on_request","name":"game_nope","viewer":"Steve"}"#);
        assert_eq!(reply, br#"{"ok":true,"on_request":{}}"#.to_vec());
    }
}
