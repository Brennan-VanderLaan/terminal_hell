//! Persistent corpse entities. When an enemy dies the shell body persists
//! in the world as a `Corpse` carrying the archetype's sprite with a
//! growing punch-out overlay. Each projectile that hits the corpse
//! deterministically removes sprite pixels around the hit coord and
//! spawns a gore burst; accumulated damage eventually collapses the
//! corpse into a `blood_pool` substance on the ground layer.
//!
//! Corpses live in the object layer of the 3-layer tile stack (`Arena`).
//! They don't block movement — players step over them — but projectiles
//! can hit them directly (see `Corpse::try_hit` + the `hit_radius` test).
//!
//! Sync model:
//! - The host broadcasts the corpse list in `Snapshot` so clients (and
//!   late joiners) see every corpse's position immediately.
//! - The *hole state* isn't in the snapshot. Instead, each hit generates
//!   a reliable `CorpseHit { id, seed }` event — clients compute
//!   identical hole positions via deterministic seeded synthesis.
//! - On collapse the host broadcasts a `SubstancePaint` with the
//!   blood_pool substance id and drops the corpse from its list;
//!   clients remove it from theirs.

use crate::camera::{Camera, MipLevel};
use crate::enemy::Archetype;
use crate::fb::{Framebuffer, Pixel};
use crate::sprite::{self, SpriteOverlay, Sprite};

pub struct Corpse {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub archetype: Archetype,
    /// Sprite-local pixel coords that have been punched out by hits.
    /// Kept as `(u8, u8)` — archetype sprites are all < 256 on either
    /// axis.
    pub holes: Vec<(u8, u8)>,
    /// Remaining "structural integrity" of the corpse. Each hit lowers
    /// this by 1; at 0 the corpse collapses into a blood-pool ground
    /// decal (handled by the caller).
    pub hp: i32,
}

impl Corpse {
    pub const MAX_HP: i32 = 6;
    /// World-space radius for hit detection. Roughly matches enemy
    /// hit_radius so shooting "at the body" still lands.
    pub const HIT_RADIUS: f32 = 2.6;

    pub fn new(id: u32, archetype: Archetype, x: f32, y: f32) -> Self {
        Self {
            id,
            x,
            y,
            archetype,
            holes: Vec::new(),
            hp: Self::MAX_HP,
        }
    }

    /// Punch out a small cluster of sprite pixels using `seed` for
    /// deterministic layout, decrement hp, and return the gore color the
    /// caller should spawn gibs in. Host + clients call this with the
    /// *same* seed → identical hole patterns.
    pub fn apply_hit(&mut self, seed: u64) -> Pixel {
        let tmpl = sprite::enemy_sprite(self.archetype);
        let w = tmpl.w.max(1) as u64;
        let h = tmpl.h.max(1) as u64;
        let cx = (seed % w) as i16;
        let cy = ((seed / w) % h) as i16;

        let punch = |hx: i16, hy: i16| -> Option<(u8, u8)> {
            if hx < 0 || hy < 0 {
                return None;
            }
            if (hx as u16) >= tmpl.w || (hy as u16) >= tmpl.h {
                return None;
            }
            Some((hx as u8, hy as u8))
        };

        let candidates = [
            (cx, cy),
            (cx + 1, cy),
            (cx - 1, cy),
            (cx, cy + 1),
            (cx, cy - 1),
            (cx + 1, cy + 1),
            (cx - 1, cy - 1),
        ];
        // Use bits of the seed to pick which neighbours also get punched
        // — at least the center is always nuked, plus 2-4 neighbors.
        for (i, c) in candidates.iter().enumerate() {
            let keep = if i == 0 {
                true
            } else {
                ((seed >> (i as u32 + 7)) & 1) == 1
            };
            if !keep {
                continue;
            }
            if let Some(hc) = punch(c.0, c.1) {
                if !self.holes.contains(&hc) {
                    self.holes.push(hc);
                }
            }
        }

        self.hp -= 1;
        gib_color(self.archetype)
    }

    pub fn is_destroyed(&self) -> bool {
        self.hp <= 0
    }

    /// Returns true if the world-space point (hx, hy) is within this
    /// corpse's hit radius.
    pub fn point_in_bounds(&self, hx: f32, hy: f32) -> bool {
        let dx = self.x - hx;
        let dy = self.y - hy;
        dx * dx + dy * dy < Self::HIT_RADIUS * Self::HIT_RADIUS
    }

    pub fn render(
        &self,
        fb: &mut Framebuffer,
        camera: &Camera,
        content: &crate::content::ContentDb,
    ) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let center = (sx.round() as i32, sy.round() as i32);
        let mip = camera.mip_level();
        let gib = gib_color(self.archetype);

        if mip.shows_sprite() {
            let mut s: Sprite = sprite::enemy_sprite_from_content(self.archetype, content);
            // Tint toward dead-tones: most of the base tint collapses to
            // the archetype's gib color + a darkening pass.
            s.tint_toward(gib, 0.6);
            s.tint_toward(Pixel::rgb(25, 15, 20), 0.25);

            let mut overlay = SpriteOverlay::new();
            for &(hx, hy) in &self.holes {
                overlay.punch(hx as u16, hy as u16);
            }
            // At Close/Hero tiers, crust the hole edges with blood stains
            // so the damage reads as a wound rather than a clean hole.
            if mip.shows_overlay() {
                let blood_dark = Pixel::rgb(100, 15, 10);
                let blood_bright = Pixel::rgb(160, 30, 20);
                for (i, &(hx, hy)) in self.holes.iter().enumerate() {
                    let alt = i & 1 == 0;
                    let color = if alt { blood_dark } else { blood_bright };
                    for (dx, dy) in &[(1i16, 0i16), (-1, 0), (0, 1), (0, -1)] {
                        let nx = hx as i16 + *dx;
                        let ny = hy as i16 + *dy;
                        if nx < 0 || ny < 0 {
                            continue;
                        }
                        if !overlay
                            .holes
                            .iter()
                            .any(|&(ox, oy)| ox == nx as u16 && oy == ny as u16)
                        {
                            overlay.stain(nx as u16, ny as u16, color);
                        }
                    }
                }
            }
            s.blit_scaled_with_overlay(fb, center, camera.zoom, Some(&overlay));
        } else if matches!(mip, MipLevel::Blob) {
            sprite::render_blob(fb, center, gib);
        } else {
            sprite::render_dot(fb, center, gib);
        }
    }
}

/// Mirror of the per-archetype gib color from `enemy.rs`. Kept here so
/// the corpse renderer doesn't need an enemy instance.
pub fn gib_color(archetype: Archetype) -> Pixel {
    match archetype {
        Archetype::Rusher => Pixel::rgb(180, 30, 40),
        Archetype::Pinkie => Pixel::rgb(190, 70, 120),
        Archetype::Charger => Pixel::rgb(170, 60, 40),
        Archetype::Revenant => Pixel::rgb(130, 160, 200),
        Archetype::Marksman => Pixel::rgb(90, 110, 60),
        Archetype::Pmc => Pixel::rgb(70, 90, 120),
        Archetype::Swarmling => Pixel::rgb(160, 40, 20),
        Archetype::Orb => Pixel::rgb(140, 70, 200),
        Archetype::Miniboss => Pixel::rgb(200, 120, 40),
        Archetype::Eater => Pixel::rgb(100, 20, 140),
    }
}
