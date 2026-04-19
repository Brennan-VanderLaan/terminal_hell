//! Per-effect mip rendering intensity. Every substance, body, corpse,
//! and decorative effect declares a `MipHint` — a ladder of (intensity,
//! style) entries, one per mip tier. Renderers consult the hint instead
//! of hardcoding `match mip { Hero => ..., Close => ..., ... }`.
//!
//! The `style` keyword is a `Tag`, not a closed enum — new render
//! routines register under a new keyword and any substance can name
//! it. Built-in styles live in `style_dispatch.rs`; content packs can
//! author new ones without touching the engine.
//!
//! `intensity` is a 0.0..=1.0 scalar the draw routine can use for fade,
//! thickness, alpha, bloom strength — whatever the style wants. 0.0 is
//! "skip entirely" at this tier.

use crate::camera::MipLevel;
use crate::tag::Tag;

/// Five-entry ladder covering every mip tier from Hero (most detail)
/// down to Dot (single pixel).
#[derive(Clone, Debug)]
pub struct MipHint {
    pub hero: MipStyle,
    pub close: MipStyle,
    pub normal: MipStyle,
    pub blob: MipStyle,
    pub dot: MipStyle,
}

impl MipHint {
    pub fn for_level(&self, mip: MipLevel) -> MipStyle {
        match mip {
            MipLevel::Hero => self.hero.clone(),
            MipLevel::Close => self.close.clone(),
            MipLevel::Normal => self.normal.clone(),
            MipLevel::Blob => self.blob.clone(),
            MipLevel::Dot => self.dot.clone(),
        }
    }

    /// Sensible default for ground decals: fades with mip, uses the
    /// "flat" style everywhere.
    pub fn ground_default() -> Self {
        let flat = Tag::new("flat");
        let skip = Tag::new("skip");
        Self {
            hero: MipStyle { intensity: 1.0, style: flat },
            close: MipStyle { intensity: 0.9, style: flat },
            normal: MipStyle { intensity: 0.7, style: flat },
            blob: MipStyle { intensity: 0.5, style: flat },
            dot: MipStyle { intensity: 0.0, style: skip },
        }
    }

    /// Sensible default for entities with a sprite — full sprite at
    /// Hero/Close/Normal, blob / dot at the small tiers. Distinct from
    /// ground_default so content doesn't need to author every field for
    /// the common case.
    pub fn sprite_default() -> Self {
        let sprite = Tag::new("sprite");
        let blob = Tag::new("blob");
        let dot = Tag::new("dot");
        Self {
            hero: MipStyle { intensity: 1.0, style: sprite },
            close: MipStyle { intensity: 1.0, style: sprite },
            normal: MipStyle { intensity: 1.0, style: sprite },
            blob: MipStyle { intensity: 1.0, style: blob },
            dot: MipStyle { intensity: 1.0, style: dot },
        }
    }
}

/// Render style for one mip tier. `intensity` is the loudness (0.0 =
/// skip), `style` names the draw routine the renderer should dispatch
/// to.
#[derive(Clone, Debug)]
pub struct MipStyle {
    pub intensity: f32,
    pub style: Tag,
}

impl MipStyle {
    pub fn is_skip(&self) -> bool {
        self.intensity <= 0.0 || self.style == Tag::new("skip")
    }
}

/// Optional light-bleed descriptor. Substances with bloom (fire, lava,
/// uranium, acid, shock discharge) light up nearby pixels additively.
/// Rendered via the framebuffer's bloom pass; bounded radius keeps the
/// pass cheap.
#[derive(Clone, Copy, Debug)]
pub struct Bloom {
    pub color: crate::fb::Pixel,
    /// Max bleed radius in screen pixels. Clamped to 6 by the fb pass.
    pub radius: u8,
    /// 0.0 = hard circle, 1.0 = linear falloff with radius.
    pub falloff: f32,
    /// Multiplier on the per-pixel contribution (0..1 typical).
    pub strength: f32,
}

impl Bloom {
    pub fn solid(color: crate::fb::Pixel, radius: u8, strength: f32) -> Self {
        Self { color, radius, falloff: 0.8, strength }
    }
}
