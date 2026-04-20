//! Body-on-death reactions: the bridge between an archetype's TOML
//! `body_on_death = "..."` declaration and the actual in-world effect
//! that fires when it dies.
//!
//! A `ReactionRegistry` holds named built-in reactions keyed by strings
//! (that's what archetype TOML references). Custom Rust-coded reactions
//! register themselves into this table at startup; the Corpse-Eater
//! raise logic in the next task is one such custom entry.
//!
//! When an enemy dies, `Game` resolves its archetype's body_on_death
//! name to a reaction, applies the reaction at the death site, and
//! broadcasts a `BodyReaction` event so clients replay identically
//! (same seed → same RNG-driven particles / damage / spawned entities).

use crate::interaction::Reaction;
use crate::tag::Tag;
use std::collections::HashMap;

/// Name → `Reaction` dispatch table. The name is the string archetypes
/// and interaction rules reference. Register additions at startup.
#[derive(Default)]
pub struct ReactionRegistry {
    entries: HashMap<String, Reaction>,
}

impl ReactionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, reaction: Reaction) {
        self.entries.insert(name.into(), reaction);
    }

    pub fn get(&self, name: &str) -> Option<&Reaction> {
        self.entries.get(name)
    }

    /// Install every built-in reaction shipped in the core engine.
    /// Content packs + Rust plugins add more via `register` after this.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();

        // Bleeders — spawn a blood pool on death. Radii sized to
        // roughly match the dying body's footprint plus a small
        // splatter ring; adjacent kills still overlap into wet-room
        // territory but a single rusher kill no longer leaves a
        // pool 3× the enemy's silhouette.
        r.register(
            "spawn_blood",
            Reaction::SpawnSubstance {
                id: "blood_pool",
                radius: 3.5,
                state: 10,
            },
        );
        r.register(
            "spawn_big_blood",
            Reaction::SpawnSubstance {
                id: "blood_pool",
                radius: 6.0,
                state: 0,
            },
        );

        // Flood ichor — green infection puddle. Sized to enemy
        // footprint; Flood-density still reads green because every
        // Flood kill paints, not because each pool is huge.
        r.register(
            "spawn_ichor",
            Reaction::SpawnSubstance {
                id: "flood_ichor",
                radius: 4.0,
                state: 10,
            },
        );
        r.register(
            "spawn_big_ichor",
            Reaction::SpawnSubstance {
                id: "flood_ichor",
                radius: 7.0,
                state: 0,
            },
        );

        // Mech oil — dark iridescent puddle for turrets + sentinels +
        // juggernauts. Slightly bigger than blood since mechs are
        // larger frames + the slick is the cinematic "uh-oh, fire
        // cascade waiting to happen."
        r.register(
            "spawn_oil",
            Reaction::SpawnSubstance {
                id: "oil_pool",
                radius: 5.0,
                state: 20,
            },
        );

        // Bone dust — undead shatter residue. Revenants + Eaters
        // leave a dry ivory scatter sized to body footprint.
        r.register(
            "spawn_bone_dust",
            Reaction::SpawnSubstance {
                id: "bone_dust",
                radius: 3.5,
                state: 30,
            },
        );

        // Ember scatter — fire-bug residue. Pairs with ignite_aura
        // on archetypes that also spawn a burn ring.
        r.register(
            "spawn_embers",
            Reaction::SpawnSubstance {
                id: "ember_scatter",
                radius: 4.5,
                state: 10,
            },
        );

        // Psychic residue — arcane death for Orbs, Phasers, cultists.
        // Violet bloom ties unnatural deaths into the Carcosa palette.
        r.register(
            "spawn_psychic_residue",
            Reaction::SpawnSubstance {
                id: "psychic_residue",
                radius: 4.0,
                state: 15,
            },
        );

        // Exploders — scorch + damage ring on death.
        r.register(
            "explode_small",
            Reaction::Explode {
                radius: 4.0,
                damage: 25,
            },
        );
        r.register(
            "explode_big",
            Reaction::Explode {
                radius: 7.0,
                damage: 60,
            },
        );

        // Burners — leave a burning ring behind.
        r.register(
            "ignite_aura",
            Reaction::Ignite {
                radius: 3.0,
                damage_per_sec: 10.0,
                ttl: 3.5,
            },
        );

        // Shockers — chain damage.
        r.register(
            "shock_chain",
            Reaction::Shock {
                radius: 3.5,
                damage: 18,
                chain_count: 3,
            },
        );

        // Toxic — drop a pool of uranium slag (our glowing test substance).
        r.register(
            "vent_uranium",
            Reaction::SpawnSubstance {
                id: "uranium_slag",
                radius: 2.5,
                state: 180,
            },
        );

        // Custom — the Corpse-Eater raise, registered by name here so
        // content TOML can reference `body_on_death = "raise_eater"`.
        // The actual handler is wired in game.rs's custom dispatch.
        r.register(
            "raise_eater",
            Reaction::Custom { handler: Tag::new("raise_eater") },
        );

        // Splitter fragmentation: on death, spawn several Swarmlings
        // at the corpse location. Chains nicely with other Splitters
        // if their swarmlings land near another split-ready target.
        r.register(
            "spawn_swarmlings",
            Reaction::Custom { handler: Tag::new("spawn_swarmlings") },
        );

        // FloodCarrier death — a cloud of Floodlings vents out of the
        // bursting sac. Amplifies the Flood faction via chain
        // conversions.
        r.register(
            "burst_floodlings",
            Reaction::Custom { handler: Tag::new("burst_floodlings") },
        );

        r
    }
}
