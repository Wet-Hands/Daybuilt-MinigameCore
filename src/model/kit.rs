//! Kits: `kits/<name>.toml`. Items with slots, armour and potion effects.

use serde::{Deserialize, Serialize};

use super::ids;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KitItem {
    /// Item id, e.g. `minecraft:stone_sword` (`minecraft:` may be left out).
    pub item: String,
    #[serde(default = "one")]
    pub count: u8,
    /// Custom name (`&` codes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lore: Vec<String>,
    /// Enchantment name → level, e.g. `{ sharpness = 2 }`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub enchantments: std::collections::BTreeMap<String, u32>,
    /// Inventory slot 0-35 (0-8 is the hotbar). Only for `items`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
}

fn one() -> u8 {
    1
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KitEffect {
    /// Effect name, e.g. `speed`, `jump_boost`.
    pub effect: String,
    /// Seconds; 0 = for the whole round.
    #[serde(default)]
    pub seconds: u32,
    /// 0 = level I.
    #[serde(default)]
    pub amplifier: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct KitDef {
    #[serde(skip)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    /// Permission players need to pick the kit (`/mg kit`). Empty = everyone.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permission: String,
    /// Clear the inventory before applying.
    #[serde(default = "yes")]
    pub clear: bool,
    #[serde(default)]
    pub items: Vec<KitItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helmet: Option<KitItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chestplate: Option<KitItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leggings: Option<KitItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boots: Option<KitItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offhand: Option<KitItem>,
    #[serde(default)]
    pub effects: Vec<KitEffect>,
}

fn yes() -> bool {
    true
}

impl KitDef {
    pub fn parse(id: &str, text: &str) -> Result<Self, String> {
        ids::kit(id)?;
        let mut kit: KitDef = toml::from_str(text).map_err(|e| e.to_string())?;
        kit.id = id.into();
        for item in &kit.items {
            if let Some(slot) = item.slot
                && slot > 35
            {
                return Err(format!("item {}: slot {slot} is not 0-35", item.item));
            }
            if item.count == 0 {
                return Err(format!("item {}: count is 0", item.item));
            }
        }
        Ok(kit)
    }

    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn display(&self) -> &str {
        if self.display_name.is_empty() {
            &self.id
        } else {
            &self.display_name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_example() {
        let kit = KitDef::parse("starter", include_str!("../../config/kits/starter.toml")).unwrap();
        assert!(!kit.items.is_empty());
        let again = KitDef::parse("starter", &kit.to_toml().unwrap()).unwrap();
        assert_eq!(kit, again);
        assert!(KitDef::parse("x", "[[items]]\nitem = \"stone\"\nslot = 40").is_err());
    }
}
