//! Data-driven ground substances. Blood pools, scorch marks, uranium
//! slag, toxic spills, healstuff — all defined in TOML under
//! `content/core/substances/`, looked up at runtime by a compact
//! numeric id packed into each `Ground::Substance` tile.
//!
//! A substance carries:
//!   * `tags`     — property handles for the interaction framework
//!   * `palette`  — ground-render colors (fresh + dried + optional jitter)
//!   * `mip_hint` — per-mip intensity ladder; renderer dispatches on style
//!   * `bloom`    — optional light bleed for glowing substances
//!
//! The registry is a fixed-size table built at content load. Ids are
//! `u16` indices into the table so `Ground::Substance { id, state }` is
//! 3 bytes per tile — same footprint as the old hardcoded variants.

use crate::fb::Pixel;
use crate::mip_hint::{Bloom, MipHint};
use crate::tag::{Tag, TagSet};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;

/// Numeric handle into the substance registry. Stable for the life of
/// the process; 0 is reserved for "no substance / floor".
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SubstanceId(pub u16);

impl SubstanceId {
    pub const NONE: SubstanceId = SubstanceId(0);
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// Gameplay behavior attached to a substance. Populated post-load
/// by `apply_default_behaviors` and queried by the ambient emitter,
/// standing-hazard, and fire-propagation passes. Zero-initialized
/// by default so substances without explicit behavior (e.g.
/// bone_dust) cost nothing at tick time.
#[derive(Clone, Copy, Debug, Default)]
pub struct SubstanceBehavior {
    /// Damage dealt per second to any entity whose center is in
    /// this substance's tile. Acid, fire, uranium slag.
    pub damage_per_sec: f32,
    /// Sanity drained per second from players standing in this
    /// substance. Carcosa-adjacent; psychic residue + uranium +
    /// future toxic substances.
    pub sanity_drain_per_sec: f32,
    /// Expected particle emissions per tile per second. Treat as
    /// a Poisson rate; ambient pass rolls `rate * dt` per visible
    /// tile. Zero = no ambient particles.
    pub emit_rate: f32,
    /// Up to three palette entries for ambient emissions. Picked
    /// uniformly per emission. RGB packed as `Pixel`.
    pub emit_palette: [Pixel; 3],
    /// How many of `emit_palette` entries are populated.
    pub emit_palette_len: u8,
    /// Vertical drift of ambient particles. Negative = rising
    /// (smoke/steam), positive = falling (ash/embers).
    pub emit_drift_y: f32,
    /// Max horizontal drift magnitude (signed by per-particle rng).
    pub emit_drift_x: f32,
    /// Particle TTL range (secs) for ambient emissions.
    pub emit_ttl_min: f32,
    pub emit_ttl_max: f32,
    /// Particle size in pixels for ambient emissions.
    pub emit_size: u8,
    /// When true, adjacent flammable substance tiles have a chance
    /// to ignite into `fire` when this substance is present.
    /// Applies to the fire substance itself.
    pub ignites_neighbors: bool,
    /// When true, this substance is catchable by adjacent fire —
    /// converts to fire on contact. Blood, oil, ember, ichor.
    pub flammable: bool,
    /// Optional lifetime. Substance decays its state field over
    /// this many seconds, then the tile is reset to Floor. Used
    /// by `fire` so burning tiles eventually go out.
    pub ttl_secs: Option<f32>,
    /// Burn status dot applied on entry (damage-per-sec, ttl-secs).
    /// Stacks with the standing damage. Fire only, for now.
    pub applies_burn: Option<(f32, f32)>,
}

/// Runtime substance definition, resolved from `SubstanceToml`.
#[derive(Clone, Debug)]
pub struct SubstanceDef {
    pub id: SubstanceId,
    pub name: &'static str,
    pub tags: TagSet,
    pub fresh: Pixel,
    pub dried: Pixel,
    /// How state 0..=255 maps to the palette. `state` is typically
    /// "freshness" (0 fresh, 255 dry) or "intensity" (0 faint, 255
    /// strong) — meaning is substance-specific.
    pub state_curve: StateCurve,
    pub mip_hint: MipHint,
    pub bloom: Option<Bloom>,
    /// Small jitter between sibling tiles so wide pools read as
    /// textured rather than flat rectangles. 0 = flat.
    pub jitter: u8,
    /// Gameplay behavior — damage, emitters, flammability, TTL.
    /// Populated by `SubstanceRegistry::apply_default_behaviors`
    /// at content load after all substances are inserted.
    pub behavior: SubstanceBehavior,
}

#[derive(Clone, Copy, Debug)]
pub enum StateCurve {
    /// state 0 = fresh, 255 = dried. Linear lerp between the two
    /// palette entries.
    FreshToDried,
    /// state 0 = faint, 255 = intense. Lerp from `dried` (dark) up to
    /// `fresh` (bright).
    FaintToIntense,
}

impl StateCurve {
    pub fn resolve(self, state: u8, fresh: Pixel, dried: Pixel) -> Pixel {
        let t = state as f32 / 255.0;
        match self {
            StateCurve::FreshToDried => lerp_pixel(fresh, dried, t),
            StateCurve::FaintToIntense => lerp_pixel(dried, fresh, t),
        }
    }
}

fn lerp_pixel(a: Pixel, b: Pixel, t: f32) -> Pixel {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8
    };
    Pixel::rgb(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

/// Registry of all loaded substances. Indexed by `SubstanceId`.
#[derive(Default)]
pub struct SubstanceRegistry {
    defs: Vec<SubstanceDef>,
    by_name: HashMap<&'static str, SubstanceId>,
}

impl SubstanceRegistry {
    pub fn new() -> Self {
        let mut reg = Self::default();
        // Slot 0 is the reserved "no substance" sentinel.
        reg.defs.push(SubstanceDef {
            id: SubstanceId::NONE,
            name: "",
            tags: TagSet::default(),
            fresh: Pixel::BLACK,
            dried: Pixel::BLACK,
            state_curve: StateCurve::FreshToDried,
            mip_hint: MipHint::ground_default(),
            bloom: None,
            jitter: 0,
            behavior: SubstanceBehavior::default(),
        });
        reg
    }

    pub fn get(&self, id: SubstanceId) -> Option<&SubstanceDef> {
        self.defs.get(id.0 as usize)
    }

    pub fn by_name(&self, name: &str) -> Option<SubstanceId> {
        self.by_name.get(name).copied()
    }

    pub fn insert(&mut self, toml: SubstanceToml) -> Result<SubstanceId> {
        let next_id = self.defs.len();
        if next_id >= u16::MAX as usize {
            return Err(anyhow!("substance registry full"));
        }
        let id = SubstanceId(next_id as u16);
        let name: &'static str = Box::leak(toml.id.clone().into_boxed_str());
        let tags = TagSet {
            tags: toml.tags.iter().map(|s| Tag::new(s)).collect(),
        };
        let fresh = parse_hex(&toml.fresh)
            .with_context(|| format!("substance {} fresh color", toml.id))?;
        let dried = parse_hex(&toml.dried)
            .with_context(|| format!("substance {} dried color", toml.id))?;
        let bloom = match (&toml.bloom_color, toml.bloom_radius) {
            (Some(col), Some(r)) => Some(Bloom {
                color: parse_hex(col)
                    .with_context(|| format!("substance {} bloom color", toml.id))?,
                radius: r,
                falloff: toml.bloom_falloff.unwrap_or(0.8),
                strength: toml.bloom_strength.unwrap_or(0.6),
            }),
            _ => None,
        };
        let def = SubstanceDef {
            id,
            name,
            tags,
            fresh,
            dried,
            state_curve: match toml.state_curve.as_deref() {
                Some("faint_to_intense") => StateCurve::FaintToIntense,
                _ => StateCurve::FreshToDried,
            },
            mip_hint: MipHint::ground_default(),
            bloom,
            jitter: toml.jitter.unwrap_or(0),
            behavior: SubstanceBehavior::default(),
        };
        self.defs.push(def);
        self.by_name.insert(name, id);
        Ok(id)
    }

    /// Patch the behavior for a substance in-place after load.
    /// Pairs with `apply_default_behaviors` below.
    pub fn set_behavior(&mut self, id: SubstanceId, behavior: SubstanceBehavior) {
        if let Some(def) = self.defs.get_mut(id.0 as usize) {
            def.behavior = behavior;
        }
    }

    /// Assign gameplay behaviors to every substance we ship. Called
    /// by `ContentDb::load_core` after all substance TOMLs have
    /// loaded. Keyed by name so content packs can add substances
    /// without touching this table (they just won't have behavior
    /// until authored here).
    pub fn apply_default_behaviors(&mut self) {
        // Helper to make palette declaration compact.
        fn pal(a: (u8, u8, u8), b: (u8, u8, u8), c: (u8, u8, u8), n: u8) -> ([Pixel; 3], u8) {
            (
                [
                    Pixel::rgb(a.0, a.1, a.2),
                    Pixel::rgb(b.0, b.1, b.2),
                    Pixel::rgb(c.0, c.1, c.2),
                ],
                n,
            )
        }

        // Scorch — thin dark smoke drifting up slowly. No damage,
        // just atmospheric. Smoke rises (negative y-drift).
        if let Some(id) = self.by_name("scorch") {
            let (palette, n) = pal(
                (60, 55, 48),
                (90, 82, 70),
                (40, 36, 32),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                emit_rate: 0.45,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -6.0,
                emit_drift_x: 1.2,
                emit_ttl_min: 0.9,
                emit_ttl_max: 1.8,
                emit_size: 1,
                ..SubstanceBehavior::default()
            });
        }

        // Fresh blood — barely-there warm steam. Bloodhound perk
        // still heals on it; ambient makes the kill floor feel
        // recent rather than static.
        if let Some(id) = self.by_name("blood_pool") {
            let (palette, n) = pal(
                (120, 40, 40),
                (90, 24, 28),
                (60, 20, 28),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                emit_rate: 0.12,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -3.5,
                emit_drift_x: 0.7,
                emit_ttl_min: 0.6,
                emit_ttl_max: 1.1,
                emit_size: 1,
                flammable: true,
                ..SubstanceBehavior::default()
            });
        }

        // Acid — green bubbles + sickly fumes, plus DPS to anything
        // standing in it. Bumped emit rate so the bubbling reads as
        // active corrosion not an occasional pop.
        if let Some(id) = self.by_name("acid_pool") {
            let (palette, n) = pal(
                (130, 255, 90),
                (170, 255, 130),
                (90, 200, 60),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                damage_per_sec: 7.0,
                emit_rate: 3.2,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -10.0,
                emit_drift_x: 1.6,
                emit_ttl_min: 0.4,
                emit_ttl_max: 0.9,
                emit_size: 1,
                ..SubstanceBehavior::default()
            });
        }

        // Uranium slag — radioactive green motes + slow damage +
        // sanity drain for players. "Cross this puddle carefully."
        if let Some(id) = self.by_name("uranium_slag") {
            let (palette, n) = pal(
                (130, 255, 90),
                (190, 255, 150),
                (80, 220, 60),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                damage_per_sec: 4.0,
                sanity_drain_per_sec: 3.0,
                emit_rate: 1.1,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -5.0,
                emit_drift_x: 1.1,
                emit_ttl_min: 0.8,
                emit_ttl_max: 1.5,
                emit_size: 1,
                ..SubstanceBehavior::default()
            });
        }

        // Oil pool — iridescent shimmer, flammable. No damage; the
        // threat is "one stray Ignite and the whole pool becomes
        // fire."
        if let Some(id) = self.by_name("oil_pool") {
            let (palette, n) = pal(
                (80, 60, 120),
                (40, 30, 60),
                (130, 90, 200),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                emit_rate: 0.22,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -2.0,
                emit_drift_x: 0.5,
                emit_ttl_min: 0.5,
                emit_ttl_max: 1.0,
                emit_size: 1,
                flammable: true,
                ..SubstanceBehavior::default()
            });
        }

        // Flood ichor — sickly green infection wisps, flammable
        // (Flood biomass burns eagerly in the lore). Bumped to read
        // as actively-infected ground, not a faint stain.
        if let Some(id) = self.by_name("flood_ichor") {
            let (palette, n) = pal(
                (90, 255, 120),
                (60, 200, 90),
                (150, 255, 160),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                emit_rate: 1.4,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -5.0,
                emit_drift_x: 1.2,
                emit_ttl_min: 0.5,
                emit_ttl_max: 1.0,
                emit_size: 1,
                flammable: true,
                ..SubstanceBehavior::default()
            });
        }

        // Ember scatter — glowing orange flickers. Highly flammable
        // (fire-bug leftovers re-ignite at the drop of a hat).
        // Higher emit rate so the residue actually crackles instead
        // of looking like dim spots on the floor.
        if let Some(id) = self.by_name("ember_scatter") {
            let (palette, n) = pal(
                (255, 140, 40),
                (255, 200, 80),
                (200, 80, 20),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                emit_rate: 3.0,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -8.0,
                emit_drift_x: 1.6,
                emit_ttl_min: 0.3,
                emit_ttl_max: 0.6,
                emit_size: 1,
                flammable: true,
                ..SubstanceBehavior::default()
            });
        }

        // Psychic residue — violet sparkle drifting upward. Faint
        // sanity bleed for players (Carcosa-adjacent flavor).
        if let Some(id) = self.by_name("psychic_residue") {
            let (palette, n) = pal(
                (180, 110, 255),
                (230, 180, 255),
                (110, 60, 180),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                sanity_drain_per_sec: 1.5,
                emit_rate: 0.8,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -4.5,
                emit_drift_x: 1.6,
                emit_ttl_min: 0.6,
                emit_ttl_max: 1.2,
                emit_size: 1,
                ..SubstanceBehavior::default()
            });
        }

        // Fire — the NEW substance that makes Ignite actually leave
        // burning ground. Damage-per-sec + burn status + propagates
        // to flammable neighbors + auto-decays after ttl. Emit rate
        // bumped to 6.5/s/tile so a row of fire reads as a wall of
        // flame, not occasional flickers.
        if let Some(id) = self.by_name("fire") {
            let (palette, n) = pal(
                (255, 120, 40),
                (255, 200, 60),
                (200, 60, 20),
                3,
            );
            self.set_behavior(id, SubstanceBehavior {
                damage_per_sec: 10.0,
                emit_rate: 6.5,
                emit_palette: palette,
                emit_palette_len: n,
                emit_drift_y: -14.0,
                emit_drift_x: 2.4,
                emit_ttl_min: 0.25,
                emit_ttl_max: 0.7,
                emit_size: 2,
                ignites_neighbors: true,
                ttl_secs: Some(3.5),
                applies_burn: Some((8.0, 1.2)),
                ..SubstanceBehavior::default()
            });
        }
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.defs.len() <= 1 // sentinel doesn't count
    }
}

/// Raw TOML schema for a substance. Reads from
/// `content/core/substances/<id>.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct SubstanceToml {
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub fresh: String,
    pub dried: String,
    #[serde(default)]
    pub state_curve: Option<String>,
    #[serde(default)]
    pub bloom_color: Option<String>,
    #[serde(default)]
    pub bloom_radius: Option<u8>,
    #[serde(default)]
    pub bloom_falloff: Option<f32>,
    #[serde(default)]
    pub bloom_strength: Option<f32>,
    #[serde(default)]
    pub jitter: Option<u8>,
}

/// Parse `#rrggbb`, `#rgb`, or `rrggbb` hex color strings.
pub fn parse_hex(s: &str) -> Result<Pixel> {
    let s = s.trim().trim_start_matches('#');
    let bytes = match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16)?;
            let g = u8::from_str_radix(&s[2..4], 16)?;
            let b = u8::from_str_radix(&s[4..6], 16)?;
            (r, g, b)
        }
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16)? * 17;
            let g = u8::from_str_radix(&s[1..2], 16)? * 17;
            let b = u8::from_str_radix(&s[2..3], 16)? * 17;
            (r, g, b)
        }
        _ => return Err(anyhow!("expected #rrggbb or #rgb, got `{s}`")),
    };
    Ok(Pixel::rgb(bytes.0, bytes.1, bytes.2))
}
