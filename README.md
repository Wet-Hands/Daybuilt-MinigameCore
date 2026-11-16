# Daybuilt MinigameCore

The shared base for round-based minigames on [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin): arenas, teams, matchmaking, the round lifecycle, kits, item spawners, the sidebar, stats and putting the map back after every round.

It ships as one WebAssembly plugin, `daybuilt_minigame_core.wasm`. Game modes such as Bedwars, Duels, Hunger Games, Spleef, TNT Run or COD Zombies are **separate plugins** that talk to it over IPC through the [`minigamecore-client`](client/) crate. Simple modes need no plugin at all: the bundled `spleef` mode runs from its config file alone.

Several games, of different modes, can run on one server at the same time, one per arena. Matchmaking puts players into an arena that can run the mode they asked for with the right team layout.

**Status:** builds for `wasm32-wasip2`, clippy clean, 40 native tests pass (rules, round lifecycle, matchmaking, protocol, stats, placeholders, crash log). **It has not been run on a live Pumpkin server yet.** See [Not yet verified](#not-yet-verified) for what to check first.

## Contents

- [Install](#install)
- [If /mg and /dbmg don't appear](#if-mg-and-dbmg-dont-appear)
- [Concepts](#concepts)
- [Setting up an arena](#setting-up-an-arena)
- [Mode files](#mode-files)
- [Kits](#kits)
- [Commands and permissions](#commands-and-permissions)
- [Sidebar](#sidebar)
- [PumpkinPIE placeholders](#pumpkinpie-placeholders)
- [Integrations](#integrations)
- [Writing a mode plugin](#writing-a-mode-plugin)
- [How resets work](#how-resets-work)
- [Not yet verified](#not-yet-verified)
- [Building](#building)

## Install

1. Copy `daybuilt_minigame_core.wasm` into the server's `plugins/` folder.
2. The plugin asks for `fs.read.data` and `fs.write.data` (arenas, modes, kits and stats are files). Pumpkin asks on the console before loading. On a panel with no console input, pre-approve it in the server config:

   ```toml
   [plugins.overrides.daybuilt_minigame_core]
   allowed_permissions = ["fs.read.data", "fs.write.data"]
   ```

   Without these it still loads, with built-in defaults, but nothing is saved. The console says so.
3. On first start it writes `plugins/data/daybuilt_minigame_core/` with `config.toml`, an example mode (`modes/spleef.toml`) and example kits. The console line `daybuilt_minigame_core v0.1.0 loaded: ...` confirms it loaded.
4. Stand where players should wait between rounds and run `/dbmg setlobby`.

```text
plugins/data/daybuilt_minigame_core/
  config.toml          main settings (comments explain every key)
  lobby.toml           lobby spawn, written by /dbmg setlobby
  arenas/<id>.toml     one per arena, written by /dbmg arena commands
  modes/<mode>.toml    one per game mode
  kits/<kit>.toml      one per kit
  stats.json           player statistics
  resets/<arena>.jsonl blocks changed in a running round (crash safety; deleted after each reset)
```

`/dbmg reload` re-reads everything. Games already running keep the arena and mode settings they started with.

### If /mg and /dbmg don't appear

The plugin needs no mode plugin: `/mg`, `/dbmg` and the bundled `spleef` mode work on their own. Missing commands mean the plugin did not load. Look in the console:

| Console line | Fix |
|---|---|
| `Permission denied for plugin "daybuilt_minigame_core", skipping loading.` | Add the `[plugins.overrides.daybuilt_minigame_core]` block above. The key must be exactly the plugin id `daybuilt_minigame_core`. Or answer `y` at the prompt. |
| `Found cached permission decision for plugin ... (approved: false)` | An earlier "no" is cached in `plugins/permission_cache.json`. Delete that entry, or use the override block, then restart. |
| `requests permissions from a non-main thread ... Denying permissions` | The `.wasm` was added or replaced while the server was running (hot reload). Restart the server instead. |
| `Failed to initialize plugin daybuilt_minigame_core: ...` | Please send this line; it is a bug. |
| `daybuilt_minigame_core v0.1.0 loaded: ... Commands: /mg (players), /dbmg (admin).` | Loaded. `/dbmg` needs op level 2. |

## Concepts

| Thing | What it is | Where |
|---|---|---|
| **Mode** | A game type: `bedwars`, `spleef`, `duels`. Has rules, formats, kits, a schedule, a sidebar and rewards. | `modes/<mode>.toml` |
| **Format** | A team layout of a mode: `solo` (8 teams of 1), `doubles` (8 teams of 2), `ffa` (16 players, everyone for themselves). | `[formats.<name>]` in the mode file |
| **Arena** | A built map in a world: bounds, team spawns, spectator spawn, spawners, regions, named points. Lists the modes it can host. | `arenas/<id>.toml` |
| **Game** | One round of a mode running in an arena. Its id is the arena id. | in memory |
| **Mode plugin** | Optional `.wasm` that adds logic the config can't express (beds, shops, zombies). | separate plugin |

**Round lifecycle:** waiting → countdown (starts at `min_players`, shortens when full, stops if players leave) → running → ending (winners, rewards, stats) → resetting (players back to the lobby, map restored) → the arena is free again.

**Matchmaking:** `/mg join bedwars` (or a mode plugin, NPC or menu) asks for a mode and optionally a format. The core first fills a game of that mode that hasn't started and has room (the fullest one, so games start sooner), otherwise it opens a free arena that can host the mode with that format. Parties are kept on one team when one has room. An arena can host a format when it is enabled, lists the mode (or no modes at all), has bounds, and has enough teams with spawns (or, for solo formats, spawn points).

Players who disconnect mid-round count as a loss and can't rejoin. After a round, players stay on this server, in its lobby.

## Setting up an arena

All of this is done in game by an operator. Every command saves the arena file immediately.

```text
/dbmg arena create castle            # in the world the arena is built in; starts disabled
/dbmg arena castle pos1              # stand at one corner of the arena
/dbmg arena castle pos2              # stand at the opposite corner (include everything players can reach)
/dbmg arena castle team red spawn    # adds a spawn for team "red" where you stand (repeat for more spawns)
/dbmg arena castle team blue spawn
/dbmg arena castle team blue color blue   # colour is guessed from the team name; set it if not
/dbmg arena castle spectator         # where dead and eliminated players watch from
/dbmg arena castle mode add bedwars  # which modes may use it (none listed = any mode that fits)
/dbmg arena castle info              # shows what it can host and anything missing
/dbmg arena castle enable
```

For free-for-all formats (`team_mode = "solo"`), add individual spawns with `/dbmg arena castle spawn` instead of team spawns.

Optional extras:

| Command | Use |
|---|---|
| `/dbmg arena <id> waiting` | Players wait here (inside the arena) instead of the server lobby until the round starts. |
| `/dbmg arena <id> spawner add <group> <item> <ticks>` | An item spawner where you stand. Groups can be sped up or switched off at runtime. Labels and caps are edited in the file. `spawner remove` removes the nearest one. |
| `/dbmg arena <id> region <name> pos1` / `pos2` | A named box. `region <name> flag build false` (also `break`, `pvp`, `damage`, `track`) overrides the round's rules inside it. Mode plugins get enter/leave events. |
| `/dbmg arena <id> point add <name>` | A named list of positions for mode plugins (bed locations, chest locations, zombie spawns). |
| `/dbmg arena <id> value <key> <text>` | A free-form value for mode plugins. |
| `/dbmg arena <id> maxplayers <n>` | Caps players below what the format allows. |

Spawner labels: in `arenas/<id>.toml`, set `label = "&bDiamond &7({stock})\n&e{seconds}s"` on a spawner. Placeholders: `{stock}`, `{seconds}` (until the next drop), `{group}`, and `{var:<key>}` for values a mode plugin sets.

Arenas of different games can share a world. Keep their bounds from overlapping: block events without a world (fluids, explosions) are matched to arenas by position.

## Mode files

A mode is a TOML file in `modes/`. The file name is the mode id (lowercase letters and digits, starting with a letter; `game`, `stat`, `all`, `playing`, `waiting`, `games` are reserved). The bundled `modes/spleef.toml` is a complete example that runs without a plugin.

```toml
display_name = "Bedwars"
default_format = "solo"          # used when a player doesn't pick one
countdown_seconds = 30           # optional overrides of [timing] in config.toml
full_countdown_seconds = 10
ending_seconds = 8
kits = []                        # kits players may pick with /mg kit (empty = any kit)
cleanup_entities = []            # extra entity types removed at reset, e.g. ["zombie"]

[rules]                          # see the rules table below
respawn = true
break = "placed"
breakable = ["*_bed"]

[formats.solo]
teams = 8
team_size = 1
min_players = 2

[formats.doubles]
teams = 8
team_size = 2
min_players = 4
team_choice = false              # true = players may pick a team with /mg team
rules = { respawn_seconds = 3 }  # rules for this format only

[[schedule]]                     # timed events, seconds after the start
at = 360
id = "diamond_2"
label = "&bDiamond II"           # shown on the sidebar as the next event
actions = ["spawner_interval:diamond:400"]

[sidebar]                        # optional; config.toml [sidebar] is the default
title = "&e&lBED WARS"
waiting = ["&fPlayers: &a{players}/{max}", "&f{status}"]
running = ["{next_event}", "", "{teams}", "", "&fKills: &a{kills}"]
ending = ["&6Winner:", "{winners}"]

[rewards]                        # console commands; {player} {uuid} {mode} {format} {arena} {kills} {deaths} {counter:<key>}
win = ["eco give {player} 100"]
loss = []
played = ["eco give {player} 10"]
kill = ["eco give {player} 5"]   # also {victim}
```

Schedule actions (all optional; a `timer` event is always sent to the mode plugin):

| Action | Effect |
|---|---|
| `spawner_interval:<group>:<ticks>` | Change a spawner group's interval |
| `spawner_amount:<group>:<n>` | Items per drop |
| `spawner_on:<group>` / `spawner_off:<group>` | Switch a group |
| `rule:<key>:<json>` | Change a rule, e.g. `rule:pvp:true` to end a grace period |
| `no_respawn` | No team can respawn any more (e.g. "all beds destroyed") |
| `end_draw` | End the round as a draw |
| `broadcast:<text>` / `title:<text>` | Tell the players |

### Rules

Set in `[rules]`, overridden per format, and changeable while a round runs (by schedule actions or mode plugins).

| Rule | Default | Meaning |
|---|---|---|
| `gamemode` | `survival` | Gamemode in the round |
| `build` | `true` | Players may place blocks |
| `break` | `placed` | `none`, `placed` (only blocks placed this round) or `any` |
| `breakable` | `[]` | Blocks that may always be broken; `*` wildcards, e.g. `"*_bed"` |
| `max_build_y` | unset | Highest Y a block may be placed at |
| `hunger` | `false` | Food goes down |
| `item_drop` / `item_pickup` | `true` | |
| `damage` | `true` | Any damage at all |
| `fall_damage` | `true` | |
| `pvp` | `true` | Players may hurt each other |
| `friendly_fire` | `false` | Teammates may hurt each other |
| `death_drops` | `drop` | `drop`, `keep` or `clear` |
| `respawn` | `false` | Players come back after dying (teams and players can be switched off at runtime) |
| `respawn_seconds` | `5` | |
| `spectate_on_elimination` | `true` | Eliminated players watch; otherwise they go to the lobby |
| `void_y` | arena bottom | Falling below this is a death |
| `out_of_bounds` | `kill` | Leaving the arena box: `kill`, `teleport` (back to spawn) or `ignore` |
| `time_limit` | `0` | Seconds; 0 = none |
| `time_up` | `draw` | At the limit: `draw`, `most_alive` (teams with most players left win), or `event` (the mode plugin decides) |
| `win` | `last_team` | `last_team` (round ends when one team is left) or `manual` (the mode plugin ends it; the core still ends it when everyone is out) |
| `explosions` | `placed` | What explosions may destroy: `none`, `placed`, `any` |
| `fluid_flow` | `true` | Water and lava flow |
| `fire_spread` | `false` | |
| `nature` | `false` | Grass spread, leaf decay, ice and snow forming/melting |
| `start_freeze` | `0` | Seconds players are held in place at the start (Hunger Games pedestals) |
| `kit` | unset | Kit given at the start to players who chose none |
| `kit_on_respawn` | `true` | Give the kit again on respawn |
| `death_messages` | `true` | The core announces deaths to the game's players |
| `classic_combat` | unset | `true`/`false` switches Daybuilt PvP Manager's 1.8 combat for the round |
| `isolate_chat` | `true` | Players in this game only chat with each other |
| `kill_credit_seconds` | `15` | A death within this time of a hit counts as a kill for the attacker (void and fall included) |

Pistons are always cancelled inside arenas, because what they move is not reported to plugins.

## Kits

`kits/<id>.toml`. The fastest way to make one: fill your inventory and armour, then `/dbmg kit save <id>`.

```toml
display_name = "&aStarter"
permission = ""          # needed to pick it with /mg kit; empty = everyone
clear = true             # empty the inventory first

[[items]]
slot = 0                 # 0-8 hotbar, 9-35 inventory; leave out to just add it
item = "minecraft:stone_sword"
count = 1
name = "&fSword"
lore = ["&7A sword"]
enchantments = { sharpness = 1 }

[chestplate]             # also helmet, leggings, boots, offhand
item = "minecraft:leather_chestplate"

[[effects]]
effect = "speed"         # speed, jump_boost, regeneration, invisibility, ...
seconds = 10             # 0 = for the whole round
amplifier = 0
```

A player's kit comes from, in order: their choice (`/mg kit`, `/dbmg setkit`, or a mode plugin), then the round's `kit` rule.

## Commands and permissions

| Command | Permission (default) | |
|---|---|---|
| `/mg join <mode> [format]` | `daybuilt_minigame_core:command.mg` (everyone) | Matchmaking |
| `/mg leave` | same | Leave (a loss if the round is running) |
| `/mg list`, `/mg modes` | same | Games and modes |
| `/mg kit <kit>` | same, plus the kit's own `permission` | Choose a kit |
| `/mg team <team>` | same | Choose a team (formats with `team_choice`) |
| `/mg stats [player]` | same | Statistics |
| `/dbmg ...` | `daybuilt_minigame_core:command.dbmg` (op level 2) | Setup and control, see `/dbmg` |
| | `daybuilt_minigame_core:build` (op level 2) | Change blocks in the lobby and in arenas |
| | `daybuilt_minigame_core:kit.all` (op level 2) | Use any kit |

Admin commands also work from the console, which is how NPCs and menus should send players into games:

| Command | |
|---|---|
| `/dbmg join <player> <mode> [format]` | Matchmaking for someone else (RustedNPC, Economy Manager menus) |
| `/dbmg leave <player>` | |
| `/dbmg setkit <player> <kit>` | Select a kit (e.g. from a kit menu) |
| `/dbmg start <arena>` | Start now, even below `min_players` |
| `/dbmg stop <arena>` | End now, no results, reset |
| `/dbmg kit give <kit> [player]` | Apply a kit immediately |
| `/dbmg reload`, `/dbmg setlobby` | |

Lobby protection (`[lobby] protect`, `no_hunger`, `no_damage`, `no_item_drop`) applies to everyone not in a game. Turn it off in `config.toml` if another plugin handles the lobby.

Set `[lobby] auto_join = "bedwars:doubles"` on a server that only runs one mode: players are queued as they arrive and again after each round.

## Sidebar

Shown to players in games. Lines come from the mode's `[sidebar]`, or `[sidebar]` in `config.toml`. Up to 15 lines; `&` colour codes.

| Placeholder | |
|---|---|
| `{arena}` `{mode}` `{mode_name}` `{format}` `{round}` | |
| `{players}` `{max}` `{min}` | Players in the game |
| `{status}` `{seconds}` | Waiting/countdown text, countdown seconds |
| `{time}` `{time_left}` | Round time, time limit left (`mm:ss`) |
| `{alive}` `{teams_alive}` | |
| `{kills}` `{deaths}` `{team}` `{kit}` | The viewer's |
| `{counter:<key>}` | The viewer's mode counter, e.g. `{counter:beds_broken}` |
| `{var:<key>}` | A value a mode plugin set (per player first, then per game) |
| a line that is exactly `{teams}` | One line per team: colour, name, ✔ (can respawn), players left, or ✘ |
| a line that is exactly `{winners}` | One line per winning team, or "Draw" |
| a line that is exactly `{next_event}` | The next labelled schedule entry and its countdown (or the time limit); no line if none |

## PumpkinPIE placeholders

With [PumpkinPIE](https://github.com/Rennex07/PumpkinPIE) installed (optional; registered automatically), the namespace is `mg` (`[placeholders] namespace`):

| Placeholder | |
|---|---|
| `%mg_wins%`, `%mg_losses%`, `%mg_draws%`, `%mg_played%`, `%mg_kills%`, `%mg_deaths%`, `%mg_best_win_streak%`, `%mg_win_streak%` | Totals over all modes |
| `%mg_<mode>_<stat>%` | One mode, e.g. `%mg_bedwars_wins%`, and mode counters such as `%mg_bedwars_beds_broken%` |
| `%mg_game_arena%`, `_mode`, `_mode_name`, `_format`, `_team`, `_state`, `_alive`, `_players`, `_max`, `_kills`, `_deaths`, `_time`, `_kit` | The viewer's current game (`state` is `lobby` when not in one) |
| `%mg_playing_<mode>%`, `%mg_waiting_<mode>%`, `%mg_games_<mode>%` | Server counts, for NPC holograms and menus |

## Integrations

| Plugin | How | Required |
|---|---|---|
| Mode plugins | IPC with the `minigamecore-client` crate | No |
| [Daybuilt PvP Manager](#daybuilt-pvp-manager) | IPC (`daybuilt_pvp_manager`) | No |
| RustedVault | `[rewards]` console commands such as `eco give {player} 100` | No |
| PumpkinPIE | Placeholder expansion | No |
| RustedNPC / Economy Manager | Run `/dbmg join <player> <mode>` and `/dbmg setkit <player> <kit>` as console | No |

None of these are listed as load dependencies, so a missing one never stops the core from loading.

### Daybuilt PvP Manager

PvP Manager keeps enforcing its rules during rounds. The core only cancels hits it must: spectators, players outside the round, teammates (unless `friendly_fire`), `pvp = false`, and regions with `pvp` off. Two things are handled with PvP Manager's existing API:

- A player who switched PvP off would be unhittable in a round. At the round start the core turns their PvP on (`pvp.set`) and puts it back when they leave the round.
- With `classic_combat` set in a mode's rules, the core sets each player's classic combat for the round (`classic.set`) and restores it afterwards. This needs `classic_combat.allow_player_toggle = true` in PvP Manager.
- Combat tags are cleared when players return to the lobby (`combat.untag`).

Features that would make this cleaner if added to PvP Manager:

1. **Temporary, unsaved overrides** (`pvp.override <player> <true|false|clear>`, `classic.override ...`). Today the core changes the saved setting and changes it back; a crash mid-round leaves the round's value saved.
2. **A combat-log exemption for minigame players** (for example `combatlog.exempt <player> <bool>`), so leaving a round, which already counts as a loss, isn't also punished as combat logging, and `/mg leave` isn't blocked by restricted-command rules while tagged. Until then, add `mg` to PvP Manager's allowed commands if it restricts commands in combat.

## Writing a mode plugin

A mode plugin is a normal Pumpkin plugin that depends on the client crate:

```toml
[dependencies]
minigamecore-client = { git = "https://github.com/Wet-Hands/Daybuilt-MinigameCore" }
```

It claims its mode id, polls for events every tick, and sends requests:

```rust
use minigamecore_client as mg;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

static REGISTERED: AtomicBool = AtomicBool::new(false);

// in on_load
pumpkin_plugin_api::scheduler::schedule_repeating_task(1, 1, |_server| {
    if !REGISTERED.load(Relaxed) {
        // Fails until the core has loaded; retrying is harmless.
        if mg::register_mode("bedwars").is_err() { return; }
        REGISTERED.store(true, Relaxed);
    }
    let Ok(batch) = mg::poll(None) else { return };
    for env in batch.events {
        match env.event {
            mg::Event::RoundStarted { game, teams, .. } => { /* set up beds for `teams` */ }
            mg::Event::PlayerDied { game, player, killer, respawns, .. } => { /* hand resources to the killer */ }
            mg::Event::Timer { game, id } if id == "beds_gone" => { /* ... */ }
            _ => {}
        }
    }
});
```

The mode still needs a `modes/<mode>.toml` in the core's data folder: that is where formats, rules, schedule, sidebar and rewards live. The plugin adds what config can't express.

### Events

Queued for the plugin that registered the game's mode, and returned by `poll` oldest first (each has a `seq`). Up to 4096 are kept between polls; older ones are dropped and counted in `dropped`.

| Event | When |
|---|---|
| `game_created {game, mode, format}` | Matchmaking opened an arena |
| `player_joined {game, player, team}` / `player_left {game, player, during_round}` | |
| `countdown_started {game, seconds}` / `countdown_cancelled {game}` | |
| `round_started {game, round, teams}` | Teams with their players |
| `player_died {game, player, killer, cause, respawns}` | `respawns = false` is a final death |
| `player_respawned {game, player}` / `player_eliminated {game, player}` | |
| `team_eliminated {game, team}` | |
| `region_entered` / `region_left {game, player, region}` | Regions with `track = true` |
| `timer {game, id}` | A schedule entry came due |
| `time_up {game}` | The time limit passed and `time_up = "event"` |
| `round_ended {game, winners, reason}` | |
| `game_reset {game}` | The arena is restored and free |

### Requests

`player` arguments take an online player's name or UUID. `game` is the arena id. Changing a game is only allowed for the plugin that registered its mode; reading is open to every plugin.

| Function (`mg::`) | |
|---|---|
| `register_mode`, `poll`, `is_available` | |
| `game`, `list_games`, `player`, `arena`, `same_team`, `regions_at` | Read state, the arena's spawns, regions, points and values |
| `join(players, mode, format, arena)`, `leave` | Matchmaking (any plugin, e.g. a lobby NPC plugin) |
| `end_round(game, winners, reason)` | Winners are team ids (in solo formats, the lowercase player name). Empty = draw |
| `eliminate(player)` | |
| `set_team_respawn(game, team, allowed)`, `set_player_respawn` | E.g. a bed was broken |
| `set_rule(game, key, json)` | Any rule from the table above |
| `spawner_group(game, group, interval, amount, enabled)` | Generator upgrades |
| `schedule(game, id, at_secs, label)`, `cancel_schedule` | Extra timed events |
| `start(game)` | Skip the countdown |
| `set_var(game, player, key, value)` | Values for `{var:<key>}` in the sidebar and spawner labels |
| `counter(player, key, delta)` | Per-round counters saved into stats (`beds_broken`, `final_kills`) |
| `set_block(game, pos, block)` | Change a block; the core records the original for reset (TNT Run, Spleef) |
| `record_block(game, pos, state)` | Tell the core the original state before you change a block yourself |
| `message(game, team, player, text, kind)` | Chat, action bar, title or subtitle |
| `give_kit(player, kit)`, `select_kit(player, kit)` | |

Host effects of requests (teleports, kits, blocks, messages) are applied on the core's next tick.

### Rules for mode plugins

- **Don't list `daybuilt_minigame_core` in your `dependencies`** unless you want your plugin to refuse to load without it. Treat `ClientError::Unavailable` as "core not loaded" and keep retrying `register_mode` from your ticker.
- The core never calls your plugin, so calling it from any event handler, command or task can't deadlock.
- If your plugin cancels or un-cancels block events yourself, remember the reset log: call `record_block` before you let a block change that the core refused, or use `set_block`.
- Pumpkin runs event handlers in plugin **load order** (the priority value is not used for ordering). If your plugin must see the core's decision on an event first, list the core in `dependencies` so it loads before you.
- For other languages: the wire format is JSON over IPC to `daybuilt_minigame_core`, `{"v":1,"op":"<op>",...}` → `{"ok":true,"reply":...}` or `{"ok":false,"error":{"kind":"...","message":"..."}}`. The complete definition is [`client/src/protocol.rs`](client/src/protocol.rs).

### How the example modes map onto the core

| Mode | Config does | Plugin adds |
|---|---|---|
| Spleef, TNT Run | Solo format, `break = "none"` + `breakable`, void deaths, last player wins | TNT Run: remove blocks under players with `set_block` |
| Duels | Teams of 1-4, fixed kit, `respawn = false` | Best-of-N rounds, ranked rating |
| Bedwars | Teams, `respawn = true`, `breakable = ["*_bed"]`, spawner groups and schedule | Beds (`set_team_respawn` on break), shops, team upgrades, traps (`region_entered`) |
| Hunger Games | Solo or teams, `start_freeze`, schedule (`rule:pvp:true` after grace), `time_up` | Filling chests at `points`, world border, deathmatch |
| COD Zombies | One team (`teams = 1`), `win = "manual"`, `respawn` per round | Zombie waves, points, doors; `end_round` when the team falls; `cleanup_entities = ["zombie"]` |

## How resets work

There is no API to copy a world, so the core keeps a **change log**: the first time a block changes during a round, its original state is recorded; at the end every recorded block is put back (a few hundred per tick, newest first) and leftover items, arrows and the like are removed from the arena box.

- Player breaks and places, buckets, multi-block placements (beds, doors) and the neighbours of broken multi-blocks are recorded.
- Fluid flow, fire, nature (grass, leaves, ice, snow), falling blocks and entity block changes are recorded or cancelled according to the rules.
- Explosions: every block within `explosion_radius` is read first. Blocks the `explosions` rule allows to break are recorded; the rest are put back right after the blast.
- Pistons are cancelled in arenas.
- Arenas with no round running are protected from all changes (except players with `daybuilt_minigame_core:build`).
- **Crash safety:** each recorded block is also appended to `resets/<arena>.jsonl`. If the server stops mid-round, the arena is repaired on the next start, before it can host a game.

Keeping `break = "placed"` keeps the log small. `break = "any"` on a big map works, but every broken block is one entry.

## Not yet verified

Built against `pumpkin-plugin-api` 0.2.1 and Pumpkin's source, but not run on a server yet. Check these first:

- Block break/place events fire **before** the change (the original is read from the world in the handler).
- Hotbar slot numbering for `slot = 0..8` in kits, and `get_item` slots 0-35 being the main inventory.
- The per-player scoreboard from `java_player.get_scoreboard()` shows a sidebar with `§` codes in score names and the blank number format.
- `set_movement_locked` for `start_freeze`; `respawn()` skipping the death screen; `keep_inventory` in the death event being honoured.
- Changing `recipients` in the chat event (`isolate_chat`).
- Text display labels spawned with `spawn_entity(TextDisplay)`.
- Projectile kill credit (needs `ProjectileLaunchEvent.shooter_id` and the projectile's entity id as `damager_id`).
- `spawners.mode = "command"` needs a `/summon` that accepts item NBT and runs in the arena's world.
- The vanilla death message is still broadcast to the whole server. Set the `showDeathMessages` game rule to false in arena worlds if you use the core's game-only death messages.

## Building

```bash
rustup target add wasm32-wasip2                          # once
scripts/build.sh                                         # → dist/daybuilt_minigame_core.wasm
cargo test --workspace --target x86_64-unknown-linux-gnu # native tests
cargo clippy -- -D warnings && cargo fmt --check
```

Layout:

```text
client/            minigamecore-client: protocol types + IPC helpers for mode plugins
config/            bundled defaults (config.toml, example mode and kits)
src/model/         arena, mode, rules, kit definitions (pure)
src/game/          one game's state machine, matchmaking, sidebar text, change log (pure)
src/state.rs       all games and indexes behind one lock (pure)
src/ipc.rs         the mode-plugin protocol (pure; host work is deferred to the next tick)
src/host/          everything that calls Pumpkin: effects, files, kits, sidebar, spawners, reset,
                   PvP Manager, PumpkinPIE
src/listeners/     Pumpkin event handlers
src/commands/      /mg and /dbmg
src/ticker.rs      the single repeating task
```

The game logic never calls the host. It returns a list of effects that the host layer applies after releasing the state lock, which is why it can be tested natively.
