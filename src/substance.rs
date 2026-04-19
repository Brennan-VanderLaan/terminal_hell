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
        };
        self.defs.push(def);
        self.by_name.insert(name, id);
        Ok(id)
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
