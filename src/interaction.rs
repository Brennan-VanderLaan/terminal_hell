//! Decentralized interaction framework. Any module (archetype, content
//! pack, rust plugin) registers rules into the `InteractionBook` keyed
//! on tag pairs. When two tagged things meet — a burning projectile
//! lands on a blood pool, a toxic corpse explodes next to a gas cloud,
//! a necromantic stain touches a corpse — the book is queried for
//! matching rules and the reactions fire.
//!
//! A `DirtyTracker` batches dirty tiles / entities each tick so cold
//! (no-recent-change) parts of the arena don't get re-scanned every
//! frame. Chained reactions are bounded by `MAX_CHAIN_HOPS = 8`; beyond
//! that the chain terminates to prevent runaway cascades.
//!
//! `Reaction::Custom(fn)` is the Rust escape hatch for effects too
//! complicated for declarative rules (the Corpse-Eater raise logic,
//! multi-stage shock arcs, etc).

use crate::tag::{Tag, TagPair, TagSet};
use std::collections::{HashMap, HashSet};

/// Hard cap on chain length per tick. Snap explosions, flame spread,
/// shock arcs all stop after this many hops — keeps emergent loops
/// bounded and deterministic for netcode replay.
pub const MAX_CHAIN_HOPS: u8 = 8;

/// Declarative reactions authorable from TOML. The Custom variant
/// carries a function pointer for Rust-coded effects; the engine
/// invokes it with a context struct giving access to mutable world
/// state.
#[derive(Clone, Debug)]
pub enum Reaction {
    /// Paint a substance onto the ground at the reaction site.
    SpawnSubstance {
        id: &'static str,
        radius: f32,
        state: u8,
    },
    /// Ignite nearby tiles + entities for `ttl` seconds at `dps`.
    Ignite {
        radius: f32,
        damage_per_sec: f32,
        ttl: f32,
    },
    /// Instantaneous explosion: damage ring + scorch + corpse chain hit.
    Explode {
        radius: f32,
        damage: i32,
    },
    /// Shock arc: damage + chain to N nearby conductive things.
    Shock {
        radius: f32,
        damage: i32,
        chain_count: u8,
    },
    /// Spawn an enemy archetype (by content id) at the reaction site.
    /// Used for necromantic raise → Corpse-Eater, etc.
    SpawnEnemy {
        archetype_id: &'static str,
        count: u32,
    },
    /// Remove whatever triggered the reaction (corpse consumed, decal
    /// neutralized). Caller semantics — what's "the trigger" depends on
    /// the event.
    ConsumeTrigger,
    /// Rust-coded escape hatch. The handler is named by interned tag;
    /// `game.rs` holds the dispatch table mapping handler name to a
    /// real function that reaches into mutable world state. Keeps this
    /// module free of any `Game` dependency.
    Custom { handler: Tag },
}

/// A rule in the interaction book. `chance` gates firing — 1.0 always,
/// 0.0 never; 0.002 is "very rare" (the intended Corpse-Eater spawn
/// chance).
#[derive(Clone, Debug)]
pub struct InteractionRule {
    pub reaction: Reaction,
    pub chance: f32,
}

impl InteractionRule {
    pub fn always(reaction: Reaction) -> Self {
        Self { reaction, chance: 1.0 }
    }
    pub fn with_chance(reaction: Reaction, chance: f32) -> Self {
        Self { reaction, chance }
    }
}

/// The interaction book. Append-from-anywhere; rules are keyed on a
/// sorted `TagPair`, and single-tag event rules (fired by events like
/// OnDeath independent of a second participant) are keyed on `Tag`.
#[derive(Default)]
pub struct InteractionBook {
    pair_rules: HashMap<TagPair, Vec<InteractionRule>>,
    event_rules: HashMap<(Tag, Event), Vec<InteractionRule>>,
}

/// Events a body/substance can react to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Event {
    OnDeath,
    OnHit,
    OnBurnTick,
    OnSteppedOn,
    OnProximity,
}

impl InteractionBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the rules shipping with the core engine. Content packs
    /// and plugins can append more afterwards.
    pub fn with_builtins() -> Self {
        let mut book = Self::new();

        // Rare emergent: a `necromantic`-tagged body dying sometimes
        // raises a Corpse-Eater boss. Keeps the spawn rate ~5% per
        // qualifying death (miniboss is the only stock `necromantic`
        // archetype, so this is very rare in practice).
        book.on_event(
            Tag::new("necromantic"),
            Event::OnDeath,
            InteractionRule::with_chance(
                Reaction::SpawnEnemy {
                    archetype_id: "eater",
                    count: 1,
                },
                0.05,
            ),
        );

        // Volatile bodies: an `explosive`-tagged body dying triggers
        // the Explode reaction on top of its named body_on_death.
        // Illustrates tag-driven composition — a body can be both
        // volatile AND bleed, etc.
        book.on_event(
            Tag::new("volatile"),
            Event::OnDeath,
            InteractionRule::always(Reaction::Explode {
                radius: 3.0,
                damage: 18,
            }),
        );

        book
    }

    /// Register a tag-pair rule. Both (`a`, `b`) and (`b`, `a`)
    /// contacts will fire it.
    pub fn on_contact(&mut self, a: Tag, b: Tag, rule: InteractionRule) {
        self.pair_rules
            .entry(TagPair::new(a, b))
            .or_default()
            .push(rule);
    }

    /// Register a single-tag event rule. Fires when a thing with `tag`
    /// experiences `event`.
    pub fn on_event(&mut self, tag: Tag, event: Event, rule: InteractionRule) {
        self.event_rules.entry((tag, event)).or_default().push(rule);
    }

    pub fn contact_rules(&self, a: Tag, b: Tag) -> &[InteractionRule] {
        self.pair_rules
            .get(&TagPair::new(a, b))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn event_rules(&self, tag: Tag, event: Event) -> &[InteractionRule] {
        self.event_rules
            .get(&(tag, event))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Collect all contact rules that match the intersection of two
    /// tag sets. Every `(a ∈ tags_a, b ∈ tags_b)` pair is checked;
    /// duplicates not deduped (each match is a distinct triggering
    /// axis and may be intended to stack).
    pub fn collect_contact_rules<'s>(
        &'s self,
        tags_a: &TagSet,
        tags_b: &TagSet,
        out: &mut Vec<&'s InteractionRule>,
    ) {
        for a in tags_a.iter() {
            for b in tags_b.iter() {
                for r in self.contact_rules(a, b) {
                    out.push(r);
                }
            }
        }
    }
}


/// Dirty-set tracker. Cold tiles / entities (no recent modification,
/// no active timer) skip re-scanning each tick, which keeps a long run
/// with thousands of inert corpses and decals cheap.
#[derive(Default)]
pub struct DirtyTracker {
    /// Tiles marked for interaction re-check this tick.
    pub tiles: HashSet<(i32, i32)>,
    /// Entities (corpses, enemies, etc) marked for re-check.
    pub entities: HashSet<u32>,
    /// Chain depth counter — reset each frame, incremented per hop.
    pub chain_depth: u8,
}

impl DirtyTracker {
    pub fn dirty_tile(&mut self, x: i32, y: i32) {
        self.tiles.insert((x, y));
    }
    pub fn dirty_entity(&mut self, id: u32) {
        self.entities.insert(id);
    }
    /// Mark the tile and its 4-neighbors dirty for next tick — used
    /// when a new substance paints over an existing one, or when a
    /// body effect detonates. Callers can hop the chain by dirtying
    /// further if needed.
    pub fn dirty_cross(&mut self, x: i32, y: i32) {
        self.tiles.insert((x, y));
        self.tiles.insert((x + 1, y));
        self.tiles.insert((x - 1, y));
        self.tiles.insert((x, y + 1));
        self.tiles.insert((x, y - 1));
    }
    pub fn can_chain(&self) -> bool {
        self.chain_depth < MAX_CHAIN_HOPS
    }
    pub fn note_hop(&mut self) {
        self.chain_depth = self.chain_depth.saturating_add(1);
    }
    pub fn reset_chain(&mut self) {
        self.chain_depth = 0;
    }
    pub fn reset_frame(&mut self) {
        self.tiles.clear();
        self.entities.clear();
        self.chain_depth = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_rule_symmetric() {
        let mut book = InteractionBook::new();
        let a = Tag::new("flammable");
        let b = Tag::new("on_fire");
        book.on_contact(
            a,
            b,
            InteractionRule::always(Reaction::Ignite {
                radius: 1.0,
                damage_per_sec: 4.0,
                ttl: 2.0,
            }),
        );
        assert_eq!(book.contact_rules(a, b).len(), 1);
        assert_eq!(book.contact_rules(b, a).len(), 1);
    }

    #[test]
    fn chain_cap_respected() {
        let mut d = DirtyTracker::default();
        for _ in 0..MAX_CHAIN_HOPS {
            assert!(d.can_chain());
            d.note_hop();
        }
        assert!(!d.can_chain());
    }
}
