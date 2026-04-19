//! Client-side glyph-particle effects (debris, gibs). Pure visual. Spawn
//! takes a seed so that once netcode lands the host can broadcast
//! `(tick, source_id, tile_id)` and every client draws identical particles
//! without syncing them over the wire.

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

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32) {
        let px = ox + self.x.round() as i32;
        let py = oy + self.y.round() as i32;
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
        fb.set(px as u16, py as u16, faded);
    }
}

/// Spawn a burst of destruction particles at `(x, y)` in `color`. Seeded RNG
/// so particles are deterministic given the same seed.
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
        });
    }
}
