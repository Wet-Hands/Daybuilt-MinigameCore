//! Sidebar text for a player in a game. Pure: returns lines with `&` codes.
//!
//! Placeholders: {arena} {mode} {mode_name} {format} {players} {max} {min} {status}
//! {seconds} {time} {time_left} {alive} {teams_alive} {kills} {deaths} {team} {kit}
//! {round} {var:<key>} {counter:<key>}. A line that is exactly `{teams}`, `{winners}` or
//! `{next_event}` expands to several lines (or none).

use super::{Game, GamePhase, TICKS};
use crate::config::Config;
use crate::model::text;

/// Most lines a sidebar shows.
pub const MAX_LINES: usize = 15;

pub fn render(game: &Game, cfg: &Config, now: u64, uuid: &str) -> (String, Vec<String>) {
    let (title, templates) = match &game.mode.sidebar {
        Some(sb) => (
            if sb.title.is_empty() {
                cfg.sidebar.title.clone()
            } else {
                sb.title.clone()
            },
            match game.phase {
                GamePhase::Waiting | GamePhase::Countdown { .. } => &sb.waiting,
                GamePhase::Running { .. } => &sb.running,
                _ => &sb.ending,
            },
        ),
        None => (
            cfg.sidebar.title.clone(),
            match game.phase {
                GamePhase::Waiting | GamePhase::Countdown { .. } => &cfg.sidebar.waiting,
                GamePhase::Running { .. } => &cfg.sidebar.running,
                _ => &cfg.sidebar.ending,
            },
        ),
    };
    let values = values(game, cfg, now, uuid);
    let pairs: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut lines = Vec::new();
    for t in templates {
        match t.trim() {
            "{teams}" => lines.extend(team_lines(game, uuid)),
            "{winners}" => {
                if game.winners.is_empty() {
                    lines.push("&eDraw".to_string());
                } else {
                    lines.extend(
                        game.winners
                            .iter()
                            .filter_map(|w| game.team(w))
                            .map(|t| format!(" {}", t.display())),
                    );
                }
            }
            "{next_event}" => {
                if let Some((label, secs)) = game.next_event(now) {
                    lines.push(format!("{label} &7in &a{}", text::clock(secs)));
                } else if game.rules.time_limit > 0
                    && matches!(game.phase, GamePhase::Running { .. })
                {
                    let left = game.rules.time_limit.saturating_sub(game.elapsed(now));
                    lines.push(format!("&fEnds in &a{}", text::clock(left)));
                }
            }
            _ => lines.push(fill_counters(
                game,
                &game.fill_vars(&text::fill(t, &pairs), Some(uuid)),
                uuid,
            )),
        }
        if lines.len() >= MAX_LINES {
            lines.truncate(MAX_LINES);
            break;
        }
    }
    let title = game.fill_vars(&text::fill(&title, &pairs), Some(uuid));
    (title, lines)
}

fn values(game: &Game, cfg: &Config, now: u64, uuid: &str) -> Vec<(&'static str, String)> {
    let p = game.players.get(uuid);
    let seconds = match game.phase {
        GamePhase::Countdown { ends_at } => ends_at.saturating_sub(now).div_ceil(TICKS) as u32,
        _ => 0,
    };
    let status = match game.phase {
        GamePhase::Countdown { .. } => text::fill(
            &cfg.messages.countdown_status,
            &[("seconds", &seconds.to_string())],
        ),
        _ => cfg.messages.waiting_status.clone(),
    };
    let elapsed = game.elapsed(now);
    let time_left = if game.rules.time_limit > 0 {
        text::clock(game.rules.time_limit.saturating_sub(elapsed))
    } else {
        "--:--".into()
    };
    let alive = game.players.values().filter(|p| p.in_round()).count();
    let teams_alive = game.teams.iter().filter(|t| !t.eliminated).count();
    let team = p
        .and_then(|p| p.team.as_deref())
        .and_then(|t| game.team(t))
        .map(|t| t.display())
        .unwrap_or_else(|| "-".into());
    vec![
        ("arena", game.arena.id.clone()),
        ("mode", game.mode.id.clone()),
        ("mode_name", game.mode.display_name.clone()),
        ("format", game.format.display_name.clone()),
        ("players", game.players.len().to_string()),
        ("max", game.capacity().to_string()),
        ("min", game.format.min_players.to_string()),
        ("status", status),
        ("seconds", seconds.to_string()),
        ("time", text::clock(elapsed)),
        ("time_left", time_left),
        ("alive", alive.to_string()),
        ("teams_alive", teams_alive.to_string()),
        ("kills", p.map(|p| p.kills).unwrap_or(0).to_string()),
        ("deaths", p.map(|p| p.deaths).unwrap_or(0).to_string()),
        ("team", team),
        (
            "kit",
            p.and_then(|p| p.kit.clone()).unwrap_or_else(|| "-".into()),
        ),
        ("round", game.round.to_string()),
    ]
}

fn fill_counters(game: &Game, s: &str, uuid: &str) -> String {
    let mut s = s.to_string();
    let mut guard = 0;
    while let Some(start) = s.find("{counter:") {
        guard += 1;
        if guard > 16 {
            break;
        }
        let Some(end) = s[start..].find('}') else {
            break;
        };
        let key = &s[start + 9..start + end];
        let v = game
            .players
            .get(uuid)
            .and_then(|p| p.counters.get(key))
            .copied()
            .unwrap_or(0)
            .to_string();
        s.replace_range(start..start + end + 1, &v);
    }
    s
}

fn team_lines(game: &Game, uuid: &str) -> Vec<String> {
    let mine = game.players.get(uuid).and_then(|p| p.team.clone());
    game.teams
        .iter()
        .map(|t| {
            let alive = game
                .players
                .values()
                .filter(|p| p.team.as_deref() == Some(&t.id) && p.in_round())
                .count();
            let state = if t.eliminated {
                "&c✘".to_string()
            } else if !t.respawn {
                format!("&e{alive}")
            } else {
                "&a✔".to_string()
            };
            let you = if mine.as_deref() == Some(&t.id) {
                " &7(you)"
            } else {
                ""
            };
            format!("{}■ &f{} {state}{you}", text::color_code(&t.color), t.name)
        })
        .collect()
}
