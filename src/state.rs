//! The plugin's whole in-memory state behind one lock, and the operations on it.
//!
//! Rules: no host calls while the lock is held (that includes logging). Operations
//! return the host work as `Vec<Effect>`; callers apply it after the lock is released.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};

use minigamecore_client::{ErrorKind, ErrorReply, Event, EventEnvelope};

use crate::config::Config;
use crate::game::{
    Ctx, Game, GamePhase,
    effects::{Effect, Out},
    matchmaking::{self, Choice},
};
use crate::model::{BlockPos, Cuboid, arena::ArenaDef, kit::KitDef, mode::ModeDef};
use crate::stats::Stats;

/// Most events kept per plugin between polls.
pub const QUEUE_CAP: usize = 4096;

#[derive(Debug, Default)]
pub struct Queue {
    pub events: VecDeque<EventEnvelope>,
    pub dropped: u64,
}

/// An arena being put back after a round.
#[derive(Debug)]
pub struct Restore {
    pub game: String,
    pub world: String,
    pub blocks: Vec<(BlockPos, u16)>,
    pub bounds: Cuboid,
    pub cleanup: Vec<String>,
    /// Entities were cleared.
    pub cleaned: bool,
}

pub struct State {
    pub cfg: Arc<Config>,
    pub arenas: BTreeMap<String, Arc<ArenaDef>>,
    pub modes: BTreeMap<String, Arc<ModeDef>>,
    pub kits: BTreeMap<String, Arc<KitDef>>,
    pub games: BTreeMap<String, Game>,
    /// uuid → game id.
    pub index: HashMap<String, String>,
    /// Ticks since load.
    pub now: u64,
    rounds: u64,
    /// mode → owning plugin id.
    pub owners: BTreeMap<String, String>,
    /// plugin id → queued events.
    pub queues: BTreeMap<String, Queue>,
    seq: u64,
    /// Effects requested from IPC handlers, applied by the next tick.
    pub deferred: Vec<Effect>,
    pub restores: VecDeque<Restore>,
    pub stats: Stats,
    pub stats_dirty: bool,
    /// lowercase name → uuid, and uuid → name, for online players.
    pub names: HashMap<String, String>,
    pub uuid_names: HashMap<String, String>,
    /// entity id → player uuid.
    pub entities: HashMap<i32, String>,
    /// projectile entity id → (shooter entity id, tick).
    pub projectiles: HashMap<i32, (i32, u64)>,
    /// Data folder works.
    pub persist: bool,
    /// Placeholder registration must be (re)sent.
    pub pie_dirty: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            cfg: Arc::new(Config::default_config()),
            arenas: BTreeMap::new(),
            modes: BTreeMap::new(),
            kits: BTreeMap::new(),
            games: BTreeMap::new(),
            index: HashMap::new(),
            now: 0,
            rounds: 0,
            owners: BTreeMap::new(),
            queues: BTreeMap::new(),
            seq: 0,
            deferred: Vec::new(),
            restores: VecDeque::new(),
            stats: Stats::default(),
            stats_dirty: false,
            names: HashMap::new(),
            uuid_names: HashMap::new(),
            entities: HashMap::new(),
            projectiles: HashMap::new(),
            persist: false,
            pie_dirty: true,
        }
    }

    fn ctx(&self) -> (u64, Arc<Config>) {
        (self.now, Arc::clone(&self.cfg))
    }

    // ── players ─────────────────────────────────────────────────────────────────

    pub fn player_online(&mut self, uuid: &str, name: &str, entity_id: i32) {
        if let Some(old) = self.uuid_names.insert(uuid.into(), name.into())
            && !old.eq_ignore_ascii_case(name)
        {
            self.names.remove(&old.to_ascii_lowercase());
        }
        self.names.insert(name.to_ascii_lowercase(), uuid.into());
        self.entities.retain(|_, u| u != uuid);
        self.entities.insert(entity_id, uuid.into());
    }

    pub fn player_offline(&mut self, uuid: &str) {
        if let Some(name) = self.uuid_names.remove(uuid) {
            self.names.remove(&name.to_ascii_lowercase());
        }
        self.entities.retain(|_, u| u != uuid);
    }

    /// A name or uuid → (uuid, name) for an online player.
    pub fn resolve(&self, who: &str) -> Option<(String, String)> {
        if let Some(u) = self.names.get(&who.to_ascii_lowercase()) {
            return Some((u.clone(), self.uuid_names.get(u).cloned().unwrap_or_default()));
        }
        let lower = who.to_ascii_lowercase();
        self.uuid_names.get(&lower).map(|n| (lower, n.clone()))
    }

    pub fn game_of(&self, uuid: &str) -> Option<&Game> {
        self.index.get(uuid).and_then(|g| self.games.get(g))
    }

    pub fn game_of_mut(&mut self, uuid: &str) -> Option<&mut Game> {
        let id = self.index.get(uuid)?.clone();
        self.games.get_mut(&id)
    }

    // ── events, results, resets ─────────────────────────────────────────────────

    pub fn push_event(&mut self, mode: &str, event: Event) {
        let Some(plugin) = self.owners.get(mode).cloned() else {
            return;
        };
        self.seq += 1;
        let q = self.queues.entry(plugin).or_default();
        if q.events.len() >= QUEUE_CAP {
            q.events.pop_front();
            q.dropped += 1;
        }
        q.events.push_back(EventEnvelope { seq: self.seq, event });
    }

    /// Routes an `Out` from a game step: events to queues, results to stats, index
    /// updates, resets. Returns the host effects.
    pub fn finish(&mut self, mode: &str, out: Out) -> Vec<Effect> {
        let Out {
            effects,
            events,
            results,
            resets,
            removed,
        } = out;
        for e in events {
            self.push_event(mode, e);
        }
        for r in &results {
            self.stats.apply(r);
            self.stats_dirty = true;
        }
        for u in removed {
            self.index.remove(&u);
        }
        for g in resets {
            self.start_restore(&g);
        }
        effects
    }

    fn start_restore(&mut self, game: &str) {
        let Some(g) = self.games.get_mut(game) else { return };
        let changes = std::mem::take(&mut g.changes);
        let mut cleanup = self.cfg.reset.cleanup_entities.clone();
        cleanup.extend(g.mode.cleanup_entities.iter().cloned());
        self.restores.push_back(Restore {
            game: game.into(),
            world: g.arena.world.clone(),
            blocks: changes.into_restore_list(),
            bounds: g.arena.bounds(),
            cleanup,
            cleaned: false,
        });
    }

    /// Called when a restore job finished: frees the arena.
    pub fn restore_done(&mut self, game: &str) {
        if let Some(g) = self.games.remove(game) {
            for u in g.players.keys() {
                self.index.remove(u);
            }
            let mode = g.mode.id.clone();
            self.push_event(&mode, Event::GameReset { game: game.into() });
        }
    }

    /// Drops games that emptied out before they started.
    fn drop_empty_waiting(&mut self) {
        let empty: Vec<String> = self
            .games
            .values()
            .filter(|g| g.phase.joinable() && g.is_empty() && g.changes.is_empty())
            .map(|g| g.id.clone())
            .collect();
        for id in empty {
            if let Some(g) = self.games.remove(&id) {
                let mode = g.mode.id.clone();
                self.push_event(&mode, Event::GameReset { game: id });
            }
        }
    }

    // ── matchmaking ─────────────────────────────────────────────────────────────

    /// Puts a party into a game. Returns the game id and effects.
    pub fn join(
        &mut self,
        party: Vec<(String, String)>,
        mode: &str,
        format: Option<&str>,
        arena: Option<&str>,
    ) -> Result<(String, Vec<Effect>), ErrorReply> {
        let err = |kind, message: String| ErrorReply { kind, message };
        if party.is_empty() {
            return Err(err(ErrorKind::BadRequest, "no players".into()));
        }
        let Some(mode_def) = self.modes.get(mode).cloned() else {
            return Err(err(ErrorKind::UnknownMode, format!("unknown mode '{mode}'")));
        };
        if let Some(f) = format
            && !mode_def.formats.contains_key(f)
        {
            return Err(err(ErrorKind::BadRequest, format!("mode '{mode}' has no format '{f}'")));
        }
        for (u, name) in &party {
            if self.index.contains_key(u) {
                return Err(err(ErrorKind::AlreadyInGame, format!("{name} is already in a game")));
            }
        }
        let choice = matchmaking::choose(&self.games, &self.arenas, &mode_def, format, arena, party.len())
            .ok_or_else(|| err(ErrorKind::NoArena, format!("no arena is free for {mode}")))?;
        let (now, cfg) = self.ctx();
        let ctx = Ctx { now, cfg: &cfg };
        let mut out = Out::default();
        let id = match choice {
            Choice::Existing(id) => id,
            Choice::New { arena, format } => {
                let def = self.arenas.get(&arena).cloned().ok_or_else(|| err(ErrorKind::UnknownArena, arena.clone()))?;
                self.rounds += 1;
                let game = Game::new(def, Arc::clone(&mode_def), &format, self.rounds)
                    .map_err(|m| err(ErrorKind::NoArena, m))?;
                out.event(Event::GameCreated {
                    game: arena.clone(),
                    mode: mode.into(),
                    format: format.clone(),
                });
                self.games.insert(arena.clone(), game);
                arena
            }
        };
        let game = self.games.get_mut(&id).ok_or_else(|| err(ErrorKind::UnknownGame, id.clone()))?;
        if let Err(e) = game.add_players(&ctx, &party, &mut out) {
            let effects = self.finish(mode, out);
            self.drop_empty_waiting();
            let _ = effects;
            return Err(err(ErrorKind::WrongState, format!("{e:?}")));
        }
        for (u, _) in &party {
            self.index.insert(u.clone(), id.clone());
        }
        let effects = self.finish(mode, out);
        Ok((id, effects))
    }

    /// Takes a player out of their game. `quit` = disconnected.
    pub fn leave(&mut self, uuid: &str, quit: bool) -> Vec<Effect> {
        let (now, cfg) = self.ctx();
        let ctx = Ctx { now, cfg: &cfg };
        let Some(game) = self.game_of_mut(uuid) else {
            return Vec::new();
        };
        let mode = game.mode.id.clone();
        let mut out = Out::default();
        game.remove_player(&ctx, uuid, quit, &mut out);
        self.index.remove(uuid);
        let effects = self.finish(&mode, out);
        self.drop_empty_waiting();
        effects
    }

    /// Runs `f` on a game and routes its output.
    pub fn step<R>(&mut self, game: &str, f: impl FnOnce(&mut Game, &Ctx, &mut Out) -> R) -> Option<(R, Vec<Effect>)> {
        let (now, cfg) = self.ctx();
        let ctx = Ctx { now, cfg: &cfg };
        let g = self.games.get_mut(game)?;
        let mode = g.mode.id.clone();
        let mut out = Out::default();
        let r = f(g, &ctx, &mut out);
        let effects = self.finish(&mode, out);
        Some((r, effects))
    }

    /// One server tick.
    pub fn tick(&mut self) -> Vec<Effect> {
        self.now += 1;
        let now = self.now;
        let stockpile = self.cfg.spawners.mode == crate::config::SpawnerMode::Stockpile;
        let ids: Vec<String> = self.games.keys().cloned().collect();
        let mut effects = std::mem::take(&mut self.deferred);
        let second = now.is_multiple_of(crate::game::TICKS);
        for id in ids {
            if let Some((_, e)) = self.step(&id, |g, ctx, out| {
                g.spawner_tick(now, stockpile, out);
                if second {
                    g.second(ctx, out);
                }
            }) {
                effects.extend(e);
            }
        }
        if now.is_multiple_of(200) {
            self.projectiles.retain(|_, (_, at)| now.saturating_sub(*at) < 1200);
        }
        effects
    }

    /// A player died for real or fell into the void. Returns effects if the core
    /// handled the death.
    pub fn death(&mut self, uuid: &str, cause: &str) -> Option<Vec<Effect>> {
        let id = self.index.get(uuid)?.clone();
        let (handled, effects) = self.step(&id, |g, ctx, out| g.on_death(ctx, uuid, cause, out))?;
        handled.then_some(effects)
    }

    // ── block log ───────────────────────────────────────────────────────────────

    /// The game whose arena contains `pos` (in `world`, if known). Only games past
    /// waiting (their arena is in use). `None` if ambiguous.
    pub fn game_at(&self, world: Option<&str>, pos: BlockPos) -> Option<String> {
        let mut hits = self.games.values().filter(|g| {
            !g.phase.joinable()
                && !matches!(g.phase, GamePhase::Resetting)
                && world.is_none_or(|w| w == g.arena.world)
                && g.arena.bounds().contains_block(pos)
        });
        let first = hits.next()?;
        if hits.next().is_some() {
            return None;
        }
        Some(first.id.clone())
    }

    /// An arena (with no running round) containing `pos`.
    pub fn idle_arena_at(&self, world: Option<&str>, pos: BlockPos) -> Option<String> {
        self.arenas
            .values()
            .filter(|a| a.enabled && world.is_none_or(|w| w == a.world))
            .filter(|a| self.games.get(&a.id).is_none_or(|g| g.phase.joinable()))
            .find(|a| a.bounds().contains_block(pos))
            .map(|a| a.id.clone())
    }

    /// Records an original block state. Returns true if new (for the crash log).
    pub fn record_block(&mut self, game: &str, pos: BlockPos, state: u16) -> bool {
        self.games
            .get_mut(game)
            .is_some_and(|g| g.changes.record(pos, state))
    }

    // ── reloading ───────────────────────────────────────────────────────────────

    /// Replaces definitions. Running games keep the definitions they started with.
    pub fn replace_defs(
        &mut self,
        cfg: Config,
        arenas: BTreeMap<String, ArenaDef>,
        modes: BTreeMap<String, ModeDef>,
        kits: BTreeMap<String, KitDef>,
    ) {
        if cfg.placeholders.namespace != self.cfg.placeholders.namespace
            || cfg.placeholders.enabled != self.cfg.placeholders.enabled
        {
            self.pie_dirty = true;
        }
        self.cfg = Arc::new(cfg);
        self.arenas = arenas.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        self.modes = modes.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        self.kits = kits.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
    }

    /// Takes the next event batch for a plugin.
    pub fn poll(&mut self, plugin: &str, max: usize) -> (Vec<EventEnvelope>, u64) {
        let Some(q) = self.queues.get_mut(plugin) else {
            return (Vec::new(), 0);
        };
        let n = max.min(q.events.len());
        let events = q.events.drain(..n).collect();
        let dropped = std::mem::take(&mut q.dropped);
        (events, dropped)
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));

/// Runs `f` with the state locked. Never call the host inside `f`.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

#[cfg(test)]
mod tests;
