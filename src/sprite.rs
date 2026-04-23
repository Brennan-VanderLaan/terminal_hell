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

#[derive(Clone)]
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
    let visor = Pixel::rgb(20, 40, 60);
    let accent = Pixel::rgb(200, 255, 250);

    // 8×11 — cyan humanoid survivor. Helmet with a horizontal visor
    // band, defined torso with outline + chest accent, weapon-ready
    // braced stance with a short arm stub reading as a grip, stubby
    // legs with boot highlights.
    let mut s = Sprite::new(8, 11);
    // Helmet — top crown.
    s.fill_rect(2, 0, 4, 1, dark);
    s.fill_rect(2, 1, 4, 1, head);
    // Visor band across the face.
    s.fill_rect(2, 2, 4, 1, visor);
    s.set(3, 2, accent);
    s.set(4, 2, accent);
    // Neck.
    s.fill_rect(3, 3, 2, 1, dark);
    // Shoulders.
    s.fill_rect(1, 4, 6, 1, dark);
    // Torso — body fill with outline.
    s.fill_rect(1, 5, 6, 3, body);
    // Chest accent highlight.
    s.set(3, 5, accent);
    s.set(4, 5, accent);
    // Central torso shadow for depth.
    s.set(3, 6, dark);
    s.set(4, 6, dark);
    // Side outline on torso.
    for y in 5..8 {
        s.set(1, y, dark);
        s.set(6, y, dark);
    }
    // Weapon-holding arm stub extending forward (right side).
    s.set(7, 5, dark);
    s.set(7, 6, body);
    // Belt.
    s.fill_rect(1, 8, 6, 1, dark);
    // Legs.
    s.fill_rect(1, 9, 2, 2, body);
    s.fill_rect(5, 9, 2, 2, body);
    s.set(1, 9, dark);
    s.set(6, 9, dark);
    // Boots.
    s.fill_rect(1, 10, 2, 1, dark);
    s.fill_rect(5, 10, 2, 1, dark);
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
    // Legacy hardcoded builders — used when no ASCII-art override is
    // registered in the content pack. See `enemy_sprite_from_content`
    // for the art-aware variant.
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
        Archetype::Eater => eater(),
        Archetype::Breacher => breacher(),
        Archetype::Rocketeer => rocketeer(),
        Archetype::Leaper => leaper(),
        Archetype::Sapper => sapper(),
        Archetype::Juggernaut => juggernaut(),
        Archetype::Howler => howler(),
        Archetype::Sentinel => sentinel(),
        Archetype::Splitter => splitter(),
        Archetype::Killa => killa(),
        Archetype::Zergling => zergling(),
        Archetype::Floodling => floodling(),
        Archetype::Flood => flood(),
        Archetype::FloodCarrier => flood_carrier(),
        // B4 new archetypes — MVP art: Headcrab reuses the Floodling
        // silhouette (they're mechanically cousins and visually similar),
        // Zombie reuses the Flood silhouette. Brand-level sprite_overrides
        // in half_life.toml point to distinct headcrab.art / headcrab_
        // zombie.art files when that brand is active, so in practice
        // players see dedicated art via the existing override path.
        Archetype::Headcrab => floodling(),
        Archetype::Zombie => flood(),
        // Rat + DireRat MVP art: reuse Swarmling (small scurrying) and
        // Splitter (larger biological) silhouettes. Dedicated rat art
        // lands with a future art pass.
        Archetype::Rat => swarmling(),
        Archetype::DireRat => splitter(),
        // PlayerTurret borrows the Sentinel silhouette recolored
        // survivor-cyan so friendly vs hostile turrets are
        // immediately distinguishable.
        Archetype::PlayerTurret => {
            let mut s = sentinel();
            s.tint_toward(Pixel::rgb(80, 220, 255), 0.55);
            s
        }
        // Phaser: borrows the Leaper silhouette tinted violet to
        // signal the teleport flavor. Real dedicated sprite lands
        // when the Phaser archetype gets its full polish pass.
        Archetype::Phaser => {
            let mut s = leaper();
            s.tint_toward(Pixel::rgb(160, 100, 240), 0.55);
            s
        }
    }
}

/// Preferred lookup: consults the content pack for an ASCII-art
/// sprite override first, falls back to the hardcoded Rust builder.
/// Clone is O(sprite-size) which is tiny (≤ a few hundred pixels).
pub fn enemy_sprite_from_content(
    archetype: Archetype,
    content: &crate::content::ContentDb,
) -> Sprite {
    enemy_sprite_branded(archetype, None, None, content)
}

/// Resolve a sprite with brand + unit preferences. Lookup order:
///   1. `(brand_id, unit_id)` BrandUnit sprite — the strongest
///      precedence. Lets a brand ship multiple distinct units
///      backed by the same archetype (pistol_scav vs shotgun_scav
///      both using Rusher) with distinct silhouettes each.
///   2. `(brand_id, archetype)` legacy brand-level override from the
///      pre-BrandUnit `sprite_overrides` table. Backward compat.
///   3. `archetype_sprites[archetype]` archetype-wide art.
///   4. Hardcoded Rust builder.
pub fn enemy_sprite_branded(
    archetype: Archetype,
    brand_id: Option<&str>,
    unit_id: Option<&str>,
    content: &crate::content::ContentDb,
) -> Sprite {
    if let (Some(bid), Some(uid)) = (brand_id, unit_id) {
        if let Some(art) = content
            .brand_unit_sprites
            .get(&(bid.to_string(), uid.to_string()))
        {
            return art.clone();
        }
    }
    if let Some(bid) = brand_id {
        if let Some(art) = content
            .brand_sprites
            .get(&(bid.to_string(), archetype))
        {
            return art.clone();
        }
    }
    if let Some(art) = content.archetype_sprites.get(&archetype) {
        return art.clone();
    }
    enemy_sprite(archetype)
}

fn rusher() -> Sprite {
    let body = Pixel::rgb(255, 60, 60);
    let dark = Pixel::rgb(130, 20, 30);
    let horn = Pixel::rgb(230, 200, 170);
    let horn_dark = Pixel::rgb(140, 100, 80);
    let eye = Pixel::rgb(255, 230, 120);

    // 8x10 — Doom imp. Twin curling horns, glowing eyes, hunched shoulders,
    // clawed feet. Narrower at the bottom so the silhouette reads demon,
    // not generic humanoid.
    let mut s = Sprite::new(8, 10);
    // Curled horns rising off the skull.
    s.set(1, 0, horn);
    s.set(6, 0, horn);
    s.set(0, 1, horn_dark);
    s.set(1, 1, horn);
    s.set(6, 1, horn);
    s.set(7, 1, horn_dark);
    // Head — skull with a heavy brow ridge.
    s.fill_rect(2, 1, 4, 3, body);
    s.fill_rect(2, 1, 4, 1, dark);
    // Glowing eyes, fanged mouth.
    s.set(2, 2, eye);
    s.set(3, 2, eye);
    s.set(4, 2, eye);
    s.set(5, 2, eye);
    s.set(3, 3, dark);
    s.set(4, 3, dark);
    // Shoulders — hunched, wider than head.
    s.fill_rect(0, 4, 8, 2, body);
    s.set(0, 4, dark);
    s.set(7, 4, dark);
    s.set(1, 4, dark);
    s.set(6, 4, dark);
    // Torso — narrowing.
    s.fill_rect(1, 6, 6, 2, body);
    s.set(1, 6, dark);
    s.set(6, 6, dark);
    // Spine/chest shadow.
    s.set(3, 6, dark);
    s.set(4, 7, dark);
    // Clawed legs, splayed apart.
    s.set(1, 8, dark);
    s.set(2, 8, dark);
    s.set(5, 8, dark);
    s.set(6, 8, dark);
    // Claws at the feet.
    s.set(0, 9, horn);
    s.set(2, 9, horn_dark);
    s.set(5, 9, horn_dark);
    s.set(7, 9, horn);
    s
}

fn pinkie() -> Sprite {
    let body = Pixel::rgb(255, 130, 170);
    let dark = Pixel::rgb(160, 60, 90);
    let accent = Pixel::rgb(255, 200, 220);
    let eye = Pixel::rgb(255, 230, 100);
    let tusk = Pixel::rgb(240, 240, 230);
    let maw = Pixel::rgb(60, 10, 30);

    // 12x11 — Doom pinkie. Wide gaping maw with tusks, heavy bull-like
    // body, short stout legs. Low silhouette that reads "charge tank".
    let mut s = Sprite::new(12, 11);
    // Huge jawed head — splits the top half.
    s.fill_rect(1, 0, 10, 2, dark);
    s.fill_rect(2, 1, 8, 1, body);
    // Beady glowing eyes set high.
    s.set(3, 1, eye);
    s.set(8, 1, eye);
    // Gaping maw — dark rectangle across the jaw line.
    s.fill_rect(2, 2, 8, 3, maw);
    // Teeth top row.
    s.set(3, 2, tusk);
    s.set(5, 2, tusk);
    s.set(6, 2, tusk);
    s.set(8, 2, tusk);
    // Tusks jutting from jaw corners.
    s.set(1, 2, tusk);
    s.set(10, 2, tusk);
    s.set(1, 3, tusk);
    s.set(10, 3, tusk);
    s.set(2, 4, tusk);
    s.set(9, 4, tusk);
    // Lower jaw/lip.
    s.fill_rect(3, 4, 6, 1, dark);
    // Heavy shoulders sloping down.
    s.fill_rect(0, 5, 12, 2, body);
    s.set(0, 5, dark);
    s.set(11, 5, dark);
    // Torso proper — flanks with ribbed shading.
    s.fill_rect(1, 7, 10, 2, body);
    s.set(1, 7, dark);
    s.set(10, 7, dark);
    // Belly highlight.
    s.fill_rect(4, 7, 4, 1, accent);
    s.set(5, 8, accent);
    s.set(6, 8, accent);
    // Stout legs spaced wide, hooves at the base.
    s.fill_rect(1, 9, 3, 2, dark);
    s.fill_rect(8, 9, 3, 2, dark);
    s.set(1, 10, tusk);
    s.set(3, 10, tusk);
    s.set(8, 10, tusk);
    s.set(10, 10, tusk);
    s
}

fn charger() -> Sprite {
    let body = Pixel::rgb(255, 100, 80);
    let dark = Pixel::rgb(150, 40, 30);
    let horn = Pixel::rgb(255, 240, 210);
    let horn_dark = Pixel::rgb(170, 140, 100);
    let eye = Pixel::rgb(255, 220, 100);
    let hoof = Pixel::rgb(80, 40, 30);

    // 10x10 — angry red bull. Forward-sloped horns like a ram mid-charge,
    // shoulders hunched forward, sprint legs splayed. Reads running.
    let mut s = Sprite::new(10, 10);
    // Forward-sloped horns — long curved shapes leading the charge.
    s.set(0, 2, horn);
    s.set(1, 1, horn);
    s.set(2, 1, horn_dark);
    s.set(9, 2, horn);
    s.set(8, 1, horn);
    s.set(7, 1, horn_dark);
    // Head — wedge shape sloping forward.
    s.fill_rect(2, 2, 6, 3, dark);
    s.fill_rect(3, 2, 4, 1, body);
    // Blazing eyes.
    s.set(3, 3, eye);
    s.set(6, 3, eye);
    // Snout / nostrils.
    s.set(4, 4, horn_dark);
    s.set(5, 4, horn_dark);
    // Shoulder hump.
    s.fill_rect(1, 5, 8, 2, body);
    s.set(1, 5, dark);
    s.set(8, 5, dark);
    // Chest shadow — muscle striation.
    s.set(3, 6, dark);
    s.set(4, 6, dark);
    s.set(5, 6, dark);
    s.set(6, 6, dark);
    // Flanks.
    s.fill_rect(2, 7, 6, 1, body);
    s.set(2, 7, dark);
    s.set(7, 7, dark);
    // Sprint legs — front pair forward, back pair back.
    s.set(0, 8, dark);
    s.set(1, 8, dark);
    s.set(2, 8, dark);
    s.set(7, 8, dark);
    s.set(8, 8, dark);
    s.set(9, 8, dark);
    // Hooves.
    s.set(0, 9, hoof);
    s.set(2, 9, hoof);
    s.set(7, 9, hoof);
    s.set(9, 9, hoof);
    s
}

fn revenant() -> Sprite {
    let bone = Pixel::rgb(220, 230, 240);
    let bone_shade = Pixel::rgb(160, 170, 190);
    let dark = Pixel::rgb(70, 90, 120);
    let glow = Pixel::rgb(120, 200, 255);
    let eye = Pixel::rgb(255, 160, 80);
    let tube = Pixel::rgb(90, 100, 120);

    // 8x12 — skeletal revenant with twin shoulder-mounted rocket tubes.
    // Wide shoulder rack clearly flags him as a ranged threat; skull +
    // ribcage read as bone at any zoom.
    let mut s = Sprite::new(8, 12);
    // Skull — wider top, narrow jaw.
    s.fill_rect(2, 0, 4, 3, bone);
    s.set(2, 0, bone_shade);
    s.set(5, 0, bone_shade);
    // Eye sockets — twin burning pits.
    s.set(3, 1, eye);
    s.set(4, 1, eye);
    // Jaw line / teeth.
    s.set(2, 2, dark);
    s.set(3, 2, bone_shade);
    s.set(4, 2, bone_shade);
    s.set(5, 2, dark);
    // Narrow jawbone.
    s.fill_rect(3, 3, 2, 1, bone_shade);
    // Shoulder rack — twin rocket tubes on either side of the torso.
    s.fill_rect(0, 4, 2, 2, tube);
    s.fill_rect(6, 4, 2, 2, tube);
    s.set(0, 4, glow);
    s.set(7, 4, glow);
    s.set(1, 5, dark);
    s.set(6, 5, dark);
    // Collarbone / clavicle.
    s.fill_rect(2, 4, 4, 1, bone);
    // Ribcage — bones with gaps.
    s.fill_rect(2, 5, 4, 4, bone);
    s.set(2, 5, dark);
    s.set(5, 5, dark);
    s.set(3, 6, dark);
    s.set(4, 6, dark);
    s.set(2, 7, dark);
    s.set(5, 7, dark);
    s.set(3, 8, dark);
    s.set(4, 8, dark);
    // Pelvis / spine knob.
    s.fill_rect(3, 9, 2, 1, bone_shade);
    // Spindly leg bones.
    s.set(2, 9, dark);
    s.set(5, 9, dark);
    s.set(2, 10, bone);
    s.set(3, 10, dark);
    s.set(4, 10, dark);
    s.set(5, 10, bone);
    // Bony feet.
    s.set(1, 11, bone);
    s.set(2, 11, bone_shade);
    s.set(5, 11, bone_shade);
    s.set(6, 11, bone);
    s
}

fn marksman() -> Sprite {
    let body = Pixel::rgb(150, 170, 100);
    let dark = Pixel::rgb(70, 90, 40);
    let helmet = Pixel::rgb(180, 200, 130);
    let visor = Pixel::rgb(90, 180, 120);
    let rifle = Pixel::rgb(60, 55, 45);
    let muzzle = Pixel::rgb(255, 240, 80);

    // 9×12 — tactical rifleman. Helmet with a green-tinted visor band,
    // plate carrier, rifle held diagonally across the chest, legs in a
    // braced firing stance.
    let mut s = Sprite::new(9, 12);
    // Helmet crown with chinstrap shadow.
    s.fill_rect(2, 0, 5, 2, helmet);
    s.set(2, 0, dark);
    s.set(6, 0, dark);
    // Visor band — full-face, horizontal green slit.
    s.fill_rect(2, 2, 5, 1, dark);
    s.set(3, 2, visor);
    s.set(4, 2, visor);
    s.set(5, 2, visor);
    // Neck / chinstrap.
    s.set(4, 3, dark);
    // Shoulders + plate carrier.
    s.fill_rect(1, 3, 7, 1, dark);
    s.fill_rect(1, 4, 7, 4, body);
    // Chest plate with MOLLE shadow lines.
    s.fill_rect(3, 5, 3, 3, dark);
    s.set(4, 6, body);
    // Torso outline for silhouette.
    s.set(1, 4, dark);
    s.set(7, 4, dark);
    s.set(1, 7, dark);
    s.set(7, 7, dark);
    // Rifle slung diagonally across the body — stock high-left, barrel
    // extending to the low-right.
    s.set(1, 5, rifle);
    s.set(2, 5, rifle);
    s.set(5, 6, rifle);
    s.set(6, 6, rifle);
    s.set(7, 6, rifle);
    s.set(8, 7, muzzle);
    // Belt line.
    s.fill_rect(2, 8, 5, 1, dark);
    // Legs — wide braced firing stance.
    s.fill_rect(2, 9, 2, 3, body);
    s.fill_rect(5, 9, 2, 3, body);
    s.set(2, 9, dark);
    s.set(6, 9, dark);
    // Boots.
    s.fill_rect(2, 11, 2, 1, dark);
    s.fill_rect(5, 11, 2, 1, dark);
    s
}

fn pmc() -> Sprite {
    let plate = Pixel::rgb(120, 140, 170);
    let dark = Pixel::rgb(50, 70, 100);
    let accent = Pixel::rgb(160, 180, 210);
    let visor = Pixel::rgb(70, 180, 255);
    let glow = Pixel::rgb(180, 230, 255);

    // 11×12 — heavy armored operator. Wide pauldrons, full-face helmet
    // with a cybernetic visor glow, segmented plate torso.
    let mut s = Sprite::new(11, 12);
    // Helmet crown.
    s.fill_rect(3, 0, 5, 3, dark);
    s.set(3, 0, plate);
    s.set(7, 0, plate);
    // Cybernetic visor — bright cyan with a glow highlight.
    s.fill_rect(3, 1, 5, 1, visor);
    s.set(4, 1, glow);
    s.set(6, 1, glow);
    // Helmet chin.
    s.fill_rect(4, 2, 3, 1, dark);
    // Wide pauldrons extending past the torso width.
    s.fill_rect(0, 3, 11, 2, dark);
    s.fill_rect(0, 3, 3, 2, plate);
    s.fill_rect(8, 3, 3, 2, plate);
    s.set(0, 3, dark);
    s.set(10, 3, dark);
    // Torso — layered plates.
    s.fill_rect(2, 5, 7, 5, plate);
    // Chest rig (lighter accent panel).
    s.fill_rect(3, 5, 5, 4, accent);
    // Central armored core block.
    s.fill_rect(4, 6, 3, 2, dark);
    s.set(5, 7, glow);
    // Ammo pouches along the belt.
    s.fill_rect(2, 9, 7, 1, dark);
    s.set(3, 9, accent);
    s.set(5, 9, accent);
    s.set(7, 9, accent);
    // Side outlines.
    for y in 5..10 {
        s.set(2, y, dark);
        s.set(8, y, dark);
    }
    // Thick boots.
    s.fill_rect(2, 10, 3, 2, dark);
    s.fill_rect(6, 10, 3, 2, dark);
    s.set(3, 11, plate);
    s.set(7, 11, plate);
    s
}

fn swarmling() -> Sprite {
    let body = Pixel::rgb(255, 80, 40);
    let dark = Pixel::rgb(140, 20, 10);
    let eye = Pixel::rgb(255, 220, 80);
    let fang = Pixel::rgb(255, 240, 200);

    // 5×5 — tiny scuttling swarm bug. Two antennae, beady eyes, tiny
    // fangs, splayed legs at the bottom.
    let mut s = Sprite::new(5, 5);
    // Antennae.
    s.set(0, 0, dark);
    s.set(4, 0, dark);
    // Head band.
    s.fill_rect(1, 0, 3, 1, dark);
    // Body bulge.
    s.fill_rect(0, 1, 5, 2, body);
    s.set(0, 1, dark);
    s.set(4, 1, dark);
    // Eyes + fangs.
    s.set(1, 1, eye);
    s.set(3, 1, eye);
    s.set(2, 2, fang);
    // Belly band.
    s.fill_rect(1, 3, 3, 1, dark);
    // Splayed legs.
    s.set(0, 4, dark);
    s.set(2, 4, dark);
    s.set(4, 4, dark);
    s
}

fn orb() -> Sprite {
    let body = Pixel::rgb(200, 120, 255);
    let core = Pixel::rgb(255, 200, 255);
    let dark = Pixel::rgb(90, 40, 150);
    let glow = Pixel::rgb(230, 180, 255);
    let pupil = Pixel::rgb(40, 10, 80);

    // 8x8 — floating eye/halo. Roundest possible silhouette at this
    // resolution: chamfered corners. Central pupil reads as an eye;
    // outer ring pulses with glow. Symmetric so rotation is irrelevant.
    let mut s = Sprite::new(8, 8);
    // Outer halo ring — chamfered square to approximate a circle.
    // Top/bottom edges (inset corners).
    s.set(2, 0, dark);
    s.set(3, 0, dark);
    s.set(4, 0, dark);
    s.set(5, 0, dark);
    s.set(2, 7, dark);
    s.set(3, 7, dark);
    s.set(4, 7, dark);
    s.set(5, 7, dark);
    // Side edges.
    s.set(0, 2, dark);
    s.set(0, 3, dark);
    s.set(0, 4, dark);
    s.set(0, 5, dark);
    s.set(7, 2, dark);
    s.set(7, 3, dark);
    s.set(7, 4, dark);
    s.set(7, 5, dark);
    // Corner chamfers.
    s.set(1, 1, dark);
    s.set(6, 1, dark);
    s.set(1, 6, dark);
    s.set(6, 6, dark);
    // Body fill — inside the ring.
    s.fill_rect(2, 1, 4, 1, body);
    s.fill_rect(1, 2, 6, 4, body);
    s.fill_rect(2, 6, 4, 1, body);
    // Inner glow ring — one pixel inside the dark ring.
    s.set(3, 1, glow);
    s.set(4, 1, glow);
    s.set(1, 3, glow);
    s.set(1, 4, glow);
    s.set(6, 3, glow);
    s.set(6, 4, glow);
    s.set(3, 6, glow);
    s.set(4, 6, glow);
    // Bright core — iris of the eye.
    s.fill_rect(2, 2, 4, 4, core);
    // Defined pupil — the eye's focus point.
    s.set(3, 3, pupil);
    s.set(4, 3, pupil);
    s.set(3, 4, pupil);
    s.set(4, 4, pupil);
    // Highlight glint on the iris (top-left).
    s.set(2, 2, glow);
    s
}

fn eater() -> Sprite {
    let flesh = Pixel::rgb(140, 30, 180);
    let dark = Pixel::rgb(60, 10, 80);
    let bone = Pixel::rgb(230, 210, 240);
    let maw = Pixel::rgb(40, 0, 30);
    let eye = Pixel::rgb(255, 200, 80);
    let bone_shade = Pixel::rgb(170, 140, 180);

    // 12×14 — purple corrupted humanoid. Wide gaping maw full of teeth,
    // hunched shoulders with spinal ridges, long clawed arms dangling
    // past the torso, stubby legs.
    let mut s = Sprite::new(12, 14);
    // Hunched back spinal ridges.
    s.set(3, 0, dark);
    s.set(5, 0, dark);
    s.set(6, 0, dark);
    s.set(8, 0, dark);
    // Skull crown.
    s.fill_rect(2, 1, 8, 2, flesh);
    s.set(2, 1, dark);
    s.set(9, 1, dark);
    // Burning yellow eyes set deep.
    s.set(3, 2, eye);
    s.set(4, 2, eye);
    s.set(7, 2, eye);
    s.set(8, 2, eye);
    // Wide open maw.
    s.fill_rect(3, 3, 6, 3, maw);
    // Upper row of teeth.
    s.set(3, 3, bone);
    s.set(5, 3, bone);
    s.set(6, 3, bone);
    s.set(8, 3, bone);
    // Lower row of teeth.
    s.set(4, 5, bone_shade);
    s.set(7, 5, bone_shade);
    // Tusks jutting from maw corners.
    s.set(2, 3, bone);
    s.set(9, 3, bone);
    s.set(2, 4, bone);
    s.set(9, 4, bone);
    s.set(1, 5, bone_shade);
    s.set(10, 5, bone_shade);
    // Hunched shoulders wider than head.
    s.fill_rect(0, 6, 12, 1, dark);
    // Heavy torso with shadow core.
    s.fill_rect(1, 6, 10, 5, flesh);
    s.fill_rect(3, 7, 6, 3, dark);
    s.set(5, 8, flesh);
    s.set(6, 8, flesh);
    // Long clawed arms reaching down.
    s.fill_rect(0, 7, 2, 5, dark);
    s.fill_rect(10, 7, 2, 5, dark);
    // Clawed hands at the end of arms.
    s.set(0, 12, bone);
    s.set(1, 12, bone_shade);
    s.set(10, 12, bone_shade);
    s.set(11, 12, bone);
    // Pelvis belt.
    s.fill_rect(2, 11, 8, 1, dark);
    // Stubby legs.
    s.fill_rect(2, 12, 3, 2, flesh);
    s.fill_rect(7, 12, 3, 2, flesh);
    s.set(2, 12, dark);
    s.set(4, 12, dark);
    s.set(7, 12, dark);
    s.set(9, 12, dark);
    // Clawed feet.
    s.set(2, 13, bone);
    s.set(4, 13, bone);
    s.set(7, 13, bone);
    s.set(9, 13, bone);
    s
}

fn breacher() -> Sprite {
    let body = Pixel::rgb(160, 80, 40);
    let dark = Pixel::rgb(80, 40, 20);
    let iron = Pixel::rgb(60, 60, 70);
    let spark = Pixel::rgb(255, 200, 120);
    let eye = Pixel::rgb(255, 60, 40);

    // 10×12 — squat armored wall-smasher. Riveted helm with horizontal
    // visor slit, oversized hammer-fist pauldron on the right side, small
    // clenched fist on the left.
    let mut s = Sprite::new(10, 12);
    // Helmet crown with rivets.
    s.fill_rect(2, 0, 6, 2, iron);
    s.set(2, 0, dark);
    s.set(7, 0, dark);
    s.set(3, 1, spark);
    s.set(4, 1, dark);
    s.set(5, 1, dark);
    s.set(6, 1, spark);
    // Visor slit — thick horizontal band with two glowing eye dots.
    s.fill_rect(2, 2, 6, 1, dark);
    s.set(3, 2, eye);
    s.set(6, 2, eye);
    // Jaw shadow.
    s.fill_rect(3, 3, 4, 1, dark);
    // Wide armored shoulders.
    s.fill_rect(0, 4, 10, 1, iron);
    s.set(0, 4, dark);
    s.set(9, 4, dark);
    // Torso plate with central dark core.
    s.fill_rect(1, 5, 8, 4, body);
    s.fill_rect(3, 6, 4, 2, dark);
    s.set(4, 7, spark);
    // Rivets on the chest corners.
    s.set(2, 5, spark);
    s.set(7, 5, spark);
    // Oversized hammer-fist on the right — extends past body silhouette.
    s.fill_rect(7, 4, 3, 6, iron);
    s.fill_rect(8, 5, 2, 4, dark);
    s.set(8, 6, spark);
    s.set(9, 7, spark);
    s.set(7, 9, dark);
    s.set(9, 9, dark);
    // Smaller left arm with fist.
    s.fill_rect(0, 5, 2, 3, dark);
    s.set(0, 8, iron);
    s.set(1, 8, iron);
    // Belt line.
    s.fill_rect(2, 9, 5, 1, dark);
    // Legs.
    s.fill_rect(2, 10, 2, 2, dark);
    s.fill_rect(6, 10, 2, 2, dark);
    s.set(2, 11, iron);
    s.set(3, 11, iron);
    s.set(6, 11, iron);
    s.set(7, 11, iron);
    s
}

fn leaper() -> Sprite {
    let body = Pixel::rgb(180, 60, 100);
    let dark = Pixel::rgb(80, 20, 50);
    let claw = Pixel::rgb(230, 230, 230);
    let claw_shade = Pixel::rgb(170, 160, 170);
    let eye = Pixel::rgb(255, 220, 80);

    // 8×7 — low hunched pouncer. Twin eyes on a compact head, extended
    // claws reaching forward on both sides, coiled back haunches ready
    // to launch.
    let mut s = Sprite::new(8, 7);
    // Spiked head crown.
    s.set(3, 0, dark);
    s.set(4, 0, dark);
    // Head with twin eyes.
    s.fill_rect(2, 1, 4, 2, body);
    s.set(2, 1, dark);
    s.set(5, 1, dark);
    s.set(3, 2, eye);
    s.set(4, 2, eye);
    // Forward-reaching claws — two-cell talons on each side.
    s.set(0, 2, claw);
    s.set(7, 2, claw);
    s.set(0, 3, claw_shade);
    s.set(7, 3, claw_shade);
    s.set(1, 3, claw);
    s.set(6, 3, claw);
    // Sinewy body with spine shadow.
    s.fill_rect(1, 4, 6, 1, body);
    s.set(1, 4, dark);
    s.set(6, 4, dark);
    s.set(3, 4, dark);
    s.set(4, 4, dark);
    // Coiled back haunches — bunched high.
    s.set(0, 5, dark);
    s.set(1, 5, body);
    s.set(2, 5, dark);
    s.set(5, 5, dark);
    s.set(6, 5, body);
    s.set(7, 5, dark);
    // Clawed feet.
    s.set(0, 6, claw_shade);
    s.set(2, 6, claw_shade);
    s.set(5, 6, claw_shade);
    s.set(7, 6, claw_shade);
    s
}

fn rocketeer() -> Sprite {
    let body = Pixel::rgb(110, 160, 200);
    let dark = Pixel::rgb(40, 70, 90);
    let tube = Pixel::rgb(70, 80, 95);
    let warn = Pixel::rgb(255, 200, 70);
    let eye = Pixel::rgb(255, 100, 60);
    let accent = Pixel::rgb(200, 230, 255);

    // 11×11 — lean operator with a shoulder-mounted launcher reaching
    // across the full width. Visible warhead tip with warning stripe,
    // narrow torso for a clearly ranged silhouette.
    let mut s = Sprite::new(11, 11);
    // Helmet with brim.
    s.fill_rect(2, 0, 4, 3, body);
    s.set(2, 0, dark);
    s.set(5, 0, dark);
    // Visor slit.
    s.fill_rect(2, 2, 4, 1, dark);
    s.set(3, 1, eye);
    s.set(4, 1, eye);
    // Shoulder band.
    s.fill_rect(1, 3, 6, 1, dark);
    // Chest / vest.
    s.fill_rect(1, 4, 6, 1, body);
    s.set(1, 4, dark);
    // Launcher tube — extends from shoulder across and past the right
    // edge, with two warning stripes and a bright warhead tip.
    s.fill_rect(3, 3, 8, 2, tube);
    s.fill_rect(3, 3, 8, 1, dark);
    s.set(7, 4, warn);
    s.set(9, 4, warn);
    // Warhead tip (rightmost cell).
    s.set(10, 3, warn);
    s.set(10, 4, accent);
    // Torso under the launcher.
    s.fill_rect(1, 5, 5, 3, body);
    s.fill_rect(2, 6, 3, 1, accent);
    s.set(1, 5, dark);
    s.set(5, 5, dark);
    s.set(1, 7, dark);
    s.set(5, 7, dark);
    // Belt.
    s.fill_rect(2, 7, 3, 1, dark);
    // Thin legs.
    s.fill_rect(2, 8, 1, 3, body);
    s.fill_rect(4, 8, 1, 3, body);
    s.set(2, 8, dark);
    s.set(4, 8, dark);
    // Boots.
    s.set(2, 10, dark);
    s.set(4, 10, dark);
    s
}

fn sapper() -> Sprite {
    let body = Pixel::rgb(255, 120, 30);
    let dark = Pixel::rgb(100, 40, 8);
    let fuse = Pixel::rgb(255, 230, 100);
    let canister = Pixel::rgb(160, 60, 20);
    let eye = Pixel::rgb(255, 255, 220);
    let cloth = Pixel::rgb(70, 45, 30);

    // 8×10 — suicide bomber humanoid. Lit fuse sparking off the top of
    // the head, dirty hood, TNT canister belt strapped across the chest
    // with three canisters, grimy trousers.
    let mut s = Sprite::new(8, 10);
    // Fuse sparks above the head.
    s.set(3, 0, fuse);
    s.set(4, 0, eye);
    // Hooded head.
    s.fill_rect(2, 1, 4, 2, cloth);
    s.set(2, 1, dark);
    s.set(5, 1, dark);
    // Face / eyes peeking out.
    s.set(3, 2, eye);
    s.set(4, 2, eye);
    // Shoulders.
    s.fill_rect(1, 3, 6, 1, cloth);
    // TNT belt — three canister blocks across the chest with fuses on top.
    s.fill_rect(1, 4, 6, 2, canister);
    s.set(1, 4, dark);
    s.set(3, 4, dark);
    s.set(5, 4, dark);
    s.set(6, 4, dark);
    s.set(2, 3, fuse);
    s.set(4, 3, fuse);
    s.set(6, 3, fuse);
    // Canister stripes.
    s.fill_rect(1, 5, 6, 1, body);
    s.set(2, 5, dark);
    s.set(4, 5, dark);
    s.set(6, 5, dark);
    // Grimy torso below belt.
    s.fill_rect(2, 6, 4, 1, cloth);
    // Dirty trousers.
    s.fill_rect(2, 7, 2, 2, cloth);
    s.fill_rect(4, 7, 2, 2, cloth);
    s.set(3, 8, dark);
    s.set(4, 8, dark);
    // Worn boots.
    s.fill_rect(2, 9, 2, 1, dark);
    s.fill_rect(4, 9, 2, 1, dark);
    s
}

fn juggernaut() -> Sprite {
    let plate = Pixel::rgb(120, 120, 140);
    let dark = Pixel::rgb(40, 40, 55);
    let rivet = Pixel::rgb(220, 220, 220);
    let eye = Pixel::rgb(255, 60, 40);
    let shadow = Pixel::rgb(75, 75, 95);

    // 15×14 — hulking mech. Small helm sunk between massive pauldrons,
    // armored chest plate with rivet grid, enormous low-hanging fists,
    // tree-trunk legs with kneeplates.
    let mut s = Sprite::new(15, 14);
    // Helm sunk into shoulders.
    s.fill_rect(5, 0, 5, 2, dark);
    s.set(5, 0, shadow);
    s.set(9, 0, shadow);
    // Twin glowing optics.
    s.set(6, 1, eye);
    s.set(7, 1, eye);
    s.set(8, 1, eye);
    // Helm chin band.
    s.fill_rect(6, 2, 3, 1, shadow);
    // Massive pauldrons extending past body width.
    s.fill_rect(0, 2, 4, 4, plate);
    s.fill_rect(11, 2, 4, 4, plate);
    s.set(0, 2, dark);
    s.set(14, 2, dark);
    s.set(0, 5, dark);
    s.set(14, 5, dark);
    // Pauldron rivets.
    s.set(1, 3, rivet);
    s.set(13, 3, rivet);
    // Armored torso.
    s.fill_rect(3, 2, 9, 7, plate);
    // Central chest plate (darker) with rivet grid.
    s.fill_rect(4, 3, 7, 5, shadow);
    s.fill_rect(5, 4, 5, 3, dark);
    s.set(4, 3, rivet);
    s.set(10, 3, rivet);
    s.set(4, 7, rivet);
    s.set(10, 7, rivet);
    s.set(7, 5, eye);
    // Huge fists hanging low, extending past shoulder line.
    s.fill_rect(0, 6, 3, 4, dark);
    s.fill_rect(12, 6, 3, 4, dark);
    s.set(1, 7, plate);
    s.set(13, 7, plate);
    s.set(1, 8, rivet);
    s.set(13, 8, rivet);
    // Belt under torso.
    s.fill_rect(3, 9, 9, 1, dark);
    // Tree-trunk legs with kneeplates.
    s.fill_rect(3, 10, 4, 4, plate);
    s.fill_rect(8, 10, 4, 4, plate);
    s.fill_rect(4, 11, 2, 1, dark);
    s.fill_rect(9, 11, 2, 1, dark);
    // Feet.
    s.fill_rect(3, 13, 4, 1, dark);
    s.fill_rect(8, 13, 4, 1, dark);
    s
}

fn howler() -> Sprite {
    let robe = Pixel::rgb(240, 180, 220);
    let dark = Pixel::rgb(80, 20, 60);
    let mouth = Pixel::rgb(30, 10, 20);
    let eye = Pixel::rgb(255, 255, 200);
    let sigil = Pixel::rgb(255, 220, 120);
    let shadow = Pixel::rgb(150, 90, 150);

    // 8×12 — tall gaunt robed cultist. Pointed witch-hat hood tapering
    // to a single point, sunken hollow eyes, wide shrieking black maw,
    // glowing sigil stitched into the chest, tattered robe hem.
    let mut s = Sprite::new(8, 12);
    // Pointed hood tip.
    s.set(3, 0, dark);
    s.set(4, 0, dark);
    s.set(3, 1, dark);
    s.set(4, 1, dark);
    // Hood widening.
    s.fill_rect(2, 2, 4, 1, dark);
    s.fill_rect(1, 3, 6, 1, dark);
    // Inner face shadow inside the hood.
    s.set(3, 3, shadow);
    s.set(4, 3, shadow);
    // Hollow glowing eyes.
    s.set(2, 4, eye);
    s.set(5, 4, eye);
    s.fill_rect(3, 4, 2, 1, dark);
    // Wide shrieking maw.
    s.fill_rect(2, 5, 4, 2, mouth);
    s.set(3, 6, eye);
    s.set(4, 6, eye);
    // Draped robe — starts at shoulders and flares out.
    s.fill_rect(1, 7, 6, 1, robe);
    s.fill_rect(0, 8, 8, 3, robe);
    s.set(0, 8, dark);
    s.set(7, 8, dark);
    // Shadow fold down the center.
    s.set(3, 9, shadow);
    s.set(4, 9, shadow);
    // Glowing sigil on the chest.
    s.set(4, 8, sigil);
    s.set(3, 9, sigil);
    s.set(4, 9, sigil);
    s.set(3, 10, sigil);
    s.set(4, 10, sigil);
    // Fraying, tattered hem.
    s.set(0, 11, dark);
    s.set(1, 11, robe);
    s.set(3, 11, dark);
    s.set(4, 11, dark);
    s.set(6, 11, robe);
    s.set(7, 11, dark);
    s
}

fn sentinel() -> Sprite {
    let shell = Pixel::rgb(80, 200, 180);
    let dark = Pixel::rgb(20, 60, 70);
    let lens = Pixel::rgb(255, 80, 80);
    let tripod = Pixel::rgb(120, 130, 140);
    let warn = Pixel::rgb(255, 200, 60);

    // 10×11 — tripod turret. Round lens-head on a pivot, barrel extends
    // to the right with a warning tip, three splayed tripod legs bolted
    // to the ground.
    let mut s = Sprite::new(10, 11);
    // Round lens-head — chamfered square for a circular feel.
    s.set(2, 0, dark);
    s.set(3, 0, shell);
    s.set(4, 0, shell);
    s.set(5, 0, dark);
    s.fill_rect(1, 1, 6, 4, shell);
    s.set(1, 1, dark);
    s.set(6, 1, dark);
    s.set(1, 4, dark);
    s.set(6, 4, dark);
    // Lens cluster — dark housing with a glowing red iris.
    s.fill_rect(2, 2, 3, 2, dark);
    s.set(3, 2, lens);
    s.set(3, 3, lens);
    s.set(2, 2, warn);
    // Barrel extending rightward with warn-tipped muzzle.
    s.fill_rect(5, 2, 4, 2, dark);
    s.fill_rect(5, 2, 4, 1, tripod);
    s.set(8, 2, warn);
    s.set(9, 2, warn);
    s.set(9, 3, warn);
    // Pivot socket beneath the head.
    s.fill_rect(2, 5, 4, 2, dark);
    s.fill_rect(3, 5, 2, 1, tripod);
    // Central support column.
    s.fill_rect(3, 7, 2, 2, dark);
    // Three splayed tripod legs.
    s.set(1, 8, tripod);
    s.set(4, 8, tripod);
    s.set(7, 8, tripod);
    s.set(0, 9, tripod);
    s.set(4, 9, tripod);
    s.set(8, 9, tripod);
    // Foot pads.
    s.set(0, 10, dark);
    s.set(1, 10, dark);
    s.set(4, 10, dark);
    s.set(8, 10, dark);
    s.set(9, 10, dark);
    s
}

fn splitter() -> Sprite {
    let body = Pixel::rgb(170, 120, 60);
    let dark = Pixel::rgb(70, 40, 10);
    let seam = Pixel::rgb(255, 220, 100);
    let crack = Pixel::rgb(40, 20, 0);
    let eye = Pixel::rgb(255, 80, 40);

    // 8×8 — bulbous round creature. Chamfered circular silhouette,
    // glowing horizontal split-seam across the middle with dark cracks
    // radiating from it, twin eyes up top.
    let mut s = Sprite::new(8, 8);
    // Round body — chamfered square.
    s.fill_rect(2, 0, 4, 1, body);
    s.fill_rect(1, 1, 6, 1, body);
    s.fill_rect(0, 2, 8, 4, body);
    s.fill_rect(1, 6, 6, 1, body);
    s.fill_rect(2, 7, 4, 1, body);
    // Dark outline — chamfered corners and sides.
    s.set(2, 0, dark);
    s.set(5, 0, dark);
    s.set(1, 1, dark);
    s.set(6, 1, dark);
    s.set(0, 2, dark);
    s.set(7, 2, dark);
    s.set(0, 5, dark);
    s.set(7, 5, dark);
    s.set(1, 6, dark);
    s.set(6, 6, dark);
    s.set(2, 7, dark);
    s.set(5, 7, dark);
    // Split-line seam — bright glowing horizontal across the middle.
    for i in 0..8 {
        s.set(i, 4, seam);
    }
    s.set(0, 4, crack);
    s.set(7, 4, crack);
    // Seam highlight line above.
    s.set(2, 3, seam);
    s.set(5, 3, seam);
    // Dark cracks radiating from the seam.
    s.set(1, 3, crack);
    s.set(6, 3, crack);
    s.set(3, 5, crack);
    s.set(4, 5, crack);
    s.set(2, 6, crack);
    s.set(5, 6, crack);
    // Glowing eyes up top.
    s.set(2, 2, eye);
    s.set(5, 2, eye);
    s.set(3, 2, dark);
    s.set(4, 2, dark);
    s
}

fn killa() -> Sprite {
    let plate = Pixel::rgb(90, 130, 100);
    let dark = Pixel::rgb(30, 55, 35);
    let iron = Pixel::rgb(60, 60, 70);
    let lmg = Pixel::rgb(40, 40, 50);
    let muzzle = Pixel::rgb(255, 200, 60);
    let visor = Pixel::rgb(180, 40, 30);
    let strap = Pixel::rgb(130, 90, 50);
    let brass = Pixel::rgb(220, 180, 80);

    // 14×14 — Tarkov Killa: OD-green armored brute with a slit-visor
    // helm, plate carrier festooned with ammo belt + pouches, bulging
    // gear, LMG held horizontally across the body.
    let mut s = Sprite::new(14, 14);
    // Big iron helm with rivet studs.
    s.fill_rect(4, 0, 6, 3, iron);
    s.set(4, 0, dark);
    s.set(9, 0, dark);
    s.set(5, 0, brass);
    s.set(8, 0, brass);
    // Horizontal slit visor with a thin red glow inside.
    s.fill_rect(4, 2, 6, 1, dark);
    s.set(5, 2, visor);
    s.set(6, 2, visor);
    s.set(7, 2, visor);
    s.set(8, 2, visor);
    // Chinstrap band.
    s.fill_rect(5, 3, 4, 1, dark);
    // Massive shoulder pauldrons.
    s.fill_rect(1, 3, 4, 3, plate);
    s.fill_rect(9, 3, 4, 3, plate);
    s.set(1, 3, dark);
    s.set(12, 3, dark);
    s.set(1, 5, dark);
    s.set(12, 5, dark);
    // Plate carrier torso.
    s.fill_rect(3, 4, 8, 6, plate);
    s.fill_rect(4, 5, 6, 4, dark);
    // Ammo belt — diagonal brass rounds across the chest.
    for x in 3..11 {
        s.set(x, 6, strap);
    }
    s.set(4, 6, brass);
    s.set(6, 6, brass);
    s.set(8, 6, brass);
    // Bulging pouches on the vest.
    s.set(4, 7, strap);
    s.set(9, 7, strap);
    s.set(5, 8, brass);
    s.set(8, 8, brass);
    // LMG held across body — receiver mid-chest, barrel exiting right.
    s.fill_rect(5, 9, 9, 2, lmg);
    s.set(13, 9, muzzle);
    s.set(13, 10, muzzle);
    // Drum mag hanging beneath the receiver.
    s.set(7, 11, lmg);
    s.set(8, 11, lmg);
    // Front grip / forestock.
    s.set(10, 11, lmg);
    // Arm guards gripping the weapon.
    s.fill_rect(1, 6, 2, 4, dark);
    s.fill_rect(11, 7, 2, 3, dark);
    // Thick armored legs.
    s.fill_rect(3, 11, 3, 3, plate);
    s.fill_rect(8, 11, 3, 3, plate);
    s.set(3, 11, dark);
    s.set(5, 11, dark);
    s.set(8, 11, dark);
    s.set(10, 11, dark);
    // Chunky boots.
    s.fill_rect(3, 13, 3, 1, dark);
    s.fill_rect(8, 13, 3, 1, dark);
    s
}

fn zergling() -> Sprite {
    let body = Pixel::rgb(160, 80, 200);
    let dark = Pixel::rgb(60, 20, 90);
    let claw = Pixel::rgb(240, 220, 255);
    let claw_shade = Pixel::rgb(170, 150, 190);
    let eye = Pixel::rgb(255, 200, 80);

    // 7x6 — StarCraft zergling. Low hunched body, twin scythe claws sweep
    // out in front, back legs coiled. Wider than tall so it reads fast.
    let mut s = Sprite::new(7, 6);
    // Scythe claws out front — curved, point toward attack direction.
    s.set(0, 0, claw);
    s.set(6, 0, claw);
    s.set(0, 1, claw_shade);
    s.set(1, 1, claw);
    s.set(5, 1, claw);
    s.set(6, 1, claw_shade);
    // Spiked carapace top.
    s.set(3, 0, dark);
    // Head — narrow, set between claws.
    s.fill_rect(2, 1, 3, 1, dark);
    s.set(2, 2, body);
    s.set(4, 2, body);
    // Glowing eyes.
    s.set(3, 2, eye);
    // Main body — low, flat, wider in the middle.
    s.fill_rect(1, 3, 5, 1, body);
    s.set(1, 3, dark);
    s.set(5, 3, dark);
    // Spine ridge.
    s.set(3, 3, dark);
    // Back haunches — coiled legs at the rear.
    s.set(0, 4, dark);
    s.set(1, 4, body);
    s.set(2, 4, dark);
    s.set(4, 4, dark);
    s.set(5, 4, body);
    s.set(6, 4, dark);
    // Clawed feet.
    s.set(0, 5, claw_shade);
    s.set(2, 5, claw_shade);
    s.set(4, 5, claw_shade);
    s.set(6, 5, claw_shade);
    s
}

fn floodling() -> Sprite {
    let body = Pixel::rgb(180, 220, 120);
    let dark = Pixel::rgb(60, 110, 40);
    let vein = Pixel::rgb(240, 180, 200);
    let eye = Pixel::rgb(255, 100, 80);

    // 5×5 — Halo parasite infection form. Tadpole body, pink veins
    // running along the sides, twin eyes up top, tendril tail.
    let mut s = Sprite::new(5, 5);
    // Head dome.
    s.set(1, 0, dark);
    s.set(2, 0, body);
    s.set(3, 0, dark);
    // Body upper — with eyes.
    s.fill_rect(0, 1, 5, 1, body);
    s.set(0, 1, dark);
    s.set(4, 1, dark);
    s.set(1, 1, eye);
    s.set(3, 1, eye);
    // Body mid with vein stripes.
    s.fill_rect(0, 2, 5, 1, body);
    s.set(0, 2, vein);
    s.set(4, 2, vein);
    s.set(2, 2, vein);
    // Tail taper.
    s.set(1, 3, body);
    s.set(2, 3, dark);
    s.set(3, 3, body);
    // Tendril tail tips.
    s.set(0, 4, dark);
    s.set(2, 4, vein);
    s.set(4, 4, dark);
    s
}

fn flood() -> Sprite {
    let body = Pixel::rgb(120, 255, 140);
    let dark = Pixel::rgb(30, 90, 40);
    let vein = Pixel::rgb(255, 180, 220);
    let eye = Pixel::rgb(255, 40, 40);
    let claw = Pixel::rgb(230, 255, 200);

    // 8×10 — Flood combat form. Warped asymmetric humanoid with an
    // elongated tendril arm extending right and claws out left, pink
    // veins and pustules running across the torso, twisted legs.
    let mut s = Sprite::new(8, 10);
    // Lopsided head with a pustule growth.
    s.set(2, 0, dark);
    s.set(3, 0, vein);
    s.set(4, 0, dark);
    s.fill_rect(2, 1, 4, 2, body);
    s.set(2, 1, dark);
    // Burning red eyes.
    s.set(3, 1, eye);
    s.set(5, 1, eye);
    // Jaw.
    s.set(3, 2, dark);
    s.set(4, 2, dark);
    // Pustule growths on shoulders.
    s.set(0, 3, vein);
    s.set(1, 3, dark);
    s.set(6, 3, dark);
    s.set(7, 3, vein);
    // Twisted torso.
    s.fill_rect(1, 3, 6, 4, body);
    s.fill_rect(2, 4, 4, 2, dark);
    // Pustules on chest.
    s.set(3, 4, vein);
    s.set(4, 5, vein);
    // Left arm — short with clawed hand.
    s.set(0, 4, dark);
    s.set(0, 5, claw);
    s.set(1, 5, claw);
    // Elongated tendril arm to the right — multi-segment, ending in
    // long claws.
    s.set(7, 4, dark);
    s.set(7, 5, vein);
    s.set(7, 6, dark);
    s.set(6, 6, claw);
    s.set(7, 7, claw);
    // Belt transition.
    s.fill_rect(2, 6, 4, 1, dark);
    // Twisted legs.
    s.fill_rect(2, 7, 2, 3, body);
    s.fill_rect(4, 7, 2, 3, body);
    s.set(2, 7, dark);
    s.set(5, 7, dark);
    s.set(3, 8, dark);
    s.set(4, 8, dark);
    // Feet.
    s.set(2, 9, claw);
    s.set(5, 9, claw);
    s
}

fn flood_carrier() -> Sprite {
    let sac = Pixel::rgb(200, 240, 150);
    let dark = Pixel::rgb(90, 140, 60);
    let vein = Pixel::rgb(255, 180, 220);
    let pustule = Pixel::rgb(255, 220, 120);
    let drip = Pixel::rgb(180, 60, 100);

    // 10×11 — bloated walking sac. Enormous round body clearly reading
    // "bomb", pustules breaking through the skin, pink veins snaking
    // across, dripping fluid from underneath, tiny useless legs.
    let mut s = Sprite::new(10, 11);
    // Stubby head on top.
    s.fill_rect(4, 0, 2, 1, dark);
    s.fill_rect(3, 1, 4, 1, sac);
    s.set(3, 1, dark);
    s.set(6, 1, dark);
    // Huge bloated sac body — chamfered corners for a rounder read.
    s.fill_rect(2, 2, 6, 1, sac);
    s.fill_rect(1, 3, 8, 5, sac);
    s.fill_rect(2, 8, 6, 1, sac);
    // Outline.
    s.set(2, 2, dark);
    s.set(7, 2, dark);
    s.set(1, 3, dark);
    s.set(8, 3, dark);
    s.set(1, 7, dark);
    s.set(8, 7, dark);
    s.set(2, 8, dark);
    s.set(7, 8, dark);
    // Interior shadow.
    s.fill_rect(3, 4, 4, 3, dark);
    // Pustules bursting through the skin.
    s.set(2, 4, pustule);
    s.set(7, 4, pustule);
    s.set(4, 3, pustule);
    s.set(5, 5, pustule);
    s.set(3, 6, pustule);
    s.set(6, 6, pustule);
    s.set(4, 7, pustule);
    // Pink veins snaking across the sac.
    s.set(3, 3, vein);
    s.set(6, 3, vein);
    s.set(2, 5, vein);
    s.set(7, 5, vein);
    s.set(4, 6, vein);
    s.set(5, 7, vein);
    // Dripping fluid from underneath.
    s.set(3, 9, drip);
    s.set(6, 9, drip);
    s.set(4, 10, drip);
    // Tiny useless legs dragging.
    s.set(3, 10, dark);
    s.set(6, 10, dark);
    s
}

fn miniboss() -> Sprite {
    let body = Pixel::rgb(255, 190, 60);
    let dark = Pixel::rgb(160, 90, 20);
    let armor = Pixel::rgb(90, 50, 10);
    let spike = Pixel::rgb(255, 245, 200);
    let eye = Pixel::rgb(255, 80, 80);
    let tusk = Pixel::rgb(230, 210, 160);

    // 18×18 — giant armored brute. Massive curling horns, pauldrons with
    // spikes, heavy chest plate with boss glow, jutting tusks, tree-trunk
    // legs ending in hoof-spikes.
    let mut s = Sprite::new(18, 18);
    // Curling horns rising off the helm.
    s.set(2, 0, spike);
    s.set(3, 0, spike);
    s.set(14, 0, spike);
    s.set(15, 0, spike);
    s.set(1, 1, spike);
    s.set(4, 1, spike);
    s.set(13, 1, spike);
    s.set(16, 1, spike);
    // Helm band wrapping the head.
    s.fill_rect(4, 1, 10, 4, dark);
    s.fill_rect(5, 2, 8, 2, armor);
    // Glowing twin eyes.
    s.set(6, 3, eye);
    s.set(7, 3, eye);
    s.set(10, 3, eye);
    s.set(11, 3, eye);
    // Jaw/face plate.
    s.fill_rect(5, 4, 8, 1, dark);
    // Tusks jutting down from the jaw corners.
    s.set(4, 5, tusk);
    s.set(13, 5, tusk);
    s.set(5, 5, tusk);
    s.set(12, 5, tusk);
    // Massive pauldrons with a spike cap.
    s.fill_rect(0, 4, 4, 5, armor);
    s.fill_rect(14, 4, 4, 5, armor);
    s.set(0, 4, spike);
    s.set(17, 4, spike);
    s.set(1, 5, dark);
    s.set(16, 5, dark);
    // Huge torso.
    s.fill_rect(3, 5, 12, 9, body);
    // Chest plate with rivets / studs.
    s.fill_rect(5, 6, 8, 6, armor);
    s.fill_rect(6, 7, 6, 4, dark);
    // Central boss glow / gem.
    s.set(8, 8, eye);
    s.set(9, 8, eye);
    s.set(8, 9, spike);
    s.set(9, 9, spike);
    // Rivet corners on plate.
    s.set(5, 6, spike);
    s.set(12, 6, spike);
    s.set(5, 11, spike);
    s.set(12, 11, spike);
    // Arm guards / bracers.
    s.fill_rect(0, 9, 3, 5, dark);
    s.fill_rect(15, 9, 3, 5, dark);
    s.set(1, 10, armor);
    s.set(16, 10, armor);
    // Fists.
    s.fill_rect(0, 13, 3, 2, armor);
    s.fill_rect(15, 13, 3, 2, armor);
    // Belt.
    s.fill_rect(3, 13, 12, 1, dark);
    // Tree-trunk legs with kneepads.
    s.fill_rect(3, 14, 5, 4, armor);
    s.fill_rect(10, 14, 5, 4, armor);
    s.fill_rect(4, 15, 3, 1, dark);
    s.fill_rect(11, 15, 3, 1, dark);
    // Hoof-spikes at the base.
    s.set(3, 17, spike);
    s.set(7, 17, spike);
    s.set(10, 17, spike);
    s.set(14, 17, spike);
    s.set(4, 17, dark);
    s.set(6, 17, dark);
    s.set(11, 17, dark);
    s.set(13, 17, dark);
    s
}

/// Grenade: small dark sphere with a blinking fuse. Blinks faster
/// as ttl runs out — telegraphs imminent detonation.
pub fn render_grenade_screen(
    fb: &mut Framebuffer,
    screen_x: f32,
    screen_y: f32,
    zoom: f32,
    ttl: f32,
) {
    let shell = Pixel::rgb(90, 120, 70);
    let dark = Pixel::rgb(30, 40, 20);
    let spark = Pixel::rgb(255, 230, 100);
    let bright = Pixel::rgb(255, 80, 60);

    let cx = screen_x.round() as i32;
    let cy = screen_y.round() as i32;
    let r = (zoom.max(1.0) * 0.9).round() as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, shell);
            }
        }
    }
    // Dark bottom half.
    for dy in 0..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, dark);
            }
        }
    }
    // Fuse — blinks faster as ttl drops.
    let blink_rate = (2.0 / ttl.max(0.1)).min(20.0);
    let on = ((ttl * blink_rate).sin() > 0.0) || ttl < 0.4;
    let fuse_color = if ttl < 0.35 { bright } else { spark };
    if on {
        let fx = cx;
        let fy = cy - (r + 1);
        if fx >= 0 && fy >= 0 {
            fb.set(fx as u16, fy as u16, fuse_color);
        }
        fb.bloom_seed(screen_x, screen_y - (r + 1) as f32, fuse_color, 3, 0.6);
    }
}

/// Render a rocket-kind projectile: a bigger warhead + fuselage reading
/// along the motion direction, with a thick smoke/fire trail dragging
/// behind. Scales with zoom so it reads as substantial at any mip.
pub fn render_rocket_screen(
    fb: &mut Framebuffer,
    screen_x: f32,
    screen_y: f32,
    ux: f32,
    uy: f32,
    zoom: f32,
) {
    let warhead = Pixel::rgb(255, 220, 120);
    let fuselage = Pixel::rgb(210, 90, 50);
    let dark = Pixel::rgb(90, 40, 20);
    let smoke_hot = Pixel::rgb(255, 150, 40);
    let smoke_cool = Pixel::rgb(110, 90, 80);
    let spark = Pixel::rgb(255, 245, 180);
    let scale = zoom.max(1.0);

    // Warhead cluster — glowing tip. 3×3 block at the lead, tinted
    // bright at center.
    let cx = screen_x.round() as i32;
    let cy = screen_y.round() as i32;
    let r = (scale * 1.2).round() as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, warhead);
            }
        }
    }
    // Fuselage tail — a few cells back along -motion.
    for step in 1..=3 {
        let t = step as f32 * scale * 1.1;
        let bx = screen_x - ux * t;
        let by = screen_y - uy * t;
        let color = if step == 3 { dark } else { fuselage };
        let bp_x = bx.round() as i32;
        let bp_y = by.round() as i32;
        for dy in 0..(scale as i32).max(1) {
            for dx in 0..(scale as i32).max(1) {
                let px = bp_x + dx;
                let py = bp_y + dy;
                if px >= 0 && py >= 0 {
                    fb.set(px as u16, py as u16, color);
                }
            }
        }
    }

    // Fire-and-smoke trail: long, tapering, alternating warm/cool so
    // the rocket reads like it's pulling a jet behind it.
    for step in 4..=18 {
        let t = step as f32 * scale * 0.9;
        let px = screen_x - ux * t;
        let py = screen_y - uy * t;
        let bx = px.round() as i32;
        let by = py.round() as i32;
        if bx < 0 || by < 0 {
            continue;
        }
        let color = if step % 3 == 0 {
            smoke_hot
        } else if step < 10 {
            spark
        } else {
            smoke_cool
        };
        fb.set(bx as u16, by as u16, color);
        // A little lateral jitter sold as heat shimmer.
        let perp_x = -uy;
        let perp_y = ux;
        let jitter = ((step as i32) % 5) - 2;
        let jx = (px + perp_x * jitter as f32 * 0.35).round() as i32;
        let jy = (py + perp_y * jitter as f32 * 0.35).round() as i32;
        if jx >= 0 && jy >= 0 {
            fb.set(jx as u16, jy as u16, color);
        }
    }

    // Glow bloom seed at the warhead — draws additive bloom onto the
    // floor/walls behind the rocket.
    fb.bloom_seed(screen_x, screen_y, Pixel::rgb(255, 180, 60), 4, 0.9);
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
