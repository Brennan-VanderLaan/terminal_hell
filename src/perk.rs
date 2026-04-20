//! Run-persistent passive modifiers. Perks drop from miniboss kills
//! and at wave milestones; picking one up stacks it into the
//! player's perk vector and its modifier hooks fire for the rest of
//! the run.
//!
//! Each perk is a simple enum variant — zero state beyond "I have
//! it." Per-player counters that some perks need (Rampage kill
//! chain, Adrenaline timer, etc) live on `Player`, not `Perk`, so
//! the enum stays cheap and serializable.

use crate::fb::Pixel;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Perk {
    /// +4 HP on every enemy kill.
    CorpsePulse,
    /// Baseline sanity regen +1.5 / s (stacks with existing regen).
    IronWill,
    /// +25% fire rate on every weapon.
    QuickHands,
    /// +50 max armor. Each new wave starts with +10 armor.
    Survivor,
    /// 3 kills within 4s → +30% outgoing damage for 8s.
    Rampage,
    /// When HP < 25% → 40% incoming damage resistance.
    LastStand,
    /// Standing on a blood pool regens +2 HP/s.
    Bloodhound,
    /// 2× chance of consumable drops on kills + miniboss loot.
    Butcher,
    /// Carcosa terrain drains sanity 60% slower.
    GloomShroud,
    /// Taking damage → 0.6s of +60% movement speed.
    Adrenaline,
}

impl Perk {
    pub fn all() -> &'static [Perk] {
        &[
            Perk::CorpsePulse,
            Perk::IronWill,
            Perk::QuickHands,
            Perk::Survivor,
            Perk::Rampage,
            Perk::LastStand,
            Perk::Bloodhound,
            Perk::Butcher,
            Perk::GloomShroud,
            Perk::Adrenaline,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Perk::CorpsePulse => "Corpse Pulse",
            Perk::IronWill => "Iron Will",
            Perk::QuickHands => "Quick Hands",
            Perk::Survivor => "Survivor",
            Perk::Rampage => "Rampage",
            Perk::LastStand => "Last Stand",
            Perk::Bloodhound => "Bloodhound",
            Perk::Butcher => "Butcher",
            Perk::GloomShroud => "Gloom Shroud",
            Perk::Adrenaline => "Adrenaline",
        }
    }

    /// Single-letter glyph for HUD pickup + strip rendering.
    pub fn glyph(self) -> char {
        match self {
            Perk::CorpsePulse => '+',
            Perk::IronWill => 'I',
            Perk::QuickHands => 'Q',
            Perk::Survivor => 'S',
            Perk::Rampage => 'R',
            Perk::LastStand => 'L',
            Perk::Bloodhound => 'B',
            Perk::Butcher => 'U',
            Perk::GloomShroud => 'G',
            Perk::Adrenaline => 'A',
        }
    }

    /// Signature color for HUD + pickup halo.
    pub fn color(self) -> Pixel {
        match self {
            Perk::CorpsePulse => Pixel::rgb(255, 100, 120),
            Perk::IronWill => Pixel::rgb(140, 200, 255),
            Perk::QuickHands => Pixel::rgb(255, 220, 100),
            Perk::Survivor => Pixel::rgb(200, 200, 240),
            Perk::Rampage => Pixel::rgb(255, 80, 40),
            Perk::LastStand => Pixel::rgb(200, 60, 180),
            Perk::Bloodhound => Pixel::rgb(180, 30, 60),
            Perk::Butcher => Pixel::rgb(255, 140, 40),
            Perk::GloomShroud => Pixel::rgb(120, 100, 180),
            Perk::Adrenaline => Pixel::rgb(80, 255, 180),
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "corpse_pulse" => Perk::CorpsePulse,
            "iron_will" => Perk::IronWill,
            "quick_hands" => Perk::QuickHands,
            "survivor" => Perk::Survivor,
            "rampage" => Perk::Rampage,
            "last_stand" => Perk::LastStand,
            "bloodhound" => Perk::Bloodhound,
            "butcher" => Perk::Butcher,
            "gloom_shroud" => Perk::GloomShroud,
            "adrenaline" => Perk::Adrenaline,
            _ => return None,
        })
    }

    /// Short flavor line rendered at pickup.
    #[allow(dead_code)]
    pub fn flavor(self) -> &'static str {
        match self {
            Perk::CorpsePulse => "+4 HP per kill",
            Perk::IronWill => "+1.5 sanity regen",
            Perk::QuickHands => "+25% fire rate",
            Perk::Survivor => "+50 max armor",
            Perk::Rampage => "kill chains buff damage",
            Perk::LastStand => "low HP resists damage",
            Perk::Bloodhound => "blood pools heal you",
            Perk::Butcher => "2× loot drops",
            Perk::GloomShroud => "Carcosa drains slower",
            Perk::Adrenaline => "hits grant burst speed",
        }
    }
}

/// Pick a random perk the player doesn't already have. Returns None
/// if they have every perk.
pub fn roll_new_perk(owned: &[Perk], rng: &mut rand::rngs::SmallRng) -> Option<Perk> {
    use rand::seq::SliceRandom;
    let available: Vec<Perk> = Perk::all()
        .iter()
        .copied()
        .filter(|p| !owned.contains(p))
        .collect();
    available.choose(rng).copied()
}
