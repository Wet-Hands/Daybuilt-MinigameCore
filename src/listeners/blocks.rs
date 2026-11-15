//! Block rules and the reset log.
//!
//! - Players in a round: the game's rules decide; allowed changes record the original
//!   block first.
//! - Players outside games: running arenas are off limits, idle arenas and the lobby are
//!   protected (unless they have `<ID>:build`).
//! - Changes with no player (fluids, fire, nature, explosions, falling blocks) in a
//!   running arena follow the round's rules and are recorded; in idle arenas they are
//!   cancelled. Pistons are cancelled in arenas because what they move is not reported.

use pumpkin_plugin_api::{
    Context, Result, Server, common::BlockPos as HostPos, events::*, player::Player, world::World,
};

use super::{has, uuid_of};
use crate::game::{BlockAction, effects::Effect};
use crate::host::record_block;
use crate::model::BlockPos;
use crate::state;

pub fn register(context: &Context) -> Result<()> {
    let p = EventPriority::Normal;
    context.register_event_handler::<BlockBreakEvent, _>(Break, p, true)?;
    context.register_event_handler::<BlockPlaceEvent, _>(Place, p, true)?;
    context.register_event_handler::<BlockMultiPlaceEvent, _>(MultiPlace, p, true)?;
    context.register_event_handler::<PlayerBucketEmptyEvent, _>(Bucket, p, true)?;
    context.register_event_handler::<BlockBurnEvent, _>(Burn, p, true)?;
    context.register_event_handler::<BlockIgniteEvent, _>(Ignite, p, true)?;
    context.register_event_handler::<BlockFromToEvent, _>(Flow, p, true)?;
    context.register_event_handler::<BlockFormEvent, _>(Form, p, true)?;
    context.register_event_handler::<BlockFadeEvent, _>(Fade, p, true)?;
    context.register_event_handler::<BlockSpreadEvent, _>(Spread, p, true)?;
    context.register_event_handler::<LeavesDecayEvent, _>(Decay, p, true)?;
    context.register_event_handler::<EntityChangeBlockEvent, _>(EntityChange, p, true)?;
    context.register_event_handler::<BlockPistonExtendEvent, _>(PistonOut, p, true)?;
    context.register_event_handler::<BlockPistonRetractEvent, _>(PistonIn, p, true)?;
    context.register_event_handler::<BlockExplodeEvent, _>(BlockBoom, p, true)?;
    context.register_event_handler::<EntityExplodeEvent, _>(EntityBoom, p, true)?;
    Ok(())
}

fn ours(p: HostPos) -> BlockPos {
    BlockPos::new(p.x, p.y, p.z)
}

fn host_pos(p: BlockPos) -> HostPos {
    HostPos {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

/// Where a block change happens, from the core's point of view.
enum Site {
    /// Player in a game; `allowed` by the game's rules.
    InGame { game: String, allowed: bool },
    /// Inside the arena of a running game the player is not part of.
    RunningArena,
    /// Inside an idle arena.
    IdleArena,
    /// Anywhere else.
    Outside,
}

fn classify(uuid: &str, world: &str, pos: BlockPos, action: BlockAction) -> (Site, bool, bool) {
    state::with(|s| {
        let protect_idle = s.cfg.protection.idle_arenas;
        let protect_lobby = s.cfg.lobby.protect;
        if let Some(gid) = s.index.get(uuid).cloned()
            && let Some(g) = s.games.get_mut(&gid)
        {
            let allowed = g.arena.world == world && g.allow_block(uuid, pos, action);
            return (
                Site::InGame { game: gid, allowed },
                protect_idle,
                protect_lobby,
            );
        }
        let place = if s.game_at(Some(world), pos).is_some() {
            Site::RunningArena
        } else if s.idle_arena_at(Some(world), pos).is_some() {
            Site::IdleArena
        } else {
            Site::Outside
        };
        (place, protect_idle, protect_lobby)
    })
}

/// Shared decision for player place/break. Returns true to cancel.
fn player_change(
    p: &Player,
    pos: BlockPos,
    action: BlockAction,
    extra_record: &[BlockPos],
) -> bool {
    let uuid = uuid_of(p);
    let world = p.get_world();
    let wname = world.get_name();
    let (place, protect_idle, protect_lobby) = classify(&uuid, &wname, pos, action);
    match place {
        Site::InGame { game, allowed } => {
            if allowed {
                record(&world, &wname, &game, pos);
                for x in extra_record {
                    record(&world, &wname, &game, *x);
                }
            }
            !allowed
        }
        Site::RunningArena => {
            // Admins may still edit; the change is recorded so reset undoes it.
            if has(p, "build") {
                if let Some(game) = state::with(|s| s.game_at(Some(&wname), pos)) {
                    record(&world, &wname, &game, pos);
                }
                false
            } else {
                true
            }
        }
        Site::IdleArena => protect_idle && !has(p, "build"),
        Site::Outside => protect_lobby && !has(p, "build"),
    }
}

fn record(world: &World, wname: &str, game: &str, pos: BlockPos) {
    let original = world.get_block_state_id(host_pos(pos));
    record_block(game, wname, pos, original);
}

/// Blocks that take a neighbour with them when broken.
fn multi_block(name: &str) -> bool {
    [
        "_bed",
        "_door",
        "tall_",
        "large_fern",
        "sunflower",
        "lilac",
        "rose_bush",
        "peony",
        "pitcher_plant",
    ]
    .iter()
    .any(|k| name.contains(k))
}

fn neighbours(p: BlockPos) -> [BlockPos; 6] {
    [
        BlockPos::new(p.x + 1, p.y, p.z),
        BlockPos::new(p.x - 1, p.y, p.z),
        BlockPos::new(p.x, p.y + 1, p.z),
        BlockPos::new(p.x, p.y - 1, p.z),
        BlockPos::new(p.x, p.y, p.z + 1),
        BlockPos::new(p.x, p.y, p.z - 1),
    ]
}

/// The game whose running arena holds `pos` → (game, world name, rules snapshot).
fn running_game(
    world: Option<&str>,
    pos: BlockPos,
) -> Option<(String, String, crate::model::rules::Rules)> {
    state::with(|s| {
        let id = s.game_at(world, pos)?;
        let g = s.games.get(&id)?;
        Some((id, g.arena.world.clone(), g.rules.clone()))
    })
}

fn idle_arena(world: Option<&str>, pos: BlockPos) -> bool {
    state::with(|s| s.cfg.protection.idle_arenas && s.idle_arena_at(world, pos).is_some())
}

/// Non-player change at `pos`: cancel if `allowed(rules)` is false or the arena is idle,
/// otherwise record. Returns true to cancel.
fn natural_change(
    server: &Server,
    world: Option<&str>,
    pos: BlockPos,
    allowed: impl Fn(&crate::model::rules::Rules) -> bool,
) -> bool {
    if let Some((game, wname, rules)) = running_game(world, pos) {
        if !allowed(&rules) {
            return true;
        }
        if let Some(w) = server.get_world_by_name(&wname) {
            record(&w, &wname, &game, pos);
        }
        return false;
    }
    idle_arena(world, pos)
}

struct Break;
impl EventHandler<BlockBreakEvent> for Break {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockBreakEvent>,
    ) -> EventData<BlockBreakEvent> {
        let pos = ours(e.block_pos);
        match &e.player {
            Some(p) => {
                let mut extra = vec![BlockPos::new(pos.x, pos.y + 1, pos.z)];
                if multi_block(&e.block) {
                    extra.extend(neighbours(pos));
                }
                let block = e.block.clone();
                if player_change(p, pos, BlockAction::Break { block: &block }, &extra) {
                    e.cancelled = true;
                }
            }
            None => {
                if natural_change(&server, None, pos, |_| true) {
                    e.cancelled = true;
                }
            }
        }
        e
    }
}

struct Place;
impl EventHandler<BlockPlaceEvent> for Place {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<BlockPlaceEvent>,
    ) -> EventData<BlockPlaceEvent> {
        if player_change(&e.player, ours(e.block_pos), BlockAction::Place, &[]) {
            e.cancelled = true;
        }
        e
    }
}

struct MultiPlace;
impl EventHandler<BlockMultiPlaceEvent> for MultiPlace {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<BlockMultiPlaceEvent>,
    ) -> EventData<BlockMultiPlaceEvent> {
        let positions: Vec<BlockPos> = e.placed_blocks.iter().map(|(p, _)| ours(*p)).collect();
        let Some(first) = positions.first().copied() else {
            return e;
        };
        if player_change(&e.player, first, BlockAction::Place, &positions[1..]) {
            e.cancelled = true;
        } else {
            // Every part counts as player-placed.
            let uuid = uuid_of(&e.player);
            state::with(|s| {
                if let Some(g) = s.game_of_mut(&uuid) {
                    g.placed.extend(positions.iter().copied());
                }
            });
        }
        e
    }
}

struct Bucket;
impl EventHandler<PlayerBucketEmptyEvent> for Bucket {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<PlayerBucketEmptyEvent>,
    ) -> EventData<PlayerBucketEmptyEvent> {
        if player_change(&e.player, ours(e.block_pos), BlockAction::Place, &[]) {
            e.cancelled = true;
        }
        e
    }
}

struct Burn;
impl EventHandler<BlockBurnEvent> for Burn {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockBurnEvent>,
    ) -> EventData<BlockBurnEvent> {
        let world = e.target_world.get_name();
        if natural_change(&server, Some(&world), ours(e.block_pos), |r| r.fire_spread) {
            e.cancelled = true;
        }
        e
    }
}

struct Ignite;
impl EventHandler<BlockIgniteEvent> for Ignite {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockIgniteEvent>,
    ) -> EventData<BlockIgniteEvent> {
        if natural_change(&server, None, ours(e.block_pos), |_| true) {
            e.cancelled = true;
        }
        e
    }
}

struct Flow;
impl EventHandler<BlockFromToEvent> for Flow {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockFromToEvent>,
    ) -> EventData<BlockFromToEvent> {
        if natural_change(&server, None, ours(e.to_pos), |r| r.fluid_flow) {
            e.cancelled = true;
        }
        e
    }
}

struct Form;
impl EventHandler<BlockFormEvent> for Form {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockFormEvent>,
    ) -> EventData<BlockFormEvent> {
        if natural_change(&server, None, ours(e.block_pos), |r| r.nature) {
            e.cancelled = true;
        }
        e
    }
}

struct Fade;
impl EventHandler<BlockFadeEvent> for Fade {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockFadeEvent>,
    ) -> EventData<BlockFadeEvent> {
        if natural_change(&server, None, ours(e.block_pos), |r| r.nature) {
            e.cancelled = true;
        }
        e
    }
}

struct Spread;
impl EventHandler<BlockSpreadEvent> for Spread {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockSpreadEvent>,
    ) -> EventData<BlockSpreadEvent> {
        let world = e.target_world.get_name();
        if natural_change(&server, Some(&world), ours(e.target_pos), |r| r.nature) {
            e.cancelled = true;
        }
        e
    }
}

struct Decay;
impl EventHandler<LeavesDecayEvent> for Decay {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<LeavesDecayEvent>,
    ) -> EventData<LeavesDecayEvent> {
        let world = e.target_world.get_name();
        if natural_change(&server, Some(&world), ours(e.block_pos), |r| r.nature) {
            e.cancelled = true;
        }
        e
    }
}

struct EntityChange;
impl EventHandler<EntityChangeBlockEvent> for EntityChange {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<EntityChangeBlockEvent>,
    ) -> EventData<EntityChangeBlockEvent> {
        if natural_change(&server, None, ours(e.block_pos), |_| true) {
            e.cancelled = true;
        }
        e
    }
}

fn in_any_arena(pos: BlockPos) -> bool {
    state::with(|s| s.game_at(None, pos).is_some() || s.idle_arena_at(None, pos).is_some())
}

struct PistonOut;
impl EventHandler<BlockPistonExtendEvent> for PistonOut {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<BlockPistonExtendEvent>,
    ) -> EventData<BlockPistonExtendEvent> {
        if in_any_arena(ours(e.block_pos)) {
            e.cancelled = true;
        }
        e
    }
}

struct PistonIn;
impl EventHandler<BlockPistonRetractEvent> for PistonIn {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<BlockPistonRetractEvent>,
    ) -> EventData<BlockPistonRetractEvent> {
        if in_any_arena(ours(e.block_pos)) {
            e.cancelled = true;
        }
        e
    }
}

/// An explosion centred on `center`. Records what it may destroy and schedules the rest
/// to be put back. Returns true to cancel it outright.
fn explosion(server: &Server, center: BlockPos) -> bool {
    let info = state::with(|s| {
        let id = s.game_at(None, center)?;
        let g = s.games.get(&id)?;
        Some((
            id,
            g.arena.world.clone(),
            g.rules.explosions,
            s.cfg.reset.explosion_radius as i32,
        ))
    });
    let Some((game, wname, rule, r)) = info else {
        return idle_arena(None, center);
    };
    if rule == crate::model::rules::BreakRule::None {
        return true;
    }
    let Some(w) = server.get_world_by_name(&wname) else {
        return false;
    };
    let mut sphere = Vec::new();
    for dx in -r..=r {
        for dy in -r..=r {
            for dz in -r..=r {
                if dx * dx + dy * dy + dz * dz <= r * r {
                    sphere.push(BlockPos::new(center.x + dx, center.y + dy, center.z + dz));
                }
            }
        }
    }
    let verdicts: Vec<Option<bool>> = state::with(|s| {
        let g = s.games.get(&game);
        sphere
            .iter()
            .map(|p| {
                g.filter(|g| g.arena.bounds().contains_block(*p))
                    .map(|g| g.explosion_may_break(*p))
            })
            .collect()
    });
    let mut keep = Vec::new();
    let mut may_break = Vec::new();
    for (pos, verdict) in sphere.into_iter().zip(verdicts) {
        let Some(breakable) = verdict else { continue };
        let id = w.get_block_state_id(host_pos(pos));
        if id == 0 {
            continue; // air
        }
        if breakable {
            may_break.push((pos, id));
        } else {
            keep.push((pos, id));
        }
    }
    for (pos, id) in may_break {
        record_block(&game, &wname, pos, id);
    }
    if !keep.is_empty() {
        // Anything the rules protect is put back right after the blast.
        state::with(|s| {
            s.after(
                1,
                vec![Effect::PutBlocks {
                    world: wname,
                    blocks: keep,
                }],
            )
        });
    }
    false
}

struct BlockBoom;
impl EventHandler<BlockExplodeEvent> for BlockBoom {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<BlockExplodeEvent>,
    ) -> EventData<BlockExplodeEvent> {
        if explosion(&server, ours(e.block_pos)) {
            e.cancelled = true;
        }
        e
    }
}

struct EntityBoom;
impl EventHandler<EntityExplodeEvent> for EntityBoom {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<EntityExplodeEvent>,
    ) -> EventData<EntityExplodeEvent> {
        let (x, y, z) = e.position;
        let center = BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
        if explosion(&server, center) {
            e.cancelled = true;
        }
        e
    }
}
