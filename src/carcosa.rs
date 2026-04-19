//! Carcosa antagonist systems: the Hastur Daemon, Yellow Sign manifestations,
//! and (client-side) sanity-driven phantom enemies.
//!
//! The Daemon is an always-on AI that starts producing notice events once
//! Corruption reaches 25%. It schedules Yellow Sign flashes in the arena
//! which drain sanity from every living player while visible.
//!
//! Phantoms are purely local visual illusions that only appear on clients
//! whose sanity is low. They have no authority state — the host never knows
//! about them. This keeps per-player hallucinations cheap without requiring
//! per-player render divergence on the server.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::sprite::{self, Sprite};

/// A visible Yellow Sign appearing in the arena. The first portion of its
/// lifetime is a "tell" (dim flash), then it blooms to full visibility, then
/// fades out. Sign drains sanity from every living player while visible.
#[derive(Clone, Debug)]
pub struct YellowSign {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub ttl: f32,
    pub ttl_max: f32,
}

impl YellowSign {
    pub const BLOOM_START: f32 = 0.8; // fraction of ttl at which bloom begins
    pub const FULL_DURATION: f32 = 3.0;

    pub fn new(id: u32, x: f32, y: f32) -> Self {
        Self { id, x, y, ttl: Self::FULL_DURATION, ttl_max: Self::FULL_DURATION }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.ttl -= dt;
        self.ttl > 0.0
    }

    pub fn sanity_drain_per_sec(&self) -> f32 {
        // Ramps up: the Sign doesn't bite until it blooms.
        let t = 1.0 - (self.ttl / self.ttl_max);
        if t > (1.0 - Self::BLOOM_START) {
            6.0
        } else {
            1.0
        }
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let cx = sx.round() as i32;
        let cy = sy.round() as i32;
        let t = 1.0 - (self.ttl / self.ttl_max);
        let bloom = (t / (1.0 - Self::BLOOM_START)).clamp(0.0, 1.0);

        let sign_color = Pixel::rgb(
            (255.0 * (0.5 + bloom * 0.5)) as u8,
            (220.0 * (0.5 + bloom * 0.5)) as u8,
            (60.0 * (0.3 + bloom * 0.7)) as u8,
        );

        match camera.mip_level() {
            0 => {
                let halo = Pixel::rgb(
                    (200.0 * (0.4 + bloom * 0.6)) as u8,
                    (140.0 * (0.4 + bloom * 0.6)) as u8,
                    (30.0 * (0.4 + bloom * 0.6)) as u8,
                );
                for dy in -4..=4_i32 {
                    for dx in -4..=4_i32 {
                        let d2 = dx * dx + dy * dy;
                        if d2 > 20 && d2 < 25 {
                            set(fb, cx + dx, cy + dy, halo);
                        }
                    }
                }
                for dy in -3..=3_i32 {
                    set(fb, cx, cy + dy, sign_color);
                }
                set(fb, cx - 1, cy - 2, sign_color);
                set(fb, cx - 2, cy - 1, sign_color);
                set(fb, cx - 3, cy, sign_color);
                set(fb, cx + 1, cy - 2, sign_color);
                set(fb, cx + 2, cy - 1, sign_color);
                set(fb, cx + 3, cy, sign_color);
                set(fb, cx - 1, cy + 3, sign_color);
                set(fb, cx + 1, cy + 3, sign_color);
            }
            1 => sprite::render_blob(fb, (cx, cy), sign_color),
            _ => sprite::render_dot(fb, (cx, cy), sign_color),
        }
    }
}

/// Client-side phantom enemy. Pure illusion; doesn't exist on the server.
pub struct Phantom {
    pub x: f32,
    pub y: f32,
    pub ttl: f32,
    pub ttl_max: f32,
    pub archetype_kind: u8,
}

impl Phantom {
    pub fn new(x: f32, y: f32, archetype_kind: u8) -> Self {
        Self { x, y, ttl: 3.0, ttl_max: 3.0, archetype_kind }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.ttl -= dt;
        self.ttl > 0.0
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let cx = sx.round() as i32;
        let cy = sy.round() as i32;
        let archetype = crate::enemy::Archetype::from_kind(self.archetype_kind);

        match camera.mip_level() {
            0 => {
                let mut s: Sprite = sprite::enemy_sprite(archetype);
                let t = 1.0 - (self.ttl / self.ttl_max);
                let spawn_flash = if t < 0.08 { 0.9 } else { 0.0 };
                if spawn_flash > 0.0 {
                    s.tint_toward(Pixel::rgb(255, 255, 255), spawn_flash);
                } else {
                    s.tint_toward(Pixel::rgb(180, 140, 40), 0.55);
                }
                s.blit_scaled(fb, (cx, cy), camera.zoom);
            }
            1 => sprite::render_blob(fb, (cx, cy), Pixel::rgb(180, 140, 40)),
            _ => sprite::render_dot(fb, (cx, cy), Pixel::rgb(180, 140, 40)),
        }
    }
}

fn set(fb: &mut Framebuffer, px: i32, py: i32, color: Pixel) {
    if px >= 0 && py >= 0 {
        fb.set(px as u16, py as u16, color);
    }
}
