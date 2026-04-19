//! Universal effect primitives.
//!
//! Every primitive is a small, well-defined modifier that slots into weapons
//! today and will extend to movement verbs, enemy modifiers, hazards, and
//! terrain effects in later milestones. See `spec.md` §6.
//!
//! A weapon carries a `Vec<Primitive>`; at fire time, those primitives are
//! copied onto the projectile. Projectile + game-state event handlers
//! consult the primitive list to apply behavior.

use crate::fb::Pixel;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    /// Enemy hit applies a burning status for a short duration.
    Ignite,
    /// Wall hit damages the tile and a ring of adjacent tiles.
    Breach,
    /// Projectile reflects off walls up to N bounces instead of despawning.
    Ricochet,
    /// Enemy hit arcs to the nearest other enemies for splash damage.
    Chain,
    /// Projectile passes through enemies up to N times before despawning.
    Pierce,
    /// First shot fired after a kill deals bonus damage.
    Overdrive,
}

impl Primitive {
    pub fn glyph(self) -> char {
        match self {
            Primitive::Ignite => 'I',
            Primitive::Breach => 'B',
            Primitive::Ricochet => 'R',
            Primitive::Chain => 'C',
            Primitive::Pierce => 'P',
            Primitive::Overdrive => 'O',
        }
    }

    /// Tint color used for pickup halos and in-world visual cues.
    pub fn color(self) -> Pixel {
        match self {
            Primitive::Ignite => Pixel::rgb(255, 130, 40),
            Primitive::Breach => Pixel::rgb(255, 220, 80),
            Primitive::Ricochet => Pixel::rgb(120, 200, 255),
            Primitive::Chain => Pixel::rgb(180, 140, 255),
            Primitive::Pierce => Pixel::rgb(230, 230, 230),
            Primitive::Overdrive => Pixel::rgb(255, 80, 200),
        }
    }
}

/// Drop-rarity tier. Controls the number of primitive slots a weapon rolls
/// with. Matches §6.2.2 of the spec minus the Elite and Carcosa tiers which
/// arrive with M7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
}

impl Rarity {
    pub fn slot_count(self) -> usize {
        match self {
            Rarity::Common => 1,
            Rarity::Uncommon => 2,
            Rarity::Rare => 3,
        }
    }

    pub fn color(self) -> Pixel {
        match self {
            Rarity::Common => Pixel::rgb(200, 200, 220),
            Rarity::Uncommon => Pixel::rgb(120, 200, 255),
            Rarity::Rare => Pixel::rgb(255, 200, 80),
        }
    }
}

/// Burning status applied by the Ignite primitive. Ticked by the host each
/// sim step. `accum` is fractional damage carried between ticks so that a
/// low DPS still lands integer hits on schedule.
#[derive(Clone, Copy, Debug)]
pub struct BurnStatus {
    pub damage_per_sec: f32,
    pub ttl: f32,
    pub accum: f32,
}
