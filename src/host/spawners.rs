//! Spawner pickups (stockpile mode) and the floating labels above spawners.

use std::cell::RefCell;
use std::collections::HashMap;

use pumpkin_plugin_api::{
    EntityType, Server, display::TextDisplayEntity, display_wit::BillboardMode, world::Entity,
};

use crate::model::Location;
use crate::state;

struct Label {
    entity: Entity,
    text: String,
}

thread_local! {
    // (game, spawner index) → label entity. WASM plugins are single-threaded.
    static LABELS: RefCell<HashMap<(String, usize), Label>> = RefCell::new(HashMap::new());
}

/// Gives stockpiled items to players standing on spawners.
/// `positions`: (uuid, world, location) of online players in games.
pub fn pickups(server: &Server, positions: &[(String, String, Location)]) {
    let candidates: Vec<(String, usize, String, String, u32)> = state::with(|s| {
        let radius = s.cfg.spawners.pickup_radius;
        s.games
            .values()
            .flat_map(|g| {
                let mine: Vec<_> = positions
                    .iter()
                    .filter(|(u, _, _)| g.players.contains_key(u))
                    .cloned()
                    .collect();
                g.pickups(&mine, radius)
                    .into_iter()
                    .map(|(i, u, item, n)| (g.id.clone(), i, u, item, n))
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    for (game, idx, uuid, item, count) in candidates {
        let Some(p) = super::player(server, &uuid) else {
            continue;
        };
        let given = super::kits::give(&p, &item, count);
        if given > 0 {
            p.play_custom_sound(
                "minecraft:entity.item.pickup",
                pumpkin_plugin_api::wit::pumpkin::plugin::sounds::SoundCategory::Players,
                0.4,
                1.6,
            );
        }
        state::with(|s| {
            if let Some(g) = s.games.get_mut(&game) {
                g.pickup_done(idx, given);
            }
        });
    }
}

/// Creates/updates labels once a second.
pub fn refresh_labels(server: &Server) {
    let wanted: Vec<(String, usize, String, Location, String)> = state::with(|s| {
        let now = s.now;
        s.games
            .values()
            .flat_map(|g| {
                g.hologram_texts(now).into_iter().map(|(i, t)| {
                    let at = g.arena.spawners[i].at;
                    (g.id.clone(), i, g.arena.world.clone(), at, t)
                })
            })
            .collect()
    });
    for (game, idx, world, at, label) in wanted {
        let key = (game, idx);
        let existing = LABELS.with(|m| m.borrow().get(&key).map(|l| l.text == label));
        match existing {
            Some(true) => {}
            Some(false) => LABELS.with(|m| {
                if let Some(l) = m.borrow_mut().get_mut(&key) {
                    if let Some(d) = TextDisplayEntity::from_entity(&l.entity) {
                        d.set_text(super::colored(&label));
                    }
                    l.text = label.clone();
                }
            }),
            None => {
                let Some(w) = server.get_world_by_name(&world) else {
                    continue;
                };
                let entity = w.spawn_entity(EntityType::TextDisplay, (at.x, at.y + 1.6, at.z));
                if let Some(d) = TextDisplayEntity::from_entity(&entity) {
                    d.set_text(super::colored(&label));
                    d.get_display().set_billboard(BillboardMode::Center);
                }
                LABELS.with(|m| {
                    m.borrow_mut().insert(
                        key,
                        Label {
                            entity,
                            text: label,
                        },
                    )
                });
            }
        }
    }
}

/// Removes a game's labels.
pub fn remove_holograms(game: &str) {
    let gone: Vec<Label> = LABELS.with(|m| {
        let mut m = m.borrow_mut();
        let keys: Vec<_> = m.keys().filter(|(g, _)| g == game).cloned().collect();
        keys.into_iter().filter_map(|k| m.remove(&k)).collect()
    });
    for l in gone {
        l.entity.remove();
    }
}

/// Removes every label (plugin unload).
pub fn remove_all() {
    let gone: Vec<Label> = LABELS.with(|m| m.borrow_mut().drain().map(|(_, l)| l).collect());
    for l in gone {
        l.entity.remove();
    }
}
