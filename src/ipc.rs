//! The protocol for mode plugins (see the `minigamecore-client` crate).
//!
//! Pure: reads and changes state in memory only. Host work (teleports, kits, blocks)
//! is queued in `State::deferred` and done by the next tick. The core never calls
//! another plugin from here, so mode plugins may call it from anywhere.

use minigamecore_client::{
    ErrorKind, ErrorReply, MessageKind, PROTOCOL_VERSION, Reply, Request, WireRequest, WireResponse,
};

use crate::game::{GamePhase, Sched, effects::Effect};
use crate::model::{ids, text};
use crate::state::State;

/// Default and largest number of events per poll.
const POLL_DEFAULT: u32 = 256;
const POLL_MAX: u32 = 4096;

fn err(kind: ErrorKind, message: impl Into<String>) -> ErrorReply {
    ErrorReply {
        kind,
        message: message.into(),
    }
}

/// Decodes, runs and encodes one request.
pub fn handle(sender: &str, bytes: &[u8]) -> Vec<u8> {
    let result = match serde_json::from_slice::<WireRequest>(bytes) {
        Ok(req) if req.v > PROTOCOL_VERSION => Err(err(
            ErrorKind::UnsupportedVersion,
            format!("protocol {} is newer than {PROTOCOL_VERSION}", req.v),
        )),
        Ok(req) => crate::state::with(|s| run(s, sender, req.request)),
        Err(e) => Err(err(ErrorKind::BadRequest, e.to_string())),
    };
    serde_json::to_vec(&WireResponse::from_result(result)).unwrap_or_default()
}

/// Checks that `sender` owns the game's mode and returns the game id.
fn owned(s: &State, sender: &str, game: &str) -> Result<String, ErrorReply> {
    let g = s
        .games
        .get(game)
        .ok_or_else(|| err(ErrorKind::UnknownGame, format!("no game in arena '{game}'")))?;
    match s.owners.get(&g.mode.id) {
        Some(owner) if owner == sender => Ok(game.to_string()),
        _ => Err(err(
            ErrorKind::NotOwner,
            format!("'{sender}' does not own mode '{}'", g.mode.id),
        )),
    }
}

/// An online player → (uuid, name).
fn player(s: &State, who: &str) -> Result<(String, String), ErrorReply> {
    s.resolve(who).ok_or_else(|| {
        err(
            ErrorKind::UnknownPlayer,
            format!("player '{who}' is not online"),
        )
    })
}

/// A player in a game whose mode `sender` owns → (uuid, game id).
fn owned_player(s: &State, sender: &str, who: &str) -> Result<(String, String), ErrorReply> {
    let (uuid, _) = player(s, who)?;
    let game = s.index.get(&uuid).cloned().ok_or_else(|| {
        err(
            ErrorKind::UnknownPlayer,
            format!("'{who}' is not in a game"),
        )
    })?;
    owned(s, sender, &game)?;
    Ok((uuid, game))
}

pub fn run(s: &mut State, sender: &str, req: Request) -> Result<Reply, ErrorReply> {
    let now = s.now;
    match req {
        Request::Ping => Ok(Reply::Pong {
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").into(),
        }),
        Request::RegisterMode { mode } => {
            ids::mode(&mode).map_err(|m| err(ErrorKind::BadRequest, m))?;
            match s.owners.get(&mode) {
                Some(owner) if owner != sender => Err(err(
                    ErrorKind::ModeTaken,
                    format!("mode '{mode}' belongs to '{owner}'"),
                )),
                _ => {
                    s.owners.insert(mode, sender.to_string());
                    s.queues.entry(sender.to_string()).or_default();
                    Ok(Reply::Done)
                }
            }
        }
        Request::Poll { max } => {
            let max = max.unwrap_or(POLL_DEFAULT).clamp(1, POLL_MAX) as usize;
            let (events, dropped) = s.poll(sender, max);
            Ok(Reply::Events { events, dropped })
        }
        Request::ListGames { mode } => Ok(Reply::Games(
            s.games
                .values()
                .filter(|g| mode.as_deref().is_none_or(|m| m == g.mode.id))
                .map(|g| g.info(now))
                .collect(),
        )),
        Request::Game { game } => s
            .games
            .get(&game)
            .map(|g| Reply::Game(g.info(now)))
            .ok_or_else(|| err(ErrorKind::UnknownGame, game)),
        Request::Player { player: who } => {
            let uuid = match s.resolve(&who) {
                Some((u, _)) => u,
                None => return Ok(Reply::Player(None)),
            };
            Ok(Reply::Player(s.game_of(&uuid).and_then(|g| {
                g.players.get(&uuid).map(|p| g.player_info(p))
            })))
        }
        Request::Arena { arena } => s
            .arenas
            .get(&arena)
            .map(|a| Reply::Arena(a.info()))
            .ok_or_else(|| err(ErrorKind::UnknownArena, arena)),
        Request::ListArenas { mode } => Ok(Reply::Arenas(
            s.arenas
                .values()
                .filter(|a| mode.as_deref().is_none_or(|m| a.allows_mode(m)))
                .map(|a| a.id.clone())
                .collect(),
        )),
        Request::Join {
            players,
            mode,
            format,
            arena,
        } => {
            let party = players
                .iter()
                .map(|p| player(s, p))
                .collect::<Result<Vec<_>, _>>()?;
            let (game, effects) = s.join(party, &mode, format.as_deref(), arena.as_deref())?;
            s.deferred.extend(effects);
            Ok(Reply::Joined { game })
        }
        Request::Leave { player: who } => {
            let (uuid, _) = player(s, &who)?;
            if !s.index.contains_key(&uuid) {
                return Err(err(
                    ErrorKind::UnknownPlayer,
                    format!("'{who}' is not in a game"),
                ));
            }
            let effects = s.leave(&uuid, false);
            s.deferred.extend(effects);
            Ok(Reply::Done)
        }
        Request::Start { game } => {
            let game = owned(s, sender, &game)?;
            let (r, effects) = s
                .step(&game, |g, ctx, out| g.force_start(ctx, out))
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            s.deferred.extend(effects);
            r.map(|_| Reply::Done)
                .map_err(|m| err(ErrorKind::WrongState, m))
        }
        Request::EndRound {
            game,
            winners,
            reason,
        } => {
            let game = owned(s, sender, &game)?;
            let reason = reason.unwrap_or_else(|| "plugin".into());
            let (ended, effects) = s
                .step(&game, |g, ctx, out| g.end_round(ctx, winners, &reason, out))
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            s.deferred.extend(effects);
            if ended {
                Ok(Reply::Done)
            } else {
                Err(err(ErrorKind::WrongState, "the round is not running"))
            }
        }
        Request::Eliminate { player: who } => {
            let (uuid, game) = owned_player(s, sender, &who)?;
            let (ok, effects) = s
                .step(&game, |g, ctx, out| g.eliminate(ctx, &uuid, out))
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            s.deferred.extend(effects);
            if ok {
                Ok(Reply::Done)
            } else {
                Err(err(ErrorKind::WrongState, "the player is not in the round"))
            }
        }
        Request::SetRespawn {
            game,
            team,
            player: who,
            allowed,
        } => {
            let game = owned(s, sender, &game)?;
            let uuid = match who {
                Some(w) => Some(player(s, &w)?.0),
                None => None,
            };
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            if g.set_respawn(team.as_deref(), uuid.as_deref(), allowed) {
                Ok(Reply::Done)
            } else {
                Err(err(
                    ErrorKind::UnknownTeam,
                    "no such team or player in this game",
                ))
            }
        }
        Request::SetRule { game, key, value } => {
            let game = owned(s, sender, &game)?;
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            g.rules
                .set(&key, value)
                .map_err(|m| err(ErrorKind::BadRequest, m))?;
            Ok(Reply::Done)
        }
        Request::SpawnerGroup {
            game,
            group,
            interval_ticks,
            amount,
            enabled,
        } => {
            let game = owned(s, sender, &game)?;
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            let rt = g.group_mut(&group);
            if let Some(i) = interval_ticks {
                if i == 0 {
                    return Err(err(
                        ErrorKind::BadRequest,
                        "interval_ticks must be at least 1",
                    ));
                }
                rt.interval = Some(i);
            }
            if let Some(a) = amount {
                rt.amount = Some(a);
            }
            if let Some(e) = enabled {
                rt.enabled = e;
            }
            Ok(Reply::Done)
        }
        Request::Schedule {
            game,
            id,
            at_secs,
            label,
        } => {
            let game = owned(s, sender, &game)?;
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            g.schedule.retain(|e| e.id != id);
            g.schedule.push(Sched {
                at: at_secs,
                id,
                label: label.unwrap_or_default(),
                actions: Vec::new(),
                fired: false,
            });
            Ok(Reply::Done)
        }
        Request::CancelSchedule { game, id } => {
            let game = owned(s, sender, &game)?;
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            g.schedule.retain(|e| e.id != id);
            Ok(Reply::Done)
        }
        Request::SetVar {
            game,
            player: who,
            key,
            value,
        } => {
            let game = owned(s, sender, &game)?;
            let uuid = match who {
                Some(w) => Some(player(s, &w)?.0),
                None => None,
            };
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            let map = match uuid {
                Some(u) => {
                    &mut g
                        .players
                        .get_mut(&u)
                        .ok_or_else(|| err(ErrorKind::UnknownPlayer, "player is not in this game"))?
                        .vars
                }
                None => &mut g.vars,
            };
            match value {
                Some(v) => map.insert(key, v),
                None => map.remove(&key),
            };
            Ok(Reply::Done)
        }
        Request::Counter {
            player: who,
            key,
            delta,
        } => {
            let (uuid, game) = owned_player(s, sender, &who)?;
            let g = s
                .games
                .get_mut(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            let p = g
                .players
                .get_mut(&uuid)
                .ok_or_else(|| err(ErrorKind::UnknownPlayer, who.clone()))?;
            let v = p.counters.entry(key).or_insert(0);
            *v = v.saturating_add(delta);
            Ok(Reply::Value(*v))
        }
        Request::SetBlock { game, pos, block } => {
            let game = owned(s, sender, &game)?;
            let g = s
                .games
                .get(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            if !g.arena.bounds().contains_block(pos) {
                return Err(err(ErrorKind::BadRequest, "position is outside the arena"));
            }
            if matches!(g.phase, GamePhase::Resetting) {
                return Err(err(ErrorKind::WrongState, "the arena is resetting"));
            }
            let world = g.arena.world.clone();
            s.deferred.push(Effect::SetBlock {
                game,
                world,
                pos,
                block,
            });
            Ok(Reply::Done)
        }
        Request::RecordBlock { game, pos, state } => {
            let game = owned(s, sender, &game)?;
            s.record_block(&game, pos, state);
            Ok(Reply::Done)
        }
        Request::Message {
            game,
            team,
            player: who,
            text: msg,
            kind,
        } => {
            let game = owned(s, sender, &game)?;
            let uuid = match who {
                Some(w) => Some(player(s, &w)?.0),
                None => None,
            };
            let g = s
                .games
                .get(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            let targets = match (uuid, team) {
                (Some(u), _) => vec![u],
                (None, Some(t)) => g.members(&t),
                (None, None) => g.audience(),
            };
            let msg = text::fill(&msg, &[]);
            s.deferred.push(match kind {
                MessageKind::Chat => Effect::Chat {
                    players: targets,
                    text: msg,
                },
                MessageKind::ActionBar => Effect::ActionBar {
                    players: targets,
                    text: msg,
                },
                MessageKind::Title => Effect::Title {
                    players: targets,
                    title: msg,
                    subtitle: String::new(),
                    stay_ticks: 40,
                },
                MessageKind::Subtitle => Effect::Title {
                    players: targets,
                    title: " ".into(),
                    subtitle: msg,
                    stay_ticks: 40,
                },
            });
            Ok(Reply::Done)
        }
        Request::GiveKit { player: who, kit } => {
            let (uuid, _) = owned_player(s, sender, &who)?;
            if !s.kits.contains_key(&kit) {
                return Err(err(ErrorKind::UnknownKit, kit));
            }
            s.deferred.push(Effect::Kit { player: uuid, kit });
            Ok(Reply::Done)
        }
        Request::SelectKit { player: who, kit } => {
            let (uuid, game) = owned_player(s, sender, &who)?;
            if !s.kits.contains_key(&kit) {
                return Err(err(ErrorKind::UnknownKit, kit));
            }
            if let Some(p) = s
                .games
                .get_mut(&game)
                .and_then(|g| g.players.get_mut(&uuid))
            {
                p.kit = Some(kit);
            }
            Ok(Reply::Done)
        }
        Request::SameTeam { a, b } => {
            let (ua, _) = player(s, &a)?;
            let (ub, _) = player(s, &b)?;
            let same = match (s.index.get(&ua), s.index.get(&ub)) {
                (Some(ga), Some(gb)) if ga == gb => s.games.get(ga).is_some_and(|g| {
                    let ta = g.players.get(&ua).and_then(|p| p.team.as_ref());
                    ta.is_some() && ta == g.players.get(&ub).and_then(|p| p.team.as_ref())
                }),
                _ => false,
            };
            Ok(Reply::Bool(same))
        }
        Request::RegionsAt { game, pos } => {
            let g = s
                .games
                .get(&game)
                .ok_or_else(|| err(ErrorKind::UnknownGame, game.clone()))?;
            Ok(Reply::Regions(g.arena.regions_at(&pos)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::{arena, mode};
    use minigamecore_client::{Event, WireResponse};

    fn call(s: &mut State, sender: &str, json: &str) -> WireResponse {
        let req: WireRequest = serde_json::from_str(json).unwrap();
        WireResponse::from_result(run(s, sender, req.request))
    }

    fn state() -> State {
        let mut s = State::new();
        s.arenas.insert("castle".into(), arena());
        s.modes.insert("bedwars".into(), mode(""));
        s.player_online("u1", "Steve", 1);
        s.player_online("u2", "Alex", 2);
        s
    }

    #[test]
    fn ownership_and_flow() {
        let mut s = state();
        assert!(call(&mut s, "bw", r#"{"op":"register_mode","mode":"bedwars"}"#).ok);
        let taken = call(
            &mut s,
            "other",
            r#"{"op":"register_mode","mode":"bedwars"}"#,
        );
        assert_eq!(taken.error.unwrap().kind, ErrorKind::ModeTaken);
        let joined = call(
            &mut s,
            "lobby",
            r#"{"op":"join","players":["Steve"],"mode":"bedwars","format":"ffa"}"#,
        );
        assert_eq!(
            joined.reply,
            Some(Reply::Joined {
                game: "castle".into()
            })
        );
        call(
            &mut s,
            "lobby",
            r#"{"op":"join","players":["alex"],"mode":"bedwars"}"#,
        );
        // Only the owner may change the game.
        let denied = call(&mut s, "other", r#"{"op":"start","game":"castle"}"#);
        assert_eq!(denied.error.unwrap().kind, ErrorKind::NotOwner);
        assert!(call(&mut s, "bw", r#"{"op":"start","game":"castle"}"#).ok);
        assert!(
            call(
                &mut s,
                "bw",
                r#"{"op":"counter","player":"Steve","key":"beds_broken","delta":2}"#
            )
            .ok
        );
        let v = call(
            &mut s,
            "bw",
            r#"{"op":"counter","player":"Steve","key":"beds_broken","delta":1}"#,
        );
        assert_eq!(v.reply, Some(Reply::Value(3)));
        assert!(
            call(
                &mut s,
                "bw",
                r#"{"op":"set_rule","game":"castle","key":"pvp","value":false}"#
            )
            .ok
        );
        assert!(!s.games["castle"].rules.pvp);
        let same = call(&mut s, "x", r#"{"op":"same_team","a":"Steve","b":"Alex"}"#);
        assert_eq!(same.reply, Some(Reply::Bool(false)));
        assert!(call(&mut s, "bw", r#"{"op":"set_block","game":"castle","pos":{"x":0,"y":64,"z":0},"block":"minecraft:air"}"#).ok);
        assert!(matches!(s.deferred.last(), Some(Effect::SetBlock { .. })));
        let outside = call(
            &mut s,
            "bw",
            r#"{"op":"set_block","game":"castle","pos":{"x":999,"y":64,"z":0},"block":"minecraft:air"}"#,
        );
        assert!(!outside.ok);
        assert!(
            call(
                &mut s,
                "bw",
                r#"{"op":"end_round","game":"castle","winners":["steve"]}"#
            )
            .ok
        );
        let polled = call(&mut s, "bw", r#"{"op":"poll"}"#);
        let Some(Reply::Events { events, .. }) = polled.reply else {
            panic!()
        };
        assert!(events.iter().any(|e| matches!(&e.event, Event::RoundEnded { winners, .. } if winners == &["steve".to_string()])));
        assert_eq!(s.stats.get("u1", Some("bedwars"), "beds_broken"), 3);
    }

    #[test]
    fn bad_requests() {
        let mut s = state();
        let out: WireResponse = serde_json::from_slice(&handle("x", b"{nope")).unwrap();
        assert_eq!(out.error.unwrap().kind, ErrorKind::BadRequest);
        let out: WireResponse =
            serde_json::from_slice(&handle("x", br#"{"v":99,"op":"ping"}"#)).unwrap();
        assert_eq!(out.error.unwrap().kind, ErrorKind::UnsupportedVersion);
        let r = call(
            &mut s,
            "x",
            r#"{"op":"join","players":["Nobody"],"mode":"bedwars"}"#,
        );
        assert_eq!(r.error.unwrap().kind, ErrorKind::UnknownPlayer);
        let r = call(&mut s, "x", r#"{"op":"player","player":"Steve"}"#);
        assert_eq!(r.reply, Some(Reply::Player(None)));
    }
}
