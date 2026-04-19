//! Procedural pixel sprites, stamped into the sextant framebuffer.
//!
//! Each sprite is a tiny 2D grid of `Option<Pixel>`. `None` is transparent so
//! silhouettes render cleanly over terrain. Sprite centers are placed at
//! entity positions; the blit clips at buffer edges.

use crate::enemy::Archetype;
use crate::fb::{Framebuffer, Pixel};

pub struct Sprite {
    pub w: u16,
    pub h: u16,
    pixels: Vec<Option<Pixel>>,
}

impl Sprite {
    pub fn new(w: u16, h: u16) -> Self {
        Self { w, h, pixels: vec![None; (w as usize) * (h as usize)] }
    }

    pub fn set(&mut self, x: u16, y: u16, color: Pixel) {
        if x >= self.w || y >= self.h {
            return;
        }
        self.pixels[(y as usize) * (self.w as usize) + (x as usize)] = Some(color);
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: Pixel) {
        let x1 = x.saturating_add(w).min(self.w);
        let y1 = y.saturating_add(h).min(self.h);
        for yy in y..y1 {
            for xx in x..x1 {
                self.set(xx, yy, color);
            }
        }
    }

    /// Draw the sprite centered on `(cx, cy)` in framebuffer pixel coords.
    pub fn blit(&self, fb: &mut Framebuffer, cx: i32, cy: i32) {
        let ox = cx - (self.w as i32) / 2;
        let oy = cy - (self.h as i32) / 2;
        for y in 0..self.h {
            for x in 0..self.w {
                let idx = (y as usize) * (self.w as usize) + (x as usize);
                if let Some(color) = self.pixels[idx] {
                    let px = ox + x as i32;
                    let py = oy + y as i32;
                    if px >= 0 && py >= 0 {
                        fb.set(px as u16, py as u16, color);
                    }
                }
            }
        }
    }
}

/// Humanoid silhouette for the survivor. Body, head, stubby legs. The weapon
/// barrel is drawn separately during entity rendering so it can track the aim
/// angle without needing eight hand-authored variants.
pub fn player_body() -> Sprite {
    let body = Pixel::rgb(90, 255, 230);
    let dark = Pixel::rgb(40, 150, 130);
    let head = Pixel::rgb(130, 255, 245);
    let eye = Pixel::rgb(20, 40, 60);

    let mut s = Sprite::new(8, 11);
    // Head — 4×3 with eyes.
    s.fill_rect(2, 0, 4, 3, head);
    s.set(3, 1, eye);
    s.set(4, 1, eye);
    // Neck — 2×1.
    s.fill_rect(3, 3, 2, 1, dark);
    // Torso — 6×4.
    s.fill_rect(1, 4, 6, 4, body);
    // Dark outline on torso edges for silhouette legibility.
    for y in 4..8 {
        s.set(1, y, dark);
        s.set(6, y, dark);
    }
    // Legs — 2×3 each, gap between.
    s.fill_rect(1, 8, 2, 3, dark);
    s.fill_rect(5, 8, 2, 3, dark);
    s
}

/// Draw the player's weapon barrel extending from their body center in the
/// aim direction. Simple 4-pixel ray with a bright muzzle tip.
pub fn render_player_barrel(
    fb: &mut Framebuffer,
    cx: f32,
    cy: f32,
    aim_x: f32,
    aim_y: f32,
) {
    let shaft = Pixel::rgb(200, 220, 255);
    let tip = Pixel::rgb(255, 245, 150);
    for step in 1..=5 {
        let t = step as f32;
        let px = (cx + aim_x * (t + 2.5)).round() as i32;
        let py = (cy + aim_y * (t + 2.5)).round() as i32;
        if px >= 0 && py >= 0 {
            let color = if step == 5 { tip } else { shaft };
            fb.set(px as u16, py as u16, color);
        }
    }
}

pub fn enemy_sprite(archetype: Archetype) -> Sprite {
    match archetype {
        Archetype::Rusher => rusher(),
        Archetype::Pinkie => pinkie(),
        Archetype::Miniboss => miniboss(),
    }
}

fn rusher() -> Sprite {
    let body = Pixel::rgb(255, 60, 60);
    let dark = Pixel::rgb(130, 20, 30);
    let horn = Pixel::rgb(200, 40, 40);
    let eye = Pixel::rgb(255, 230, 120);
    let mut s = Sprite::new(6, 7);
    // Horned head
    s.set(1, 0, horn);
    s.set(4, 0, horn);
    s.fill_rect(1, 1, 4, 2, body);
    s.set(2, 1, eye);
    s.set(3, 1, eye);
    // Torso
    s.fill_rect(0, 3, 6, 2, body);
    s.set(0, 3, dark);
    s.set(5, 3, dark);
    // Claw legs
    s.set(0, 5, dark);
    s.set(1, 5, dark);
    s.set(4, 5, dark);
    s.set(5, 5, dark);
    s.set(0, 6, dark);
    s.set(5, 6, dark);
    s
}

fn pinkie() -> Sprite {
    let body = Pixel::rgb(255, 130, 170);
    let dark = Pixel::rgb(160, 60, 90);
    let accent = Pixel::rgb(255, 200, 220);
    let eye = Pixel::rgb(255, 230, 100);
    let tusk = Pixel::rgb(240, 240, 230);

    let mut s = Sprite::new(10, 10);
    // Jawed head — wider at the top with tusks at the sides.
    s.fill_rect(1, 0, 8, 2, dark);
    s.set(0, 1, tusk);
    s.set(9, 1, tusk);
    s.set(3, 1, eye);
    s.set(6, 1, eye);
    // Massive torso
    s.fill_rect(0, 2, 10, 6, body);
    // Chest plate detail
    s.fill_rect(3, 3, 4, 3, accent);
    // Shoulder outlines
    for y in 2..8 {
        s.set(0, y, dark);
        s.set(9, y, dark);
    }
    // Stout legs
    s.fill_rect(0, 8, 3, 2, dark);
    s.fill_rect(7, 8, 3, 2, dark);
    s
}

fn miniboss() -> Sprite {
    let body = Pixel::rgb(255, 190, 60);
    let dark = Pixel::rgb(160, 90, 20);
    let armor = Pixel::rgb(90, 50, 10);
    let spike = Pixel::rgb(255, 245, 200);
    let eye = Pixel::rgb(255, 80, 80);

    let mut s = Sprite::new(16, 16);
    // Spiked horns
    s.set(2, 0, spike);
    s.set(4, 0, spike);
    s.set(11, 0, spike);
    s.set(13, 0, spike);
    // Skull/head band
    s.fill_rect(3, 1, 10, 3, dark);
    s.set(5, 2, eye);
    s.set(10, 2, eye);
    // Shoulder pauldrons
    s.fill_rect(0, 3, 3, 4, armor);
    s.fill_rect(13, 3, 3, 4, armor);
    // Massive torso
    s.fill_rect(2, 4, 12, 8, body);
    // Chest plate
    s.fill_rect(5, 5, 6, 5, armor);
    s.fill_rect(6, 6, 4, 3, dark);
    // Arm guards
    s.fill_rect(0, 7, 2, 4, dark);
    s.fill_rect(14, 7, 2, 4, dark);
    // Legs — heavy
    s.fill_rect(2, 12, 5, 4, dark);
    s.fill_rect(9, 12, 5, 4, dark);
    // Hoof-spikes
    s.set(2, 15, spike);
    s.set(6, 15, spike);
    s.set(9, 15, spike);
    s.set(13, 15, spike);
    s
}

/// Draw a projectile as a bright 2×2 core, a sextant smoke trail, and a
/// finer braille dot trail. The braille layer only renders where the sextant
/// trail hasn't painted, so we get two levels of temporal detail for free.
pub fn render_projectile(fb: &mut Framebuffer, x: f32, y: f32, vx: f32, vy: f32) {
    let core = Pixel::rgb(255, 245, 150);
    let trail = Pixel::rgb(200, 110, 40);
    let spark = Pixel::rgb(255, 230, 170);

    let cx = x.round() as i32;
    let cy = y.round() as i32;
    for dy in 0..2 {
        for dx in 0..2 {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, core);
            }
        }
    }

    let speed2 = vx * vx + vy * vy;
    if speed2 < 1.0 {
        return;
    }
    let inv = speed2.sqrt().recip();
    let ux = vx * inv;
    let uy = vy * inv;

    // Sextant smoke trail — a couple chunky pixels immediately behind the
    // core. Short on purpose so projectiles just spawned at the muzzle tip
    // don't paint a trail across the player's sprite.
    for step in 1..=2 {
        let t = step as f32 * 1.2;
        let px = (x - ux * t).round() as i32;
        let py = (y - uy * t).round() as i32;
        if px >= 0 && py >= 0 {
            fb.set(px as u16, py as u16, trail);
        }
    }

    // Braille dust — finer sparks continuing past the sextant trail. Capped
    // short enough that a freshly-fired bullet doesn't dust the player.
    for step in 1..=5 {
        let t = 2.5 + step as f32 * 0.9;
        let px = x - ux * t;
        let py = y - uy * t;
        let bx = px.round() as i32;
        let by = (py * 4.0 / 3.0).round() as i32;
        if bx >= 0 && by >= 0 {
            fb.braille_set(bx as u16, by as u16, spark);
        }
    }
}
