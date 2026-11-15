# PumpkinMC Plugin Template

A minimal Rust starter for [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) server plugins. It builds to a single `.wasm` file you can upload or drop into a server.

Built on [`pumpkin-plugin-api`](https://crates.io/crates/pumpkin-plugin-api) 0.2.1, targeting `wasm32-wasip2` (WebAssembly component model).

## Quick start

```bash
rustup target add wasm32-wasip2   # once
scripts/build.sh                  # → dist/my_plugin.wasm
```

Copy `dist/<name>.wasm` into your server's `plugins/` folder, or upload it to the Pumpkin marketplace.

## Make it yours

1. In `Cargo.toml`, set `name`, `version`, `authors` and `description`. This is the only place to set them: the plugin reads them at compile time. Use `snake_case` for `name`, because it also becomes the plugin id, the permission namespace (`name:...`) and the `.wasm` filename.
2. Edit or replace the examples:
   - `src/commands.rs`: `/hello [name]` (alias `/hi`), with a registered permission.
   - `src/events.rs`: a custom yellow join message.
3. Run `scripts/build.sh`.

## Layout

```
Cargo.toml            plugin identity + size-optimized release profile
.cargo/config.toml    makes every cargo command target wasm32-wasip2
rust-toolchain.toml   pins stable + the wasm target, so rustup sets it up automatically
src/lib.rs            entry point: metadata, on_load / on_unload
src/commands.rs       slash commands
src/events.rs         event listeners
scripts/build.sh      release build → dist/<name>.wasm  (--debug for a fast build)
.github/workflows/    CI: fmt + clippy + build; attaches .wasm to releases on v* tags
AGENTS.md             API cheat sheet and conventions for AI assistants
```

## Commands

| Task | Command |
|---|---|
| Build an uploadable `.wasm` | `scripts/build.sh` |
| Type-check quickly | `cargo check` |
| Lint | `cargo clippy -- -D warnings` |
| Format | `cargo fmt` |
| Publish a release | `git tag v0.1.0 && git push --tags` (CI attaches the `.wasm`) |

## Notes

- **Permissions:** host capabilities (files, network, env) must be listed in `PluginMetadata.permissions`, using the constants in `pumpkin_plugin_api::permissions`. Server owners approve them.
- **Persistent data:** request `FS_READ_DATA` / `FS_WRITE_DATA`, then use `std::fs` under `context.get_data_folder()`.
- **Signing:** builds are unsigned. Pumpkin loads unsigned plugins by default (`allow_unsigned = true`); the marketplace handles signing.
- **Logging:** `tracing::info!` / `warn!` / `error!` print to the server console.
- **API docs:** <https://docs.rs/pumpkin-plugin-api>. To upgrade, bump the version in `Cargo.toml` and rebuild.
