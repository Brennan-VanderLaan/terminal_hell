use crate::arena::Arena;
use crate::camera::{Camera, MipLevel};
use crate::content::ArchetypeStats;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::BurnStatus;
use crate::sprite::{self, SpriteOverlay};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Archetype {
    // FPS arena brand
    Rusher,
    Pinkie,
    Charger,
    Revenant,
    // Tactical mil-sim brand
    Marksman,
    Pmc,
    // Chaos roguelikes brand
    Swarmling,
    Orb,
    // Shared miniboss
    Miniboss,
}

impl Archetype {
    pub fn from_kind(kind: u8) -> Self {
        match kind {
            1 => Archetype::Pinkie,
            2 => Archetype::Miniboss,
            3 => Archetype::Charger,
            4 => Archetype::Revenant,
            5 => Archetype::Marksman,
            6 => Archetype::Pmc,
            7 => Archetype::Swarmling,
            8 => Archetype::Orb,
            _ => Archetype::Rusher,
        }
    }
    pub fn to_kind(self) -> u8 {
        match self {
            Archetype::Rusher => 0,
            Archetype::Pinkie => 1,
            Archetype::Miniboss => 2,
            Archetype::Charger => 3,
            Archetype::Revenant => 4,
            Archetype::Marksman => 5,
            Archetype::Pmc => 6,
            Archetype::Swarmling => 7,
            Archetype::Orb => 8,
        }
    }
}

pub struct Enemy {
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub archetype: Archetype,
    // Stats pulled from the content pack at spawn. Kept on the struct so
    // runtime code (movement, collision, damage) stays one field-access away
    // and doesn't need a content-db lookup per tick.
    speed: f32,
    touch_damage: i32,
    hit_radius: f32,
    reach: f32,
    preferred_distance: f32,
    ranged_damage: i32,
    touch_cooldown: f32,
    pub burn: Option<BurnStatus>,
    pub hit_flash: f32,
    pub attack_cooldown: f32,
    pub tell_timer: f32,
    tell_target: (f32, f32),
}

impl Enemy {
    pub fn spawn(archetype: Archetype, stats: &ArchetypeStats, x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            hp: stats.hp,
            archetype,
            speed: stats.speed,
            touch_damage: stats.touch_damage,
            hit_radius: stats.hit_radius,
            reach: stats.reach.unwrap_or(stats.hit_radius),
            preferred_distance: stats.preferred_distance.unwrap_or(0.0),
            ranged_damage: stats.ranged_damage.unwrap_or(0),
            touch_cooldown: 0.0,
            burn: None,
            hit_flash: 0.0,
            attack_cooldown: initial_attack_cooldown(archetype),
            tell_timer: 0.0,
            tell_target: (0.0, 0.0),
        }
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn touch_damage(&self) -> i32 {
        self.touch_damage
    }

    pub fn hit_radius(&self) -> f32 {
        self.hit_radius
    }

    pub fn color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(255, 60, 60),
            Archetype::Pinkie => Pixel::rgb(255, 130, 170),
            Archetype::Charger => Pixel::rgb(255, 100, 80),
            Archetype::Revenant => Pixel::rgb(200, 230, 255),
            Archetype::Marksman => Pixel::rgb(150, 170, 100),
            Archetype::Pmc => Pixel::rgb(120, 140, 170),
            Archetype::Swarmling => Pixel::rgb(255, 80, 40),
            Archetype::Orb => Pixel::rgb(200, 120, 255),
            Archetype::Miniboss => Pixel::rgb(255, 190, 60),
        }
    }

    pub fn gib_color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(180, 30, 40),
            Archetype::Pinkie => Pixel::rgb(190, 70, 120),
            Archetype::Charger => Pixel::rgb(170, 60, 40),
            Archetype::Revenant => Pixel::rgb(130, 160, 200),
            Archetype::Marksman => Pixel::rgb(90, 110, 60),
            Archetype::Pmc => Pixel::rgb(70, 90, 120),
            Archetype::Swarmling => Pixel::rgb(160, 40, 20),
            Archetype::Orb => Pixel::rgb(140, 70, 200),
            Archetype::Miniboss => Pixel::rgb(200, 120, 40),
        }
    }

    pub fn preferred_distance(&self) -> f32 {
        self.preferred_distance
    }

    pub fn update(&mut self, target: (f32, f32), arena: &Arena, dt: f32) {
        self.touch_cooldown = (self.touch_cooldown - dt).max(0.0);
        self.attack_cooldown = (self.attack_cooldown - dt).max(0.0);
        self.hit_flash = (self.hit_flash - dt * 6.0).max(0.0);
        if self.tell_timer > 0.0 {
            self.tell_timer = (self.tell_timer - dt).max(0.0);
        }

        let dx = target.0 - self.x;
        let dy = target.1 - self.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 < 1e-4 {
            return;
        }
        let dist = dist2.sqrt();
        let inv = 1.0 / dist;

        // Ranged units try to hold preferred distance; they back off when
        // closer than preferred * 0.7 and advance when further than
        // preferred * 1.2. Melee units always close.
        let pref = self.preferred_distance();
        let (dir_x, dir_y) = if pref > 0.0 && dist < pref * 0.7 {
            (-dx * inv, -dy * inv)
        } else if pref > 0.0 && dist <= pref * 1.2 {
            (0.0, 0.0)
        } else {
            (dx * inv, dy * inv)
        };

        let step_x = dir_x * self.speed() * dt;
        let step_y = dir_y * self.speed() * dt;

        let nx = self.x + step_x;
        if !collides(arena, nx, self.y) {
            self.x = nx;
        }
        let ny = self.y + step_y;
        if !collides(arena, self.x, ny) {
            self.y = ny;
        }
    }

    /// If this enemy is ready to fire AND has line of sight to the given
    /// player position, start (or continue) its ranged attack. Returns
    /// `Some(target)` when the shot actually fires this tick.
    pub fn try_ranged_attack(&mut self, player: (f32, f32), arena: &Arena) -> Option<RangedShot> {
        if !self.is_ranged() {
            return None;
        }
        let dx = player.0 - self.x;
        let dy = player.1 - self.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 > 40.0 * 40.0 || dist2 < 4.0 * 4.0 {
            return None;
        }
        if !arena.has_line_of_sight((self.x, self.y), player) {
            return None;
        }

        // Mid-tell: keep winding up, no fire yet.
        if self.tell_timer > 0.0 {
            return None;
        }

        // Tell just finished (timer hit 0 this tick with a target set) → fire.
        if self.tell_target != (0.0, 0.0) {
            let target = self.tell_target;
            self.tell_target = (0.0, 0.0);
            self.attack_cooldown = 2.2;
            if arena.has_line_of_sight((self.x, self.y), target) {
                return Some(RangedShot {
                    from: (self.x, self.y),
                    to: target,
                    damage: self.ranged_damage(),
                });
            }
            return None;
        }

        // No active tell and cooldown ready → start one.
        if self.attack_cooldown <= 0.0 {
            self.tell_timer = 0.45;
            self.tell_target = player;
        }
        None
    }

    pub fn is_ranged(&self) -> bool {
        matches!(self.archetype, Archetype::Revenant | Archetype::Marksman)
    }

    pub fn is_telegraphing(&self) -> bool {
        self.tell_timer > 0.0
    }

    fn ranged_damage(&self) -> i32 {
        self.ranged_damage
    }

    pub fn apply_damage(&mut self, dmg: i32) -> bool {
        self.hp -= dmg;
        self.hit_flash = 1.0;
        self.hp <= 0
    }

    pub fn apply_burn(&mut self, damage_per_sec: f32, ttl: f32) {
        match &mut self.burn {
            Some(b) if b.damage_per_sec >= damage_per_sec => {
                b.ttl = b.ttl.max(ttl);
            }
            _ => {
                self.burn = Some(BurnStatus { damage_per_sec, ttl, accum: 0.0 });
            }
        }
    }

    pub fn tick_statuses(&mut self, dt: f32) -> i32 {
        let mut damage = 0;
        if let Some(burn) = &mut self.burn {
            burn.accum += burn.damage_per_sec * dt;
            burn.ttl -= dt;
            while burn.accum >= 1.0 {
                damage += 1;
                burn.accum -= 1.0;
            }
            if burn.ttl <= 0.0 {
                self.burn = None;
            }
        }
        damage
    }

    pub fn touch_player(&mut self, px: f32, py: f32) -> i32 {
        if self.touch_cooldown > 0.0 || self.touch_damage == 0 {
            return 0;
        }
        let dx = self.x - px;
        let dy = (self.y - py).abs().min((self.y + 1.0 - py).abs());
        if dx.abs() < self.reach && dy < self.reach {
            self.touch_cooldown = 0.5;
            return self.touch_damage;
        }
        0
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let center = (sx.round() as i32, sy.round() as i32);

        let mip = camera.mip_level();
        if mip.shows_sprite() {
            let mut s = sprite::enemy_sprite(self.archetype);
            if self.hit_flash > 0.0 {
                s.tint_toward(Pixel::rgb(255, 255, 255), self.hit_flash.min(0.75));
            } else if self.tell_timer > 0.0 {
                s.tint_toward(Pixel::rgb(255, 245, 230), 0.55);
            } else if self.burn.is_some() {
                s.tint_toward(Pixel::rgb(255, 140, 40), 0.35);
            }
            // Build a sprite-local overlay at Close/Hero tiers so the
            // higher-detail zoom levels show extra state beats: burn soot
            // scatter, tell-halo sparkle, hit splatter. No overlay at the
            // Normal tier — the base tint still communicates state.
            let overlay = if mip.shows_overlay() {
                Some(build_enemy_overlay(self, mip))
            } else {
                None
            };
            s.blit_scaled_with_overlay(fb, center, camera.zoom, overlay.as_ref());
        } else if matches!(mip, MipLevel::Blob) {
            // Blob keeps the signature color and picks up burn/tell
            // feedback via a flat fill color swap.
            let color = if self.tell_timer > 0.0 {
                Pixel::rgb(255, 245, 230)
            } else if self.burn.is_some() {
                Pixel::rgb(255, 140, 40)
            } else {
                self.color()
            };
            sprite::render_blob(fb, center, color);
        } else {
            sprite::render_dot(fb, center, self.color());
        }
    }
}

/// Derive a sprite-local overlay from an enemy's current state. Called at
/// Close/Hero mip tiers only — at Normal the base tint does the work.
/// Positions are hash-derived from (archetype, x, y) so the decorations
/// stick to the enemy and don't jitter between frames.
fn build_enemy_overlay(e: &Enemy, mip: MipLevel) -> SpriteOverlay {
    let mut overlay = SpriteOverlay::new();
    let tmpl = sprite::enemy_sprite(e.archetype);
    let hi = matches!(mip, MipLevel::Hero);

    // Burn soot: a handful of dark pixels scattered over the sprite.
    // Dark-red at the Close tier, full soot black at Hero.
    if let Some(b) = &e.burn {
        let soot = if hi {
            Pixel::rgb(20, 10, 0)
        } else {
            Pixel::rgb(60, 20, 10)
        };
        let count = if hi { 5 } else { 3 };
        let seed = deterministic_seed(e.archetype.to_kind() as u64, e.x, e.y)
            ^ (b.ttl.to_bits() as u64);
        for i in 0..count {
            let (sx, sy) = sample_sprite_coord(seed.wrapping_mul(0x9E37_79B9).wrapping_add(i), tmpl.w, tmpl.h);
            overlay.stain(sx, sy, soot);
        }
    }

    // Hit flash sparkle — an extra white highlight pixel at Hero tier.
    if hi && e.hit_flash > 0.35 {
        let seed = deterministic_seed(0xFACE, e.x, e.y);
        let (sx, sy) = sample_sprite_coord(seed, tmpl.w, tmpl.h);
        overlay.stain(sx, sy, Pixel::rgb(255, 255, 255));
    }

    // Ranged enemies winding up a shot get a bright fore-eye at Hero.
    if hi && e.tell_timer > 0.0 {
        if tmpl.w >= 4 && tmpl.h >= 3 {
            overlay.stain(tmpl.w / 2, 1, Pixel::rgb(255, 230, 120));
        }
    }

    overlay
}

fn deterministic_seed(base: u64, x: f32, y: f32) -> u64 {
    let xb = x.to_bits() as u64;
    let yb = y.to_bits() as u64;
    base
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(xb.rotate_left(13))
        .wrapping_add(yb.rotate_left(29))
}

fn sample_sprite_coord(seed: u64, w: u16, h: u16) -> (u16, u16) {
    let w = w.max(1) as u64;
    let h = h.max(1) as u64;
    let sx = (seed % w) as u16;
    let sy = ((seed / w) % h) as u16;
    (sx, sy)
}

pub struct RangedShot {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub damage: i32,
}

fn initial_attack_cooldown(archetype: Archetype) -> f32 {
    match archetype {
        Archetype::Revenant | Archetype::Marksman => 1.2,
        _ => 0.0,
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}

