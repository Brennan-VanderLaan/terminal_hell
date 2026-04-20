//! Behavior framework — composable movement / targeting / attack
//! routines per archetype. Same pattern as gore profiles + substance
//! tags: an archetype is a `BehaviorSet { targeter, mover, attacker }`
//! picked from a small Rust-implemented vocabulary.
//!
//! New behaviors land as new variants in these enums plus the handler
//! code that drives them. Archetype TOML references them by name.
//!
//! The `DirectorOverride` field on every `Enemy` sits above the
//! behavior layer: when a Director (dead player) issues a move/attack
//! command, the enemy reads the override before falling back to its
//! intrinsic `Targeter`. Empty today — scaffolding for the RTS-style
//! control path.

use crate::enemy::Archetype;
use crate::tag::{Tag, TagSet};

/// Which entity this archetype wants to hurt. Resolves to a world
/// position each tick; the `Mover` takes it from there.
#[derive(Clone, Debug)]
pub enum Targeter {
    /// Classic behavior: closest living player, Hastur-mark priority
    /// enforced by the Game-layer marked_player_id override.
    NearestPlayer,
    /// Corpse-seeker that flips to players once `hunger_threshold`
    /// corpses have been eaten. Used by the Eater.
    NearestCorpse { hunger_threshold: u8 },
    /// Stationary defender — picks no target, lets its attacker fire
    /// at whatever wanders into range. Placeholder for turret-like
    /// future archetypes.
    #[allow(dead_code)]
    Stationary,
}

/// How the enemy moves toward its target.
#[derive(Clone, Debug)]
pub enum Mover {
    /// Straight-line chase respecting wall collision. What every
    /// existing enemy does today.
    Direct,
    /// A* pathfinding around walls. Recomputed every `recompute_ms`
    /// or when a wall ahead changes. Used by Breachers so they can
    /// actually route to a player instead of getting stuck.
    AStar { recompute_ms: u32 },
    /// Prefer tiles with cover on the target-facing side — stays out
    /// of direct LoS when possible while still advancing. Used by
    /// Rocketeers and future STALKER-style ambushers.
    Sneak {
        /// Multiplier on speed when in cover (0..2 typical).
        cover_speed_bias: f32,
    },
    /// Charge mover — winds up at distance, then sprints. Existing
    /// Charger behavior expressed explicitly so content can swap it.
    #[allow(dead_code)]
    Charge,
    /// Ignore wall collision entirely. For future flying / phasing
    /// archetypes.
    #[allow(dead_code)]
    Hover,
}

/// How the enemy delivers damage.
#[derive(Clone, Debug)]
pub enum Attacker {
    /// Touch damage at contact reach — existing melee.
    Melee,
    /// Hitscan shot with a windup tell — existing Marksman / Revenant.
    Hitscan { range: f32, tell: f32 },
    /// Fires a Rocket entity that flies through the world and
    /// explodes on impact. Longer cooldown + tell than hitscan.
    Rocket {
        range: f32,
        rocket_speed: f32,
        cooldown: f32,
        tell: f32,
    },
    /// Smashes through walls + hits players on contact. When the
    /// path toward the target is wall-blocked, the enemy attacks the
    /// wall instead of stalling.
    Breach {
        wall_damage: u8,
        swing_cooldown: f32,
        tell: f32,
    },
}

#[derive(Clone, Debug)]
pub struct BehaviorSet {
    pub targeter: Targeter,
    pub mover: Mover,
    pub attacker: Attacker,
    /// Additional trait tags for the interaction framework (e.g.,
    /// "silent", "armored") — keyed the same way as gore/substance
    /// tags so rules can key on them.
    pub tags: TagSet,
}

impl BehaviorSet {
    pub fn direct_melee() -> Self {
        Self {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::default(),
        }
    }
}

/// Map each archetype to its baseline `BehaviorSet`. New archetypes
/// slot their combo in here; content TOML can later override via a
/// per-archetype `behavior = "..."` reference when we wire named
/// behavior presets.
pub fn behaviors_for(archetype: Archetype) -> BehaviorSet {
    match archetype {
        Archetype::Rusher => BehaviorSet::direct_melee(),
        Archetype::Pinkie => BehaviorSet::direct_melee(),
        Archetype::Charger => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["charge"]),
        },
        Archetype::Revenant => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Hitscan { range: 40.0, tell: 0.45 },
            tags: TagSet::default(),
        },
        Archetype::Marksman => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Hitscan { range: 40.0, tell: 0.45 },
            tags: TagSet::from_strs(&["tactical"]),
        },
        Archetype::Pmc => BehaviorSet::direct_melee(),
        Archetype::Swarmling => BehaviorSet::direct_melee(),
        Archetype::Orb => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["floating"]),
        },
        Archetype::Miniboss => BehaviorSet::direct_melee(),
        Archetype::Eater => BehaviorSet {
            targeter: Targeter::NearestCorpse { hunger_threshold: 5 },
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["abomination", "hungry"]),
        },
        Archetype::Breacher => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::AStar { recompute_ms: 650 },
            attacker: Attacker::Breach {
                wall_damage: 3,
                swing_cooldown: 1.4,
                tell: 0.55,
            },
            tags: TagSet::from_strs(&["biological", "heavy", "breacher"]),
        },
        Archetype::Rocketeer => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Sneak { cover_speed_bias: 1.35 },
            attacker: Attacker::Rocket {
                range: 60.0,
                rocket_speed: 55.0,
                cooldown: 3.5,
                tell: 0.7,
            },
            tags: TagSet::from_strs(&["tactical", "rocketeer"]),
        },
        Archetype::Leaper => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["biological", "agile", "leaper"]),
        },
        // Phaser: direct pursuit in standard gameplay, but the
        // game-layer phase-hop override (see game.rs) overrides
        // position in discrete jumps. Behavior tags mark it as
        // arcane/agile so interaction rules can key off "phaser."
        Archetype::Phaser => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["arcane", "agile", "phaser"]),
        },
        Archetype::Sapper => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Breach {
                wall_damage: 8,
                swing_cooldown: 0.3,
                tell: 0.3,
            },
            tags: TagSet::from_strs(&["biological", "volatile", "breacher"]),
        },
        Archetype::Juggernaut => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Breach {
                wall_damage: 4,
                swing_cooldown: 1.1,
                tell: 0.4,
            },
            tags: TagSet::from_strs(&["mechanical", "heavy", "armored", "breacher"]),
        },
        Archetype::Howler => BehaviorSet {
            targeter: Targeter::Stationary,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["cultist", "support", "noisy"]),
        },
        Archetype::Sentinel => BehaviorSet {
            targeter: Targeter::Stationary,
            mover: Mover::Direct,
            attacker: Attacker::Hitscan { range: 48.0, tell: 0.55 },
            tags: TagSet::from_strs(&["mechanical", "turret"]),
        },
        Archetype::Splitter => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["biological", "fragmenting"]),
        },
        Archetype::Killa => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Hitscan { range: 60.0, tell: 0.18 },
            tags: TagSet::from_strs(&["biological", "armored", "heavy", "boss"]),
        },
        Archetype::Zergling => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["biological", "swarm", "agile"]),
        },
        Archetype::Floodling => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["parasite", "infectious"]),
        },
        Archetype::Flood => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["flood", "infected", "feral"]),
        },
        Archetype::FloodCarrier => BehaviorSet {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            attacker: Attacker::Melee,
            tags: TagSet::from_strs(&["flood", "carrier", "volatile"]),
        },
        Archetype::PlayerTurret => BehaviorSet {
            targeter: Targeter::Stationary,
            mover: Mover::Direct,
            attacker: Attacker::Hitscan { range: 28.0, tell: 0.3 },
            tags: TagSet::from_strs(&["mechanical", "ally", "turret"]),
        },
    }
}

/// Verbs a Director / scripted override can issue to a single enemy.
/// Empty variant today's control flow; scaffolding for the RTS-style
/// command path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandVerb {
    /// Walk to a fixed world-space point, ignoring normal targeting.
    MoveTo,
    /// Attack at a fixed world-space point (AoE lob / suppress).
    AttackAt,
    /// Chase + attack a specific player entity id.
    AttackEntity,
    /// Hold position — ignore player movement.
    Guard,
}

/// A command imposed on an enemy from outside its baseline behavior.
/// Directors fill this when the RTS UI lands; scripted waves can use
/// it for set-piece sequences. Cleared when the enemy reaches the
/// target / kills the attack target / TTL expires.
#[derive(Clone, Copy, Debug)]
pub struct DirectorOverride {
    pub verb: CommandVerb,
    pub point: (f32, f32),
    pub entity: Option<u32>,
    /// Seconds left before the override is dropped and the enemy
    /// falls back to its baseline Targeter.
    pub ttl: f32,
}

/// Helper — convenience tag queries.
#[allow(dead_code)]
pub fn has_tag(set: &BehaviorSet, tag: Tag) -> bool {
    set.tags.has(tag)
}
