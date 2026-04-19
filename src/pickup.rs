//! Dropped weapon pickups. One-way: picking one up replaces the player's
//! active weapon. No on-ground swap / litter; pickups despawn when taken or
//! when their TTL expires so the arena doesn't clog with 30-minute-old loot.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::{Primitive, Rarity};
use crate::weapon::Weapon;
use crate::sprite;

const DEFAULT_TTL: f32 = 45.0;

pub struct Pickup {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub rarity: Rarity,
    pub primitives: Vec<Primitive>,
    pub ttl: f32,
    pub flash_phase: f32,
}

impl Pickup {
    pub fn new(id: u32, x: f32, y: f32, rarity: Rarity, primitives: Vec<Primitive>) -> Self {
        Self { id, x, y, rarity, primitives, ttl: DEFAULT_TTL, flash_phase: 0.0 }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.ttl -= dt;
        self.flash_phase = (self.flash_phase + dt * 4.0) % 1.0;
        self.ttl > 0.0
    }

    /// Build a `Weapon` runtime instance from this pickup's loadout.
    pub fn into_weapon(&self) -> Weapon {
        Weapon::with_slots(self.rarity, self.primitives.clone())
    }

    /// Reach radius — player just needs to be within this distance and press
    /// interact to grab the pickup.
    pub fn pickup_radius() -> f32 {
        6.0
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let cx = sx.round() as i32;
        let cy = sy.round() as i32;

        let rarity = self.rarity.color();
        match camera.mip_level() {
            0 => {
                let pulse = 0.6 + 0.4 * (self.flash_phase * std::f32::consts::TAU).sin().abs();
                let halo = Pixel::rgb(
                    (rarity.r as f32 * pulse) as u8,
                    (rarity.g as f32 * pulse) as u8,
                    (rarity.b as f32 * pulse) as u8,
                );
                for dy in -2..=2_i32 {
                    for dx in -2..=2_i32 {
                        if dx.abs() == 2 || dy.abs() == 2 {
                            set(fb, cx + dx, cy + dy, halo);
                        }
                    }
                }
                let slot_colors: Vec<Pixel> = self.primitives.iter().map(|p| p.color()).collect();
                let mut i = 0;
                for dy in -1..=1_i32 {
                    for dx in -1..=1_i32 {
                        let color = if slot_colors.is_empty() {
                            rarity
                        } else {
                            slot_colors[i % slot_colors.len()]
                        };
                        set(fb, cx + dx, cy + dy, color);
                        i += 1;
                    }
                }
                if self.ttl < 8.0 {
                    let dim = Pixel::rgb(40, 30, 50);
                    if (self.flash_phase * 8.0).floor() as i32 % 2 == 0 {
                        set(fb, cx, cy, dim);
                    }
                }
            }
            1 => sprite::render_blob(fb, (cx, cy), rarity),
            _ => sprite::render_dot(fb, (cx, cy), rarity),
        }
    }
}

fn set(fb: &mut Framebuffer, px: i32, py: i32, color: Pixel) {
    if px >= 0 && py >= 0 {
        fb.set(px as u16, py as u16, color);
    }
}
