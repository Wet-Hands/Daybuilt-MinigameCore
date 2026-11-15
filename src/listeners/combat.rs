//! Damage, deaths, hunger and items.

use pumpkin_plugin_api::{Context, DamageType, Result, Server, events::*};

use super::{entity_player, uuid_of};
use crate::game::{GamePhase, effects::Effect};
use crate::model::rules::DeathDrops;
use crate::state;

pub fn register(context: &Context) -> Result<()> {
    let p = EventPriority::Normal;
    context.register_event_handler::<EntityDamageEvent, _>(Damage, p, true)?;
    context.register_event_handler::<EntityDamageByEntityEvent, _>(Hit, p, true)?;
    context.register_event_handler::<ProjectileLaunchEvent, _>(Launch, p, false)?;
    context.register_event_handler::<PlayerDeathEvent, _>(Death, p, true)?;
    context.register_event_handler::<FoodLevelChangeEvent, _>(Food, p, true)?;
    context.register_event_handler::<PlayerDropItemEvent, _>(Drop, p, true)?;
    context.register_event_handler::<EntityPickupItemEvent, _>(Pickup, p, true)?;
    Ok(())
}

/// Damage from any source. Hits by entities are decided again in `Hit`.
struct Damage;
impl EventHandler<EntityDamageEvent> for Damage {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<EntityDamageEvent>,
    ) -> EventData<EntityDamageEvent> {
        let Some(victim) = entity_player(&server, e.entity_id) else {
            return e;
        };
        let fall = matches!(e.damage_type, DamageType::Fall);
        let void = matches!(e.damage_type, DamageType::OutOfWorld);
        let cancel = state::with(|s| {
            let now = s.now;
            match s.game_of_mut(&victim) {
                // The core handles the void itself (position scan), so let it through.
                Some(g) => !(void || g.allow_damage(&victim, None, fall, now)),
                None => s.cfg.lobby.no_damage && !void,
            }
        });
        if cancel {
            e.cancelled = true;
        } else {
            let cause = crate::host::kits::snake(&format!("{:?}", e.damage_type));
            state::with(|s| s.causes.insert(victim, cause));
        }
        e
    }
}

/// Player hit by an entity (melee or a tracked projectile).
struct Hit;
impl EventHandler<EntityDamageByEntityEvent> for Hit {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<EntityDamageByEntityEvent>,
    ) -> EventData<EntityDamageByEntityEvent> {
        let Some(victim) = entity_player(&server, e.entity_id) else {
            return e;
        };
        let shooter = state::with(|s| s.projectiles.get(&e.damager_id).map(|(id, _)| *id));
        let attacker = entity_player(&server, shooter.unwrap_or(e.damager_id));
        let cancel = state::with(|s| {
            let now = s.now;
            let attacker_in_game = attacker.as_deref().is_some_and(|a| s.index.contains_key(a));
            match s.game_of_mut(&victim) {
                Some(g) => !g.allow_damage(&victim, attacker.as_deref(), false, now),
                // A player in a game may not hit someone outside it; lobby rules otherwise.
                None => attacker_in_game || s.cfg.lobby.no_damage,
            }
        });
        if cancel {
            e.cancelled = true;
        }
        e
    }
}

/// Remembers who shot a projectile, for kill credit.
struct Launch;
impl EventHandler<ProjectileLaunchEvent> for Launch {
    fn handle(
        &self,
        _server: Server,
        e: EventData<ProjectileLaunchEvent>,
    ) -> EventData<ProjectileLaunchEvent> {
        if let Some(shooter) = e.shooter_id {
            state::with(|s| {
                let now = s.now;
                s.projectiles.insert(e.entity_id, (shooter, now));
            });
        }
        e
    }
}

struct Death;
impl EventHandler<PlayerDeathEvent> for Death {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<PlayerDeathEvent>,
    ) -> EventData<PlayerDeathEvent> {
        let uuid = uuid_of(&e.player);
        let drops = state::with(|s| {
            let drops = s.game_of(&uuid).map(|g| g.rules.death_drops);
            let cause = s.causes.remove(&uuid).unwrap_or_else(|| "unknown".into());
            let handled = s.death(&uuid, &cause)?;
            // Skip the death screen, then apply the core's effects once respawned.
            s.after(
                1,
                vec![Effect::ForceRespawn {
                    player: uuid.clone(),
                }],
            );
            s.after(3, handled);
            drops
        });
        match drops {
            Some(DeathDrops::Keep) => {
                e.keep_inventory = true;
            }
            Some(DeathDrops::Clear) => {
                e.player.get_inventory().clear_all();
                e.dropped_exp = 0;
            }
            _ => {}
        }
        e
    }
}

struct Food;
impl EventHandler<FoodLevelChangeEvent> for Food {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<FoodLevelChangeEvent>,
    ) -> EventData<FoodLevelChangeEvent> {
        let Some(uuid) = entity_player(&server, e.entity_id) else {
            return e;
        };
        let cancel = state::with(|s| match s.game_of(&uuid) {
            Some(g) => !g.rules.hunger || !matches!(g.phase, GamePhase::Running { .. }),
            None => s.cfg.lobby.no_hunger,
        });
        if cancel {
            e.cancelled = true;
        }
        e
    }
}

struct Drop;
impl EventHandler<PlayerDropItemEvent> for Drop {
    fn handle(
        &self,
        _server: Server,
        mut e: EventData<PlayerDropItemEvent>,
    ) -> EventData<PlayerDropItemEvent> {
        let uuid = uuid_of(&e.player);
        let cancel = state::with(|s| match s.game_of(&uuid) {
            Some(g) => {
                !g.rules.item_drop
                    || !matches!(g.phase, GamePhase::Running { .. })
                    || !g
                        .players
                        .get(&uuid)
                        .is_some_and(|p| p.status == minigamecore_client::PlayerStatus::Alive)
            }
            None => s.cfg.lobby.no_item_drop,
        });
        if cancel {
            e.cancelled = true;
        }
        e
    }
}

struct Pickup;
impl EventHandler<EntityPickupItemEvent> for Pickup {
    fn handle(
        &self,
        server: Server,
        mut e: EventData<EntityPickupItemEvent>,
    ) -> EventData<EntityPickupItemEvent> {
        let Some(uuid) = entity_player(&server, e.entity_id) else {
            return e;
        };
        let cancel = state::with(|s| {
            s.game_of(&uuid).is_some_and(|g| {
                !g.rules.item_pickup
                    || !g
                        .players
                        .get(&uuid)
                        .is_some_and(|p| p.status == minigamecore_client::PlayerStatus::Alive)
            })
        });
        if cancel {
            e.cancelled = true;
        }
        e
    }
}
