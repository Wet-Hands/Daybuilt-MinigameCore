//! The Pumpkin plugin: metadata, load/unload and the IPC entry point.

use pumpkin_plugin_api::{Context, Plugin, PluginMetadata, Result, permissions, register_plugin};

use crate::host::{self, storage};
use crate::listeners::uuid_of;
use crate::{ID, commands, ipc, listeners, state, ticker};

struct MinigameCore;

impl Plugin for MinigameCore {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: ID.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: env!("CARGO_PKG_AUTHORS")
                .split(':')
                .map(Into::into)
                .collect(),
            description: env!("CARGO_PKG_DESCRIPTION").into(),
            // Optional integrations (PvP Manager, PumpkinPIE, mode plugins) are never listed:
            // a missing dependency would stop this plugin from loading.
            dependencies: vec![],
            // Arenas, modes, kits and stats live in the data folder.
            permissions: vec![
                permissions::FS_READ_DATA.into(),
                permissions::FS_WRITE_DATA.into(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> Result<()> {
        storage::set_data_dir(context.get_data_folder());
        let persist = storage::probe();
        if persist {
            storage::write_defaults();
        } else {
            tracing::error!(
                "{ID}: the data folder is not writable. Approve fs.read.data and fs.write.data \
                 (plugins.overrides.{ID}.allowed_permissions). Running with built-in defaults; \
                 arenas and stats will not be saved."
            );
        }
        state::with(|s| s.persist = persist);
        let summary = crate::model::text::strip(&commands_reload());
        if persist {
            let stats = storage::load_stats();
            state::with(|s| s.stats = stats);
            host::restore::recover_leftovers();
        }

        // A reload mid-game fires no join events: index who is online now.
        let server = context.get_server();
        let online: Vec<(String, String, i32)> = server
            .get_all_players()
            .iter()
            .map(|p| (uuid_of(p), p.get_name(), p.as_entity().get_id() as i32))
            .collect();
        state::with(|s| {
            for (u, n, id) in &online {
                s.player_online(u, n, *id);
            }
        });

        commands::register(&context);
        listeners::register(&context);
        ticker::start();
        tracing::info!(
            "{ID} v{} loaded: {summary} Commands: /mg (players), /dbmg (admin).",
            env!("CARGO_PKG_VERSION")
        );
        Ok(())
    }

    fn on_unload(&self, context: Context) -> Result<()> {
        // Stop every game and put arenas back now; players go to the lobby.
        let server = context.get_server();
        let effects = state::with(|s| {
            let ids: Vec<String> = s.games.keys().cloned().collect();
            let mut all = Vec::new();
            for id in ids {
                if let Some((_, e)) = s.step(&id, |g, _ctx, out| g.abort(out)) {
                    all.extend(e);
                }
            }
            all
        });
        host::apply(&server, effects);
        let mut guard = 0;
        while state::with(|s| !s.restores.is_empty()) && guard < 100_000 {
            host::restore::step(&server);
            guard += 1;
        }
        host::spawners::remove_all();
        host::pvp::restore_all();
        ticker::save_stats();
        tracing::info!("{ID} unloaded");
        Ok(())
    }

    fn handle_ipc_message(&self, sender: String, message: Vec<u8>) -> Result<Vec<u8>, String> {
        if sender == host::pie::PIE {
            return Ok(host::pie::handle(&message));
        }
        Ok(ipc::handle(&sender, &message))
    }
}

fn commands_reload() -> String {
    crate::commands::reload_all()
}

register_plugin!(MinigameCore);
