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

        let mip = camera.mip_level();
        if mip.shows_sprite() {
            let scale = (camera.zoom * 0.5).max(1.0);
            let pulse = 0.6 + 0.4 * (self.flash_phase * std::f32::consts::TAU).sin().abs();
            let halo = Pixel::rgb(
                (self.brand_color.r as f32 * pulse) as u8,
                (self.brand_color.g as f32 * pulse) as u8,
                (self.brand_color.b as f32 * pulse) as u8,
            );
            let core = self.brand_color;
            let stand = Pixel::rgb(80, 80, 100);

            for dx in -2..=2_i32 {
                stamp(fb, cx, cy, dx, 3, scale, stand);
                stamp(fb, cx, cy, dx, 4, scale, stand);
            }
            for dy in -2..=2_i32 {
                for dx in -1..=1_i32 {
                    stamp(fb, cx, cy, dx, dy, scale, core);
                }
            }
            stamp(fb, cx, cy, -2, -2, scale, halo);
            stamp(fb, cx, cy, -2, 2, scale, halo);
            stamp(fb, cx, cy, 2, -2, scale, halo);
            stamp(fb, cx, cy, 2, 2, scale, halo);
            stamp(fb, cx, cy, 0, -3, scale, halo);
            for i in 0..self.votes.min(4) as i32 {
                stamp(fb, cx, cy, -1 + i, -4, scale, Pixel::rgb(255, 255, 255));
            }
        } else if matches!(mip, crate::camera::MipLevel::Blob) {
            sprite::render_blob(fb, (cx, cy), self.brand_color);
        } else {
            sprite::render_dot(fb, (cx, cy), self.brand_color);
        }
    }
}

fn stamp(fb: &mut Framebuffer, cx: i32, cy: i32, dx: i32, dy: i32, scale: f32, color: Pixel) {
    let s = scale.max(1.0);
    let x0 = cx + (dx as f32 * s).round() as i32;
    let y0 = cy + (dy as f32 * s).round() as i32;
    let x1 = x0 + s.round() as i32;
    let y1 = y0 + s.round() as i32;
    for py in y0..y1 {
        for px in x0..x1 {
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, color);
            }
        }
    }
}
