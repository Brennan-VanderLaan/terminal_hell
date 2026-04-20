use crate::arena::Arena;
use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::Primitive;
use crate::sprite;
use serde::{Deserialize, Serialize};

pub enum ProjectileOutcome {
    Alive,
    Expired,
    HitWall { at: (f32, f32), tile: (i32, i32) },
}

/// Projectile kind discriminant. New kinds slot in here with their own
/// render + impact behavior; existing primitives (ignite, breach,
/// ricochet, chain, pierce) compose against every kind — a Rocket
/// with Ricochet bounces before exploding; with Chain, its explosion
/// arcs damage through nearby enemies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProjectileKind {
    /// Default fast bullet — the player's pulse weapon and anything
    /// else that wants a lightweight hitscan-feel round.
    #[default]
    Pulse,
    /// Heavy rocket — slower, larger, leaves a smoke trail, detonates
    /// on any impact instead of applying flat damage. Picks up radius
    /// + chain + ignite modifiers from its primitive list.
    Rocket,
    /// Lobbed grenade — slow travel, detonates on TTL expire OR on
    /// impact. Pairs with the rocket detonation logic for the
    /// explosion; used by Killa's grenade lobs.
    Grenade,
}

impl ProjectileKind {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => ProjectileKind::Rocket,
            2 => ProjectileKind::Grenade,
            _ => ProjectileKind::Pulse,
        }
    }
    pub fn to_byte(self) -> u8 {
        match self {
            ProjectileKind::Pulse => 0,
            ProjectileKind::Rocket => 1,
            ProjectileKind::Grenade => 2,
        }
    }
    /// True if this kind explodes on any impact (wall, enemy, corpse).
    /// Explosion resolution lives in game.rs so it can access the
    /// full world mutably.
    pub fn explodes_on_impact(self) -> bool {
        matches!(self, ProjectileKind::Rocket | ProjectileKind::Grenade)
    }
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub ttl: f32,
    pub damage: i32,
    /// Player ID that fired this projectile — used to route kill credit
    /// back to the firing weapon (e.g., for Overdrive priming). Zero
    /// for AI-fired projectiles.
    pub owner_id: u32,
    pub primitives: Vec<Primitive>,
    pub bounces_left: u8,
    pub pierces_left: u8,
    /// Discriminant controlling rendering + impact behavior. Defaults
    /// to Pulse for legacy callers; rockets set it explicitly.
    pub kind: ProjectileKind,
    /// Which side fired it. AI projectiles only hit players + walls;
    /// player projectiles only hit enemies + walls. Prevents rockets
    /// from friendly-firing other enemies.
    pub from_ai: bool,
    /// Smoke-trail counter for rockets — incremented each tick, used
    /// by the rendering code to decide whether to drop a trail spec
    /// this frame.
    pub trail_phase: f32,
}

impl Projectile {
    /// Advance the projectile by `dt`. Uses a swept ray-cast from old to
    /// new position so fast projectiles (snipers, future lasers) can't
    /// tunnel through walls that are thinner than a step. On a wall hit
    /// the position is snapped to just outside the struck tile so
    /// reflections don't immediately re-collide.
    pub fn update(&mut self, arena: &Arena, dt: f32) -> ProjectileOutcome {
        self.ttl -= dt;
        self.trail_phase += dt;
        if self.ttl <= 0.0 {
            return ProjectileOutcome::Expired;
        }
        let from = (self.x, self.y);
        let to = (self.x + self.vx * dt, self.y + self.vy * dt);

        // Projectile-specific raycast: structural walls stop rounds,
        // but broken rubble (DebrisPile) doesn't. Walking-through and
        // shooting-through are symmetric — if you can step over the
        // pile, a bullet also punches past it. Enemy sight (AI LoS)
        // still uses the stricter `raycast_los` so piles remain real
        // soft cover against enemy scans.
        if let Some(hit) = arena.raycast_projectile(from, to) {
            // Park just outside the struck tile boundary so a subsequent
            // reflection/pierce starts in free space. `normal` points
            // back toward the source; a tiny nudge along it moves the
            // projectile into the free cell.
            const EPS: f32 = 0.001;
            self.x = hit.point.0 + (hit.normal.0 as f32) * EPS;
            self.y = hit.point.1 + (hit.normal.1 as f32) * EPS;
            return ProjectileOutcome::HitWall {
                at: (self.x, self.y),
                tile: hit.tile,
            };
        }

        self.x = to.0;
        self.y = to.1;
        ProjectileOutcome::Alive
    }

    /// Factory for a rocket. Cheaper to spell out here than to
    /// remember every field at the call site.
    pub fn rocket(
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        primitives: Vec<Primitive>,
        from_ai: bool,
    ) -> Self {
        let bounces = if primitives.contains(&Primitive::Ricochet) {
            2
        } else {
            0
        };
        let pierces = if primitives.contains(&Primitive::Pierce) {
            1
        } else {
            0
        };
        Self {
            x,
            y,
            vx,
            vy,
            ttl: 3.5,
            damage: 0, // damage is done by the explosion reaction, not by body hit
            owner_id: 0,
            primitives,
            bounces_left: bounces,
            pierces_left: pierces,
            kind: ProjectileKind::Rocket,
            from_ai,
            trail_phase: 0.0,
        }
    }

    /// Factory for a grenade. Slow linear travel; detonates when TTL
    /// expires even if it never hit anything (timed fuse).
    pub fn grenade(
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        from_ai: bool,
    ) -> Self {
        Self {
            x,
            y,
            vx,
            vy,
            ttl: 1.6,
            damage: 0,
            owner_id: 0,
            primitives: Vec::new(),
            bounces_left: 0,
            pierces_left: 0,
            kind: ProjectileKind::Grenade,
            from_ai,
            trail_phase: 0.0,
        }
    }

    /// Factory for a player pulse bullet. Mirrors the old ad-hoc
    /// construction so the weapon firing code stays terse.
    pub fn pulse(
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        damage: i32,
        owner_id: u32,
        primitives: Vec<Primitive>,
    ) -> Self {
        let bounces = if primitives.contains(&Primitive::Ricochet) {
            2
        } else {
            0
        };
        let pierces = if primitives.contains(&Primitive::Pierce) {
            1
        } else {
            0
        };
        Self {
            x,
            y,
            vx,
            vy,
            ttl: 1.0,
            damage,
            owner_id,
            primitives,
            bounces_left: bounces,
            pierces_left: pierces,
            kind: ProjectileKind::Pulse,
            from_ai: false,
            trail_phase: 0.0,
        }
    }

    /// Reflect the velocity off the given tile. When the ray-cast gave
    /// us an axis-aligned normal we use it directly; otherwise we fall
    /// back to the center-of-tile heuristic.
    pub fn reflect_off(&mut self, tile: (i32, i32)) {
        // Use the axis-of-closest-approach for the surface normal — this
        // is what the old code did and it still works when the caller
        // doesn't have a ray-cast hit handy.
        let tile_cx = tile.0 as f32 + 0.5;
        let tile_cy = tile.1 as f32 + 0.5;
        let dx = self.x - tile_cx;
        let dy = self.y - tile_cy;
        if dx.abs() > dy.abs() {
            self.vx = -self.vx;
            self.x += self.vx.signum() * 0.5;
        } else {
            self.vy = -self.vy;
            self.y += self.vy.signum() * 0.5;
        }
    }

    pub fn has(&self, p: Primitive) -> bool {
        self.primitives.contains(&p)
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let mip = camera.mip_level();
        if !mip.shows_sprite() {
            let dot_color = match self.kind {
                ProjectileKind::Rocket => Pixel::rgb(255, 140, 40),
                ProjectileKind::Grenade => Pixel::rgb(90, 120, 70),
                ProjectileKind::Pulse => Pixel::rgb(255, 245, 150),
            };
            sprite::render_dot(fb, (sx.round() as i32, sy.round() as i32), dot_color);
            return;
        }
        let speed = (self.vx * self.vx + self.vy * self.vy).sqrt().max(0.001);
        let ux = self.vx / speed;
        let uy = self.vy / speed;
        match self.kind {
            ProjectileKind::Pulse => {
                sprite::render_projectile_screen(fb, sx, sy, ux, uy, camera.zoom);
            }
            ProjectileKind::Rocket => {
                sprite::render_rocket_screen(fb, sx, sy, ux, uy, camera.zoom);
            }
            ProjectileKind::Grenade => {
                sprite::render_grenade_screen(fb, sx, sy, camera.zoom, self.ttl);
            }
        }
    }
}
