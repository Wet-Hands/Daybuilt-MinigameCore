//! Files in the data folder: config, arenas, modes, kits, stats.
//!
//! ```text
//! plugins/data/daybuilt_minigame_core/
//!   config.toml
//!   arenas/<id>.toml     written by /dbmg arena commands
//!   modes/<mode>.toml
//!   kits/<kit>.toml
//!   stats.json
//!   resets/<arena>.jsonl crash log of changed blocks (deleted after each reset)
//! ```

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::config::Config;
use crate::model::{arena::ArenaDef, kit::KitDef, mode::ModeDef};
use crate::stats::Stats;

static DATA_DIR: OnceLock<String> = OnceLock::new();

pub fn set_data_dir(dir: String) {
    let _ = DATA_DIR.set(dir);
}

pub fn data_dir() -> String {
    DATA_DIR.get().cloned().unwrap_or_else(|| "data".into())
}

fn path(rel: &str) -> String {
    format!("{}/{rel}", data_dir())
}

/// Writes and deletes a probe file. True if the data folder is usable.
pub fn probe() -> bool {
    let p = path(".probe");
    let ok = std::fs::write(&p, b"ok").is_ok();
    let _ = std::fs::remove_file(&p);
    ok
}

/// Writes atomically: temp file then rename (falls back to a direct write).
pub fn write(rel: &str, contents: &str) -> Result<(), String> {
    let p = path(rel);
    if let Some(parent) = std::path::Path::new(&p).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = format!("{p}.tmp");
    match std::fs::write(&tmp, contents).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(&p, contents).map_err(|e| format!("{p}: {e}"))
        }
    }
}

pub fn read(rel: &str) -> Option<String> {
    std::fs::read_to_string(path(rel)).ok()
}

/// Bundled files written on first start so owners have something to edit.
const DEFAULTS: &[(&str, &str)] = &[
    ("config.toml", crate::config::DEFAULT_TOML),
    (
        "modes/spleef.toml",
        include_str!("../../config/modes/spleef.toml"),
    ),
    (
        "kits/starter.toml",
        include_str!("../../config/kits/starter.toml"),
    ),
    (
        "kits/spleef.toml",
        include_str!("../../config/kits/spleef.toml"),
    ),
];

pub fn write_defaults() {
    for (rel, text) in DEFAULTS {
        if std::fs::metadata(path(rel)).is_err() {
            let _ = write(rel, text);
        }
    }
    let _ = std::fs::create_dir_all(path("arenas"));
}

/// TOML files in a folder: (id from file name, text).
fn folder(rel: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(path(rel)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".toml")?.to_string();
            let text = std::fs::read_to_string(e.path()).ok()?;
            Some((id, text))
        })
        .collect();
    out.sort();
    out
}

pub struct Loaded {
    pub config: Config,
    pub arenas: BTreeMap<String, ArenaDef>,
    pub modes: BTreeMap<String, ModeDef>,
    pub kits: BTreeMap<String, KitDef>,
    /// Problems to log (a broken file is skipped, the rest still load).
    pub errors: Vec<String>,
}

/// Reads everything. Without a data folder: defaults and the bundled example mode/kits.
pub fn load_all(persist: bool) -> Loaded {
    let mut errors = Vec::new();
    let mut config =
        match Config::parse(if persist { read("config.toml") } else { None }.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("config.toml: {e} (using defaults)"));
                Config::default_config()
            }
        };
    if persist && let Some(spawn) = load_lobby_spawn() {
        config.lobby.spawn = Some(spawn);
    }
    let (mode_files, kit_files, arena_files) = if persist {
        (folder("modes"), folder("kits"), folder("arenas"))
    } else {
        let bundled = |rel: &str| {
            DEFAULTS
                .iter()
                .filter_map(|(p, t)| {
                    p.strip_prefix(rel)
                        .and_then(|f| f.strip_suffix(".toml"))
                        .map(|id| (id.to_string(), (*t).to_string()))
                })
                .collect::<Vec<_>>()
        };
        (bundled("modes/"), bundled("kits/"), Vec::new())
    };
    let mut modes = BTreeMap::new();
    for (id, text) in mode_files {
        match ModeDef::parse(&id, &text) {
            Ok(m) => {
                modes.insert(id, m);
            }
            Err(e) => errors.push(format!("modes/{id}.toml: {e}")),
        }
    }
    let mut kits = BTreeMap::new();
    for (id, text) in kit_files {
        match KitDef::parse(&id, &text) {
            Ok(k) => {
                kits.insert(id, k);
            }
            Err(e) => errors.push(format!("kits/{id}.toml: {e}")),
        }
    }
    let mut arenas = BTreeMap::new();
    for (id, text) in arena_files {
        match ArenaDef::parse(&id, &text) {
            Ok(a) => {
                arenas.insert(id, a);
            }
            Err(e) => errors.push(format!("arenas/{id}.toml: {e}")),
        }
    }
    // Cross-checks: kits named in rules exist.
    for m in modes.values() {
        for f in m.formats.values() {
            if let Some(k) = &f.rules.kit
                && !kits.contains_key(k)
            {
                errors.push(format!(
                    "modes/{}.toml: format {} uses missing kit '{k}'",
                    m.id, f.name
                ));
            }
        }
    }
    Loaded {
        config,
        arenas,
        modes,
        kits,
        errors,
    }
}

pub fn save_arena(a: &ArenaDef) -> Result<(), String> {
    write(&format!("arenas/{}.toml", a.id), &a.to_toml()?)
}

pub fn delete_arena(id: &str) -> Result<(), String> {
    std::fs::remove_file(path(&format!("arenas/{id}.toml"))).map_err(|e| e.to_string())
}

pub fn save_kit(k: &KitDef) -> Result<(), String> {
    write(&format!("kits/{}.toml", k.id), &k.to_toml()?)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LobbyFile {
    spawn: crate::model::WorldLoc,
}

/// Saves the lobby spawn to `lobby.toml` (it overrides `[lobby] spawn` in config.toml,
/// so the owner's commented config file is never rewritten).
pub fn save_lobby_spawn(spawn: &crate::model::WorldLoc) -> Result<(), String> {
    let text = toml::to_string(&LobbyFile {
        spawn: spawn.clone(),
    })
    .map_err(|e| e.to_string())?;
    write(
        "lobby.toml",
        &format!("# Written by /dbmg setlobby.\n{text}"),
    )
}

fn load_lobby_spawn() -> Option<crate::model::WorldLoc> {
    let text = read("lobby.toml")?;
    toml::from_str::<LobbyFile>(&text).ok().map(|l| l.spawn)
}

pub fn load_stats() -> Stats {
    read("stats.json")
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_stats(stats: &Stats) -> Result<(), String> {
    let text = serde_json::to_string(stats).map_err(|e| e.to_string())?;
    write("stats.json", &text)
}
