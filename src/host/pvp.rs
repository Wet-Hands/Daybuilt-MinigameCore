//! Daybuilt PvP Manager integration (optional, over its IPC text API).
//!
//! The core only cancels hits it must (friendly fire, spectators, lobby, `pvp = false`).
//! PvP Manager keeps enforcing its own rules, so a player who switched PvP off could not be
//! hit in a round. On round start the core switches such players' PvP on (and classic
//! combat if the mode sets it) and puts their own settings back when they leave.
//!
//! PvP Manager never calls this plugin, so calling it from effects is safe.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use pumpkin_plugin_api::ipc::send_ipc_message;

use crate::state;

const PLUGIN: &str = "daybuilt_pvp_manager";

#[derive(Default, Clone)]
struct Saved {
    pvp: Option<bool>,
    classic: Option<bool>,
}

/// uuid → settings to put back.
static SAVED: LazyLock<Mutex<HashMap<String, Saved>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn call(request: &str) -> Option<String> {
    match send_ipc_message(PLUGIN, request.as_bytes()) {
        Ok(Ok(reply)) => String::from_utf8(reply).ok(),
        Ok(Err(e)) => {
            tracing::debug!("{}: {PLUGIN} refused '{request}': {e}", crate::ID);
            None
        }
        Err(()) => None, // not installed
    }
}

fn enabled() -> (bool, bool) {
    state::with(|s| (s.cfg.pvp_manager.enabled, s.cfg.pvp_manager.force_pvp))
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub fn enter(uuid: &str, classic: Option<bool>) {
    let (on, force) = enabled();
    if !on {
        return;
    }
    let mut saved = Saved::default();
    if force
        && call(&format!("pvp.get {uuid}"))
            .as_deref()
            .and_then(parse_bool)
            == Some(false)
        && call(&format!("pvp.set {uuid} true")).is_some()
    {
        saved.pvp = Some(false);
    }
    if let Some(want) = classic
        && let Some(cur) = call(&format!("classic.get {uuid}"))
            .as_deref()
            .and_then(parse_bool)
        && cur != want
        && call(&format!("classic.set {uuid} {want}")).is_some()
    {
        saved.classic = Some(cur);
    }
    if saved.pvp.is_some() || saved.classic.is_some() {
        SAVED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(uuid.into(), saved);
    }
}

pub fn leave(uuid: &str) {
    let saved = SAVED.lock().unwrap_or_else(|e| e.into_inner()).remove(uuid);
    let Some(saved) = saved else { return };
    if let Some(v) = saved.pvp {
        call(&format!("pvp.set {uuid} {v}"));
    }
    if let Some(v) = saved.classic {
        call(&format!("classic.set {uuid} {v}"));
    }
}

pub fn untag(uuid: &str) {
    call(&format!("combat.untag {uuid}"));
}

/// Puts everyone's settings back (plugin unload).
pub fn restore_all() {
    let all: Vec<String> = SAVED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    for u in all {
        leave(&u);
    }
}
