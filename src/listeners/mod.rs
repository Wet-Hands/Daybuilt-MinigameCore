//! Pumpkin event handlers. Each one decides under the state lock, releases it, then
//! does host work.
//!
//! Note (verified in Pumpkin source): handlers run in plugin load order; the
//! priority value is not used for ordering. Mode plugins that list this plugin as a
//! dependency load after it, so their handlers see the core's decision first.

mod blocks;
mod combat;
mod players;

use pumpkin_plugin_api::{Context, Server, player::Player};

use crate::host::uuid_string;
use crate::state;

pub fn register(context: &Context) {
    for (name, result) in [
        ("players", players::register(context)),
        ("blocks", blocks::register(context)),
        ("combat", combat::register(context)),
    ] {
        if let Err(e) = result {
            tracing::error!("{}: could not register {name} listeners: {e}", crate::ID);
        }
    }
}

pub fn uuid_of(p: &Player) -> String {
    uuid_string(&p.get_id())
}

/// Player uuid for an entity id, refreshing the index from the server on a miss.
pub fn entity_player(server: &Server, entity_id: i32) -> Option<String> {
    if let Some(u) = state::with(|s| s.entities.get(&entity_id).cloned()) {
        return Some(u);
    }
    let players = server.get_all_players();
    let found: Vec<(String, String, i32)> = players
        .iter()
        .map(|p| (uuid_of(p), p.get_name(), p.as_entity().get_id() as i32))
        .collect();
    state::with(|s| {
        for (u, n, id) in &found {
            s.player_online(u, n, *id);
        }
        s.entities.get(&entity_id).cloned()
    })
}

/// Does the player have `<ID>:<node>`?
pub fn has(p: &Player, node: &str) -> bool {
    p.has_permission(&format!("{}:{node}", crate::ID))
}
