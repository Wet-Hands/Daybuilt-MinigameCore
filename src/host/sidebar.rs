//! Per-player sidebar through the player's own (Java) scoreboard. Each line is a score
//! holder name with `§` codes; numbers are hidden with the blank number format.

use std::cell::RefCell;
use std::collections::HashMap;

use pumpkin_plugin_api::{
    Server,
    scoreboard::{DisplaySlot, NumberFormat, RenderType},
};

use crate::model::text;
use crate::state;

const OBJECTIVE: &str = "mgsidebar";

/// What each player currently sees: (title, unique line names).
type Shown = (String, Vec<String>);

thread_local! {
    // Plugins run single-threaded in WASM; this is only touched from host code.
    static SHOWN: RefCell<HashMap<String, Shown>> = RefCell::new(HashMap::new());
}

/// Makes `lines` unique by appending an invisible colour code per index.
fn unique(lines: &[String]) -> Vec<String> {
    const CODES: &[u8] = b"0123456789abcdef";
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let a = CODES[i % 16] as char;
            format!("{}§{a}§r", text::legacy(l))
        })
        .collect()
}

/// Recomputes and sends every in-game player's sidebar. Called once a second.
pub fn refresh(server: &Server) {
    let (enabled, views) = state::with(|s| {
        let cfg = s.cfg.clone();
        let now = s.now;
        let views: Vec<(String, String, Vec<String>)> = s
            .games
            .values()
            .flat_map(|g| {
                let cfg = cfg.clone();
                g.order.iter().map(move |u| {
                    let (t, l) = crate::game::sidebar::render(g, &cfg, now, u);
                    (u.clone(), t, l)
                })
            })
            .collect();
        (cfg.sidebar.enabled, views)
    });
    if !enabled {
        return;
    }
    for (uuid, title, lines) in views {
        if let Some(p) = super::player(server, &uuid) {
            show(&p, &uuid, &title, &lines);
        }
    }
}

fn show(p: &pumpkin_plugin_api::player::Player, uuid: &str, title: &str, lines: &[String]) {
    let Some(java) = p.as_java() else {
        return; // Bedrock sidebars are not supported yet.
    };
    let names = unique(lines);
    let prev = SHOWN.with(|m| m.borrow().get(uuid).cloned());
    if prev
        .as_ref()
        .is_some_and(|(t, l)| t == title && l == &names)
    {
        return;
    }
    let sb = java.get_scoreboard();
    match &prev {
        None => {
            sb.add_objective(
                OBJECTIVE,
                super::colored(title),
                RenderType::Integer,
                Some(NumberFormat::Blank),
            );
            sb.set_display_slot(DisplaySlot::Sidebar, OBJECTIVE);
        }
        Some((t, _)) if t != title => {
            sb.update_objective(
                OBJECTIVE,
                super::colored(title),
                RenderType::Integer,
                Some(NumberFormat::Blank),
            );
        }
        _ => {}
    }
    let old = prev.map(|(_, l)| l).unwrap_or_default();
    for o in &old {
        if !names.contains(o) {
            sb.remove_score(o, OBJECTIVE);
        }
    }
    let n = names.len() as i32;
    for (i, name) in names.iter().enumerate() {
        let score = n - i as i32;
        let unchanged = old
            .iter()
            .position(|o| o == name)
            .is_some_and(|j| j == i && old.len() == names.len());
        if !unchanged {
            sb.update_score(name, OBJECTIVE, score, Some(NumberFormat::Blank));
        }
    }
    SHOWN.with(|m| {
        m.borrow_mut()
            .insert(uuid.to_string(), (title.to_string(), names))
    });
}

/// Removes the sidebar (player left a game).
pub fn clear(server: &Server, uuid: &str) {
    let had = SHOWN.with(|m| m.borrow_mut().remove(uuid)).is_some();
    if had
        && let Some(p) = super::player(server, uuid)
        && let Some(java) = p.as_java()
    {
        java.get_scoreboard().remove_objective(OBJECTIVE);
    }
}

/// Forgets a player who went offline.
pub fn forget(uuid: &str) {
    SHOWN.with(|m| m.borrow_mut().remove(uuid));
}
