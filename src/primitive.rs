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
    /// Enemy hit leaves an acid pool on the ground that ticks DoT.
    Acid,
    /// Enemy hit slows target. Stacking Cryo stacks the slow; 3
    /// stacks shatter for a big crit.
    Cryo,
    /// On-hit status effects (burn, slow) spread to adjacent enemies.
    Contagion,
    /// Hit point briefly pulls nearby enemies in — soft crowd
    /// control for flanker control.
    GravityWell,
    /// On-kill restores a chunk of sanity — rewards clean kills in
    /// the Carcosa spiral.
    Siphon,
    /// Ignores armored/shielded tags, dealing bonus damage to
    /// armored enemies.
    ShieldBreak,
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
            Primitive::Acid => 'A',
            Primitive::Cryo => 'F', // Freeze
            Primitive::Contagion => 'T', // Transmit
            Primitive::GravityWell => 'V', // Vortex
            Primitive::Siphon => 'S',
            Primitive::ShieldBreak => 'X',
        }
    }

    /// Snake-case content-id → enum. Used by TOML parsing of
    /// `signature_primitives` lists on archetypes.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "ignite" => Primitive::Ignite,
            "breach" => Primitive::Breach,
            "ricochet" => Primitive::Ricochet,
            "chain" => Primitive::Chain,
            "pierce" => Primitive::Pierce,
            "overdrive" => Primitive::Overdrive,
            "acid" => Primitive::Acid,
            "cryo" => Primitive::Cryo,
            "contagion" => Primitive::Contagion,
            "gravity_well" => Primitive::GravityWell,
            "siphon" => Primitive::Siphon,
            "shield_break" => Primitive::ShieldBreak,
            _ => return None,
        })
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
            Primitive::Acid => Pixel::rgb(120, 255, 100),
            Primitive::Cryo => Pixel::rgb(140, 220, 255),
            Primitive::Contagion => Pixel::rgb(200, 255, 120),
            Primitive::GravityWell => Pixel::rgb(80, 60, 180),
            Primitive::Siphon => Pixel::rgb(255, 200, 220),
            Primitive::ShieldBreak => Pixel::rgb(255, 100, 80),
        }
    }
}

/// Cryo slow status applied by the Cryo primitive. Each stack slows
/// the enemy by a further 15% (cap 3 stacks = fully frozen).
#[derive(Clone, Copy, Debug, Default)]
pub struct FrostStatus {
    pub stacks: u8,
    pub ttl: f32,
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
