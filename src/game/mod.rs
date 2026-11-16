//! One game: a round of a mode running in an arena. Pure state machine; host work is
//! returned as [`Effect`]s in an [`Out`].
//!
//! Phases: Waiting → Countdown → Running → Ending → Resetting (then the state layer
//! restores the arena and drops the game).

pub mod changelog;
pub mod effects;
pub mod matchmaking;
pub mod sidebar;

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use minigamecore_client::{Event, GameInfo, Phase, PlayerInfo, PlayerRef, PlayerStatus, TeamInfo};

use crate::config::Config;
use crate::model::{
    BlockPos, Location, WorldLoc,
    arena::ArenaDef,
    mode::{FormatDef, ModeDef, TeamMode},
    rules::{BreakRule, DeathDrops, GameModeName, OutOfBounds, Rules, TimeUp, WinRule},
    text,
};
use changelog::ChangeLog;
use effects::{Effect, Out, PlayerResult, RoundResult, TeamSpec};

pub const TICKS: u64 = 20;

/// Everything a game step needs from outside.
pub struct Ctx<'a> {
    pub now: u64,
    pub cfg: &'a Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePhase {
    Waiting,
    Countdown { ends_at: u64 },
    Running { started_at: u64 },
    Ending { ends_at: u64, elapsed: u32 },
    Resetting,
}

impl GamePhase {
    pub fn public(&self) -> Phase {
        match self {
            GamePhase::Waiting => Phase::Waiting,
            GamePhase::Countdown { .. } => Phase::Countdown,
            GamePhase::Running { .. } => Phase::Running,
            GamePhase::Ending { .. } => Phase::Ending,
            GamePhase::Resetting => Phase::Resetting,
        }
    }

    pub fn joinable(&self) -> bool {
        matches!(self, GamePhase::Waiting | GamePhase::Countdown { .. })
    }
}

#[derive(Debug, Clone)]
pub struct GPlayer {
    pub uuid: String,
    pub name: String,
    pub team: Option<String>,
    pub status: PlayerStatus,
    pub kills: u32,
    pub deaths: u32,
    pub counters: BTreeMap<String, i64>,
    pub kit: Option<String>,
    pub last_attacker: Option<(String, u64)>,
    pub regions: Vec<String>,
    pub respawn_allowed: bool,
    pub respawn_at: u64,
    pub spawn: Option<Location>,
    pub vars: BTreeMap<String, String>,
    /// Set when the player left mid-round (always a loss).
    pub left: bool,
}

impl GPlayer {
    fn new(uuid: &str, name: &str) -> Self {
        Self {
            uuid: uuid.into(),
            name: name.into(),
            team: None,
            status: PlayerStatus::Waiting,
            kills: 0,
            deaths: 0,
            counters: BTreeMap::new(),
            kit: None,
            last_attacker: None,
            regions: Vec::new(),
            respawn_allowed: true,
            respawn_at: 0,
            spawn: None,
            vars: BTreeMap::new(),
            left: false,
        }
    }

    pub fn in_round(&self) -> bool {
        matches!(self.status, PlayerStatus::Alive | PlayerStatus::Respawning)
    }

    pub fn reference(&self) -> PlayerRef {
        PlayerRef {
            uuid: self.uuid.clone(),
            name: self.name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GTeam {
    pub id: String,
    pub name: String,
    pub color: String,
    /// Scoreboard team name.
    pub key: String,
    pub respawn: bool,
    pub eliminated: bool,
    pub spawns: Vec<Location>,
    cursor: usize,
}

impl GTeam {
    fn next_spawn(&mut self) -> Option<Location> {
        if self.spawns.is_empty() {
            return None;
        }
        let s = self.spawns[self.cursor % self.spawns.len()];
        self.cursor += 1;
        Some(s)
    }

    pub fn display(&self) -> String {
        format!("{}{}", text::color_code(&self.color), self.name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SpawnerRt {
    pub next_at: u64,
    pub stock: u32,
}

#[derive(Debug, Clone)]
pub struct GroupRt {
    pub interval: Option<u32>,
    pub amount: Option<u32>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Sched {
    pub at: u32,
    pub id: String,
    pub label: String,
    pub actions: Vec<String>,
    pub fired: bool,
}

#[derive(Debug)]
pub struct Game {
    pub id: String,
    pub round: u64,
    pub arena: Arc<ArenaDef>,
    pub mode: Arc<ModeDef>,
    pub format: FormatDef,
    pub rules: Rules,
    pub phase: GamePhase,
    pub players: BTreeMap<String, GPlayer>,
    /// Join order (uuids), for stable team and spawn assignment.
    pub order: Vec<String>,
    pub teams: Vec<GTeam>,
    /// Players who are no longer in `players` but count for this round's results.
    pub departed: Vec<GPlayer>,
    pub schedule: Vec<Sched>,
    pub vars: BTreeMap<String, String>,
    pub spawners: Vec<SpawnerRt>,
    pub groups: BTreeMap<String, GroupRt>,
    pub placed: HashSet<BlockPos>,
    pub changes: ChangeLog,
    pub winners: Vec<String>,
    pub end_reason: String,
    started_teams: usize,
    time_up_sent: bool,
    unfrozen: bool,
    forced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    WrongState,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAction<'a> {
    Place,
    Break { block: &'a str },
}

fn short_hash(s: &str) -> u32 {
    // FNV-1a, folded to 16 bits: stable across runs, unlike the std hasher.
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    (h >> 16) ^ (h & 0xffff)
}

impl Game {
    pub fn new(
        arena: Arc<ArenaDef>,
        mode: Arc<ModeDef>,
        format_name: &str,
        round: u64,
    ) -> Result<Self, String> {
        let format = mode
            .formats
            .get(format_name)
            .cloned()
            .ok_or_else(|| format!("mode {} has no format {format_name}", mode.id))?;
        let mut game = Self {
            id: arena.id.clone(),
            round,
            rules: format.rules.clone(),
            format,
            phase: GamePhase::Waiting,
            players: BTreeMap::new(),
            order: Vec::new(),
            teams: Vec::new(),
            departed: Vec::new(),
            schedule: Vec::new(),
            vars: BTreeMap::new(),
            spawners: vec![SpawnerRt::default(); arena.spawners.len()],
            groups: BTreeMap::new(),
            placed: HashSet::new(),
            changes: ChangeLog::default(),
            winners: Vec::new(),
            end_reason: String::new(),
            started_teams: 0,
            time_up_sent: false,
            unfrozen: false,
            forced: false,
            arena,
            mode,
        };
        if game.format.team_mode == TeamMode::Teams {
            let prefix = game.team_prefix();
            let usable: Vec<_> = game.arena.usable_teams().cloned().collect();
            if usable.len() < game.format.teams as usize {
                return Err(format!(
                    "arena {} has {} teams with spawns; format {} needs {}",
                    game.arena.id,
                    usable.len(),
                    game.format.name,
                    game.format.teams
                ));
            }
            game.teams = usable
                .into_iter()
                .take(game.format.teams as usize)
                .enumerate()
                .map(|(i, t)| GTeam {
                    name: t.display_name(),
                    key: format!("{prefix}{i}"),
                    id: t.id,
                    color: t.color,
                    respawn: true,
                    eliminated: false,
                    spawns: t.spawns,
                    cursor: 0,
                })
                .collect();
        }
        Ok(game)
    }

    fn team_prefix(&self) -> String {
        format!("mg{:04x}_", short_hash(&self.arena.id))
    }

    pub fn capacity(&self) -> usize {
        let f = self.format.max_players() as usize;
        if self.arena.max_players > 0 {
            f.min(self.arena.max_players as usize)
        } else {
            f
        }
    }

    pub fn audience(&self) -> Vec<String> {
        self.order.clone()
    }

    pub fn team(&self, id: &str) -> Option<&GTeam> {
        self.teams.iter().find(|t| t.id == id)
    }

    fn team_mut(&mut self, id: &str) -> Option<&mut GTeam> {
        self.teams.iter_mut().find(|t| t.id == id)
    }

    pub fn members(&self, team: &str) -> Vec<String> {
        self.order
            .iter()
            .filter(|u| self.players.get(*u).and_then(|p| p.team.as_deref()) == Some(team))
            .cloned()
            .collect()
    }

    /// Coloured name of a player in this game.
    pub fn display(&self, uuid: &str) -> String {
        match self.players.get(uuid) {
            Some(p) => {
                let color = p
                    .team
                    .as_deref()
                    .and_then(|t| self.team(t))
                    .map(|t| text::color_code(&t.color))
                    .unwrap_or("&f");
                format!("{color}{}", p.name)
            }
            None => uuid.to_string(),
        }
    }

    pub fn elapsed(&self, now: u64) -> u32 {
        match self.phase {
            GamePhase::Running { started_at } => (now.saturating_sub(started_at) / TICKS) as u32,
            GamePhase::Ending { elapsed, .. } => elapsed,
            _ => 0,
        }
    }

    fn msg(&self, ctx: &Ctx, template: &str, extra: &[(&str, &str)]) -> String {
        if template.is_empty() {
            return String::new();
        }
        let players = self.players.len().to_string();
        let max = self.capacity().to_string();
        let mut values: Vec<(&str, &str)> = vec![
            ("arena", &self.arena.id),
            ("mode", &self.mode.id),
            ("mode_name", &self.mode.display_name),
            ("format", &self.format.display_name),
            ("players", &players),
            ("max", &max),
        ];
        values.extend_from_slice(extra);
        text::fill(&format!("{}{}", ctx.cfg.messages.prefix, template), &values)
    }

    fn plain(&self, template: &str, extra: &[(&str, &str)]) -> String {
        let mut values: Vec<(&str, &str)> = vec![("mode_name", &self.mode.display_name)];
        values.extend_from_slice(extra);
        text::fill(template, &values)
    }

    fn arena_loc(&self, loc: Location) -> WorldLoc {
        WorldLoc::new(&self.arena.world, loc)
    }

    fn spectator_spot(&self, uuid: &str) -> Option<Location> {
        self.arena
            .spectator
            .or_else(|| self.players.get(uuid).and_then(|p| p.spawn))
            .or_else(|| self.teams.iter().find_map(|t| t.spawns.first().copied()))
    }

    fn kit_for(&self, p: &GPlayer) -> Option<String> {
        p.kit.clone().or_else(|| self.rules.kit.clone())
    }

    // ── joining and leaving ─────────────────────────────────────────────────────

    /// Adds a party. Members are kept on one team when one has room.
    pub fn add_players(
        &mut self,
        ctx: &Ctx,
        party: &[(String, String)],
        out: &mut Out,
    ) -> Result<(), JoinError> {
        if !self.phase.joinable() {
            return Err(JoinError::WrongState);
        }
        if self.players.len() + party.len() > self.capacity() {
            return Err(JoinError::Full);
        }
        let teams_for_party = if self.format.team_mode == TeamMode::Teams {
            Some(self.assign_teams(party.len()))
        } else {
            None
        };
        for (i, (uuid, name)) in party.iter().enumerate() {
            let mut p = GPlayer::new(uuid, name);
            p.team = teams_for_party.as_ref().and_then(|t| t.get(i).cloned());
            self.players.insert(uuid.clone(), p);
            self.order.push(uuid.clone());
            out.push(Effect::Reset {
                player: uuid.clone(),
                gamemode: GameModeName::Adventure,
                clear_inventory: true,
            });
            if let Some(w) = self.arena.waiting {
                out.push(Effect::Teleport {
                    player: uuid.clone(),
                    to: self.arena_loc(w),
                });
            }
            let shown = self.display(uuid);
            let line = self.msg(ctx, &ctx.cfg.messages.joined, &[("player", &shown)]);
            out.chat(self.audience(), line);
            out.event(Event::PlayerJoined {
                game: self.id.clone(),
                player: PlayerRef {
                    uuid: uuid.clone(),
                    name: name.clone(),
                },
                team: self.players.get(uuid).and_then(|p| p.team.clone()),
            });
        }
        self.check_countdown(ctx, out);
        Ok(())
    }

    /// Team ids for `n` new players: one team with room for all of them if possible,
    /// otherwise spread over the emptiest teams.
    fn assign_teams(&self, n: usize) -> Vec<String> {
        let size = self.format.team_size as usize;
        let mut counts: Vec<(String, usize)> = self
            .teams
            .iter()
            .map(|t| {
                let c = self
                    .players
                    .values()
                    .filter(|p| p.team.as_deref() == Some(&t.id))
                    .count();
                (t.id.clone(), c)
            })
            .collect();
        // Fewest players first; ties keep arena order.
        let mut order: Vec<usize> = (0..counts.len()).collect();
        order.sort_by_key(|&i| counts[i].1);
        if let Some(&i) = order.iter().find(|&&i| size - counts[i].1.min(size) >= n) {
            return vec![counts[i].0.clone(); n];
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            if let Some(i) = (0..counts.len())
                .filter(|&i| counts[i].1 < size)
                .min_by_key(|&i| counts[i].1)
            {
                counts[i].1 += 1;
                out.push(counts[i].0.clone());
            }
        }
        out
    }

    /// `/mg team <id>` before the round.
    pub fn choose_team(&mut self, uuid: &str, team: &str) -> Result<(), &'static str> {
        if !self.format.team_choice {
            return Err("team choice is off for this game");
        }
        if !self.phase.joinable() {
            return Err("the round already started");
        }
        if self.team(team).is_none() {
            return Err("unknown team");
        }
        let count = self
            .players
            .values()
            .filter(|p| p.team.as_deref() == Some(team))
            .count();
        if count >= self.format.team_size as usize {
            return Err("full");
        }
        match self.players.get_mut(uuid) {
            Some(p) => {
                p.team = Some(team.into());
                Ok(())
            }
            None => Err("not in this game"),
        }
    }

    /// Removes a player. `quit` = they disconnected (no lobby teleport).
    pub fn remove_player(&mut self, ctx: &Ctx, uuid: &str, quit: bool, out: &mut Out) {
        let Some(mut p) = self.players.remove(uuid) else {
            return;
        };
        self.order.retain(|u| u != uuid);
        let during_round = matches!(self.phase, GamePhase::Running { .. }) && p.in_round();
        let was_in_round = !matches!(p.status, PlayerStatus::Waiting);
        if !quit {
            out.push(Effect::ToLobby {
                player: uuid.into(),
            });
        }
        out.push(Effect::SidebarClear {
            player: uuid.into(),
        });
        out.push(Effect::Freeze {
            player: uuid.into(),
            frozen: false,
        });
        if was_in_round {
            out.push(Effect::PvpLeave {
                player: uuid.into(),
            });
            if let Some(key) = p
                .team
                .as_deref()
                .and_then(|t| self.team(t))
                .map(|t| t.key.clone())
            {
                out.push(Effect::LeaveTeam {
                    world: self.arena.world.clone(),
                    key,
                    player_name: p.name.clone(),
                });
            }
        }
        let shown = match p.team.as_deref().and_then(|t| self.team(t)) {
            Some(t) => format!("{}{}", text::color_code(&t.color), p.name),
            None => format!("&f{}", p.name),
        };
        let line = self.msg(ctx, &ctx.cfg.messages.left, &[("player", &shown)]);
        out.chat(self.audience(), line);
        out.event(Event::PlayerLeft {
            game: self.id.clone(),
            player: p.reference(),
            during_round,
        });
        out.removed.push(uuid.into());
        if matches!(
            self.phase,
            GamePhase::Running { .. } | GamePhase::Ending { .. }
        ) && was_in_round
        {
            p.left = during_round;
            if during_round {
                p.status = PlayerStatus::Spectating;
            }
            self.departed.push(p);
        }
        match self.phase {
            GamePhase::Waiting | GamePhase::Countdown { .. } => self.check_countdown(ctx, out),
            GamePhase::Running { .. } => {
                self.update_teams(ctx, out);
                self.check_win(ctx, out);
            }
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    // ── countdown and start ─────────────────────────────────────────────────────

    fn countdown_secs(&self, ctx: &Ctx) -> u32 {
        self.mode
            .countdown_seconds
            .unwrap_or(ctx.cfg.timing.countdown_seconds)
    }

    fn check_countdown(&mut self, ctx: &Ctx, out: &mut Out) {
        let n = self.players.len();
        let min = self.format.min_players as usize;
        match self.phase {
            GamePhase::Waiting if n >= min && n > 0 => {
                let secs = self.countdown_secs(ctx);
                self.phase = GamePhase::Countdown {
                    ends_at: ctx.now + u64::from(secs) * TICKS,
                };
                out.event(Event::CountdownStarted {
                    game: self.id.clone(),
                    seconds: secs,
                });
                self.announce_countdown(ctx, secs, out);
                self.shorten_if_full(ctx);
            }
            GamePhase::Countdown { .. } if n < min && !self.forced => {
                self.phase = GamePhase::Waiting;
                let line = self.msg(ctx, &ctx.cfg.messages.countdown_cancelled, &[]);
                out.chat(self.audience(), line);
                out.event(Event::CountdownCancelled {
                    game: self.id.clone(),
                });
            }
            GamePhase::Countdown { .. } => self.shorten_if_full(ctx),
            _ => {}
        }
    }

    fn shorten_if_full(&mut self, ctx: &Ctx) {
        if let GamePhase::Countdown { ends_at } = self.phase
            && self.players.len() >= self.capacity()
        {
            let full = self
                .mode
                .full_countdown_seconds
                .unwrap_or(ctx.cfg.timing.full_countdown_seconds);
            let short = ctx.now + u64::from(full) * TICKS;
            if short < ends_at {
                self.phase = GamePhase::Countdown { ends_at: short };
            }
        }
    }

    fn announce_countdown(&self, ctx: &Ctx, secs: u32, out: &mut Out) {
        let s = secs.to_string();
        if matches!(secs, 60 | 30 | 20 | 10) || secs <= 5 {
            let line = self.msg(ctx, &ctx.cfg.messages.countdown, &[("seconds", &s)]);
            out.chat(self.audience(), line);
        }
        if secs <= 5 || secs == 10 {
            let title = self.plain(&ctx.cfg.messages.countdown_title, &[("seconds", &s)]);
            out.title(self.audience(), title, String::new(), 20);
            out.sound(self.audience(), "minecraft:block.note_block.hat", 1.0);
        }
    }

    /// `/dbmg start` or the `start` op: end the countdown now. Needs at least one player.
    pub fn force_start(&mut self, ctx: &Ctx, out: &mut Out) -> Result<(), &'static str> {
        if !self.phase.joinable() {
            return Err("the round already started");
        }
        if self.players.is_empty() {
            return Err("no players");
        }
        self.forced = true;
        self.start_round(ctx, out);
        Ok(())
    }

    fn start_round(&mut self, ctx: &Ctx, out: &mut Out) {
        // Solo: every player becomes a team, using the arena's shared spawns.
        if self.format.team_mode == TeamMode::Solo {
            let spawns = self.arena.solo_spawns();
            let prefix = self.team_prefix();
            self.teams = self
                .order
                .iter()
                .enumerate()
                .filter_map(|(i, u)| self.players.get(u).map(|p| (i, p)))
                .map(|(i, p)| GTeam {
                    id: p.name.to_ascii_lowercase(),
                    name: p.name.clone(),
                    color: "white".into(),
                    key: format!("{prefix}{i}"),
                    respawn: true,
                    eliminated: false,
                    spawns: if spawns.is_empty() {
                        Vec::new()
                    } else {
                        vec![spawns[i % spawns.len()]]
                    },
                    cursor: 0,
                })
                .collect();
            for u in self.order.clone() {
                if let Some(p) = self.players.get_mut(&u) {
                    p.team = Some(p.name.to_ascii_lowercase());
                }
            }
        } else {
            let used: HashSet<String> = self
                .players
                .values()
                .filter_map(|p| p.team.clone())
                .collect();
            self.teams.retain(|t| used.contains(&t.id));
        }
        self.started_teams = self.teams.len();
        out.push(Effect::CreateTeams {
            world: self.arena.world.clone(),
            teams: self
                .teams
                .iter()
                .map(|t| TeamSpec {
                    key: t.key.clone(),
                    display: t.name.clone(),
                    color: t.color.clone(),
                    friendly_fire: self.rules.friendly_fire,
                })
                .collect(),
        });
        let freeze = self.rules.start_freeze > 0;
        for u in self.order.clone() {
            let Some(team_id) = self.players.get(&u).and_then(|p| p.team.clone()) else {
                continue;
            };
            let (spawn, key) = match self.team_mut(&team_id) {
                Some(t) => (t.next_spawn(), t.key.clone()),
                None => (None, String::new()),
            };
            let kit = self.players.get(&u).and_then(|p| self.kit_for(p));
            let Some(p) = self.players.get_mut(&u) else {
                continue;
            };
            p.status = PlayerStatus::Alive;
            p.spawn = spawn;
            let name = p.name.clone();
            if let Some(s) = spawn {
                out.push(Effect::Teleport {
                    player: u.clone(),
                    to: WorldLoc::new(&self.arena.world, s),
                });
            }
            out.push(Effect::Reset {
                player: u.clone(),
                gamemode: self.rules.gamemode,
                clear_inventory: true,
            });
            if let Some(kit) = kit {
                out.push(Effect::Kit {
                    player: u.clone(),
                    kit,
                });
            }
            if freeze {
                out.push(Effect::Freeze {
                    player: u.clone(),
                    frozen: true,
                });
            }
            out.push(Effect::JoinTeam {
                world: self.arena.world.clone(),
                key,
                player_name: name,
            });
            out.push(Effect::PvpEnter {
                player: u.clone(),
                classic: self.rules.classic_combat,
            });
        }
        // Schedule: the mode's entries plus any the plugin added before the start.
        for s in &self.mode.schedule {
            if !self.schedule.iter().any(|e| e.id == s.id) {
                self.schedule.push(Sched {
                    at: s.at,
                    id: s.id.clone(),
                    label: s.label.clone(),
                    actions: s.actions.clone(),
                    fired: false,
                });
            }
        }
        for (i, s) in self.arena.spawners.iter().enumerate() {
            self.spawners[i] = SpawnerRt {
                next_at: ctx.now + u64::from(s.interval),
                stock: 0,
            };
        }
        self.phase = GamePhase::Running {
            started_at: ctx.now,
        };
        self.unfrozen = !freeze;
        let title = self.plain(&ctx.cfg.messages.round_start_title, &[]);
        let sub = self.plain(&ctx.cfg.messages.round_start_subtitle, &[]);
        out.title(self.audience(), title, sub, 30);
        out.sound(self.audience(), "minecraft:entity.player.levelup", 1.0);
        out.event(Event::RoundStarted {
            game: self.id.clone(),
            round: self.round,
            teams: self.team_infos(),
        });
    }

    // ── per-second step ─────────────────────────────────────────────────────────

    pub fn second(&mut self, ctx: &Ctx, out: &mut Out) {
        match self.phase {
            GamePhase::Countdown { ends_at } => {
                if ctx.now >= ends_at {
                    if self.players.len() >= self.format.min_players as usize || self.forced {
                        self.start_round(ctx, out);
                    } else {
                        self.phase = GamePhase::Waiting;
                    }
                } else {
                    let left = (ends_at - ctx.now).div_ceil(TICKS) as u32;
                    self.announce_countdown(ctx, left, out);
                }
            }
            GamePhase::Running { .. } => self.running_second(ctx, out),
            GamePhase::Ending { ends_at, .. } if ctx.now >= ends_at => self.begin_reset(out),
            _ => {}
        }
    }

    fn running_second(&mut self, ctx: &Ctx, out: &mut Out) {
        let elapsed = self.elapsed(ctx.now);
        if !self.unfrozen && elapsed >= self.rules.start_freeze {
            self.unfrozen = true;
            for p in self
                .players
                .values()
                .filter(|p| p.status == PlayerStatus::Alive)
            {
                out.push(Effect::Freeze {
                    player: p.uuid.clone(),
                    frozen: false,
                });
            }
        }
        // Respawns.
        let due: Vec<String> = self
            .players
            .values()
            .filter(|p| p.status == PlayerStatus::Respawning)
            .map(|p| p.uuid.clone())
            .collect();
        for u in due {
            let at = self.players.get(&u).map(|p| p.respawn_at).unwrap_or(0);
            if ctx.now >= at {
                self.respawn(&u, out);
            } else {
                let secs = (at - ctx.now).div_ceil(TICKS).to_string();
                let sub = self.plain(&ctx.cfg.messages.respawn_subtitle, &[("seconds", &secs)]);
                let title = self.plain(&ctx.cfg.messages.respawn_title, &[]);
                out.title(vec![u], title, sub, 25);
            }
        }
        // Schedule.
        let mut fire = Vec::new();
        for s in self.schedule.iter_mut() {
            if !s.fired && elapsed >= s.at {
                s.fired = true;
                fire.push((s.id.clone(), s.actions.clone()));
            }
        }
        for (id, actions) in fire {
            out.event(Event::Timer {
                game: self.id.clone(),
                id,
            });
            for a in actions {
                self.run_action(ctx, &a, out);
            }
            if !matches!(self.phase, GamePhase::Running { .. }) {
                return;
            }
        }
        // Time limit.
        let limit = self.rules.time_limit;
        if limit > 0 {
            let left = limit.saturating_sub(elapsed);
            if matches!(left, 60 | 30 | 10) {
                let s = left.to_string();
                let line = self.msg(ctx, &ctx.cfg.messages.time_left, &[("seconds", &s)]);
                out.chat(self.audience(), line);
            }
            if elapsed >= limit && !self.time_up_sent {
                match self.rules.time_up {
                    TimeUp::Draw => {
                        self.end_round(ctx, Vec::new(), "time_up", out);
                        return;
                    }
                    TimeUp::MostAlive => {
                        let best = self
                            .teams_alive()
                            .iter()
                            .map(|(_, n)| *n)
                            .max()
                            .unwrap_or(0);
                        let winners = if best == 0 {
                            Vec::new()
                        } else {
                            self.teams_alive()
                                .into_iter()
                                .filter(|(_, n)| *n == best)
                                .map(|(t, _)| t)
                                .collect()
                        };
                        self.end_round(ctx, winners, "time_up", out);
                        return;
                    }
                    TimeUp::Event => {
                        self.time_up_sent = true;
                        out.event(Event::TimeUp {
                            game: self.id.clone(),
                        });
                    }
                }
            }
        }
        self.check_win(ctx, out);
    }

    /// Built-in schedule actions (see `ScheduleDef::actions`).
    fn run_action(&mut self, ctx: &Ctx, action: &str, out: &mut Out) {
        let mut parts = action.splitn(3, ':');
        let kind = parts.next().unwrap_or("");
        let a = parts.next().unwrap_or("");
        let b = parts.next().unwrap_or("");
        match kind {
            "spawner_interval" => {
                if let Ok(n) = b.parse::<u32>()
                    && n > 0
                {
                    self.group_mut(a).interval = Some(n);
                }
            }
            "spawner_amount" => {
                if let Ok(n) = b.parse::<u32>() {
                    self.group_mut(a).amount = Some(n);
                }
            }
            "spawner_on" => self.group_mut(a).enabled = true,
            "spawner_off" => self.group_mut(a).enabled = false,
            "rule" => {
                if let Ok(v) = serde_json::from_str(b) {
                    let _ = self.rules.set(a, v);
                }
            }
            "no_respawn" => {
                for t in &mut self.teams {
                    t.respawn = false;
                }
            }
            "end_draw" => {
                self.end_round(ctx, Vec::new(), "schedule", out);
            }
            "broadcast" => {
                let rest = action.split_once(':').map_or("", |x| x.1);
                let line = self.msg(ctx, rest, &[]);
                out.chat(self.audience(), line);
            }
            "title" => {
                let rest = action.split_once(':').map_or("", |x| x.1).to_string();
                out.title(self.audience(), rest, String::new(), 40);
            }
            _ => {}
        }
    }

    pub fn group_mut(&mut self, group: &str) -> &mut GroupRt {
        self.groups.entry(group.to_string()).or_insert(GroupRt {
            interval: None,
            amount: None,
            enabled: true,
        })
    }

    // ── deaths, respawns, elimination ───────────────────────────────────────────

    /// Records a hit for kill credit.
    pub fn record_attack(&mut self, victim: &str, attacker: &str, now: u64) {
        if victim != attacker
            && let Some(p) = self.players.get_mut(victim)
        {
            p.last_attacker = Some((attacker.to_string(), now));
        }
    }

    /// Handles a death (real or from the void). Returns false if the player was not
    /// alive in a running round (the core then leaves the death alone).
    pub fn on_death(&mut self, ctx: &Ctx, uuid: &str, cause: &str, out: &mut Out) -> bool {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return false;
        }
        let credit = u64::from(self.rules.kill_credit_seconds) * TICKS;
        let Some(p) = self.players.get(uuid) else {
            return false;
        };
        if p.status != PlayerStatus::Alive {
            return false;
        }
        let killer = p
            .last_attacker
            .clone()
            .filter(|(k, at)| ctx.now.saturating_sub(*at) <= credit && k != uuid)
            .map(|(k, _)| k)
            .filter(|k| self.players.contains_key(k));
        let Some(p) = self.players.get_mut(uuid) else {
            return false;
        };
        p.deaths += 1;
        p.last_attacker = None;
        let team_id = p.team.clone();
        let player_ok = p.respawn_allowed;
        let team_ok = team_id
            .as_deref()
            .and_then(|t| self.team(t))
            .map(|t| t.respawn)
            .unwrap_or(false);
        let respawns = self.rules.respawn && team_ok && player_ok;

        let victim_ref = self.players.get(uuid).map(GPlayer::reference);
        let killer_ref = killer
            .as_deref()
            .and_then(|k| self.players.get(k))
            .map(GPlayer::reference);
        if let Some(k) = killer.as_deref() {
            if let Some(kp) = self.players.get_mut(k) {
                kp.kills += 1;
            }
            let victim_name = victim_ref
                .as_ref()
                .map(|r| r.name.clone())
                .unwrap_or_default();
            if let Some(kp) = self.players.get(k) {
                for cmd in &self.mode.rewards.kill {
                    out.push(Effect::Console {
                        command: self.reward_text(cmd, kp, &[("victim", &victim_name)]),
                    });
                }
            }
        }
        if self.rules.death_messages {
            let shown = self.display(uuid);
            let mut line = match killer.as_deref() {
                Some(k) => {
                    let ks = self.display(k);
                    self.msg(
                        ctx,
                        &ctx.cfg.messages.killed,
                        &[("player", &shown), ("killer", &ks)],
                    )
                }
                None => self.msg(ctx, &ctx.cfg.messages.death, &[("player", &shown)]),
            };
            if !respawns && !line.is_empty() {
                line.push_str(&ctx.cfg.messages.final_death);
            }
            out.chat(self.audience(), line);
        }
        if let Some(r) = victim_ref.clone() {
            out.event(Event::PlayerDied {
                game: self.id.clone(),
                player: r,
                killer: killer_ref,
                cause: cause.to_string(),
                respawns,
            });
        }
        if respawns {
            let at = ctx.now + u64::from(self.rules.respawn_seconds) * TICKS;
            if let Some(p) = self.players.get_mut(uuid) {
                p.status = PlayerStatus::Respawning;
                p.respawn_at = at;
            }
            out.push(Effect::Gamemode {
                player: uuid.into(),
                mode: GameModeName::Spectator,
            });
            if let Some(s) = self.spectator_spot(uuid) {
                out.push(Effect::Teleport {
                    player: uuid.into(),
                    to: self.arena_loc(s),
                });
            }
            let secs = self.rules.respawn_seconds.to_string();
            let sub = self.plain(&ctx.cfg.messages.respawn_subtitle, &[("seconds", &secs)]);
            let title = self.plain(&ctx.cfg.messages.respawn_title, &[]);
            out.title(vec![uuid.into()], title, sub, 25);
        } else {
            let announce = !self.rules.death_messages;
            self.eliminate_inner(ctx, uuid, announce, out);
        }
        self.update_teams(ctx, out);
        self.check_win(ctx, out);
        true
    }

    fn respawn(&mut self, uuid: &str, out: &mut Out) {
        let Some(team_id) = self.players.get(uuid).and_then(|p| p.team.clone()) else {
            return;
        };
        let spawn = self.team_mut(&team_id).and_then(GTeam::next_spawn);
        let keep = self.rules.death_drops == DeathDrops::Keep;
        let kit = self
            .players
            .get(uuid)
            .and_then(|p| self.kit_for(p))
            .filter(|_| self.rules.kit_on_respawn && !keep);
        let Some(p) = self.players.get_mut(uuid) else {
            return;
        };
        p.status = PlayerStatus::Alive;
        if spawn.is_some() {
            p.spawn = spawn;
        }
        let r = p.reference();
        if let Some(s) = spawn {
            out.push(Effect::Teleport {
                player: uuid.into(),
                to: self.arena_loc(s),
            });
        }
        out.push(Effect::Reset {
            player: uuid.into(),
            gamemode: self.rules.gamemode,
            clear_inventory: !keep,
        });
        if let Some(kit) = kit {
            out.push(Effect::Kit {
                player: uuid.into(),
                kit,
            });
        }
        out.title(vec![uuid.into()], " ".into(), String::new(), 1);
        out.event(Event::PlayerRespawned {
            game: self.id.clone(),
            player: r,
        });
    }

    /// Eliminates a player (the `eliminate` op). Works while alive or respawning.
    pub fn eliminate(&mut self, ctx: &Ctx, uuid: &str, out: &mut Out) -> bool {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return false;
        }
        if !self.players.get(uuid).is_some_and(GPlayer::in_round) {
            return false;
        }
        self.eliminate_inner(ctx, uuid, true, out);
        self.update_teams(ctx, out);
        self.check_win(ctx, out);
        true
    }

    fn eliminate_inner(&mut self, ctx: &Ctx, uuid: &str, announce: bool, out: &mut Out) {
        let Some(p) = self.players.get_mut(uuid) else {
            return;
        };
        p.status = PlayerStatus::Spectating;
        let r = p.reference();
        let shown = self.display(uuid);
        let line = self.msg(ctx, &ctx.cfg.messages.eliminated, &[("player", &shown)]);
        if announce {
            out.chat(self.audience(), line);
        }
        let title = self.plain(&ctx.cfg.messages.eliminated_title, &[]);
        out.title(vec![uuid.into()], title, String::new(), 40);
        out.event(Event::PlayerEliminated {
            game: self.id.clone(),
            player: r,
        });
        out.push(Effect::Freeze {
            player: uuid.into(),
            frozen: false,
        });
        if self.rules.spectate_on_elimination {
            out.push(Effect::Reset {
                player: uuid.into(),
                gamemode: GameModeName::Spectator,
                clear_inventory: true,
            });
            if let Some(s) = self.spectator_spot(uuid) {
                out.push(Effect::Teleport {
                    player: uuid.into(),
                    to: self.arena_loc(s),
                });
            }
        } else if let Some(p) = self.players.remove(uuid) {
            // Back to the lobby; the result still counts at round end.
            self.order.retain(|u| u != uuid);
            out.push(Effect::ToLobby {
                player: uuid.into(),
            });
            out.push(Effect::SidebarClear {
                player: uuid.into(),
            });
            out.push(Effect::PvpLeave {
                player: uuid.into(),
            });
            out.removed.push(uuid.into());
            self.departed.push(p);
        }
    }

    /// Changes whether a team or player may respawn.
    pub fn set_respawn(&mut self, team: Option<&str>, player: Option<&str>, allowed: bool) -> bool {
        let mut hit = false;
        if let Some(t) = team.and_then(|t| self.team_mut(t)) {
            t.respawn = allowed;
            hit = true;
        }
        if let Some(p) = player.and_then(|p| self.players.get_mut(p)) {
            p.respawn_allowed = allowed;
            hit = true;
        }
        hit
    }

    /// (team id, members still in the round) for teams not eliminated.
    pub fn teams_alive(&self) -> Vec<(String, usize)> {
        self.teams
            .iter()
            .filter(|t| !t.eliminated)
            .map(|t| {
                let n = self
                    .players
                    .values()
                    .filter(|p| p.team.as_deref() == Some(&t.id) && p.in_round())
                    .count();
                (t.id.clone(), n)
            })
            .collect()
    }

    fn update_teams(&mut self, ctx: &Ctx, out: &mut Out) {
        let alive = self.teams_alive();
        let solo = self.format.team_mode == TeamMode::Solo;
        for (id, n) in alive {
            if n == 0 {
                if let Some(t) = self.team_mut(&id) {
                    t.eliminated = true;
                }
                if !solo && let Some(t) = self.team(&id) {
                    let shown = t.display();
                    let line =
                        self.msg(ctx, &ctx.cfg.messages.team_eliminated, &[("team", &shown)]);
                    out.chat(self.audience(), line);
                }
                out.event(Event::TeamEliminated {
                    game: self.id.clone(),
                    team: id,
                });
            }
        }
    }

    fn check_win(&mut self, ctx: &Ctx, out: &mut Out) {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return;
        }
        let left: Vec<String> = self
            .teams
            .iter()
            .filter(|t| !t.eliminated)
            .map(|t| t.id.clone())
            .collect();
        if left.is_empty() {
            self.end_round(ctx, Vec::new(), "all_eliminated", out);
        } else if self.rules.win == WinRule::LastTeam && self.started_teams > 1 && left.len() == 1 {
            self.end_round(ctx, left, "last_team", out);
        }
    }

    // ── ending ──────────────────────────────────────────────────────────────────

    fn reward_text(&self, template: &str, p: &GPlayer, extra: &[(&str, &str)]) -> String {
        let kills = p.kills.to_string();
        let deaths = p.deaths.to_string();
        let mut values: Vec<(&str, &str)> = vec![
            ("player", &p.name),
            ("uuid", &p.uuid),
            ("mode", &self.mode.id),
            ("format", &self.format.name),
            ("arena", &self.arena.id),
            ("kills", &kills),
            ("deaths", &deaths),
        ];
        values.extend_from_slice(extra);
        let mut s = text::fill(template, &values);
        // {counter:<key>}
        while let Some(start) = s.find("{counter:") {
            let Some(end) = s[start..].find('}') else {
                break;
            };
            let key = &s[start + 9..start + end];
            let v = p.counters.get(key).copied().unwrap_or(0).to_string();
            s.replace_range(start..start + end + 1, &v);
        }
        s
    }

    /// Ends the round. Empty `winners` is a draw. Only from Running.
    pub fn end_round(
        &mut self,
        ctx: &Ctx,
        winners: Vec<String>,
        reason: &str,
        out: &mut Out,
    ) -> bool {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return false;
        }
        let elapsed = self.elapsed(ctx.now);
        let secs = self
            .mode
            .ending_seconds
            .unwrap_or(ctx.cfg.timing.ending_seconds);
        self.phase = GamePhase::Ending {
            ends_at: ctx.now + u64::from(secs) * TICKS,
            elapsed,
        };
        let winners: Vec<String> = winners
            .into_iter()
            .filter(|w| self.team(w).is_some())
            .collect();
        self.winners = winners.clone();
        self.end_reason = reason.into();
        let draw = winners.is_empty();

        let won = |p: &GPlayer| !p.left && p.team.as_ref().is_some_and(|t| winners.contains(t));
        let mut results = Vec::new();
        for p in self.players.values().chain(self.departed.iter()) {
            if matches!(p.status, PlayerStatus::Waiting) {
                continue;
            }
            let w = won(p);
            results.push(PlayerResult {
                uuid: p.uuid.clone(),
                name: p.name.clone(),
                won: w,
                kills: p.kills,
                deaths: p.deaths,
                counters: p.counters.clone(),
            });
            let list = if draw {
                &[][..]
            } else if w {
                &self.mode.rewards.win[..]
            } else {
                &self.mode.rewards.loss[..]
            };
            for cmd in list.iter().chain(self.mode.rewards.played.iter()) {
                out.push(Effect::Console {
                    command: self.reward_text(cmd, p, &[]),
                });
            }
        }
        out.results.push(RoundResult {
            mode: self.mode.id.clone(),
            draw,
            players: results,
        });

        let names: Vec<String> = winners
            .iter()
            .filter_map(|w| self.team(w))
            .map(GTeam::display)
            .collect();
        if draw {
            let line = self.msg(ctx, &ctx.cfg.messages.draw, &[]);
            out.chat(self.audience(), line);
        } else {
            let joined = names.join("&7, ");
            let line = self.msg(ctx, &ctx.cfg.messages.winners, &[("winners", &joined)]);
            out.chat(self.audience(), line);
        }
        let (win_p, lose_p): (Vec<String>, Vec<String>) = self
            .order
            .iter()
            .cloned()
            .partition(|u| self.players.get(u).is_some_and(&won));
        if draw {
            let t = self.plain(&ctx.cfg.messages.draw_title, &[]);
            out.title(self.audience(), t, String::new(), 60);
        } else {
            let t = self.plain(&ctx.cfg.messages.win_title, &[]);
            out.title(win_p.clone(), t, String::new(), 60);
            let t = self.plain(&ctx.cfg.messages.lose_title, &[]);
            out.title(lose_p, t, String::new(), 60);
            out.sound(win_p, "minecraft:ui.toast.challenge_complete", 1.0);
        }
        for u in &self.order {
            out.push(Effect::Freeze {
                player: u.clone(),
                frozen: false,
            });
        }
        out.event(Event::RoundEnded {
            game: self.id.clone(),
            winners,
            reason: reason.into(),
        });
        true
    }

    fn begin_reset(&mut self, out: &mut Out) {
        self.phase = GamePhase::Resetting;
        for u in std::mem::take(&mut self.order) {
            out.push(Effect::ToLobby { player: u.clone() });
            out.push(Effect::SidebarClear { player: u.clone() });
            out.push(Effect::PvpLeave { player: u.clone() });
            out.removed.push(u);
        }
        self.players.clear();
        out.push(Effect::RemoveTeams {
            world: self.arena.world.clone(),
            keys: self.teams.iter().map(|t| t.key.clone()).collect(),
        });
        out.push(Effect::RemoveHolograms {
            game: self.id.clone(),
        });
        out.resets.push(self.id.clone());
    }

    /// Stops the game at once (admin `/dbmg stop`, unload): no results, straight to reset.
    pub fn abort(&mut self, out: &mut Out) {
        if matches!(self.phase, GamePhase::Resetting) {
            return;
        }
        if matches!(self.phase, GamePhase::Running { .. }) {
            out.event(Event::RoundEnded {
                game: self.id.clone(),
                winners: Vec::new(),
                reason: "stopped".into(),
            });
        }
        self.begin_reset(out);
    }

    // ── rules applied to the world ──────────────────────────────────────────────

    /// Decides a hit on `victim` (a player in this game). Records kill credit when allowed.
    pub fn allow_damage(
        &mut self,
        victim: &str,
        attacker: Option<&str>,
        fall: bool,
        now: u64,
    ) -> bool {
        if !matches!(self.phase, GamePhase::Running { .. }) || !self.unfrozen {
            return false;
        }
        let Some(v) = self.players.get(victim) else {
            return false;
        };
        if v.status != PlayerStatus::Alive {
            return false;
        }
        let region = |f: fn(&crate::model::arena::RegionDef) -> Option<bool>| {
            self.arena
                .regions
                .iter()
                .filter(|r| v.regions.contains(&r.name))
                .find_map(f)
        };
        if !region(|r| r.damage).unwrap_or(self.rules.damage) {
            return false;
        }
        if fall && !self.rules.fall_damage {
            return false;
        }
        if let Some(a) = attacker {
            if a == victim {
                return true;
            }
            let Some(ap) = self.players.get(a) else {
                return false;
            };
            if ap.status != PlayerStatus::Alive {
                return false;
            }
            if !region(|r| r.pvp).unwrap_or(self.rules.pvp) {
                return false;
            }
            if ap.team.is_some() && ap.team == v.team && !self.rules.friendly_fire {
                return false;
            }
            self.record_attack(victim, a, now);
        }
        true
    }

    /// Decides a player's block place/break at `pos`.
    pub fn allow_block(&mut self, uuid: &str, pos: BlockPos, action: BlockAction) -> bool {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return false;
        }
        if !self
            .players
            .get(uuid)
            .is_some_and(|p| p.status == PlayerStatus::Alive)
        {
            return false;
        }
        if !self.arena.bounds().contains_block(pos) {
            return false;
        }
        let regions: Vec<_> = self
            .arena
            .regions
            .iter()
            .filter(|r| r.cuboid().contains_block(pos))
            .collect();
        match action {
            BlockAction::Place => {
                let allowed = regions
                    .iter()
                    .find_map(|r| r.build)
                    .unwrap_or(self.rules.build)
                    && self.rules.max_build_y.is_none_or(|y| pos.y <= y);
                if allowed {
                    self.placed.insert(pos);
                }
                allowed
            }
            BlockAction::Break { block } => {
                let allowed = match regions.iter().find_map(|r| r.break_blocks) {
                    Some(v) => v,
                    None => {
                        self.rules.is_breakable(block)
                            || match self.rules.break_rule {
                                BreakRule::None => false,
                                BreakRule::Placed => self.placed.contains(&pos),
                                BreakRule::Any => true,
                            }
                    }
                };
                if allowed {
                    self.placed.remove(&pos);
                }
                allowed
            }
        }
    }

    /// May an explosion destroy the block at `pos`?
    pub fn explosion_may_break(&self, pos: BlockPos) -> bool {
        if !self.arena.bounds().contains_block(pos) {
            return false;
        }
        if self
            .arena
            .regions
            .iter()
            .any(|r| r.break_blocks == Some(false) && r.cuboid().contains_block(pos))
        {
            return false;
        }
        match self.rules.explosions {
            BreakRule::None => false,
            BreakRule::Placed => self.placed.contains(&pos),
            BreakRule::Any => true,
        }
    }

    /// Position scan: void, bounds, region enter/leave.
    pub fn on_positions(
        &mut self,
        ctx: &Ctx,
        positions: &[(String, String, Location)],
        out: &mut Out,
    ) {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return;
        }
        let bounds = self.arena.bounds();
        let void_y = self.rules.void_y.unwrap_or(f64::from(bounds.min.y));
        let tracked: HashSet<String> = self
            .arena
            .regions
            .iter()
            .filter(|r| r.track)
            .map(|r| r.name.clone())
            .collect();
        for (uuid, world, loc) in positions {
            let Some(p) = self.players.get(uuid) else {
                continue;
            };
            match p.status {
                PlayerStatus::Alive => {}
                PlayerStatus::Respawning | PlayerStatus::Spectating => {
                    if (world != &self.arena.world
                        || !bounds.contains_xz(loc)
                        || loc.y < void_y - 32.0)
                        && let Some(s) = self.spectator_spot(uuid)
                    {
                        out.push(Effect::Teleport {
                            player: uuid.clone(),
                            to: self.arena_loc(s),
                        });
                    }
                    continue;
                }
                PlayerStatus::Waiting => continue,
            }
            if world == &self.arena.world && loc.y < void_y {
                self.on_death(ctx, uuid, "void", out);
                continue;
            }
            if world != &self.arena.world || !bounds.contains(loc) {
                match self.rules.out_of_bounds {
                    OutOfBounds::Kill => {
                        self.on_death(ctx, uuid, "out_of_bounds", out);
                        continue;
                    }
                    OutOfBounds::Teleport => {
                        if let Some(s) = p.spawn {
                            out.push(Effect::Teleport {
                                player: uuid.clone(),
                                to: self.arena_loc(s),
                            });
                        }
                    }
                    OutOfBounds::Ignore => {}
                }
            }
            let now_in = self.arena.regions_at(loc);
            let Some(p) = self.players.get_mut(uuid) else {
                continue;
            };
            let before = std::mem::replace(&mut p.regions, now_in.clone());
            let r = p.reference();
            for name in now_in
                .iter()
                .filter(|n| !before.contains(n) && tracked.contains(*n))
            {
                out.event(Event::RegionEntered {
                    game: self.id.clone(),
                    player: r.clone(),
                    region: name.clone(),
                });
            }
            for name in before
                .iter()
                .filter(|n| !now_in.contains(n) && tracked.contains(*n))
            {
                out.event(Event::RegionLeft {
                    game: self.id.clone(),
                    player: r.clone(),
                    region: name.clone(),
                });
            }
        }
    }

    // ── spawners ────────────────────────────────────────────────────────────────

    /// Runs every tick. Stockpile mode fills spawners; command mode returns drops.
    pub fn spawner_tick(&mut self, now: u64, stockpile: bool, out: &mut Out) {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return;
        }
        for (i, def) in self.arena.spawners.iter().enumerate() {
            let g = self.groups.get(&def.group);
            if g.is_some_and(|g| !g.enabled) {
                continue;
            }
            let interval = g.and_then(|g| g.interval).unwrap_or(def.interval).max(1);
            let amount = g.and_then(|g| g.amount).unwrap_or(def.amount);
            let rt = &mut self.spawners[i];
            if now < rt.next_at {
                continue;
            }
            rt.next_at = now + u64::from(interval);
            if stockpile {
                rt.stock = (rt.stock + amount).min(def.cap);
            } else if amount > 0 {
                out.push(Effect::SpawnerDrop {
                    world: self.arena.world.clone(),
                    x: def.at.x,
                    y: def.at.y,
                    z: def.at.z,
                    item: def.item.clone(),
                    count: amount,
                });
            }
        }
    }

    /// Stockpiles that a nearby alive player can take: (spawner, player, item, count).
    pub fn pickups(
        &self,
        positions: &[(String, String, Location)],
        radius: f64,
    ) -> Vec<(usize, String, String, u32)> {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return Vec::new();
        }
        let r2 = radius * radius;
        let mut out = Vec::new();
        for (i, def) in self.arena.spawners.iter().enumerate() {
            let stock = self.spawners[i].stock;
            if stock == 0 {
                continue;
            }
            let taker = positions.iter().find(|(u, w, l)| {
                w == &self.arena.world
                    && self
                        .players
                        .get(u)
                        .is_some_and(|p| p.status == PlayerStatus::Alive)
                    && (l.x - def.at.x).powi(2)
                        + (l.y - def.at.y).powi(2)
                        + (l.z - def.at.z).powi(2)
                        <= r2
            });
            if let Some((u, _, _)) = taker {
                out.push((i, u.clone(), def.item.clone(), stock));
            }
        }
        out
    }

    /// After a pickup: `given` items went to the player.
    pub fn pickup_done(&mut self, spawner: usize, given: u32) {
        if let Some(rt) = self.spawners.get_mut(spawner) {
            rt.stock = rt.stock.saturating_sub(given);
        }
    }

    /// Label text for each spawner with a label: (index, text with `&` codes).
    pub fn hologram_texts(&self, now: u64) -> Vec<(usize, String)> {
        if !matches!(self.phase, GamePhase::Running { .. }) {
            return Vec::new();
        }
        self.arena
            .spawners
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.label.is_empty())
            .map(|(i, d)| {
                let rt = &self.spawners[i];
                let stock = rt.stock.to_string();
                let secs = rt.next_at.saturating_sub(now).div_ceil(TICKS).to_string();
                let s = text::fill(
                    &d.label,
                    &[("stock", &stock), ("seconds", &secs), ("group", &d.group)],
                );
                (i, self.fill_vars(&s, None))
            })
            .collect()
    }

    /// Replaces `{var:<key>}` with game (or player) variables.
    pub fn fill_vars(&self, s: &str, player: Option<&str>) -> String {
        let mut s = s.to_string();
        let mut guard = 0;
        while let Some(start) = s.find("{var:") {
            guard += 1;
            if guard > 32 {
                break;
            }
            let Some(end) = s[start..].find('}') else {
                break;
            };
            let key = s[start + 5..start + end].to_string();
            let v = player
                .and_then(|u| self.players.get(u))
                .and_then(|p| p.vars.get(&key))
                .or_else(|| self.vars.get(&key))
                .cloned()
                .unwrap_or_default();
            s.replace_range(start..start + end + 1, &v);
        }
        s
    }

    // ── info for the protocol ───────────────────────────────────────────────────

    pub fn player_info(&self, p: &GPlayer) -> PlayerInfo {
        PlayerInfo {
            uuid: p.uuid.clone(),
            name: p.name.clone(),
            game: self.id.clone(),
            mode: self.mode.id.clone(),
            team: p.team.clone(),
            status: p.status,
            kills: p.kills,
            deaths: p.deaths,
            counters: p.counters.clone(),
            regions: p.regions.clone(),
            kit: p.kit.clone(),
        }
    }

    pub fn team_infos(&self) -> Vec<TeamInfo> {
        self.teams
            .iter()
            .map(|t| {
                let members: Vec<&GPlayer> = self
                    .order
                    .iter()
                    .filter_map(|u| self.players.get(u))
                    .filter(|p| p.team.as_deref() == Some(&t.id))
                    .collect();
                TeamInfo {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    color: t.color.clone(),
                    alive: members.iter().filter(|p| p.in_round()).count() as u32,
                    players: members.iter().map(|p| p.reference()).collect(),
                    respawn: t.respawn,
                    eliminated: t.eliminated,
                }
            })
            .collect()
    }

    pub fn info(&self, now: u64) -> GameInfo {
        GameInfo {
            game: self.id.clone(),
            mode: self.mode.id.clone(),
            format: self.format.name.clone(),
            phase: self.phase.public(),
            round: self.round,
            elapsed: self.elapsed(now),
            max_players: self.capacity() as u32,
            min_players: self.format.min_players,
            players: self
                .order
                .iter()
                .filter_map(|u| self.players.get(u))
                .map(|p| self.player_info(p))
                .collect(),
            teams: self.team_infos(),
            winners: self.winners.clone(),
        }
    }

    /// The next labelled schedule entry: (label, seconds until it).
    pub fn next_event(&self, now: u64) -> Option<(String, u32)> {
        let elapsed = self.elapsed(now);
        self.schedule
            .iter()
            .filter(|s| !s.fired && !s.label.is_empty() && s.at >= elapsed)
            .min_by_key(|s| s.at)
            .map(|s| (s.label.clone(), s.at - elapsed))
    }
}

#[cfg(test)]
pub(crate) mod tests;
