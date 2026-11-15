//! Plugin entry point. Wire up commands/events here; put logic in modules.
//!
//! Layout (one concern per file — add new files, register them in `on_load`):
//! - `commands.rs` — slash commands
//! - `events.rs`   — event listeners

mod commands;
mod events;

use pumpkin_plugin_api::{Context, Plugin, PluginMetadata, Result, register_plugin};

/// Plugin id from Cargo.toml. Also the permission namespace (`<ID>:...`).
pub const ID: &str = env!("CARGO_PKG_NAME");

struct MyPlugin;

impl Plugin for MyPlugin {
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
            // Other plugin ids this plugin needs loaded first.
            dependencies: vec![],
            // Host capabilities, e.g. `pumpkin_plugin_api::permissions::FS_WRITE_DATA`.
            // Request only what you use; server owners must approve each one.
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<()> {
        commands::register(&context)?;
        events::register(&context)?;
        tracing::info!("{ID} v{} loaded", env!("CARGO_PKG_VERSION"));
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<()> {
        // Commands and listeners are removed by the server automatically.
        tracing::info!("{ID} unloaded");
        Ok(())
    }
}

register_plugin!(MyPlugin);
