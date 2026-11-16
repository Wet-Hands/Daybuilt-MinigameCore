//! `/dbmg` — arena setup, kits, game control, reload. Changes to arenas are saved at once.

use std::sync::Arc;

use pumpkin_plugin_api::{
    Server,
    command::{
        ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
    },
    commands::CommandHandler,
    player::Player,
};

use super::{boolean, fail, int, lines, need_player, reply, word};
use crate::host::{self, kits, storage};
use crate::listeners::uuid_of;
use crate::model::{
    Location, WorldLoc,
    arena::{ArenaDef, RegionDef, SpawnerDef, TeamDef},
    ids,
    kit::{KitDef, KitItem},
    text,
};
use crate::state;

#[derive(Clone, Copy)]
enum Act {
    Help,
    Reload,
    SetLobby,
    Start,
    Stop,
    Join,
    Leave,
    SetKit,
    KitSave,
    KitGive,
    ArenaList,
    ArenaCreate,
    ArenaDelete,
    Info,
    Pos1,
    Pos2,
    Spectator,
    Waiting,
    SoloSpawn,
    TeamSpawn,
    TeamColor,
    TeamRemove,
    ModeAdd,
    ModeRemove,
    SpawnerAdd,
    SpawnerRemove,
    RegionPos1,
    RegionPos2,
    RegionFlag,
    RegionRemove,
    PointAdd,
    PointClear,
    Value,
    Enable,
    Disable,
    MaxPlayers,
}

struct H(Act);

fn w() -> ArgumentType {
    ArgumentType::String(StringType::SingleWord)
}

fn lit(name: &str, act: Act) -> CommandNode {
    CommandNode::literal(name).execute(H(act))
}

fn arg(name: &str) -> CommandNode {
    CommandNode::argument(name, &w())
}

pub fn command() -> Command {
    let arena_sub = arg("arena")
        .then(lit("info", Act::Info))
        .then(lit("pos1", Act::Pos1))
        .then(lit("pos2", Act::Pos2))
        .then(lit("spectator", Act::Spectator))
        .then(lit("waiting", Act::Waiting))
        .then(lit("spawn", Act::SoloSpawn))
        .then(lit("enable", Act::Enable))
        .then(lit("disable", Act::Disable))
        .then(
            CommandNode::literal("maxplayers").then(
                CommandNode::argument("n", &ArgumentType::Integer((Some(0), Some(1000))))
                    .execute(H(Act::MaxPlayers)),
            ),
        )
        .then(
            CommandNode::literal("team").then(
                arg("team")
                    .then(lit("spawn", Act::TeamSpawn))
                    .then(lit("remove", Act::TeamRemove))
                    .then(
                        CommandNode::literal("color").then(arg("color").execute(H(Act::TeamColor))),
                    ),
            ),
        )
        .then(
            CommandNode::literal("mode")
                .then(CommandNode::literal("add").then(arg("mode").execute(H(Act::ModeAdd))))
                .then(CommandNode::literal("remove").then(arg("mode").execute(H(Act::ModeRemove)))),
        )
        .then(
            CommandNode::literal("spawner")
                .then(
                    CommandNode::literal("add").then(
                        arg("group").then(
                            arg("item").then(
                                CommandNode::argument(
                                    "interval",
                                    &ArgumentType::Integer((Some(1), Some(72000))),
                                )
                                .execute(H(Act::SpawnerAdd)),
                            ),
                        ),
                    ),
                )
                .then(lit("remove", Act::SpawnerRemove)),
        )
        .then(
            CommandNode::literal("region").then(
                arg("region")
                    .then(lit("pos1", Act::RegionPos1))
                    .then(lit("pos2", Act::RegionPos2))
                    .then(lit("remove", Act::RegionRemove))
                    .then(
                        CommandNode::literal("flag").then(
                            arg("flag").then(
                                CommandNode::argument("value", &ArgumentType::Bool)
                                    .execute(H(Act::RegionFlag)),
                            ),
                        ),
                    ),
            ),
        )
        .then(
            CommandNode::literal("point")
                .then(CommandNode::literal("add").then(arg("point").execute(H(Act::PointAdd))))
                .then(CommandNode::literal("clear").then(arg("point").execute(H(Act::PointClear)))),
        )
        .then(
            CommandNode::literal("value").then(
                arg("key").then(
                    CommandNode::argument("value", &ArgumentType::String(StringType::Greedy))
                        .execute(H(Act::Value)),
                ),
            ),
        );

    Command::new(
        &["dbmg".into(), "minigameadmin".into()],
        "Minigame setup and control.",
    )
    .execute(H(Act::Help))
    .then(lit("reload", Act::Reload))
    .then(lit("setlobby", Act::SetLobby))
    .then(CommandNode::literal("start").then(arg("arena").execute(H(Act::Start))))
    .then(CommandNode::literal("stop").then(arg("arena").execute(H(Act::Stop))))
    .then(
        CommandNode::literal("join").then(
            arg("player").then(
                arg("mode")
                    .execute(H(Act::Join))
                    .then(arg("format").execute(H(Act::Join))),
            ),
        ),
    )
    .then(CommandNode::literal("leave").then(arg("player").execute(H(Act::Leave))))
    .then(
        CommandNode::literal("setkit").then(arg("player").then(arg("kit").execute(H(Act::SetKit)))),
    )
    .then(
        CommandNode::literal("kit")
            .then(CommandNode::literal("save").then(arg("kit").execute(H(Act::KitSave))))
            .then(
                CommandNode::literal("give").then(
                    arg("kit")
                        .execute(H(Act::KitGive))
                        .then(arg("player").execute(H(Act::KitGive))),
                ),
            ),
    )
    .then(
        CommandNode::literal("arena")
            .then(lit("list", Act::ArenaList))
            .then(CommandNode::literal("create").then(arg("id").execute(H(Act::ArenaCreate))))
            .then(CommandNode::literal("delete").then(arg("id").execute(H(Act::ArenaDelete))))
            .then(arena_sub),
    )
}

const HELP: &[&str] = &[
    "&6/dbmg reload &7| &6setlobby &7| &6start <arena> &7| &6stop <arena>",
    "&6/dbmg join <player> <mode> [format] &7| &6leave <player> &7| &6setkit <player> <kit>",
    "&6/dbmg kit save <kit> &7| &6kit give <kit> [player]",
    "&6/dbmg arena list &7| &6create <id> &7| &6delete <id>",
    "&6/dbmg arena <id> &finfo | pos1 | pos2 | spectator | waiting | spawn | enable | disable | maxplayers <n>",
    "&6/dbmg arena <id> team <team> &fspawn | color <color> | remove",
    "&6/dbmg arena <id> mode &fadd|remove <mode>",
    "&6/dbmg arena <id> spawner &fadd <group> <item> <ticks> | remove",
    "&6/dbmg arena <id> region <name> &fpos1 | pos2 | remove | flag <build|break|pvp|damage|track> <true|false>",
    "&6/dbmg arena <id> point &fadd|clear <name> &7| &6value <key> <text>",
];

fn here(p: &Player) -> (String, Location) {
    let (x, y, z) = p.get_position();
    let mut l = Location::new(x, y, z);
    l.yaw = p.get_yaw();
    l.pitch = p.get_pitch();
    (p.get_world().get_name(), l)
}

/// Snaps to the block centre (x.5, z.5) and rounds the facing, for tidy spawn points.
fn tidy(mut l: Location) -> Location {
    l.x = l.x.floor() + 0.5;
    l.z = l.z.floor() + 0.5;
    l.y = (l.y * 2.0).round() / 2.0;
    l.yaw = (l.yaw / 45.0).round() * 45.0;
    l.pitch = 0.0;
    l
}

/// Edits an arena (copy, change, validate, save). Running games keep their own copy.
fn edit(
    id: &str,
    f: impl FnOnce(&mut ArenaDef) -> Result<String, String>,
) -> Result<String, String> {
    let def = state::with(|s| {
        let a = s
            .arenas
            .get(id)
            .ok_or_else(|| format!("&cUnknown arena '{id}'."))?;
        let mut a = (**a).clone();
        let msg = f(&mut a)?;
        a.validate()?;
        s.arenas.insert(id.to_string(), Arc::new(a.clone()));
        Ok::<_, String>((a, msg))
    })?;
    storage::save_arena(&def.0).map_err(|e| format!("&cSaved in memory but not on disk: {e}"))?;
    Ok(def.1)
}

fn same_world(a: &ArenaDef, world: &str) -> Result<(), String> {
    if a.world == world {
        Ok(())
    } else {
        Err(format!(
            "&cThis arena is in world '{}', you are in '{world}'.",
            a.world
        ))
    }
}

impl CommandHandler for H {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let arena = word(&args, "arena")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let result: Result<String, String> = match self.0 {
            Act::Help => {
                lines(
                    &sender,
                    &HELP.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                );
                return Ok(1);
            }
            Act::Reload => Ok(reload()),
            Act::SetLobby => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let spawn = WorldLoc::new(&world, l);
                let saved = storage::save_lobby_spawn(&spawn);
                state::with(|s| {
                    let mut cfg = (*s.cfg).clone();
                    cfg.lobby.spawn = Some(spawn);
                    s.cfg = Arc::new(cfg);
                });
                match saved {
                    Ok(()) => Ok("&aLobby spawn set.".into()),
                    Err(e) => Err(format!("&cLobby spawn set, but not saved: {e}")),
                }
            }
            Act::Start => {
                let r = state::with(|s| {
                    if !s.games.contains_key(&arena) {
                        return Err(format!("&cNo game in '{arena}'."));
                    }
                    let (r, effects) = s
                        .step(&arena, |g, ctx, out| g.force_start(ctx, out))
                        .unwrap_or((Err("gone"), Vec::new()));
                    r.map(|_| effects).map_err(|e| format!("&c{e}"))
                });
                r.map(|effects| {
                    host::apply(&server, effects);
                    format!("&aStarted '{arena}'.")
                })
            }
            Act::Stop => {
                let r =
                    state::with(|s| s.step(&arena, |g, _ctx, out| g.abort(out)).map(|(_, e)| e));
                match r {
                    Some(effects) => {
                        host::apply(&server, effects);
                        Ok(format!("&aStopped '{arena}'; it is resetting."))
                    }
                    None => Err(format!("&cNo game in '{arena}'.")),
                }
            }
            Act::Join => {
                let name = word(&args, "player").unwrap_or_default();
                let mode = word(&args, "mode").unwrap_or_default().to_ascii_lowercase();
                let format = word(&args, "format").map(|f| f.to_ascii_lowercase());
                let p = server
                    .get_player_by_name(&name)
                    .ok_or_else(|| fail(&format!("&c{name} is not online.")))?;
                return super::player::join(&server, &sender, &p, &mode, format.as_deref());
            }
            Act::Leave => {
                let name = word(&args, "player").unwrap_or_default();
                let p = server
                    .get_player_by_name(&name)
                    .ok_or_else(|| fail(&format!("&c{name} is not online.")))?;
                let uuid = uuid_of(&p);
                let effects = state::with(|s| s.leave(&uuid, false));
                host::apply(&server, effects);
                Ok(format!("&a{name} left their game."))
            }
            Act::SetKit => {
                let name = word(&args, "player").unwrap_or_default();
                let kit = word(&args, "kit").unwrap_or_default().to_ascii_lowercase();
                let p = server
                    .get_player_by_name(&name)
                    .ok_or_else(|| fail(&format!("&c{name} is not online.")))?;
                let uuid = uuid_of(&p);
                state::with(|s| {
                    if !s.kits.contains_key(&kit) {
                        return Err(format!("&cUnknown kit '{kit}'."));
                    }
                    let msgs = s.cfg.messages.clone();
                    match s.game_of_mut(&uuid).and_then(|g| g.players.get_mut(&uuid)) {
                        Some(pl) => {
                            pl.kit = Some(kit.clone());
                            Ok(text::fill(&msgs.kit_selected, &[("kit", &kit)]))
                        }
                        None => Err(format!("&c{name} is not in a game.")),
                    }
                })
            }
            Act::KitSave => {
                let p = need_player(&sender)?;
                let id = word(&args, "kit").unwrap_or_default().to_ascii_lowercase();
                save_kit(&p, &id)
            }
            Act::KitGive => {
                let id = word(&args, "kit").unwrap_or_default().to_ascii_lowercase();
                let target = match word(&args, "player") {
                    Some(n) => server
                        .get_player_by_name(&n)
                        .ok_or_else(|| fail(&format!("&c{n} is not online.")))?,
                    None => need_player(&sender)?,
                };
                match state::with(|s| s.kits.get(&id).cloned()) {
                    Some(k) => {
                        kits::apply(&target, &k);
                        Ok(format!("&aGave kit '{id}' to {}.", target.get_name()))
                    }
                    None => Err(format!("&cUnknown kit '{id}'.")),
                }
            }
            Act::ArenaList => {
                let out = state::with(|s| {
                    s.arenas
                        .values()
                        .map(|a| {
                            let busy = s
                                .games
                                .get(&a.id)
                                .map(|g| format!("{:?}", g.phase.public()))
                                .unwrap_or_else(|| "free".into());
                            format!(
                                " &e{} &7({}) {} &f{} &7modes: {}",
                                a.id,
                                a.world,
                                if a.enabled { "&aon" } else { "&coff" },
                                busy,
                                if a.modes.is_empty() {
                                    "any".into()
                                } else {
                                    a.modes.join(", ")
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                });
                lines(&sender, &out);
                return Ok(1);
            }
            Act::ArenaCreate => {
                let p = need_player(&sender)?;
                let id = word(&args, "id").unwrap_or_default().to_ascii_lowercase();
                ids::arena(&id).map_err(|e| fail(&format!("&c{e}")))?;
                let (world, l) = here(&p);
                let def = ArenaDef::new(&id, &world, l.block());
                let exists = state::with(|s| {
                    if s.arenas.contains_key(&id) {
                        return true;
                    }
                    s.arenas.insert(id.clone(), Arc::new(def.clone()));
                    false
                });
                if exists {
                    Err(format!("&cArena '{id}' already exists."))
                } else {
                    storage::save_arena(&def).map_err(|e| fail(&format!("&c{e}")))?;
                    Ok(format!(
                        "&aCreated '{id}' in {world} (disabled). Next: pos1, pos2, spawns, then /dbmg arena {id} enable."
                    ))
                }
            }
            Act::ArenaDelete => {
                let id = word(&args, "id").unwrap_or_default().to_ascii_lowercase();
                let r = state::with(|s| {
                    if s.games.contains_key(&id) {
                        return Err("&cA game is using it; /dbmg stop it first.".to_string());
                    }
                    s.arenas
                        .remove(&id)
                        .map(|_| ())
                        .ok_or_else(|| format!("&cUnknown arena '{id}'."))
                });
                r.and_then(|()| storage::delete_arena(&id).map_err(|e| format!("&c{e}")))
                    .map(|()| format!("&aDeleted '{id}'."))
            }
            Act::Info => state::with(|s| {
                let a = s
                    .arenas
                    .get(&arena)
                    .ok_or_else(|| format!("&cUnknown arena '{arena}'."))?;
                let b = a.bounds();
                let mut out = vec![
                    format!(
                        "&6Arena &e{}&6 in &e{}&6: {}",
                        a.id,
                        a.world,
                        if a.enabled { "&aenabled" } else { "&cdisabled" }
                    ),
                    format!(
                        " &fBounds: &7{},{},{} → {},{},{}",
                        b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
                    ),
                    format!(
                        " &fModes: &7{}",
                        if a.modes.is_empty() {
                            "any".into()
                        } else {
                            a.modes.join(", ")
                        }
                    ),
                    format!(" &fSolo spawns: &7{}", a.spawns.len()),
                ];
                for t in &a.teams {
                    out.push(format!(
                        " &fTeam {}{} &7spawns: {}",
                        text::color_code(&t.color),
                        t.id,
                        t.spawns.len()
                    ));
                }
                out.push(format!(
                    " &fSpawners: &7{}  &fRegions: &7{}  &fPoints: &7{}",
                    a.spawners.len(),
                    a.regions.len(),
                    a.points.len()
                ));
                let fits: Vec<String> = s
                    .modes
                    .values()
                    .flat_map(|m| {
                        m.formats
                            .values()
                            .filter(|f| crate::game::matchmaking::arena_fits(a, m, f))
                            .map(move |f| format!("{}:{}", m.id, f.name))
                    })
                    .collect();
                out.push(format!(
                    " &fCan host: &7{}",
                    if fits.is_empty() {
                        "nothing yet".into()
                    } else {
                        fits.join(", ")
                    }
                ));
                for p in a.problems() {
                    out.push(format!(" &c! {p}"));
                }
                Ok(out.join("\n"))
            })
            .map(|t| {
                lines(&sender, &t.lines().map(str::to_string).collect::<Vec<_>>());
                String::new()
            }),
            Act::Pos1 | Act::Pos2 => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let first = matches!(self.0, Act::Pos1);
                edit(&arena, |a| {
                    same_world(a, &world)?;
                    if first {
                        a.pos1 = l.block();
                    } else {
                        a.pos2 = l.block();
                    }
                    let b = a.bounds();
                    Ok(format!(
                        "&aCorner set. Bounds: {},{},{} → {},{},{}",
                        b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
                    ))
                })
            }
            Act::Spectator | Act::Waiting | Act::SoloSpawn => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let act = self.0;
                edit(&arena, |a| {
                    same_world(a, &world)?;
                    let l = tidy(l);
                    Ok(match act {
                        Act::Spectator => {
                            a.spectator = Some(l);
                            "&aSpectator spawn set.".into()
                        }
                        Act::Waiting => {
                            a.waiting = Some(l);
                            "&aWaiting spawn set.".into()
                        }
                        _ => {
                            a.spawns.push(l);
                            format!("&aSolo spawn #{} added.", a.spawns.len())
                        }
                    })
                })
            }
            Act::TeamSpawn => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let team = word(&args, "team").unwrap_or_default().to_ascii_lowercase();
                ids::name(&team, "team id").map_err(|e| fail(&format!("&c{e}")))?;
                edit(&arena, |a| {
                    same_world(a, &world)?;
                    if a.team(&team).is_none() {
                        a.teams.push(TeamDef {
                            id: team.clone(),
                            name: String::new(),
                            color: if text::is_color(&team) {
                                team.clone()
                            } else {
                                "white".into()
                            },
                            spawns: Vec::new(),
                        });
                    }
                    let t = a
                        .teams
                        .iter_mut()
                        .find(|t| t.id == team)
                        .ok_or("team vanished")?;
                    t.spawns.push(tidy(l));
                    Ok(format!(
                        "&aSpawn #{} added for team {team}.",
                        t.spawns.len()
                    ))
                })
            }
            Act::TeamColor => {
                let team = word(&args, "team").unwrap_or_default().to_ascii_lowercase();
                let color = word(&args, "color")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                edit(&arena, |a| {
                    let t = a
                        .teams
                        .iter_mut()
                        .find(|t| t.id == team)
                        .ok_or(format!("&cNo team '{team}'."))?;
                    t.color = color.clone();
                    Ok(format!(
                        "&aTeam {team} is now {}{color}&a.",
                        text::color_code(&color)
                    ))
                })
            }
            Act::TeamRemove => {
                let team = word(&args, "team").unwrap_or_default().to_ascii_lowercase();
                edit(&arena, |a| {
                    let before = a.teams.len();
                    a.teams.retain(|t| t.id != team);
                    if a.teams.len() == before {
                        return Err(format!("&cNo team '{team}'."));
                    }
                    Ok(format!("&aRemoved team {team}."))
                })
            }
            Act::ModeAdd | Act::ModeRemove => {
                let mode = word(&args, "mode").unwrap_or_default().to_ascii_lowercase();
                let add = matches!(self.0, Act::ModeAdd);
                edit(&arena, |a| {
                    ids::mode(&mode)?;
                    a.modes.retain(|m| m != &mode);
                    if add {
                        a.modes.push(mode.clone());
                    }
                    Ok(format!(
                        "&aModes: {}",
                        if a.modes.is_empty() {
                            "any".into()
                        } else {
                            a.modes.join(", ")
                        }
                    ))
                })
            }
            Act::SpawnerAdd => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let group = word(&args, "group")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let item = word(&args, "item").unwrap_or_default().to_ascii_lowercase();
                let interval = int(&args, "interval").unwrap_or(20).max(1) as u32;
                edit(&arena, |a| {
                    same_world(a, &world)?;
                    let mut at = tidy(l);
                    at.yaw = 0.0;
                    a.spawners.push(SpawnerDef {
                        group: group.clone(),
                        item: if item.contains(':') {
                            item.clone()
                        } else {
                            format!("minecraft:{item}")
                        },
                        at,
                        interval,
                        amount: 1,
                        cap: 64,
                        label: String::new(),
                        team: None,
                    });
                    Ok(format!(
                        "&aSpawner added to group {group} (every {interval} ticks). Edit arenas/{}.toml for labels and caps.",
                        a.id
                    ))
                })
            }
            Act::SpawnerRemove => {
                let p = need_player(&sender)?;
                let (_, l) = here(&p);
                edit(&arena, |a| {
                    let near = a
                        .spawners
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            (
                                i,
                                (s.at.x - l.x).powi(2)
                                    + (s.at.y - l.y).powi(2)
                                    + (s.at.z - l.z).powi(2),
                            )
                        })
                        .filter(|(_, d)| *d <= 9.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i)
                        .ok_or("&cNo spawner within 3 blocks.")?;
                    let s = a.spawners.remove(near);
                    Ok(format!("&aRemoved a {} spawner.", s.group))
                })
            }
            Act::RegionPos1 | Act::RegionPos2 => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let name = word(&args, "region")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let first = matches!(self.0, Act::RegionPos1);
                edit(&arena, |a| {
                    same_world(a, &world)?;
                    ids::name(&name, "region name")?;
                    let b = l.block();
                    match a.regions.iter_mut().find(|r| r.name == name) {
                        Some(r) => {
                            if first {
                                r.pos1 = b;
                            } else {
                                r.pos2 = b;
                            }
                        }
                        None => a.regions.push(RegionDef {
                            name: name.clone(),
                            pos1: b,
                            pos2: b,
                            build: None,
                            break_blocks: None,
                            pvp: None,
                            damage: None,
                            track: true,
                        }),
                    }
                    Ok(format!("&aRegion {name} corner set."))
                })
            }
            Act::RegionFlag => {
                let name = word(&args, "region")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let flag = word(&args, "flag").unwrap_or_default().to_ascii_lowercase();
                let value = boolean(&args, "value").unwrap_or(false);
                edit(&arena, |a| {
                    let r = a
                        .regions
                        .iter_mut()
                        .find(|r| r.name == name)
                        .ok_or(format!("&cNo region '{name}'."))?;
                    match flag.as_str() {
                        "build" => r.build = Some(value),
                        "break" => r.break_blocks = Some(value),
                        "pvp" => r.pvp = Some(value),
                        "damage" => r.damage = Some(value),
                        "track" => r.track = value,
                        _ => return Err("&cFlags: build, break, pvp, damage, track.".into()),
                    }
                    Ok(format!("&aRegion {name}: {flag} = {value}."))
                })
            }
            Act::RegionRemove => {
                let name = word(&args, "region")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                edit(&arena, |a| {
                    let before = a.regions.len();
                    a.regions.retain(|r| r.name != name);
                    if before == a.regions.len() {
                        Err(format!("&cNo region '{name}'."))
                    } else {
                        Ok(format!("&aRemoved region {name}."))
                    }
                })
            }
            Act::PointAdd => {
                let p = need_player(&sender)?;
                let (world, l) = here(&p);
                let name = word(&args, "point")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                edit(&arena, |a| {
                    same_world(a, &world)?;
                    ids::name(&name, "point name")?;
                    let list = a.points.entry(name.clone()).or_default();
                    list.push(l);
                    Ok(format!("&aPoint {name} #{} added.", list.len()))
                })
            }
            Act::PointClear => {
                let name = word(&args, "point")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                edit(&arena, |a| {
                    a.points.remove(&name);
                    Ok(format!("&aCleared points '{name}'."))
                })
            }
            Act::Value => {
                let key = word(&args, "key").unwrap_or_default();
                let value = word(&args, "value").unwrap_or_default();
                edit(&arena, |a| {
                    a.values.insert(key.clone(), value.clone());
                    Ok(format!("&aValue {key} = {value}"))
                })
            }
            Act::Enable | Act::Disable => {
                let on = matches!(self.0, Act::Enable);
                edit(&arena, |a| {
                    if on {
                        let problems: Vec<String> = a
                            .problems()
                            .into_iter()
                            .filter(|p| !p.starts_with("no spectator"))
                            .collect();
                        if !problems.is_empty() {
                            return Err(format!("&cCan't enable: {}", problems.join("; ")));
                        }
                    }
                    a.enabled = on;
                    Ok(format!(
                        "&aArena {} {}.",
                        a.id,
                        if on { "enabled" } else { "disabled" }
                    ))
                })
            }
            Act::MaxPlayers => {
                let n = int(&args, "n").unwrap_or(0).max(0) as u32;
                edit(&arena, |a| {
                    a.max_players = n;
                    Ok(format!(
                        "&aMax players: {}",
                        if n == 0 {
                            "format limit".into()
                        } else {
                            n.to_string()
                        }
                    ))
                })
            }
        };
        match result {
            Ok(msg) => {
                if !msg.is_empty() {
                    reply(&sender, &msg);
                }
                Ok(1)
            }
            Err(msg) => Err(fail(&msg)),
        }
    }
}

/// Reloads every file. Running games keep the definitions they started with.
pub fn reload() -> String {
    let persist = state::with(|s| s.persist);
    let loaded = storage::load_all(persist);
    let errors = loaded.errors.clone();
    let summary = format!(
        "&aReloaded: {} arenas, {} modes, {} kits.",
        loaded.arenas.len(),
        loaded.modes.len(),
        loaded.kits.len()
    );
    state::with(|s| {
        s.replace_defs(loaded.config, loaded.arenas, loaded.modes, loaded.kits);
        s.pie_dirty = true;
    });
    for e in &errors {
        tracing::warn!("{}: {e}", crate::ID);
    }
    if errors.is_empty() {
        summary
    } else {
        format!("{summary} &c{} problem(s), see the console.", errors.len())
    }
}

/// Saves the player's inventory as a kit.
fn save_kit(p: &Player, id: &str) -> Result<String, String> {
    ids::kit(id)?;
    let inv = p.get_inventory();
    let main = inv.as_inventory();
    let to_item = |stack: pumpkin_plugin_api::ItemStack, slot: Option<u8>| KitItem {
        item: stack.get_registry_key(),
        count: stack.get_count(),
        name: stack
            .get_custom_name()
            .map(|n| n.get_text())
            .filter(|n| !n.is_empty()),
        lore: Vec::new(),
        enchantments: stack
            .get_enchantments()
            .into_iter()
            .map(|e| (kits::enchantment_name(e.enchantment), e.level))
            .collect(),
        slot,
    };
    let mut kit = KitDef {
        id: id.to_string(),
        display_name: id.to_string(),
        clear: true,
        ..KitDef::default()
    };
    for slot in 0..36u32 {
        if let Some(stack) = main.get_item(slot)
            && stack.get_registry_key() != "minecraft:air"
        {
            kit.items.push(to_item(stack, Some(slot as u8)));
        }
    }
    kit.helmet = inv.get_helmet().map(|s| to_item(s, None));
    kit.chestplate = inv.get_chestplate().map(|s| to_item(s, None));
    kit.leggings = inv.get_leggings().map(|s| to_item(s, None));
    kit.boots = inv.get_boots().map(|s| to_item(s, None));
    kit.offhand = inv.get_off_hand().map(|s| to_item(s, None));
    storage::save_kit(&kit)?;
    let n = kit.items.len();
    state::with(|s| s.kits.insert(id.to_string(), Arc::new(kit)));
    Ok(format!("&aSaved kit '{id}' ({n} items) to kits/{id}.toml."))
}
