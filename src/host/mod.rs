//! Everything that calls Pumpkin. Callers must not hold the state lock here.

pub mod crashlog;
pub mod kits;
pub mod pie;
pub mod pvp;
pub mod restore;
pub mod sidebar;
pub mod spawners;
pub mod storage;

use pumpkin_plugin_api::{
    Server,
    common::{GameMode, NamedColor},
    player::Player,
    scoreboard::{CollisionRule, NametagVisibility, TeamSettings},
    server::CommandSender,
    text::TextComponent,
    uuid::Uuid,
    wit::pumpkin::plugin::sounds::SoundCategory,
    world::BlockFlags,
};

use crate::game::effects::Effect;
use crate::model::{BlockPos, WorldLoc, rules::GameModeName, text};
use crate::state;

/// Parses a hyphenated or plain UUID string (pure; `uuid::parse` is a host call).
pub fn parse_uuid(s: &str) -> Option<Uuid> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let high = u64::from_str_radix(&hex[..16], 16).ok()?;
    let low = u64::from_str_radix(&hex[16..], 16).ok()?;
    Some(Uuid { high, low })
}

pub fn uuid_string(id: &Uuid) -> String {
    let h = format!("{:016x}{:016x}", id.high, id.low);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

pub fn player(server: &Server, uuid: &str) -> Option<Player> {
    server.get_player_by_uuid(parse_uuid(uuid)?)
}

pub fn colored(s: &str) -> TextComponent {
    TextComponent::from_legacy_string_with_code(s, '&')
}

pub fn gamemode(m: GameModeName) -> GameMode {
    match m {
        GameModeName::Survival => GameMode::Survival,
        GameModeName::Adventure => GameMode::Adventure,
        GameModeName::Creative => GameMode::Creative,
        GameModeName::Spectator => GameMode::Spectator,
    }
}

pub fn named_color(c: &str) -> NamedColor {
    match text::color_code(c) {
        "&0" => NamedColor::Black,
        "&1" => NamedColor::DarkBlue,
        "&2" => NamedColor::DarkGreen,
        "&3" => NamedColor::DarkAqua,
        "&4" => NamedColor::DarkRed,
        "&5" => NamedColor::DarkPurple,
        "&6" => NamedColor::Gold,
        "&7" => NamedColor::Gray,
        "&8" => NamedColor::DarkGray,
        "&9" => NamedColor::Blue,
        "&a" => NamedColor::Green,
        "&b" => NamedColor::Aqua,
        "&c" => NamedColor::Red,
        "&d" => NamedColor::LightPurple,
        "&e" => NamedColor::Yellow,
        _ => NamedColor::White,
    }
}

pub fn console(server: &Server, command: &str) {
    let command = command.trim().trim_start_matches('/');
    if !command.is_empty() {
        server.execute_command(command, CommandSender::Console);
    }
}

pub fn teleport(server: &Server, p: &Player, to: &WorldLoc) {
    match server.get_world_by_name(&to.world) {
        Some(world) => p.teleport((to.x, to.y, to.z), Some(to.yaw), Some(to.pitch), world),
        None => tracing::warn!(
            "{}: world '{}' not found for a teleport",
            crate::ID,
            to.world
        ),
    }
}

/// Clears and refills a player for a fresh start.
pub fn reset_player(p: &Player, mode: GameModeName, clear_inventory: bool) {
    p.set_gamemode(gamemode(mode));
    if clear_inventory {
        p.get_inventory().clear_all();
    }
    p.clear_effects();
    let max = p.get_max_health();
    p.set_health(max);
    p.set_food_level(20);
    p.set_saturation(5.0);
    p.set_absorption(0.0);
    p.as_entity().set_fire_ticks(0);
    p.as_entity().set_fall_distance(0.0);
}

/// Block flags used for all core block writes: update clients, no neighbour physics.
pub fn quiet_flags() -> BlockFlags {
    BlockFlags::NOTIFY_LISTENERS | BlockFlags::FORCE_STATE | BlockFlags::SKIP_DROPS
}

/// A block name or numeric state id → state id.
pub fn block_state(block: &str) -> Option<u16> {
    if let Ok(id) = block.parse::<u16>() {
        return Some(id);
    }
    let (name, props) = match block.split_once('[') {
        Some((n, rest)) => (n, rest.trim_end_matches(']')),
        None => (block, ""),
    };
    let name = if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    };
    let props: Vec<(String, String)> = props
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    pumpkin_plugin_api::world::resolve_block_state(&name, &props)
}

/// Records the original block (for reset) and returns whether it was new.
pub fn record_block(game: &str, world: &str, pos: BlockPos, original: u16) {
    let (new, crash_safe) = state::with(|s| {
        (
            s.record_block(game, pos, original),
            s.cfg.reset.crash_safe && s.persist,
        )
    });
    if new && crash_safe {
        crashlog::append(game, world, pos, original);
    }
}

/// Carries out effects. Must be called without the state lock.
pub fn apply(server: &Server, effects: Vec<Effect>) {
    for e in effects {
        apply_one(server, e);
    }
}

fn each(server: &Server, players: &[String], f: impl Fn(&Player)) {
    for u in players {
        if let Some(p) = player(server, u) {
            f(&p);
        }
    }
}

fn apply_one(server: &Server, e: Effect) {
    match e {
        Effect::Teleport { player: u, to } => {
            if let Some(p) = player(server, &u) {
                teleport(server, &p, &to);
            }
        }
        Effect::ToLobby { player: u } => {
            if let Some(p) = player(server, &u) {
                to_lobby(server, &p, &u);
            }
        }
        Effect::Reset {
            player: u,
            gamemode: m,
            clear_inventory,
        } => {
            if let Some(p) = player(server, &u) {
                reset_player(&p, m, clear_inventory);
            }
        }
        Effect::Gamemode { player: u, mode } => {
            if let Some(p) = player(server, &u) {
                p.set_gamemode(gamemode(mode));
            }
        }
        Effect::Kit { player: u, kit } => {
            let def = state::with(|s| s.kits.get(&kit).cloned());
            match (player(server, &u), def) {
                (Some(p), Some(def)) => kits::apply(&p, &def),
                (_, None) => tracing::warn!("{}: kit '{kit}' does not exist", crate::ID),
                _ => {}
            }
        }
        Effect::Freeze { player: u, frozen } => {
            if let Some(p) = player(server, &u) {
                p.set_movement_locked(frozen);
            }
        }
        Effect::Chat { players, text: t } => {
            each(server, &players, |p| {
                p.send_system_message(colored(&t), false)
            });
        }
        Effect::ActionBar { players, text: t } => {
            each(server, &players, |p| {
                p.send_system_message(colored(&t), true)
            });
        }
        Effect::Title {
            players,
            title,
            subtitle,
            stay_ticks,
        } => {
            each(server, &players, |p| {
                p.send_title_animation(5, stay_ticks, 10);
                if !subtitle.is_empty() {
                    p.show_subtitle(colored(&subtitle));
                }
                p.show_title(colored(if title.is_empty() { " " } else { &title }));
            });
        }
        Effect::Sound {
            players,
            sound,
            pitch,
        } => {
            each(server, &players, |p| {
                p.play_custom_sound(&sound, SoundCategory::Players, 1.0, pitch);
            });
        }
        Effect::Console { command } => console(server, &command),
        Effect::ForceRespawn { player: u } => {
            if let Some(p) = player(server, &u) {
                p.respawn();
            }
        }
        Effect::CreateTeams { world, teams } => {
            if let Some(w) = server.get_world_by_name(&world) {
                let sb = w.get_scoreboard();
                for t in teams {
                    let color = named_color(&t.color);
                    sb.remove_team(&t.key);
                    sb.create_team(
                        &t.key,
                        TeamSettings {
                            display_name: colored(&t.display),
                            friendly_fire: t.friendly_fire,
                            see_friendly_invisibles: true,
                            nametag_visibility: NametagVisibility::Always,
                            collision_rule: CollisionRule::Always,
                            color,
                            prefix: TextComponent::text(""),
                            suffix: TextComponent::text(""),
                        },
                    );
                }
            }
        }
        Effect::JoinTeam {
            world,
            key,
            player_name,
        } => {
            if let Some(w) = server.get_world_by_name(&world) {
                w.get_scoreboard().add_player_to_team(&key, &player_name);
            }
        }
        Effect::LeaveTeam {
            world,
            key,
            player_name,
        } => {
            if let Some(w) = server.get_world_by_name(&world) {
                w.get_scoreboard()
                    .remove_player_from_team(&key, &player_name);
            }
        }
        Effect::RemoveTeams { world, keys } => {
            if let Some(w) = server.get_world_by_name(&world) {
                let sb = w.get_scoreboard();
                for k in keys {
                    sb.remove_team(&k);
                }
            }
        }
        Effect::SidebarClear { player: u } => sidebar::clear(server, &u),
        Effect::SetBlock {
            game,
            world,
            pos,
            block,
        } => {
            let Some(w) = server.get_world_by_name(&world) else {
                return;
            };
            let Some(new) = block_state(&block) else {
                tracing::warn!("{}: set_block: unknown block '{block}'", crate::ID);
                return;
            };
            let bp = pumpkin_plugin_api::common::BlockPos {
                x: pos.x,
                y: pos.y,
                z: pos.z,
            };
            let original = w.get_block_state_id(bp);
            record_block(&game, &world, pos, original);
            w.set_block_state(bp, new, quiet_flags());
        }
        Effect::SpawnerDrop {
            world,
            x,
            y,
            z,
            item,
            count,
        } => {
            let template = state::with(|s| s.cfg.spawners.command.clone());
            let cmd = text::fill(
                &template,
                &[
                    ("x", &format!("{x:.2}")),
                    ("y", &format!("{y:.2}")),
                    ("z", &format!("{z:.2}")),
                    ("item", &item),
                    ("count", &count.to_string()),
                    ("world", &world),
                ],
            );
            console(server, &cmd);
        }
        Effect::PutBlocks { world, blocks } => {
            if let Some(w) = server.get_world_by_name(&world) {
                for (pos, id) in blocks {
                    let bp = pumpkin_plugin_api::common::BlockPos {
                        x: pos.x,
                        y: pos.y,
                        z: pos.z,
                    };
                    if w.get_block_state_id(bp) != id {
                        w.set_block_state(bp, id, quiet_flags());
                    }
                }
            }
        }
        Effect::AutoJoin { player: u } => auto_join(server, &u),
        Effect::RemoveHolograms { game } => spawners::remove_holograms(&game),
        Effect::PvpEnter { player: u, classic } => pvp::enter(&u, classic),
        Effect::PvpLeave { player: u } => pvp::leave(&u),
    }
}

/// Queues a player for `lobby.auto_join`.
fn auto_join(server: &Server, uuid: &str) {
    let r = state::with(|s| {
        let (mode, format) = s.cfg.auto_join()?;
        if s.index.contains_key(uuid) {
            return None;
        }
        let name = s.uuid_names.get(uuid).cloned()?;
        let msg = s.cfg.messages.clone();
        Some(
            match s.join(
                vec![(uuid.to_string(), name)],
                &mode,
                format.as_deref(),
                None,
            ) {
                Ok((_, effects)) => Ok(effects),
                Err(e) => Err((e, msg, mode)),
            },
        )
    });
    match r {
        Some(Ok(effects)) => apply(server, effects),
        Some(Err((e, msg, mode))) => {
            if let Some(p) = player(server, uuid) {
                let line = match e.kind {
                    minigamecore_client::ErrorKind::NoArena => {
                        text::fill(&msg.no_arena, &[("mode", &mode)])
                    }
                    _ => e.message,
                };
                p.send_system_message(colored(&format!("{}{line}", msg.prefix)), false);
            }
        }
        None => {}
    }
}

/// Sends a player to the lobby and applies the lobby settings.
pub fn to_lobby(server: &Server, p: &Player, uuid: &str) {
    let cfg = state::with(|s| s.cfg.clone());
    match &cfg.lobby.spawn {
        Some(spawn) => teleport(server, p, spawn),
        // No lobby set: the first world's spawn, so nobody is left standing in an arena.
        None => {
            if let Some(w) = server.get_all_worlds().into_iter().next() {
                let s = w.get_spawn_location();
                let (x, y, z) = (
                    f64::from(s.pos.x) + 0.5,
                    f64::from(s.pos.y),
                    f64::from(s.pos.z) + 0.5,
                );
                p.teleport((x, y, z), Some(s.yaw), Some(s.pitch), w);
            }
        }
    }
    reset_player(p, cfg.lobby.gamemode, cfg.lobby.clear_inventory);
    p.set_movement_locked(false);
    let name = p.get_name();
    for c in &cfg.lobby.commands {
        console(server, &text::fill(c, &[("player", &name), ("uuid", uuid)]));
    }
    if cfg.pvp_manager.enabled && cfg.pvp_manager.untag_on_return {
        pvp::untag(uuid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_round_trip() {
        let s = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
        let u = parse_uuid(s).unwrap();
        assert_eq!(uuid_string(&u), s);
        assert!(parse_uuid("nope").is_none());
    }
}
