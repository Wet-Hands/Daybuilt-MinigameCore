//! Matchmaking: which arena a party of players should go to. Pure.
//!
//! 1. A game of the requested mode (and format, if given) that is still waiting or
//!    counting down and has room for the whole party. Fullest first, so games start sooner.
//! 2. Otherwise an idle arena that can host the mode/format, preferring the mode's
//!    default format when none was asked for.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Game;
use crate::model::{
    arena::ArenaDef,
    mode::{FormatDef, ModeDef, TeamMode},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Join this existing game.
    Existing(String),
    /// Open a new game in this arena with this format.
    New { arena: String, format: String },
}

/// Can `arena` host `format` of `mode` at all?
pub fn arena_fits(arena: &ArenaDef, mode: &ModeDef, format: &FormatDef) -> bool {
    if !arena.enabled || !arena.allows_format(&mode.id, &format.name) || arena.pos1 == arena.pos2 {
        return false;
    }
    if arena.max_players > 0 && arena.max_players < format.min_players {
        return false;
    }
    match format.team_mode {
        TeamMode::Teams => arena.usable_teams().count() >= format.teams as usize,
        TeamMode::Solo => !arena.solo_spawns().is_empty(),
    }
}

pub fn choose(
    games: &BTreeMap<String, Game>,
    arenas: &BTreeMap<String, Arc<ArenaDef>>,
    mode: &ModeDef,
    format: Option<&str>,
    arena: Option<&str>,
    party: usize,
) -> Option<Choice> {
    let format_ok = |f: &str| format.is_none_or(|want| want == f);
    let arena_ok = |a: &str| arena.is_none_or(|want| want == a);

    let mut open: Vec<&Game> = games
        .values()
        .filter(|g| {
            g.mode.id == mode.id
                && format_ok(&g.format.name)
                && arena_ok(&g.id)
                && g.phase.joinable()
                && g.players.len() + party <= g.capacity()
        })
        .collect();
    open.sort_by_key(|g| std::cmp::Reverse(g.players.len()));
    if let Some(g) = open.first() {
        return Some(Choice::Existing(g.id.clone()));
    }

    let mut formats: Vec<&FormatDef> = mode.formats.values().filter(|f| format_ok(&f.name)).collect();
    formats.sort_by_key(|f| f.name != mode.default_format);
    for f in formats {
        if party > f.max_players() as usize {
            continue;
        }
        let free = arenas.values().find(|a| {
            arena_ok(&a.id) && !games.contains_key(&a.id) && arena_fits(a, mode, f)
        });
        if let Some(a) = free {
            return Some(Choice::New {
                arena: a.id.clone(),
                format: f.name.clone(),
            });
        }
    }
    None
}
