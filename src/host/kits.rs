//! Applying kits and giving items.

use pumpkin_plugin_api::{
    ItemStack,
    enchantments_wit::Enchantment,
    player::Player,
    wit::pumpkin::plugin::status_effect::{StatusEffectInstance, StatusEffectType},
};

use crate::model::kit::{KitDef, KitItem};

fn item_key(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

pub fn build(item: &KitItem) -> ItemStack {
    let stack = ItemStack::new(&item_key(&item.item), item.count);
    if let Some(name) = &item.name {
        stack.set_custom_name(Some(super::colored(name)));
    }
    if !item.lore.is_empty() {
        stack.set_lore(item.lore.iter().map(|l| super::colored(l)).collect());
    }
    for (name, level) in &item.enchantments {
        match enchantment(name) {
            Some(e) => stack.add_enchantment(e, *level),
            None => stack.add_custom_enchantment(name, *level),
        }
    }
    stack
}

pub fn apply(p: &Player, kit: &KitDef) {
    let inv = p.get_inventory();
    if kit.clear {
        inv.clear_all();
        p.clear_effects();
    }
    for item in &kit.items {
        match item.slot {
            Some(slot) => p.set_inventory_item(slot, Some(build(item))),
            None => {
                give(p, &item_key(&item.item), u32::from(item.count));
            }
        }
    }
    if let Some(i) = &kit.helmet {
        inv.set_helmet(Some(build(i)));
    }
    if let Some(i) = &kit.chestplate {
        inv.set_chestplate(Some(build(i)));
    }
    if let Some(i) = &kit.leggings {
        inv.set_leggings(Some(build(i)));
    }
    if let Some(i) = &kit.boots {
        inv.set_boots(Some(build(i)));
    }
    if let Some(i) = &kit.offhand {
        inv.set_off_hand(Some(build(i)));
    }
    for e in &kit.effects {
        let Some(effect_type) = status_effect(&e.effect) else {
            tracing::warn!(
                "{}: kit {}: unknown effect '{}'",
                crate::ID,
                kit.id,
                e.effect
            );
            continue;
        };
        p.add_effect(StatusEffectInstance {
            effect_type,
            // 0 = whole round: ~27 hours of ticks.
            duration: if e.seconds == 0 {
                2_000_000
            } else {
                e.seconds.saturating_mul(20)
            },
            amplifier: e.amplifier,
            ambient: false,
            show_particles: false,
            show_icon: true,
        });
    }
}

/// Puts `count` of `item` into the main inventory (stacking first). Returns how many fit.
pub fn give(p: &Player, item: &str, count: u32) -> u32 {
    let inv = p.get_inventory().as_inventory();
    let mut left = count;
    // Top up existing stacks.
    for slot in 0..36u32 {
        if left == 0 {
            break;
        }
        if let Some(stack) = inv.get_item(slot)
            && stack.get_registry_key() == item
        {
            let max = u32::from(stack.get_max_count());
            let have = u32::from(stack.get_count());
            if have < max {
                let add = (max - have).min(left);
                stack.set_count((have + add) as u8);
                inv.set_item(slot, Some(stack));
                left -= add;
            }
        }
    }
    // Then empty slots.
    for slot in 0..36u32 {
        if left == 0 {
            break;
        }
        if inv.get_item(slot).is_none() {
            let probe = ItemStack::new(item, 1);
            let max = u32::from(probe.get_max_count()).max(1);
            let n = max.min(left);
            probe.set_count(n as u8);
            inv.set_item(slot, Some(probe));
            left -= n;
        }
    }
    count - left
}

pub fn enchantment(name: &str) -> Option<Enchantment> {
    let n = name.trim_start_matches("minecraft:").to_ascii_lowercase();
    Some(match n.as_str() {
        "aqua_affinity" => Enchantment::AquaAffinity,
        "bane_of_arthropods" => Enchantment::BaneOfArthropods,
        "binding_curse" => Enchantment::BindingCurse,
        "blast_protection" => Enchantment::BlastProtection,
        "breach" => Enchantment::Breach,
        "channeling" => Enchantment::Channeling,
        "density" => Enchantment::Density,
        "depth_strider" => Enchantment::DepthStrider,
        "efficiency" => Enchantment::Efficiency,
        "feather_falling" => Enchantment::FeatherFalling,
        "fire_aspect" => Enchantment::FireAspect,
        "fire_protection" => Enchantment::FireProtection,
        "flame" => Enchantment::Flame,
        "fortune" => Enchantment::Fortune,
        "frost_walker" => Enchantment::FrostWalker,
        "impaling" => Enchantment::Impaling,
        "infinity" => Enchantment::Infinity,
        "knockback" => Enchantment::Knockback,
        "looting" => Enchantment::Looting,
        "loyalty" => Enchantment::Loyalty,
        "luck_of_the_sea" => Enchantment::LuckOfTheSea,
        "lunge" => Enchantment::Lunge,
        "lure" => Enchantment::Lure,
        "mending" => Enchantment::Mending,
        "multishot" => Enchantment::Multishot,
        "piercing" => Enchantment::Piercing,
        "power" => Enchantment::Power,
        "projectile_protection" => Enchantment::ProjectileProtection,
        "protection" => Enchantment::Protection,
        "punch" => Enchantment::Punch,
        "quick_charge" => Enchantment::QuickCharge,
        "respiration" => Enchantment::Respiration,
        "riptide" => Enchantment::Riptide,
        "sharpness" => Enchantment::Sharpness,
        "silk_touch" => Enchantment::SilkTouch,
        "smite" => Enchantment::Smite,
        "soul_speed" => Enchantment::SoulSpeed,
        "sweeping_edge" => Enchantment::SweepingEdge,
        "swift_sneak" => Enchantment::SwiftSneak,
        "thorns" => Enchantment::Thorns,
        "unbreaking" => Enchantment::Unbreaking,
        "vanishing_curse" => Enchantment::VanishingCurse,
        "wind_burst" => Enchantment::WindBurst,
        _ => return None,
    })
}

/// The kit-file name for an enchantment (`Sharpness` → `sharpness`), via its Debug name.
pub fn enchantment_name(e: Enchantment) -> String {
    snake(&format!("{e:?}"))
}

/// `SpectralArrow` → `spectral_arrow`. Accepts Debug output of generated enums, which
/// includes the type name (`EntityType::SpectralArrow`).
pub fn snake(s: &str) -> String {
    let s = s.rsplit("::").next().unwrap_or(s);
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub fn status_effect(name: &str) -> Option<StatusEffectType> {
    let n = name.trim_start_matches("minecraft:").to_ascii_lowercase();
    Some(match n.as_str() {
        "speed" => StatusEffectType::Speed,
        "slowness" => StatusEffectType::Slowness,
        "haste" => StatusEffectType::Haste,
        "mining_fatigue" => StatusEffectType::MiningFatigue,
        "strength" => StatusEffectType::Strength,
        "instant_health" => StatusEffectType::InstantHealth,
        "instant_damage" => StatusEffectType::InstantDamage,
        "jump_boost" => StatusEffectType::JumpBoost,
        "nausea" => StatusEffectType::Nausea,
        "regeneration" => StatusEffectType::Regeneration,
        "resistance" => StatusEffectType::Resistance,
        "fire_resistance" => StatusEffectType::FireResistance,
        "water_breathing" => StatusEffectType::WaterBreathing,
        "invisibility" => StatusEffectType::Invisibility,
        "blindness" => StatusEffectType::Blindness,
        "night_vision" => StatusEffectType::NightVision,
        "hunger" => StatusEffectType::Hunger,
        "weakness" => StatusEffectType::Weakness,
        "poison" => StatusEffectType::Poison,
        "wither" => StatusEffectType::Wither,
        "health_boost" => StatusEffectType::HealthBoost,
        "absorption" => StatusEffectType::Absorption,
        "saturation" => StatusEffectType::Saturation,
        "glowing" => StatusEffectType::Glowing,
        "levitation" => StatusEffectType::Levitation,
        "slow_falling" => StatusEffectType::SlowFalling,
        "dolphins_grace" => StatusEffectType::DolphinsGrace,
        "darkness" => StatusEffectType::Darkness,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(snake("SpectralArrow"), "spectral_arrow");
        assert_eq!(
            snake(&format!(
                "{:?}",
                pumpkin_plugin_api::EntityType::TextDisplay
            )),
            "text_display"
        );
        assert_eq!(enchantment_name(Enchantment::FireAspect), "fire_aspect");
        assert!(enchantment("minecraft:sharpness").is_some());
        assert!(status_effect("jump_boost").is_some());
        assert!(status_effect("flying").is_none());
    }
}
