//! Procedural pixel sprites, stamped into the sextant framebuffer.
//!
//! Each sprite is a tiny 2D grid of `Option<Pixel>`. `None` is transparent so
//! silhouettes render cleanly over terrain. Sprite centers are placed at
//! entity positions; the blit clips at buffer edges.
//!
//! **Overlays** (`SpriteOverlay`) are an optional second layer composited on
//! top of the base sprite at the HIGH/CLOSE mip tiers. Overlays can (a)
//! suppress base pixels — used for damageable corpses, where hits punch
//! holes in the sprite — and (b) add decorative pixels over the top —
//! blood splatter, burn soot, glow, wound highlights. Overlay coordinates
//! are in *sprite-local* pixels, not screen pixels, so they scale cleanly
//! with zoom.

use crate::enemy::Archetype;
use crate::fb::{Framebuffer, Pixel};

/// Optional sprite-local decoration layer. Composited on top of a base
/// `Sprite` during `blit_scaled_with_overlay`. Sprite-local coords: (0, 0)
/// is top-left of the base sprite grid.
#[derive(Default, Clone, Debug)]
pub struct SpriteOverlay {
    /// Base-sprite pixels to suppress (render transparent). Used by
    /// damageable corpses: each hit punches out a small cluster of coords.
    pub holes: Vec<(u16, u16)>,
    /// Extra pixels painted over the top of the base sprite. Used for
    /// blood splatter, scorch, burn soot, muzzle flash, glow.
    pub stains: Vec<(u16, u16, Pixel)>,
}

impl SpriteOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn punch(&mut self, x: u16, y: u16) {
        self.holes.push((x, y));
    }

    pub fn stain(&mut self, x: u16, y: u16, color: Pixel) {
        self.stains.push((x, y, color));
    }

    pub fn is_hole(&self, x: u16, y: u16) -> bool {
        self.holes.iter().any(|&(hx, hy)| hx == x && hy == y)
    }
}

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

    /// Blend every non-transparent pixel toward `target` by `amount` in
    /// 0..1. 0 is a no-op; 1 replaces the pixel with `target`. Used for hit
    /// flashes and burn glow.
    pub fn tint_toward(&mut self, target: Pixel, amount: f32) {
        let a = amount.clamp(0.0, 1.0);
        for p in &mut self.pixels {
            if let Some(c) = p {
                c.r = lerp(c.r, target.r, a);
                c.g = lerp(c.g, target.g, a);
                c.b = lerp(c.b, target.b, a);
            }
        }
    }

    /// Draw the sprite centered on `(cx, cy)` in framebuffer pixel coords
    /// at 1:1 scale. Kept for callers that don't need scaling.
    pub fn blit(&self, fb: &mut Framebuffer, cx: i32, cy: i32) {
        self.blit_scaled(fb, (cx, cy), 1.0);
    }

    /// Scaled blit: at `scale = 1.0` each sprite pixel fills one screen
    /// pixel; at `scale = 2.0` each sprite pixel fills a 2×2 screen block;
    /// etc. `scale < 1.0` is clamped to 1.0 — callers should switch to the
    /// Blob/Dot mip levels instead of downscaling.
    pub fn blit_scaled(&self, fb: &mut Framebuffer, center: (i32, i32), scale: f32) {
        self.blit_scaled_with_overlay(fb, center, scale, None);
    }

    /// Scaled blit with optional overlay. The overlay can suppress base
    /// pixels (holes) and paint extra pixels over the top (stains). Overlay
    /// coordinates are in sprite-local pixel space and scale with `scale`.
    pub fn blit_scaled_with_overlay(
        &self,
        fb: &mut Framebuffer,
        center: (i32, i32),
        scale: f32,
        overlay: Option<&SpriteOverlay>,
    ) {
        let scale = scale.max(1.0);
        let w_px = (self.w as f32 * scale).round() as i32;
        let h_px = (self.h as f32 * scale).round() as i32;
        let ox = center.0 - w_px / 2;
        let oy = center.1 - h_px / 2;

        // Base layer: base sprite pixels, skipping any coord that the
        // overlay has marked as a hole.
        for sy in 0..self.h {
            for sx in 0..self.w {
                let idx = (sy as usize) * (self.w as usize) + (sx as usize);
                let Some(color) = self.pixels[idx] else { continue };
                if let Some(o) = overlay {
                    if o.is_hole(sx, sy) {
                        continue;
                    }
                }
                stamp_cell(fb, ox, oy, sx, sy, scale, color);
            }
        }

        // Overlay stains: drawn on top, can sit over holes or base pixels.
        if let Some(o) = overlay {
            for &(sx, sy, color) in &o.stains {
                if sx >= self.w || sy >= self.h {
                    continue;
                }
                stamp_cell(fb, ox, oy, sx, sy, scale, color);
            }
        }
    }
}

/// Fill one sprite-local pixel's screen rect with `color`. Shared by the
/// base and overlay passes so scaling stays consistent.
fn stamp_cell(
    fb: &mut Framebuffer,
    ox: i32,
    oy: i32,
    sx: u16,
    sy: u16,
    scale: f32,
    color: Pixel,
) {
    let x0 = ox + (sx as f32 * scale).round() as i32;
    let y0 = oy + (sy as f32 * scale).round() as i32;
    let x1 = ox + ((sx + 1) as f32 * scale).round() as i32;
    let y1 = oy + ((sy + 1) as f32 * scale).round() as i32;
    for py in y0..y1 {
        for px in x0..x1 {
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, color);
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

/// Draw the player's weapon barrel extending from their body center on
/// the screen in the aim direction. Scales with zoom so the barrel stays
/// visually proportional to the zoomed-in player sprite. At the Hero
/// tier the barrel gets an additional thickness pass + brighter tip.
pub fn render_player_barrel(
    fb: &mut Framebuffer,
    screen_cx: f32,
    screen_cy: f32,
    aim_x: f32,
    aim_y: f32,
    zoom: f32,
    hi_detail: bool,
) {
    let shaft = Pixel::rgb(200, 220, 255);
    let tip = Pixel::rgb(255, 245, 150);
    let scale = zoom.max(1.0);
    let start_offset = 2.5 * scale;
    // Perpendicular unit vector for thickness passes at the Hero tier.
    let perp_x = -aim_y;
    let perp_y = aim_x;
    for step in 1..=5 {
        let t = step as f32 * scale;
        let bx = screen_cx + aim_x * (t + start_offset);
        let by = screen_cy + aim_y * (t + start_offset);
        let color = if step == 5 { tip } else { shaft };
        let px = bx.round() as i32;
        let py = by.round() as i32;
        if px >= 0 && py >= 0 {
            fb.set(px as u16, py as u16, color);
        }
        if hi_detail {
            // Extra parallel pixel so the barrel reads thick at high zoom.
            let offs = (scale * 0.5).max(1.0);
            let tpx = (bx + perp_x * offs).round() as i32;
            let tpy = (by + perp_y * offs).round() as i32;
            if tpx >= 0 && tpy >= 0 {
                fb.set(tpx as u16, tpy as u16, color);
            }
        }
    }
}

/// Muzzle flash burst at the tip of a weapon barrel. Used on the firing
/// frame at the Close/Hero tiers so shots read as *something* rather than
/// just a projectile appearing somewhere. `intensity` is 0..1.
pub fn render_muzzle_flash(
    fb: &mut Framebuffer,
    screen_cx: f32,
    screen_cy: f32,
    aim_x: f32,
    aim_y: f32,
    zoom: f32,
    intensity: f32,
) {
    let i = intensity.clamp(0.0, 1.0);
    if i <= 0.0 {
        return;
    }
    let scale = zoom.max(1.0);
    // Muzzle origin — past the barrel tip.
    let start_offset = 2.5 * scale + 5.0 * scale;
    let mx = screen_cx + aim_x * start_offset;
    let my = screen_cy + aim_y * start_offset;

    let core = Pixel::rgb(
        ((255.0_f32) * i).round() as u8,
        ((250.0_f32) * i).round() as u8,
        ((180.0_f32) * i).round() as u8,
    );
    let spark = Pixel::rgb(
        ((255.0_f32) * i).round() as u8,
        ((180.0_f32) * i).round() as u8,
        ((60.0_f32) * i).round() as u8,
    );

    // Core blob at the muzzle.
    let core_r = (scale * 0.9).round() as i32;
    for dy in -core_r..=core_r {
        for dx in -core_r..=core_r {
            if dx * dx + dy * dy > core_r * core_r {
                continue;
            }
            let px = (mx + dx as f32).round() as i32;
            let py = (my + dy as f32).round() as i32;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, core);
            }
        }
    }
    // Spark tongue forward along aim.
    for step in 1..=4 {
        let t = step as f32 * scale * 0.9;
        let px = (mx + aim_x * t).round() as i32;
        let py = (my + aim_y * t).round() as i32;
        if px >= 0 && py >= 0 {
            fb.set(px as u16, py as u16, spark);
        }
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

/// Low-detail mip: 3×3 filled block at the screen position. Used when the
/// camera is zoomed out past full-sprite visibility but still wants to
/// communicate presence + color.
pub fn render_blob(fb: &mut Framebuffer, center: (i32, i32), color: Pixel) {
    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            let px = center.0 + dx;
            let py = center.1 + dy;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, color);
            }
        }
    }
}

/// Lowest mip: a single screen pixel in the entity color. Used at max
/// zoom-out — enemies and players become moving motes.
pub fn render_dot(fb: &mut Framebuffer, center: (i32, i32), color: Pixel) {
    if center.0 >= 0 && center.1 >= 0 {
        fb.set(center.0 as u16, center.1 as u16, color);
    }
}

/// Draw Hastur's gaze halo above a marked player: a yellow crown of 4 dots
/// floating above the head and a thin Yellow Sign glyph overhead.
pub fn render_hastur_mark(fb: &mut crate::fb::Framebuffer, cx: i32, cy: i32) {
    let sign = crate::fb::Pixel::rgb(255, 220, 80);
    let glow = crate::fb::Pixel::rgb(255, 180, 40);
    // Ring above head at cy-7..-5, 5 wide centered on cx.
    for dx in -2..=2_i32 {
        let px = cx + dx;
        if px < 0 {
            continue;
        }
        let py = cy - 7;
        if py >= 0 {
            fb.set(px as u16, py as u16, glow);
        }
    }
    // A "Y" crown at center.
    let top = cy - 9;
    if cx >= 0 && top >= 0 {
        fb.set(cx as u16, top as u16, sign);
        if cx >= 2 {
            fb.set((cx - 2) as u16, (top + 1) as u16, sign);
        }
        fb.set((cx + 2) as u16, (top + 1) as u16, sign);
        fb.set(cx as u16, (top + 2) as u16, sign);
    }
}

pub fn enemy_sprite(archetype: Archetype) -> Sprite {
    match archetype {
        Archetype::Rusher => rusher(),
        Archetype::Pinkie => pinkie(),
        Archetype::Charger => charger(),
        Archetype::Revenant => revenant(),
        Archetype::Marksman => marksman(),
        Archetype::Pmc => pmc(),
        Archetype::Swarmling => swarmling(),
        Archetype::Orb => orb(),
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

fn charger() -> Sprite {
    let body = Pixel::rgb(255, 100, 80);
    let dark = Pixel::rgb(150, 40, 30);
    let horn = Pixel::rgb(255, 240, 210);
    let eye = Pixel::rgb(255, 220, 100);

    let mut s = Sprite::new(8, 9);
    // Horned head — sloped forward, implying charge.
    s.set(0, 1, horn);
    s.set(7, 1, horn);
    s.fill_rect(1, 0, 6, 3, dark);
    s.set(2, 1, eye);
    s.set(5, 1, eye);
    // Wedge torso — leaner than pinkie.
    s.fill_rect(0, 3, 8, 4, body);
    s.fill_rect(2, 4, 4, 2, dark);
    // Sprint legs — one forward one back (frozen in motion).
    s.fill_rect(0, 7, 2, 2, dark);
    s.fill_rect(6, 7, 2, 2, dark);
    s
}

fn revenant() -> Sprite {
    let bone = Pixel::rgb(220, 230, 240);
    let dark = Pixel::rgb(90, 110, 140);
    let glow = Pixel::rgb(120, 200, 255);
    let eye = Pixel::rgb(255, 160, 80);

    let mut s = Sprite::new(7, 11);
    // Skull head
    s.fill_rect(2, 0, 3, 3, bone);
    s.set(3, 1, eye);
    s.set(2, 2, dark);
    s.set(4, 2, dark);
    // Ribcage torso
    s.fill_rect(1, 3, 5, 4, bone);
    s.set(1, 4, dark);
    s.set(5, 4, dark);
    s.set(1, 5, dark);
    s.set(5, 5, dark);
    s.set(3, 4, dark);
    s.set(3, 6, dark);
    // Shoulder-mounted launcher — glowing tip on one side as a ranged tell.
    s.fill_rect(5, 3, 2, 2, dark);
    s.set(6, 3, glow);
    // Spindly legs
    s.fill_rect(1, 7, 2, 4, dark);
    s.fill_rect(4, 7, 2, 4, dark);
    s.set(1, 10, bone);
    s.set(5, 10, bone);
    s
}

fn marksman() -> Sprite {
    let body = Pixel::rgb(150, 170, 100);
    let dark = Pixel::rgb(70, 90, 40);
    let helmet = Pixel::rgb(180, 200, 130);
    let muzzle = Pixel::rgb(255, 240, 80);
    let eye = Pixel::rgb(30, 50, 20);

    let mut s = Sprite::new(8, 11);
    // Helmet
    s.fill_rect(2, 0, 4, 2, helmet);
    s.fill_rect(2, 2, 4, 1, dark);
    s.set(3, 2, eye);
    s.set(4, 2, eye);
    // Shoulders + vest
    s.fill_rect(1, 3, 6, 4, body);
    s.fill_rect(2, 4, 4, 2, dark);
    s.set(1, 3, dark);
    s.set(6, 3, dark);
    // Rifle poking sideways
    s.fill_rect(6, 4, 2, 1, dark);
    s.set(7, 4, muzzle);
    // Legs — tactical stance
    s.fill_rect(2, 7, 1, 4, dark);
    s.fill_rect(5, 7, 1, 4, dark);
    s.set(2, 10, body);
    s.set(5, 10, body);
    s
}

fn pmc() -> Sprite {
    let plate = Pixel::rgb(120, 140, 170);
    let dark = Pixel::rgb(50, 70, 100);
    let accent = Pixel::rgb(160, 180, 210);
    let visor = Pixel::rgb(70, 180, 255);

    let mut s = Sprite::new(10, 11);
    // Helmet with visor
    s.fill_rect(2, 0, 6, 3, dark);
    s.fill_rect(3, 1, 4, 1, visor);
    // Shoulders
    s.fill_rect(0, 3, 10, 1, dark);
    // Torso plates
    s.fill_rect(1, 4, 8, 5, plate);
    s.fill_rect(3, 5, 4, 3, accent);
    s.fill_rect(4, 6, 2, 2, dark);
    // Side outline
    for y in 4..9 {
        s.set(0, y, dark);
        s.set(9, y, dark);
    }
    // Boots
    s.fill_rect(1, 9, 3, 2, dark);
    s.fill_rect(6, 9, 3, 2, dark);
    s
}

fn swarmling() -> Sprite {
    let body = Pixel::rgb(255, 80, 40);
    let dark = Pixel::rgb(140, 20, 10);
    let eye = Pixel::rgb(255, 220, 80);
    let mut s = Sprite::new(4, 4);
    s.fill_rect(1, 0, 2, 1, dark);
    s.fill_rect(0, 1, 4, 2, body);
    s.set(1, 1, eye);
    s.set(2, 1, eye);
    s.set(0, 3, dark);
    s.set(3, 3, dark);
    s
}

fn orb() -> Sprite {
    let body = Pixel::rgb(200, 120, 255);
    let core = Pixel::rgb(255, 200, 255);
    let dark = Pixel::rgb(90, 40, 150);
    let glow = Pixel::rgb(230, 180, 255);
    let mut s = Sprite::new(7, 7);
    // Round halo ring
    for x in 1..6 {
        s.set(x, 0, dark);
        s.set(x, 6, dark);
    }
    for y in 1..6 {
        s.set(0, y, dark);
        s.set(6, y, dark);
    }
    // Body filled
    s.fill_rect(1, 1, 5, 5, body);
    // Inner glow ring
    for x in 2..5 {
        s.set(x, 1, glow);
        s.set(x, 5, glow);
    }
    s.set(1, 3, glow);
    s.set(5, 3, glow);
    // Core
    s.fill_rect(2, 2, 3, 3, core);
    s.set(3, 3, dark);
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

/// Draw a projectile at the given screen position with an optional motion
/// trail. `ux, uy` are the unit motion vector (in world space, but it's
/// just a direction so zoom doesn't matter). `zoom` controls how chunky
/// the core + trail render.
pub fn render_projectile_screen(
    fb: &mut Framebuffer,
    screen_x: f32,
    screen_y: f32,
    ux: f32,
    uy: f32,
    zoom: f32,
) {
    let core = Pixel::rgb(255, 245, 150);
    let trail = Pixel::rgb(200, 110, 40);
    let spark = Pixel::rgb(255, 230, 170);
    let scale = zoom.max(1.0);

    let cx = screen_x.round() as i32;
    let cy = screen_y.round() as i32;
    let size = (2.0 * scale).round() as i32;
    for dy in 0..size {
        for dx in 0..size {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, core);
            }
        }
    }

    if ux == 0.0 && uy == 0.0 {
        return;
    }

    for step in 1..=2 {
        let t = step as f32 * 1.2 * scale;
        let px = (screen_x - ux * t).round() as i32;
        let py = (screen_y - uy * t).round() as i32;
        if px >= 0 && py >= 0 {
            fb.set(px as u16, py as u16, trail);
        }
    }

    for step in 1..=5 {
        let t = (2.5 + step as f32 * 0.9) * scale;
        let px = screen_x - ux * t;
        let py = screen_y - uy * t;
        let bx = px.round() as i32;
        let by = (py * 4.0 / 3.0).round() as i32;
        if bx >= 0 && by >= 0 {
            fb.braille_set(bx as u16, by as u16, spark);
        }
    }
}
