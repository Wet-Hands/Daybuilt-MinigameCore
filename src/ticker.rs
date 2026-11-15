//! The one repeating task. Every tick: game steps, deferred effects, arena restore.
//! Every few ticks: position scan. Every second: sidebars, spawner labels, PIE, stats.

use pumpkin_plugin_api::{Server, scheduler};

use crate::game::Ctx;
use crate::host;
use crate::listeners::uuid_of;
use crate::model::Location;
use crate::state;

pub fn start() {
    scheduler::schedule_repeating_task(1, 1, |server: Server| tick(&server));
}

fn tick(server: &Server) {
    let (effects, now, scan_every) = state::with(|s| {
        (
            s.tick(),
            s.now,
            u64::from(s.cfg.timing.position_check_ticks),
        )
    });
    host::apply(server, effects);
    host::restore::step(server);
    if now.is_multiple_of(scan_every) {
        scan_positions(server);
    }
    if now.is_multiple_of(20) {
        host::sidebar::refresh(server);
        host::spawners::refresh_labels(server);
        host::pie::tick();
    }
    if now.is_multiple_of(100) {
        save_stats();
    }
}

/// Void, bounds, regions and spawner pickups for players in games; lobby void rescue.
fn scan_positions(server: &Server) {
    let (any_games, lobby) = state::with(|s| (!s.index.is_empty(), s.cfg.lobby.spawn.clone()));
    if !any_games && lobby.is_none() {
        return;
    }
    let tracked: std::collections::HashSet<String> =
        state::with(|s| s.index.keys().cloned().collect());
    let mut positions = Vec::new();
    for p in server.get_all_players() {
        let uuid = uuid_of(&p);
        let (x, y, z) = p.get_position();
        if tracked.contains(&uuid) {
            positions.push((uuid, p.get_world().get_name(), Location::new(x, y, z)));
        } else if let Some(spawn) = &lobby
            && y < spawn.y - 80.0
            && p.get_world().get_name() == spawn.world
        {
            host::teleport(server, &p, spawn);
        }
    }
    if positions.is_empty() {
        return;
    }
    let effects = state::with(|s| {
        let ids: Vec<String> = s.games.keys().cloned().collect();
        let mut all = Vec::new();
        for id in ids {
            let mine: Vec<_> = match s.games.get(&id) {
                Some(g) => positions
                    .iter()
                    .filter(|(u, _, _)| g.players.contains_key(u))
                    .cloned()
                    .collect(),
                None => continue,
            };
            if mine.is_empty() {
                continue;
            }
            if let Some((_, e)) = s.step(&id, |g, ctx: &Ctx, out| g.on_positions(ctx, &mine, out)) {
                all.extend(e);
            }
        }
        all
    });
    host::apply(server, effects);
    host::spawners::pickups(server, &positions);
}

pub fn save_stats() {
    let snapshot = state::with(|s| {
        if s.stats_dirty && s.persist {
            s.stats_dirty = false;
            Some(s.stats.clone())
        } else {
            None
        }
    });
    if let Some(stats) = snapshot
        && let Err(e) = host::storage::save_stats(&stats)
    {
        tracing::warn!("{}: could not save stats: {e}", crate::ID);
        state::with(|s| s.stats_dirty = true);
    }
}
