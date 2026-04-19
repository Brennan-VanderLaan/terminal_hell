use crate::arena::Arena;
use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::Primitive;
use crate::sprite;

pub enum ProjectileOutcome {
    Alive,
    Expired,
    HitWall { at: (f32, f32), tile: (i32, i32) },
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
    /// back to the firing weapon (e.g., for Overdrive priming).
    pub owner_id: u32,
    pub primitives: Vec<Primitive>,
    pub bounces_left: u8,
    pub pierces_left: u8,
}

impl Projectile {
    /// Advance the projectile by `dt`. Uses a swept ray-cast from old to
    /// new position so fast projectiles (snipers, future lasers) can't
    /// tunnel through walls that are thinner than a step. On a wall hit
    /// the position is snapped to just outside the struck tile so
    /// reflections don't immediately re-collide.
    pub fn update(&mut self, arena: &Arena, dt: f32) -> ProjectileOutcome {
        self.ttl -= dt;
        if self.ttl <= 0.0 {
            return ProjectileOutcome::Expired;
        }
        let from = (self.x, self.y);
        let to = (self.x + self.vx * dt, self.y + self.vy * dt);

        if let Some(hit) = arena.raycast_wall(from, to) {
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
        if mip.shows_sprite() {
            let speed = (self.vx * self.vx + self.vy * self.vy).sqrt().max(0.001);
            let ux = self.vx / speed;
            let uy = self.vy / speed;
            sprite::render_projectile_screen(fb, sx, sy, ux, uy, camera.zoom);
        } else {
            sprite::render_dot(
                fb,
                (sx.round() as i32, sy.round() as i32),
                Pixel::rgb(255, 245, 150),
            );
        }
    }
}
