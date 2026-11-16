//! `/mg` — join, leave, list, kit, team, stats.

use pumpkin_plugin_api::{
    Server,
    command::{
        ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
    },
    commands::CommandHandler,
};

use super::{fail, lines, need_player, reply, word};
use crate::host;
use crate::listeners::{has, uuid_of};
use crate::model::text;
use crate::state;

#[derive(Clone, Copy)]
enum Act {
    Help,
    Join,
    Leave,
    List,
    Modes,
    Kit,
    Team,
    Stats,
}

struct H(Act);

fn w() -> ArgumentType {
    ArgumentType::String(StringType::SingleWord)
}

pub fn command() -> Command {
    Command::new(
        &["mg".into(), "minigame".into(), "minigames".into()],
        "Minigames: join, leave and more.",
    )
    .execute(H(Act::Help))
    .then(
        CommandNode::literal("join").then(
            CommandNode::argument("mode", &w())
                .execute(H(Act::Join))
                .then(CommandNode::argument("format", &w()).execute(H(Act::Join))),
        ),
    )
    .then(CommandNode::literal("leave").execute(H(Act::Leave)))
    .then(CommandNode::literal("list").execute(H(Act::List)))
    .then(CommandNode::literal("modes").execute(H(Act::Modes)))
    .then(CommandNode::literal("kit").then(CommandNode::argument("kit", &w()).execute(H(Act::Kit))))
    .then(
        CommandNode::literal("team")
            .then(CommandNode::argument("team", &w()).execute(H(Act::Team))),
    )
    .then(
        CommandNode::literal("stats")
            .execute(H(Act::Stats))
            .then(CommandNode::argument("player", &w()).execute(H(Act::Stats))),
    )
}

impl CommandHandler for H {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        match self.0 {
            Act::Help => {
                lines(
                    &sender,
                    &[
                        "&6/mg join <mode> [format] &7- join a game".into(),
                        "&6/mg leave &7- leave your game".into(),
                        "&6/mg list &7- running games".into(),
                        "&6/mg modes &7- modes and formats".into(),
                        "&6/mg kit <kit> &7- choose a kit".into(),
                        "&6/mg team <team> &7- choose a team".into(),
                        "&6/mg stats [player] &7- statistics".into(),
                    ],
                );
                Ok(1)
            }
            Act::Join => {
                let p = need_player(&sender)?;
                let mode = word(&args, "mode").unwrap_or_default().to_ascii_lowercase();
                let format = word(&args, "format").map(|f| f.to_ascii_lowercase());
                join(&server, &sender, &p, &mode, format.as_deref())
            }
            Act::Leave => {
                let p = need_player(&sender)?;
                let uuid = uuid_of(&p);
                let (effects, msg) = state::with(|s| {
                    if !s.index.contains_key(&uuid) {
                        return (None, s.cfg.messages.not_in_game.clone());
                    }
                    (
                        Some(s.leave(&uuid, false)),
                        s.cfg.messages.left_game.clone(),
                    )
                });
                let ok = effects.is_some();
                if let Some(e) = effects {
                    host::apply(&server, e);
                }
                reply(&sender, &msg);
                Ok(i32::from(ok))
            }
            Act::List => {
                let out = state::with(|s| {
                    let mut out = vec!["&6Games:".to_string()];
                    for g in s.games.values() {
                        out.push(format!(
                            " &e{} &7{}:{} &f{:?} &a{}/{}",
                            g.id,
                            g.mode.id,
                            g.format.name,
                            g.phase.public(),
                            g.players.len(),
                            g.capacity()
                        ));
                    }
                    if s.games.is_empty() {
                        out.push(" &7none".into());
                    }
                    let free = s
                        .arenas
                        .values()
                        .filter(|a| a.enabled && !s.games.contains_key(&a.id))
                        .count();
                    out.push(format!("&7Free arenas: &f{free}"));
                    out
                });
                lines(&sender, &out);
                Ok(1)
            }
            Act::Modes => {
                let out = state::with(|s| {
                    let mut out = vec!["&6Modes:".to_string()];
                    for m in s.modes.values() {
                        let formats: Vec<String> = m
                            .formats
                            .values()
                            .map(|f| format!("{} ({})", f.name, f.max_players()))
                            .collect();
                        out.push(format!(
                            " &e{} &7{} &f{}",
                            m.id,
                            m.display_name,
                            formats.join(", ")
                        ));
                    }
                    out
                });
                lines(&sender, &out);
                Ok(1)
            }
            Act::Kit => {
                let p = need_player(&sender)?;
                let uuid = uuid_of(&p);
                let kit = word(&args, "kit").unwrap_or_default().to_ascii_lowercase();
                let check = state::with(|s| {
                    let msgs = s.cfg.messages.clone();
                    let Some(def) = s.kits.get(&kit).cloned() else {
                        return Err(text::fill(&msgs.kit_unknown, &[("kit", &kit)]));
                    };
                    let Some(g) = s.game_of(&uuid) else {
                        return Err(msgs.not_in_game.clone());
                    };
                    if !g.mode.kits.is_empty() && !g.mode.kits.contains(&kit) {
                        return Err(text::fill(&msgs.kit_unknown, &[("kit", &kit)]));
                    }
                    Ok((def, msgs))
                });
                let (def, msgs) = check.map_err(|m| fail(&m))?;
                if !def.permission.is_empty()
                    && !p.has_permission(&def.permission)
                    && !has(&p, "kit.all")
                {
                    return Err(fail(&msgs.kit_denied));
                }
                state::with(|s| {
                    if let Some(pl) = s.game_of_mut(&uuid).and_then(|g| g.players.get_mut(&uuid)) {
                        pl.kit = Some(kit.clone());
                    }
                });
                reply(
                    &sender,
                    &text::fill(&msgs.kit_selected, &[("kit", def.display())]),
                );
                Ok(1)
            }
            Act::Team => {
                let p = need_player(&sender)?;
                let uuid = uuid_of(&p);
                let team = word(&args, "team").unwrap_or_default().to_ascii_lowercase();
                let r = state::with(|s| {
                    let msgs = s.cfg.messages.clone();
                    let Some(g) = s.game_of_mut(&uuid) else {
                        return Err(msgs.not_in_game);
                    };
                    match g.choose_team(&uuid, &team) {
                        Ok(()) => {
                            let shown = g.team(&team).map(|t| t.display()).unwrap_or_default();
                            Ok(text::fill(&msgs.team_selected, &[("team", &shown)]))
                        }
                        Err("full") => Err(msgs.team_full),
                        Err(e) => Err(format!("&c{e}")),
                    }
                });
                let msg = r.map_err(|m| fail(&m))?;
                reply(&sender, &msg);
                Ok(1)
            }
            Act::Stats => {
                let target = match word(&args, "player") {
                    Some(n) => n,
                    None => need_player(&sender)?.get_name(),
                };
                let out = state::with(|s| {
                    let uuid = s
                        .resolve(&target)
                        .map(|r| r.0)
                        .or_else(|| s.stats.uuid_by_name(&target))?;
                    let st = s.stats.players.get(&uuid)?;
                    let mut out = vec![format!("&6Stats for &e{}&6:", st.name)];
                    let g = |m: Option<&str>, k: &str| s.stats.get(&uuid, m, k);
                    out.push(format!(
                        " &fTotal: &a{} &fwins, &c{} &flosses, &e{} &fkills, &7{} &fdeaths, &b{} &fplayed",
                        g(None, "wins"),
                        g(None, "losses"),
                        g(None, "kills"),
                        g(None, "deaths"),
                        g(None, "played")
                    ));
                    for m in st.modes.keys() {
                        out.push(format!(
                            " &e{m}: &a{} &fW &c{} &fL &e{} &fK &7{} &fD &fbest streak &b{}",
                            g(Some(m), "wins"),
                            g(Some(m), "losses"),
                            g(Some(m), "kills"),
                            g(Some(m), "deaths"),
                            g(Some(m), "best_win_streak")
                        ));
                    }
                    Some(out)
                });
                match out {
                    Some(l) => {
                        lines(&sender, &l);
                        Ok(1)
                    }
                    None => Err(fail(&format!("&cNo stats for {target}."))),
                }
            }
        }
    }
}

/// Joins `p` to a game and reports the result. Shared with `/dbmg join`.
pub fn join(
    server: &Server,
    sender: &CommandSender,
    p: &pumpkin_plugin_api::player::Player,
    mode: &str,
    format: Option<&str>,
) -> Result<i32, CommandError> {
    let uuid = uuid_of(p);
    let name = p.get_name();
    let r = state::with(|s| {
        s.player_online(&uuid, &name, p.as_entity().get_id() as i32);
        let msgs = s.cfg.messages.clone();
        match s.join(vec![(uuid.clone(), name.clone())], mode, format, None) {
            Ok((game, effects)) => {
                let g = s.games.get(&game);
                let line = text::fill(
                    &msgs.joined_game,
                    &[
                        (
                            "mode_name",
                            g.map(|g| g.mode.display_name.as_str()).unwrap_or(mode),
                        ),
                        (
                            "format",
                            g.map(|g| g.format.display_name.as_str()).unwrap_or(""),
                        ),
                        ("arena", &game),
                    ],
                );
                Ok((effects, line))
            }
            Err(e) => Err(match e.kind {
                minigamecore_client::ErrorKind::AlreadyInGame => msgs.already_in_game,
                minigamecore_client::ErrorKind::NoArena => {
                    text::fill(&msgs.no_arena, &[("mode", mode)])
                }
                minigamecore_client::ErrorKind::UnknownMode => {
                    text::fill(&msgs.unknown_mode, &[("mode", mode)])
                }
                _ => format!("&c{}", e.message),
            }),
        }
    });
    match r {
        Ok((effects, line)) => {
            host::apply(server, effects);
            p.send_system_message(
                host::colored(&format!(
                    "{}{line}",
                    state::with(|s| s.cfg.messages.prefix.clone())
                )),
                false,
            );
            let _ = sender;
            Ok(1)
        }
        Err(m) => Err(fail(&m)),
    }
}
