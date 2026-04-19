//! Brand-bleed vote kiosks.
//!
//! During the Vote phase of the intermission, 2–3 kiosks appear in the
//! arena. Each represents an inactive brand. Players walk to a kiosk and
//! press E to register a vote. At phase end the top-voted brand is added
//! to the active brand stack, and future waves sample enemies from all
//! active brands (weighted). Kiosks despawn at vote end.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::sprite;
use std::collections::HashSet;

pub struct VoteKiosk {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub brand_id: String,
    pub brand_name: String,
    pub brand_color: Pixel,
    pub votes: u32,
    /// Which player IDs have already voted on this kiosk. One vote per
    /// player, swappable — voting on a different kiosk moves the vote.
    pub voter_ids: HashSet<u32>,
    pub flash_phase: f32,
}

impl VoteKiosk {
    pub fn new(id: u32, x: f32, y: f32, brand_id: String, brand_name: String, color: Pixel) -> Self {
        Self {
            id,
            x,
            y,
            brand_id,
            brand_name,
            brand_color: color,
            votes: 0,
            voter_ids: HashSet::new(),
            flash_phase: 0.0,
        }
    }

    pub fn interact_radius() -> f32 {
        6.0
    }

    pub fn tick(&mut self, dt: f32) {
        self.flash_phase = (self.flash_phase + dt * 3.0) % 1.0;
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let cx = sx.round() as i32;
        let cy = sy.round() as i32;

        match camera.mip_level() {
            0 => {
                let pulse = 0.6 + 0.4 * (self.flash_phase * std::f32::consts::TAU).sin().abs();
                let halo = Pixel::rgb(
                    (self.brand_color.r as f32 * pulse) as u8,
                    (self.brand_color.g as f32 * pulse) as u8,
                    (self.brand_color.b as f32 * pulse) as u8,
                );
                let core = self.brand_color;
                let stand = Pixel::rgb(80, 80, 100);

                for dx in -2..=2_i32 {
                    set(fb, cx + dx, cy + 3, stand);
                    set(fb, cx + dx, cy + 4, stand);
                }
                for dy in -2..=2_i32 {
                    for dx in -1..=1_i32 {
                        set(fb, cx + dx, cy + dy, core);
                    }
                }
                set(fb, cx - 2, cy - 2, halo);
                set(fb, cx - 2, cy + 2, halo);
                set(fb, cx + 2, cy - 2, halo);
                set(fb, cx + 2, cy + 2, halo);
                set(fb, cx, cy - 3, halo);
                for i in 0..self.votes.min(4) as i32 {
                    set(fb, cx - 1 + i, cy - 4, Pixel::rgb(255, 255, 255));
                }
            }
            1 => sprite::render_blob(fb, (cx, cy), self.brand_color),
            _ => sprite::render_dot(fb, (cx, cy), self.brand_color),
        }
    }
}

fn set(fb: &mut Framebuffer, px: i32, py: i32, color: Pixel) {
    if px >= 0 && py >= 0 {
        fb.set(px as u16, py as u16, color);
    }
}
