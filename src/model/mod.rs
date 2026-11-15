//! Pure data definitions loaded from the data folder. No host calls in here.

pub mod arena;
pub mod ids;
pub mod kit;
pub mod mode;
pub mod rules;
pub mod text;
pub mod toml_util;

pub use minigamecore_client::{BlockPos, Location};
use serde::{Deserialize, Serialize};

/// A location in a named world.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldLoc {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
}

impl WorldLoc {
    pub fn new(world: &str, loc: Location) -> Self {
        Self {
            world: world.into(),
            x: loc.x,
            y: loc.y,
            z: loc.z,
            yaw: loc.yaw,
            pitch: loc.pitch,
        }
    }
}

/// An axis-aligned box of whole blocks, inclusive on both corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cuboid {
    pub min: BlockPos,
    pub max: BlockPos,
}

impl Cuboid {
    pub fn new(a: BlockPos, b: BlockPos) -> Self {
        Self {
            min: BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)),
            max: BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)),
        }
    }

    pub fn contains_block(&self, p: BlockPos) -> bool {
        (self.min.x..=self.max.x).contains(&p.x)
            && (self.min.y..=self.max.y).contains(&p.y)
            && (self.min.z..=self.max.z).contains(&p.z)
    }

    pub fn contains(&self, l: &Location) -> bool {
        self.contains_block(l.block())
    }

    /// Same as `contains` but ignores height (for "left the arena sideways").
    pub fn contains_xz(&self, l: &Location) -> bool {
        let b = l.block();
        (self.min.x..=self.max.x).contains(&b.x) && (self.min.z..=self.max.z).contains(&b.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuboid_normalises_and_contains() {
        let c = Cuboid::new(BlockPos::new(10, 70, -5), BlockPos::new(-10, 60, 5));
        assert_eq!(c.min, BlockPos::new(-10, 60, -5));
        assert!(c.contains(&Location::new(-9.5, 60.0, 5.99)));
        assert!(!c.contains(&Location::new(-10.01, 65.0, 0.0)));
        assert!(!c.contains(&Location::new(0.0, 59.9, 0.0)));
        assert!(c.contains_xz(&Location::new(0.0, -100.0, 0.0)));
    }
}
