# AGENTS.md - PumpkinMC plugin (Rust → WebAssembly component)

## Build / verify (run after every change)
- `cargo check` — fast type check (target wasm32-wasip2 is preset in .cargo/config.toml).
- `cargo clippy -- -D warnings` and `cargo fmt` — must be clean (CI enforces).
- `cargo test --workspace --target x86_64-unknown-linux-gnu` — native unit tests (CI runs them).
- `scripts/build.sh` — produces `dist/<crate-name>.wasm`, the deliverable.
- There is no host to run tests against. `#[test]`s that call host functions can't work, but
  **pure-logic tests do**: the plugin crate (with `pumpkin-plugin-api` linked) builds and runs natively
  with `--target x86_64-unknown-linux-gnu` (verified here). Keep parsing, maths, storage and protocol code
  free of host calls so it can be tested this way.

## Rules
- Plugin identity lives ONLY in `Cargo.toml` `[package]` (read via `env!`). Never hardcode the name; use `crate::ID`.
- One concern per file. New feature → new module + one line in `on_load` (`src/lib.rs`).
- Plugin is `Send + Sync` and callbacks take `&self`. For mutable state use `static X: Mutex<..>` / `OnceLock` / atomics.
  Never hold a lock across a host API call (the host may re-enter the plugin synchronously).
- Request host capabilities in `metadata().permissions` using `pumpkin_plugin_api::permissions::*` constants. Request the minimum.
- Permission nodes MUST be `"{ID}:..."` (host rejects other namespaces).
- Don't panic in handlers; return `Err(CommandError::CommandFailed(TextComponent::text("..")))` or log with `tracing::warn!`.
- API source of truth: `~/.cargo/registry/src/*/pumpkin-plugin-api-0.2.1*/` (`src/` for Rust helpers, `wit/*.wit` for every host function).
  WIT `kebab-case` → Rust `snake_case`; WIT resources → Rust structs; `%name` → `name`.

## How the Pumpkin host behaves (verified against Pumpkin source)
### Plugin permissions & loading
- Any entry in `metadata().permissions` makes the server ask `[y/N]` on the console before loading.
  If nobody answers (hosting panels, no TTY), the plugin is **silently skipped** and the denial is cached
  in `plugins/permission_cache.json` (keyed by the .wasm hash, so a rebuild prompts again).
- Owners can pre-approve: `[plugins.overrides.<ID>] allowed_permissions = ["fs.read.data", "fs.write.data"]`
  (dot form, exactly as the `permissions::*` constants). `ask_permission_confirmation = false` auto-approves all.
- A plugin with an empty permission list loads with no prompt. If a feature needs a capability, consider a
  cargo feature that adds it, so a permission-free build is still possible.
- Log a clear line at the end of `on_load` — it's the only easy way for owners to confirm the plugin loaded.
  Register modules independently and log failures so one error doesn't disable everything.

### Files / data folder
- `context.get_data_folder()` returns `"data"`: a WASI preopen of `plugins/data/<ID>/` that exists **only**
  if `FS_READ_DATA`/`FS_WRITE_DATA` was granted. Without it, `std::fs` calls fail — fall back to defaults.
- Pattern: embed defaults with `include_str!`, write them to the data folder on first load, re-read on a reload command.

### Events
- `PlayerJoinEvent` fires after the player is spawned; for titles/effects, a short `schedule_delayed_task` is safer.
- Cancelling `PlayerInteractEvent` cancels the item's use (e.g. a bow can't be drawn, food can't be eaten).
- Bows only draw if an arrow is in the inventory (unless creative). `EntityShootBowEvent` fires only for
  mobs; for player arrows use `ProjectileLaunchEvent` (`shooter_id`), which fires **before** velocity is set
  and whose cancel is ignored — act one tick later.
- Entity events (damage, food, …) carry only `entity_id`. Map to a player by comparing
  `player.as_entity().get_id() as i32` over `server.get_all_players()`.
- `InventoryClickEvent.window_type` is `Some` for container screens; track your own GUIs (e.g. a set of
  player names cleared on `InventoryCloseEvent`) and cancel clicks in them.

### Commands
- **Never use `CommandNode::suggest(..)` (server-side tab completion).** Pumpkin 0.2.2's wasm host calls
  `tokio::runtime::Handle::current()` from the world-tick thread when a player tab-completes such an
  argument and the whole server panics ("there is no reactor running"). Use literal nodes or built-in
  argument types (`Players`, `Bool`, ...) instead, which the client completes itself.

### Items, text, misc
- Mark plugin items with `item.set_custom_data(ID, "tag", &NbtTree { root: 0, tags: vec![NbtTag::Byte(1)] })`
  and check `has_custom_data` — never rely on display names.
- Host methods take owned values (`TextComponent`, `World`, `Vec<_>`); `TextComponent` is not `Clone`
  (use `std::mem::replace` to take one out of event data). `from_legacy_string_with_code(s, '&')` for `&` codes.
- `player.play_custom_sound("minecraft:entity.player.levelup", ..)` accepts any sound id string.
- Sound category for players is `wit::pumpkin::plugin::sounds::SoundCategory` (not `command_wit`'s).
- Proxy server switch (Velocity/BungeeCord): `player.as_java()?.send_custom_payload("bungeecord:main", data)`
  where `data` = writeUTF("Connect") + writeUTF(server) (u16 BE length + bytes each).
  Velocity needs `bungee-plugin-message-channel = true`.
- Pumpkin source (useful when behaviour is unclear): github.com/Pumpkin-MC/Pumpkin —
  `crates/pumpkin-core/src/plugin/` (loading/permissions), `crates/pumpkin-wasm-host*/` (host functions),
  grep `XxxEvent::new` in `crates/pumpkin-core/src` to see when an event fires.

## PumpkinPIE placeholders (optional integration)
[PumpkinPIE](https://github.com/Rennex07/PumpkinPIE) is a PlaceholderAPI-style provider plugin. This plugin
supplies an "expansion" for it: `src/placeholders.rs` (protocol + registration) and `src/names.rs`
(name → uuid index). Protocol reference: `docs/protocol.md` and `docs/expansions.md` in that repo.

### Adding a placeholder
1. Add the name to `advertised()` in `src/placeholders.rs` (advertised in `/pie`; not a filter).
2. Add a `match` arm in `resolve()`. `name` arrives **lowercased**; return `None` to decline (the
   placeholder is then left in the text).
3. Display texts go in `[placeholders]` in `config/config.toml` + the `Placeholders` struct in `config.rs`.
4. List it in the `config.toml` comment block and the README table.

### Rules (each one breaks something if ignored)
- **Wire format:** UTF-8 JSON over `pumpkin_plugin_api::ipc::send_ipc_message("PumpkinPIE", bytes)`.
  Requests carry `"op"`. Replies are `{"ok":true,"<variant>":{...}}`. The body is wrapped in a key named
  after the result (`on_request`, `registered_expansion`, …), and a flat body is rejected.
  To decline, send `{"ok":true,"on_request":{}}`. A bare `{"ok":true}` is a protocol error.
  The `pumpkin-pie` client crate is not on crates.io and pins an exact API version, so we hand-roll the
  JSON with `serde_json`. Check changes by decoding with that crate's `protocol` types in a native
  scratch crate (it builds on the host).
- **Never list `"PumpkinPIE"` in `metadata().dependencies`.** If a listed dependency is missing,
  Pumpkin's dependency sort fails and the plugin doesn't load. Register from the ticker instead,
  retrying with backoff until it answers (`Err(())` from `send_ipc_message` = not loaded).
- **`on_request` must be a pure in-memory lookup.** PIE calls it synchronously from inside its own
  IPC handler. Calling PIE back deadlocks. Calling the host or other plugins stalls every consumer.
  No `Server`/`Player` is available, and `viewer` is a player **name** (resolve via `names::uuid_for`).
- **Message PIE as rarely as possible.** Pumpkin runs each plugin's calls one at a time. If we message PIE
  while PIE is waiting on our `on_request`, both block forever. So: register at startup (until it
  succeeds), on a config change, and on `/eco reload`. No periodic refresh. Never call PIE from an
  event or command handler directly; set state (`placeholders::reregister()`) and let the ticker send it.
- **PIE forgets expansions when it reloads**, and there is no push from it. `/eco reload` re-registers.
- **Sender check:** `handle_ipc_message` routes messages whose sender is `"PumpkinPIE"` to
  `placeholders::handle`; everything else is the plain-text API in `ipc.rs`. Keep those two separate.
- **Namespace:** lowercase letters/digits, starts with a letter, **no `_`** (namespace = text before
  the first `_`), not `player`/`server`/`pie`. Validated in `config.rs`. Placeholder names may contain `_`;
  ids may not contain `-` or `.`. If two plugins use the same namespace, the last one to load wins with no warning.
- **Caching:** `cache_ms` → `{"kind":"ttl","ms":N}` (PIE clamps to 60 s), `0` → `{"kind":"never"}`.
  Values change in real time (combat timer), so keep the default TTL short.
- **Don't send `unregister_expansion` from `on_unload`.** PIE has usually unloaded already, and messaging
  a stopped plugin traps the guest at shutdown. Unregister only at runtime (namespace change / disabled).

## More host / toolchain facts
Verified = compiled, ran in native tests, or read in the API source. Unverified = reasoned, not yet
seen on a live server. Keep that distinction when you add to this list.
- **IPC signatures (verified in source).** Send: `pumpkin_plugin_api::ipc::send_ipc_message(id, &bytes)`
  returns `Result<Result<Vec<u8>, String>, ()>`: outer `Err(())` = target not loaded, inner `Err(msg)` =
  the target's handler returned `Err`. Receive: override
  `fn handle_ipc_message(&self, sender: String, message: Vec<u8>) -> Result<Vec<u8>, String>` on your
  `Plugin`. `sender` is supplied by the host, so it is safe to use for allow-lists and audit logs;
  never trust a plugin id written inside the message.
- **IPC is synchronous with no timeout.** An IPC handler that calls the host or another plugin can
  deadlock both. Make handlers pure in-memory (plus file IO, which is WASI, not a host call).
- **Shared protocol crate pattern.** Put the wire types in a small crate (`client/`) with an optional
  `ipc` feature for the typed send functions. The server depends on it with `default-features = false`;
  the protocol module then also builds natively for tests. The client must depend on
  `pumpkin-plugin-api` with a caret requirement (`"0.2.1"`) so it unifies with the consumer's copy
  (unverified, reasoned: two copies would both export the plugin world and fail to link).
- **Cargo workspace in a plugin repo.** Add `[workspace] members = ["client"]` and
  `default-members = [".", "client"]` to the root `Cargo.toml` so `cargo check`/`clippy`/`build` cover
  both. Keep `[package]` first: `scripts/build.sh` reads the first `name =` line.
- **Optional command arguments.** Give each branch its own handler value (`Balance(0)`, `Balance(1)`, …)
  instead of reading an absent argument; what `ConsumedArgs::get_value` returns for a missing key is
  not documented. Use `ArgumentType::String(StringType::SingleWord)` for player names when offline
  players must work (`Players` only matches online players) and for decimal amounts.
- **Offline players.** The host only looks players up while online (`get_player_by_name/uuid`). Keep
  your own name → UUID index, updated from `PlayerJoinEvent` (names change; drop the old mapping).
  On `on_load`, also walk `context.get_server().get_all_players()`: a reload mid-game fires no joins.
- **UUIDs.** `player.get_id()` is `Uuid { high, low }` and implements `Display` (lowercase, hyphenated).
  `uuid::parse` is a host call; parse UUID strings yourself in pure code.
- **Probe the data folder.** `get_data_folder()` returns a path even if no permission was granted. Write
  and delete a small probe file in `on_load` to know whether persistence actually works, and log loudly
  if not.
- **Persistence that survives crashes.** Append each change to a JSON-lines log *before* changing memory
  (fail the operation if the append fails), record the resulting value (so replay is idempotent), and
  rewrite a snapshot periodically via temp file + rename. On start, replay log entries newer than the
  snapshot's sequence number. Terminate a torn last line before the next append. See `src/storage/`.
- **Config with defaults.** `toml = { version = "0.9", default-features = false, features = ["parse", "serde"] }`
  builds for wasm32-wasip2. Parse the user file and the embedded default as `toml::Table`, deep-merge
  user over default, then deserialize: owners can delete keys and new keys get defaults automatically.
  Add a native test that the embedded default parses, then a `LazyLock` `expect` on it is safe.
- **Money/quantities:** store integers in the smallest unit, never floats. Reject float literals in config
  (`#[serde(untagged)] enum { Units(i64), Decimal(String) }`).
- **`tracing` macro gotcha (verified):** a local variable named `display` (or `debug`) used inside
  `tracing::warn!("{display}")` resolves to tracing's `display` function → E0277. Rename the variable.
- **Clippy on current stable (verified):** wants `n.is_multiple_of(k)` instead of `n % k == 0`, and
  collapsed `if a && let Some(x) = ..` chains (edition 2024).
- **Logging is a host call.** Don't `tracing::*!` while holding a lock; return errors/notes and log
  after releasing it.

## API cheat sheet (pumpkin-plugin-api 0.2.1 — all verified to compile)
```rust
use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Result, Server, register_plugin,
    command::{ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType},
    command_wit::Arg,                       // values from args.get_value("name")
    commands::CommandHandler,
    events::{EventData, EventHandler, EventPriority, PlayerJoinEvent /* , BlockBreakEvent, ... */},
    permission::{Permission, PermissionDefault, PermissionLevel},
    common::NamedColor,
    text::TextComponent,
    permissions,                            // capability constants: FS_READ_DATA, HTTP_OUTBOUND, ...
    scheduler,                              // schedule_delayed_task / schedule_repeating_task
};
```

### Command
```rust
let cmd = Command::new(&["name".into(), "alias".into()], "Description")
    .execute(MyHandler)                                         // runs on bare `/name`
    .then(CommandNode::literal("sub").execute(SubHandler))      // `/name sub`
    .then(CommandNode::argument("n", &ArgumentType::Integer((Some(1), None))).execute(NHandler));
// in commands.rs: register_command(context, cmd, "command.name", "desc", PermissionDefault::Allow)?;
//   admin-only: PermissionDefault::Op(PermissionLevel::Two)

struct MyHandler;
impl CommandHandler for MyHandler {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if let Arg::Num(Ok(pumpkin_plugin_api::command_wit::Number::Int32(n))) = args.get_value("n") { /* ... */ }
        sender.send_message(TextComponent::text("hi"));   // also send_error, as_player() -> Option<Player>
        Ok(1)
    }
}
```
ArgumentType variants: `Bool, Float((min,max)), Double(..), Integer(..), Long(..), String(StringType::{SingleWord,Quotable,Greedy}),
Entities, Entity, Players, GameProfile, BlockPos, ...` — see `wit/command.wit` `variant argument-type`.

### Event
```rust
context.register_event_handler::<PlayerJoinEvent, _>(MyListener, EventPriority::Normal, /*blocking*/ true)?;

struct MyListener;
impl EventHandler<PlayerJoinEvent> for MyListener {
    fn handle(&self, server: Server, mut e: EventData<PlayerJoinEvent>) -> EventData<PlayerJoinEvent> {
        // e.player, e.join_message, e.cancelled  (fields: wit/event.wit `record player-join-event-data`)
        e
    }
}
```
`blocking = true` → you may modify/cancel. Event list: `src/events/{player,block,entity,world,server,inventory,...}/` in the crate
(type name = file name in PascalCase + `Event`, e.g. `block_break.rs` → `BlockBreakEvent`).

### Text
`TextComponent::text("x").color_named(NamedColor::Gold).bold(true)` (chainable), `TextComponent::from_legacy_string("§aHi")`.

### Scheduler (20 ticks = 1s)
`scheduler::schedule_repeating_task(0, 20 * 60, |server: Server| { /* every minute */ });`

### Player
`player.get_name()`, `player.send_system_message(text, /*overlay(actionbar)*/ false)`, more in `wit/player.wit`.

### Persistent data
Add `permissions::FS_READ_DATA` / `FS_WRITE_DATA` to metadata, then `std::fs` under `context.get_data_folder()`.

Data folder (`plugins/data/rustedvault/`): `config.toml`, `accounts.json` (snapshot), `ledger.jsonl`
(every change: seq, time, op, player, currency, amount, balance after, reason, source).

### Invariants (don't break these)
- **No read-modify-write.** Every balance change is one `Storage` call that checks and changes in one
  step (`deposit`, `withdraw`, `transfer`, `set`). There is no general "write balance" except admin `set`.
- **The ledger is written first.** If `ledger::append` fails, the operation returns `Storage` and memory
  is unchanged. Snapshots are only a speed-up; never write balances without a ledger entry.
- **No host calls under the store lock** (`economy::with_store`), including logging. `Storage`
  implementations must be pure + file IO.
- **`api.rs` and `placeholders::handle` never call the host or another plugin.** Callers may invoke the
  API from anywhere, and PIE calls `on_request` synchronously.
- **Amounts are `i64` smallest units** everywhere except user-facing text (`Currency::parse_amount`,
  `Currency::display`, `Currency::plain`).
- **Protocol changes are additive.** New request fields need `#[serde(default)]`; new ops/errors are
  fine; changing a field's meaning needs `PROTOCOL_VERSION` bumped and the old version still answered.
- A cross-server backend (MariaDB) goes in `src/storage/` as another `Storage` impl, each method one
  conditional SQL statement. Nothing above `storage/` should change.

### Status
Built, clippy/fmt clean, 12 native tests pass (currency, config, storage + replay, protocol, the IPC API
and placeholders end-to-end against in-memory storage). **Not yet run on a live Pumpkin server.**
Still to check there: IPC between two real plugins (and with the target missing), file writes from the
sandbox after approving `fs.read.data`/`fs.write.data`, `rename` inside the WASI preopen (there is a
fallback), and whether the host passes our `has_permission` check for `rustedvault:command.balance.others`.

## Integrating with RustedVault
Any plugin can use the economy without depending on it at load time.

### Rust plugins: use the client crate
```toml
[dependencies]
rustedvault-client = { git = "https://github.com/Wet-Hands/RustedVault" }
```
```rust
use rustedvault_client as vault;            // all amounts: i64 in the currency's smallest unit
let id = player.get_id();                   // pumpkin_plugin_api::uuid::Uuid
match vault::withdraw(&id, None, 2_500, "shop: diamond sword") {   // None = default currency
    Ok(new_balance) => { /* deliver the item */ }
    Err(vault::VaultError::InsufficientFunds { balance, needed }) => { /* tell the player */ }
    Err(vault::VaultError::Unavailable) => { /* RustedVault not installed: feature off */ }
    Err(e) => tracing::warn!("economy: {e}"),
}
```
Functions: `is_available`, `currencies`, `has_account`, `lookup(name)` (offline-safe name → UUID),
`balance`, `deposit`, `withdraw`, `transfer`, `set`, `top`, `format(currency, amount)`, and raw `call`.

Rules:
- **Do not list `rustedvault` in `metadata().dependencies`** unless your plugin is useless without it.
  Treat `VaultError::Unavailable` as "economy disabled" and keep working.
- **Never read a balance, compute, and `set` it.** Use `withdraw` (checks funds in the same step),
  `deposit`, or `transfer`. `set` is refused unless the owner lists your plugin id in
  `[api] set_allowed_plugins`; the owner can also limit all API use with `[api] allowed_plugins`.
- **Pass a meaningful `reason`** (`"shop: diamond sword"`). It goes into the ledger with your plugin id.
- **Convert user input with the currency's `decimals`** (from `currencies()`): 12.50 with 2 decimals is
  `1250`. Show amounts with `format()` so they match `/balance`.
- **Accounts exist only for players who have joined** since RustedVault was installed
  (`VaultError::NoSuchAccount` otherwise). Paying an offline player by UUID works if they have joined once.
- Calls are synchronous IPC. RustedVault's handler never calls back, so calling it from a command,
  event or task is safe. Don't call it in a hot loop every tick; batch or cache.
- Keep `pumpkin-plugin-api` on a version compatible with the client crate's (`0.2.x`).

### Other languages / no client crate
Send UTF-8 JSON to plugin id `rustedvault` with `send-ipc-message`. Request:
`{"v":1,"op":"deposit","player":"<uuid>","currency":"coins","amount":1250,"reason":"quest"}`
(`currency` optional). Reply: `{"ok":true,"balance":{"currency":"coins","amount":11250}}` or
`{"ok":false,"error":{"kind":"insufficient_funds","balance":100,"needed":250},"message":"..."}`.
Ops: `ping`, `currencies`, `has_account`, `lookup`, `balance`, `deposit`, `withdraw`, `transfer`
(`from`,`to`), `set`, `top` (`limit`), `format`. Error kinds: `no_such_account`, `no_such_currency`,
`insufficient_funds`, `over_maximum`, `invalid_amount`, `same_account`, `denied`, `bad_request`,
`storage`. The full definition is `client/src/protocol.rs`.

### No IPC at all: console commands
`server.execute_command("eco give <player> <amount> [currency]", pumpkin_plugin_api::server::CommandSender::Console)`
(also `take`, `set`; `server::CommandSender` is the WIT variant `console | player(player)`, verified in
`wit/server.wit`, not yet run). Simple for rewards, but you get no result back; prefer the API when you need to know
whether it worked (for example taking payment).

### Placeholders
If PumpkinPIE is installed: `%rustedvault_balance%`, `%rustedvault_balance_<currency>%` (`_raw` for a
plain number), `%rustedvault_currency_name|plural|symbol%`, `%rustedvault_top_<rank>_name|amount%`.
The namespace is configurable; see the README.

- **Don't list `daybuilt_pvp_manager` in `dependencies`** unless your plugin can't work without it. If a listed dependency is missing, Pumpkin won't load your plugin. Handle `Err(())` ("not loaded") instead.
- **Load order.** IPC only works once both plugins are loaded. Call from events, commands or scheduled tasks, not during your own `on_load`.
- **Don't call from inside a reply to this plugin.** Pumpkin runs each plugin's calls one at a time. Calls are synchronous, and if both plugins message each other at the same moment, both wait forever.
- **Calls are cheap** (in-memory lookups), but they block your handler. Don't put them in tight loops over many players every tick; cache the result if needed.
- **The API version is `1`.** New operations may be added. Existing replies keep their meaning within a version.

---

## Daybuilt PvP Manager Integration

Other Pumpkin plugins can read and control PvP state in three ways:

| Method | Best for | Reply? |
|---|---|---|
| [IPC messages](#1-ipc-messages-recommended) | Reading and changing state from code | Yes |
| [Console commands](#2-running-dbpvp-as-console) | Quick integration, scripts, command-only plugins | No (the result goes to the console) |
| [PumpkinPIE placeholders](#3-pumpkinpie-placeholders) | Showing values in scoreboards, tab lists, chat | Text only |

The plugin id is **`daybuilt_pvp_manager`**.

---

### 1. IPC messages (recommended)

Send a **UTF-8 text** message to `daybuilt_pvp_manager` with Pumpkin's inter-plugin IPC. The reply is UTF-8 text.

```rust
use pumpkin_plugin_api::ipc::send_ipc_message;

/// Sends one request to Daybuilt PvP Manager. Returns the reply text.
fn dbpvp(request: &str) -> Result<String, String> {
    match send_ipc_message("daybuilt_pvp_manager", request.as_bytes()) {
        Ok(Ok(reply)) => String::from_utf8(reply).map_err(|_| "reply was not UTF-8".to_string()),
        Ok(Err(error)) => Err(error),                               // the plugin refused the request
        Err(()) => Err("daybuilt_pvp_manager is not loaded".into()),  // plugin missing
    }
}

// Examples
let can_fight = dbpvp("pvp.can Steve Alex")? == "allowed";
let in_combat = dbpvp("combat.get Steve")?.parse::<u64>().unwrap_or(0) > 0;
dbpvp("pvp.set Steve false")?;
```

### Format

- Words are separated by spaces: `<operation> <arguments...>`. The operation is case-insensitive.
- `<player>` is an **online player's name** (case-insensitive) or a **UUID** (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
  - Offline players can only be given by UUID, and only some operations accept them (see the table below).
- Booleans accept `true/false`, `on/off`, `yes/no`, `1/0`.
- **Errors** come back as the inner `Err(String)` with a readable reason, for example `player 'Bob' is not online`, `expected true|false` or `unknown operation 'foo'`.

### Operations

| Message | Reply | Offline UUID OK? |
|---|---|---|
| `version` | API version, currently `1` | — |
| **PvP toggle** | | |
| `pvp.get <player>` | `true` / `false` (the player's own setting, or the config default) | ✅ |
| `pvp.set <player> <bool>` | `ok`; saved to `players.toml` | ✅ |
| `pvp.can <attacker> <victim>` | `allowed`, `gamerule`, `attacker_disabled` or `victim_disabled` | ❌ both online |
| **Classic (1.8) combat** | | |
| `classic.get <player>` | `true` / `false` (the mode their hits use) | ✅ |
| `classic.set <player> <bool>` | `ok`. Saved even if `classic_combat.allow_player_toggle` is off, but only used while it is on | ✅ |
| **Combat tag** | | |
| `combat.get <player>` | seconds left in combat, `0` if not tagged | ✅ |
| `combat.opponent <player>` | name of the last opponent (empty if none) | ✅ |
| `combat.tag <player> [seconds] [opponent name…]` | `ok`, or `bypassed` (combat log disabled, or the player has the bypass permission) | ❌ online |
| `combat.untag <player>` | `true` if they were tagged, else `false` | ✅ |
| **Config** | | |
| `config.get <key>` | the current value as TOML (e.g. `15`, `true`, `"text"`, `["a", "b"]`) | — |
| `config.set <key> <value…>` | the new value as TOML; saved to `config.toml` and applied at once | — |

#### What `pvp.can` means

`pvp.can` runs the same check the plugin uses on every hit:

| Reply | Reason |
|---|---|
| `allowed` | The hit would go through |
| `gamerule` | The world's `pvp` gamerule is false, and `pvp.gamerule_mode` is `respect` or `force` |
| `attacker_disabled` | The attacker has PvP off |
| `victim_disabled` | The victim has PvP off |

`pvp.can` does not check the attacker's own combat tag, and it does not tag anyone.

#### `combat.tag` details

- Without `[seconds]`, the tag lasts `combat_log.duration_seconds`.
- A new tag never shortens a running one: the longer of the two is kept.
- Without an opponent name, your plugin's id is shown as the opponent (in the action bar and in `%dbpvp_combat_opponent%`).
- While tagged, the player is under all combat-log rules: restricted commands, logout punishment, and no turning PvP off. This makes it useful for arenas, bosses, or "combat" from your own mechanics.

#### Config keys

Keys are dotted paths from `config.toml`, such as `combat_log.duration_seconds` or `pvp.gamerule_mode`. The value is parsed as TOML. If it doesn't fit the key's type, it is retried as a plain string. Invalid values are rejected with an error and nothing changes.

```text
config.set combat_log.duration_seconds 20
config.set pvp.gamerule_mode ignore
config.set combat_log.commands.list ["spawn", "home", "warp"]
config.set general.prefix &8[&cArena&8]&r
```

### Rules for callers

- **Don't list `daybuilt_pvp_manager` in `dependencies`** unless your plugin can't work without it. If a listed dependency is missing, Pumpkin won't load your plugin. Handle `Err(())` ("not loaded") instead.
- **Load order.** IPC only works once both plugins are loaded. Call from events, commands or scheduled tasks, not during your own `on_load`.
- **Don't call from inside a reply to this plugin.** Pumpkin runs each plugin's calls one at a time. Calls are synchronous, and if both plugins message each other at the same moment, both wait forever.
- **Calls are cheap** (in-memory lookups), but they block your handler. Don't put them in tight loops over many players every tick; cache the result if needed.
- **The API version is `1`.** New operations may be added. Existing replies keep their meaning within a version.

---

### 2. Running `/dbpvp` as console

A plugin that can't (or doesn't want to) use IPC can run the normal command as the console. The console has every permission, so the admin forms work:

```rust
use pumpkin_plugin_api::server::CommandSender;

server.execute_command("dbpvp pvp false Steve", CommandSender::Console);
```

| Command | Effect |
|---|---|
| `dbpvp pvp <true\|false> <players>` | Set PvP (selectors such as `@a` work) |
| `dbpvp classic <true\|false> <players>` | Set classic combat (needs `classic_combat.allow_player_toggle = true`) |
| `dbpvp tag <players> [seconds]` | Put players in combat (1–3600 s) |
| `dbpvp untag <players>` | Remove the combat tag |
| `dbpvp status <players>` | Print state to the console |
| `dbpvp config <key> <value>` | Change a config value |
| `dbpvp reload` | Re-read `config.toml` |

The targeted player gets a chat message for `pvp` and `classic`. The reply goes to the console, so your plugin can't read it. Use IPC when you need the result.

---

### 3. PumpkinPIE placeholders

If [PumpkinPIE](https://github.com/Rennex07/PumpkinPIE) is installed, values are available as placeholders through its API. This is the right choice when you only need to **display** values:

`%dbpvp_pvp%`, `%dbpvp_pvp_status%`, `%dbpvp_classic%`, `%dbpvp_combat_mode%`, `%dbpvp_in_combat%`, `%dbpvp_combat_status%`, `%dbpvp_combat_time_remain%`, `%dbpvp_combat_time_formatted%`, `%dbpvp_combat_opponent%`, `%dbpvp_combat_duration%`

- **Other players:** add `:<player>` to read someone else, for example `%dbpvp_in_combat:Steve%`.
- **Namespace:** server owners can change `dbpvp` with `placeholders.namespace`.
- **Freshness:** values may be cached for up to `placeholders.cache_ms` (default 1 s).

See the README's "PumpkinPIE placeholders" section for the full table.

---

### Permission nodes

These are useful if your plugin grants or checks them. They are all in the `daybuilt_pvp_manager:` namespace:

| Node | Meaning |
|---|---|
| `daybuilt_pvp_manager:combatlog.bypass` | Never combat tagged, never punished |
| `daybuilt_pvp_manager:admin` | Admin forms of `/dbpvp` |
| `daybuilt_pvp_manager:pvp.toggle` | May toggle own PvP |
| `daybuilt_pvp_manager:classic.toggle` | May toggle own classic combat |
| `daybuilt_pvp_manager:command.dbpvp` | May use `/dbpvp` at all |


## Keeping this file current (do this at the end of every task)
Before you finish any task in this repo, update this AGENTS.md:
1. Add what you learned that the next agent (on this plugin or any Pumpkin plugin) would otherwise have
   to rediscover: host behaviour, API signatures, toolchain or clippy surprises, patterns that worked.
   Mark each fact as verified (compiled/ran/read in source) or unverified (reasoned).
2. Correct anything here that turned out to be wrong, rather than adding a contradicting note.
3. Keep this section, including this instruction, so every future agent does the same.
