//! Gore profiles — per-archetype "how nasty is the death" presets.
//! Profile tiers from `standard` (background chatter) up to `extreme`
//! (chunky meat shower, wall splatter, persistent pool trail).
//!
//! Host-side `emit_gore` computes deterministic chunk landings from a
//! seed, paints blood-pool substance tiles along the trajectory, ray-
//! casts each chunk's flight path so ones that would crash into a
//! wall splat there instead of landing past it. Broadcasts an enhanced
//! `Blast` carrying the profile tier so clients synthesize matching
//! particle clouds with color variation pulled from the profile palette.

use crate::fb::Pixel;

/// Tier id packed into the Blast wire message. Unknown tiers fall
/// back to standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoreTier {
    Standard,
    Heavy,
    Extreme,
    /// Small directional burst for shooting a body — a punchy fwd-
    /// cone spray behind the bullet instead of an omnidirectional
    /// explosion. Much lighter than Heavy.
    CorpseHit,
}

impl GoreTier {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => GoreTier::Heavy,
            2 => GoreTier::Extreme,
            3 => GoreTier::CorpseHit,
            _ => GoreTier::Standard,
        }
    }
    pub fn to_byte(self) -> u8 {
        match self {
            GoreTier::Standard => 0,
            GoreTier::Heavy => 1,
            GoreTier::Extreme => 2,
            GoreTier::CorpseHit => 3,
        }
    }
}

/// How nasty a death looks.
#[derive(Clone, Debug)]
pub struct GoreProfile {
    pub tier: GoreTier,
    /// Big particles that travel far and leave pool decals.
    pub chunks: u16,
    /// Small fast-fade fleck particles — the cloud density.
    pub splatter: u16,
    /// Ground pool decals painted at chunk landing points.
    pub pool_tiles: u8,
    /// World-pixel radius for pool + splatter spread.
    pub spread: f32,
    /// TTL range for particles (secs).
    pub ttl_range: (f32, f32),
    /// Speed range for particles (world-px/s).
    pub speed_range: (f32, f32),
    /// Chunks that hit a wall during their flight splat on the wall
    /// rather than landing past it. Host-side only (uses arena raycast).
    pub wall_splatter: bool,
    /// Main palette — chunks pick colors weighted toward index 0.
    /// Typical order: primary body, dark muscle, bright highlight,
    /// darker clot. Empty → fall back to caller-supplied color only.
    pub palette: Vec<Pixel>,
    /// Rare bright accent — bile, fat, bone shard. None to skip.
    pub accent: Option<Pixel>,
}

impl GoreProfile {
    /// Lookup by tier name (matches the TOML `gore_profile = "..."` value).
    pub fn named(name: &str) -> Self {
        match name {
            "heavy" => Self::heavy(),
            "extreme" => Self::extreme(),
            "corpse_hit" => Self::corpse_hit(),
            _ => Self::standard(),
        }
    }

    /// Small directional burst for shooting a body. About 1/4 the
    /// volume of Heavy; particles bias forward in a cone along the
    /// bullet direction instead of an omnidirectional explosion.
    /// No pool trail (host handles that separately) and no
    /// shockwave ring — this is a bullet's worth of gore, not a
    /// death's.
    pub fn corpse_hit() -> Self {
        Self {
            tier: GoreTier::CorpseHit,
            chunks: 10,
            splatter: 28,
            pool_tiles: 0,
            spread: 3.0,
            ttl_range: (0.35, 1.1),
            speed_range: (55.0, 160.0),
            wall_splatter: true,
            palette: vec![
                Pixel::rgb(180, 20, 28),
                Pixel::rgb(130, 14, 22),
                Pixel::rgb(220, 60, 60),
                Pixel::rgb(70, 10, 20),
                Pixel::rgb(200, 40, 80),
            ],
            // Tiny bone-shard highlight.
            accent: Some(Pixel::rgb(240, 230, 200)),
        }
    }

    pub fn standard() -> Self {
        Self {
            tier: GoreTier::Standard,
            chunks: 8,
            splatter: 18,
            pool_tiles: 2,
            spread: 3.5,
            ttl_range: (0.6, 1.4),
            speed_range: (20.0, 70.0),
            wall_splatter: false,
            palette: vec![
                Pixel::rgb(180, 20, 28),
                Pixel::rgb(120, 14, 22),
                Pixel::rgb(210, 60, 60),
                Pixel::rgb(70, 10, 20),
            ],
            accent: None,
        }
    }

    pub fn heavy() -> Self {
        Self {
            tier: GoreTier::Heavy,
            chunks: 40,
            splatter: 140,
            pool_tiles: 14,
            spread: 8.5,
            ttl_range: (0.8, 2.8),
            speed_range: (35.0, 180.0),
            wall_splatter: true,
            palette: vec![
                Pixel::rgb(190, 20, 30),
                Pixel::rgb(140, 16, 24),
                Pixel::rgb(240, 70, 80),
                Pixel::rgb(80, 12, 22),
                Pixel::rgb(210, 40, 100),
                Pixel::rgb(120, 22, 60),
            ],
            // Bright fat highlight — pops on a dark arena floor.
            accent: Some(Pixel::rgb(255, 200, 60)),
        }
    }

    pub fn extreme() -> Self {
        Self {
            tier: GoreTier::Extreme,
            chunks: 90,
            splatter: 380,
            pool_tiles: 42,
            spread: 16.0,
            ttl_range: (1.2, 4.2),
            speed_range: (50.0, 280.0),
            wall_splatter: true,
            palette: vec![
                // Wide palette — chunk-to-chunk color shift reads as
                // "there used to be multiple things in there."
                Pixel::rgb(200, 22, 34),
                Pixel::rgb(150, 18, 28),
                Pixel::rgb(255, 90, 110),
                Pixel::rgb(90, 10, 20),
                Pixel::rgb(220, 45, 115),
                Pixel::rgb(130, 24, 66),
                Pixel::rgb(60, 6, 18),
                Pixel::rgb(255, 130, 60),
            ],
            // Golden bile + bone-white shards pick out the chaos.
            accent: Some(Pixel::rgb(255, 240, 120)),
        }
    }
}
