//! Client-side glyph-particle effects (debris, gibs). Pure visual. Spawn
//! takes a seed so that once netcode lands the host can broadcast
//! `(tick, source_id, tile_id)` and every client draws identical particles
//! without syncing them over the wire.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use std::f32::consts::TAU;

pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub ttl: f32,
    pub ttl_max: f32,
    pub color: Pixel,
    /// Render footprint: 1 = single pixel, 2 = 2×2 block, etc. Chunky
    /// gore particles at heavy/extreme tiers use size ≥ 2 so flying
    /// chunks read as "solid hunks of meat" rather than specks.
    pub size: u8,
}

impl Particle {
    pub fn update(&mut self, dt: f32) -> bool {
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        // Radial drag so particles coast to a stop instead of floating forever.
        let drag = 0.90_f32.powf(dt * 60.0);
        self.vx *= drag;
        self.vy *= drag;
        self.ttl -= dt;
        self.ttl > 0.0
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        use crate::camera::MipLevel;
        let mip = camera.mip_level();
        // At the Dot mip (entire arena visible), particles would be
        // sub-pixel specks flooding the overview. Skip them entirely
        // to keep max-zoom-out readable + cheap.
        if matches!(mip, MipLevel::Dot) {
            return;
        }
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let px = sx.round() as i32;
        let py = sy.round() as i32;
        if px < 0 || py < 0 {
            return;
        }
        // Linear fade to black over TTL.
        let t = (self.ttl / self.ttl_max).clamp(0.0, 1.0);
        let faded = Pixel::rgb(
            (self.color.r as f32 * t) as u8,
            (self.color.g as f32 * t) as u8,
            (self.color.b as f32 * t) as u8,
        );
        // MIP-aware footprint. Particles scale roughly with sprite
        // scaling so a size-2 chunk at Hero zoom reads as a big hunk,
        // not a tiny 2px block dwarfed by 66px-tall enemy sprites.
        let mip_mul = match mip {
            MipLevel::Hero => 2,
            MipLevel::Close => 2,
            MipLevel::Normal => 1,
            MipLevel::Blob => 1,
            MipLevel::Dot => 0,
        };
        let s = ((self.size as i32) * mip_mul).max(1);
        for dy in 0..s {
            for dx in 0..s {
                let gx = px + dx;
                let gy = py + dy;
                if gx < 0 || gy < 0 {
                    continue;
                }
                if (gx as u16) >= camera.viewport_w || (gy as u16) >= camera.viewport_h {
                    continue;
                }
                fb.set(gx as u16, gy as u16, faded);
            }
        }
    }
}

/// Spawn a burst of destruction particles at `(x, y)` in `color`. Seeded RNG
/// so particles are deterministic given the same seed. Legacy entry point
/// used by wall-damage effects; enemy deaths route through `spawn_gore`.
pub fn spawn_destruction(
    out: &mut Vec<Particle>,
    x: f32,
    y: f32,
    color: Pixel,
    seed: u64,
) {
    let mut rng = SmallRng::seed_from_u64(seed);
    let count = rng.gen_range(6..11);
    for _ in 0..count {
        let angle: f32 = rng.gen_range(0.0..TAU);
        let speed: f32 = rng.gen_range(18.0..52.0);
        let ttl: f32 = rng.gen_range(0.6..1.3);
        out.push(Particle {
            x,
            y,
            vx: angle.cos() * speed,
            vy: angle.sin() * speed,
            ttl,
            ttl_max: ttl,
            color,
            size: 1,
        });
    }
}

/// Spawn a gore burst driven by a `GoreProfile`. Large palette + chunk-vs-
/// splatter split + optional accent pixels makes deaths read as
/// "too much" rather than the 6-particle chatter of a wall break.
///
/// Same seed on host + clients → identical particle layout, so the
/// host just broadcasts seed + tier and the visual replays everywhere.
pub fn spawn_gore(
    out: &mut Vec<Particle>,
    x: f32,
    y: f32,
    fallback: Pixel,
    seed: u64,
    profile: &crate::gore::GoreProfile,
) {
    // Back-compat path — omnidirectional burst.
    spawn_gore_directional(out, x, y, fallback, seed, profile, None);
}

/// Same as `spawn_gore` but particles bias toward `direction` in a
/// cone. Used for corpse hits where the spray should fly away along
/// the bullet's direction of travel. Pass `None` for the old full-
/// circle behavior (enemy deaths / wall breaks etc).
///
/// `direction` is a unit vector in world space. Cone width is
/// embedded per-tier: tight for CorpseHit (forward spray), wider for
/// Heavy / Extreme (pseudo-directional but still mostly a sphere).
pub fn spawn_gore_directional(
    out: &mut Vec<Particle>,
    x: f32,
    y: f32,
    fallback: Pixel,
    seed: u64,
    profile: &crate::gore::GoreProfile,
    direction: Option<(f32, f32)>,
) {
    let mut rng = SmallRng::seed_from_u64(seed);
    let palette = if profile.palette.is_empty() {
        &[fallback][..]
    } else {
        &profile.palette[..]
    };

    // Chunk footprint scales with tier — extreme chunks are 3×3 blocks
    // so they read as visibly-too-much meat, not specks.
    let chunk_size: u8 = match profile.tier {
        crate::gore::GoreTier::Extreme => 3,
        crate::gore::GoreTier::Heavy => 2,
        crate::gore::GoreTier::CorpseHit => 2,
        crate::gore::GoreTier::Standard => 1,
    };

    // Directional behavior. `cone_half` is the half-angle in radians
    // that each particle can deviate from the bias direction. Tight
    // cone = forward spray; wide cone = near-omnidirectional.
    let (dir_angle, cone_half) = if let Some((dx, dy)) = direction {
        let angle = dy.atan2(dx);
        let half = match profile.tier {
            // CorpseHit is the only tier that really forwards a spray.
            // Wide enough to read as a cone (~120°) but still biased.
            crate::gore::GoreTier::CorpseHit => std::f32::consts::FRAC_PI_3, // 60° half = 120° cone
            // Heavy / Extreme can also bias if caller provides a
            // direction, but keep the cone wide so kills still read
            // as explosions, not sprays.
            _ => 2.5, // ~143° half → almost full circle
        };
        (angle, half)
    } else {
        (0.0, std::f32::consts::PI) // full circle
    };

    // Fat chunks — travel far, linger, deposit drip trails behind them.
    for i in 0..profile.chunks {
        let angle: f32 = if cone_half >= std::f32::consts::PI {
            rng.gen_range(0.0..TAU)
        } else {
            dir_angle + rng.gen_range(-cone_half..cone_half)
        };
        let speed = rng.gen_range(profile.speed_range.0..profile.speed_range.1) * 0.95
            + rng.gen_range(0.0..30.0);
        let ttl: f32 = rng.gen_range(profile.ttl_range.0..profile.ttl_range.1);
        let color = if profile.accent.is_some() && (i % 12 == 7) {
            profile.accent.unwrap()
        } else {
            let idx = weighted_palette_index(&mut rng, palette.len());
            palette[idx]
        };
        let vx = angle.cos() * speed;
        let vy = angle.sin() * speed;
        out.push(Particle {
            x,
            y,
            vx,
            vy,
            ttl,
            ttl_max: ttl,
            color,
            size: chunk_size,
        });
        // Drip trail — a handful of static specks along the chunk's
        // trajectory with staggered TTL so they fade in the chunk's
        // wake. Reads as "blood spraying out behind the chunk."
        if !matches!(profile.tier, crate::gore::GoreTier::Standard) {
            let drip_count = if matches!(profile.tier, crate::gore::GoreTier::Extreme) {
                6
            } else {
                3
            };
            for step in 1..=drip_count {
                let t = step as f32 / (drip_count as f32 + 1.0);
                let drip_x = x + vx * t * ttl * 0.6;
                let drip_y = y + vy * t * ttl * 0.6;
                let drip_ttl = ttl * (1.0 - t) * 0.8 + 0.1;
                out.push(Particle {
                    x: drip_x,
                    y: drip_y,
                    vx: 0.0,
                    vy: 0.0,
                    ttl: drip_ttl,
                    ttl_max: drip_ttl,
                    color,
                    size: 1,
                });
            }
        }
    }

    // Splatter cloud — dense, short-lived, near-ground. Fills the frame
    // with motion around the impact point.
    for _ in 0..profile.splatter {
        let angle: f32 = if cone_half >= std::f32::consts::PI {
            rng.gen_range(0.0..TAU)
        } else {
            // Splatter has a wider cone than chunks — bullet impacts
            // throw a bit of back-spray too.
            let wider = (cone_half * 1.4).min(std::f32::consts::PI);
            dir_angle + rng.gen_range(-wider..wider)
        };
        let speed: f32 =
            rng.gen_range(profile.speed_range.0 * 0.3..profile.speed_range.1 * 0.55);
        let ttl: f32 =
            rng.gen_range(profile.ttl_range.0 * 0.35..profile.ttl_range.0 * 1.1);
        let color = palette[rng.gen_range(0..palette.len())];
        let jitter_x: f32 = rng.gen_range(-0.4..0.4) * profile.spread;
        let jitter_y: f32 = rng.gen_range(-0.4..0.4) * profile.spread;
        out.push(Particle {
            x: x + jitter_x,
            y: y + jitter_y,
            vx: angle.cos() * speed,
            vy: angle.sin() * speed,
            ttl,
            ttl_max: ttl,
            color,
            size: 1,
        });
    }

    // Shockwave ring — heavy/extreme tiers get a second, delayed-read
    // burst of evenly-spaced particles moving outward. Reads as impact
    // force, not just mist. Skipped at standard + corpse_hit to keep
    // small kills cheap and the corpse-hit spray directional.
    if !matches!(
        profile.tier,
        crate::gore::GoreTier::Standard | crate::gore::GoreTier::CorpseHit
    ) {
        let ring_count = if matches!(profile.tier, crate::gore::GoreTier::Extreme) {
            24
        } else {
            12
        };
        let ring_speed = profile.speed_range.1 * 0.5;
        let ring_ttl = profile.ttl_range.1 * 0.8;
        let ring_color = profile.accent.unwrap_or(palette[0]);
        for i in 0..ring_count {
            let angle = (i as f32 / ring_count as f32) * TAU;
            out.push(Particle {
                x,
                y,
                vx: angle.cos() * ring_speed,
                vy: angle.sin() * ring_speed,
                ttl: ring_ttl,
                ttl_max: ring_ttl,
                color: ring_color,
                size: chunk_size.saturating_sub(1).max(1),
            });
        }
    }
}

/// Weighted palette index: roll once 0..len; if it lands on 0 with
/// extra probability mass, keep it; otherwise the pick is uniform on
/// the rest. Simple two-tier weight so the primary body color
/// dominates without us needing a full weighted-choice dep.
fn weighted_palette_index(rng: &mut SmallRng, len: usize) -> usize {
    let roll: f32 = rng.gen_range(0.0..1.0);
    if roll < 0.45 || len == 1 {
        0
    } else {
        rng.gen_range(1..len)
    }
}
