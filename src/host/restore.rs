//! Restoring arenas after a round, a batch of blocks per tick.

use pumpkin_plugin_api::{Server, common::BlockPos as HostPos};

use crate::state;

/// Works on the first restore job. Called every tick.
pub fn step(server: &Server) {
    let job = state::with(|s| {
        let per_tick = s.cfg.timing.reset_blocks_per_tick as usize;
        let r = s.restores.front_mut()?;
        let n = per_tick.min(r.blocks.len());
        let batch: Vec<_> = r.blocks.drain(..n).collect();
        let cleanup = if r.cleaned {
            None
        } else {
            r.cleaned = true;
            Some((r.cleanup.clone(), r.bounds))
        };
        Some((
            r.game.clone(),
            r.world.clone(),
            batch,
            cleanup,
            r.blocks.is_empty(),
        ))
    });
    let Some((game, world, batch, cleanup, done)) = job else {
        return;
    };
    let Some(w) = server.get_world_by_name(&world) else {
        tracing::warn!(
            "{}: can't reset arena '{game}': world '{world}' is not loaded",
            crate::ID
        );
        finish(&game);
        return;
    };
    if let Some((types, bounds)) = cleanup {
        let mut removed = 0;
        for e in w.get_entities() {
            let t = super::kits::snake(&format!("{:?}", e.get_type()));
            if !types
                .iter()
                .any(|x| x.trim_start_matches("minecraft:") == t)
            {
                continue;
            }
            let (x, y, z) = e.get_position();
            if bounds.contains(&crate::model::Location::new(x, y, z)) {
                e.remove();
                removed += 1;
            }
        }
        if removed > 0 {
            tracing::info!(
                "{}: removed {removed} entities from arena '{game}'",
                crate::ID
            );
        }
    }
    let flags = super::quiet_flags();
    for (pos, state_id) in batch {
        w.set_block_state(
            HostPos {
                x: pos.x,
                y: pos.y,
                z: pos.z,
            },
            state_id,
            flags,
        );
    }
    if done {
        finish(&game);
        tracing::info!("{}: arena '{game}' is reset", crate::ID);
    }
}

fn finish(game: &str) {
    state::with(|s| {
        if s.restores.front().is_some_and(|r| r.game == game) {
            s.restores.pop_front();
        }
        s.restore_done(game);
    });
    super::crashlog::remove(game);
}

/// Queues restores for crash logs left from the last run.
pub fn recover_leftovers() {
    let left = super::crashlog::leftovers();
    if left.is_empty() {
        return;
    }
    let names: Vec<String> = left
        .iter()
        .map(|(g, _, b)| format!("{g} ({} blocks)", b.len()))
        .collect();
    state::with(|s| {
        for (game, world, blocks) in left {
            let (bounds, cleanup) = match s.arenas.get(&game) {
                Some(a) => (a.bounds(), s.cfg.reset.cleanup_entities.clone()),
                None => (
                    crate::model::Cuboid::new(
                        crate::model::BlockPos::new(0, 0, 0),
                        crate::model::BlockPos::new(0, 0, 0),
                    ),
                    Vec::new(),
                ),
            };
            s.restores.push_back(state::Restore {
                game,
                world,
                blocks,
                bounds,
                cleanup,
                cleaned: false,
            });
        }
    });
    tracing::warn!(
        "{}: repairing arenas left unfinished by a crash: {}",
        crate::ID,
        names.join(", ")
    );
}
