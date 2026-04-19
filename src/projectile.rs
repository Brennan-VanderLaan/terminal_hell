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
    pub fn update(&mut self, arena: &Arena, dt: f32) -> ProjectileOutcome {
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        self.ttl -= dt;
        if self.ttl <= 0.0 {
            return ProjectileOutcome::Expired;
        }
        let tx = self.x.floor() as i32;
        let ty = self.y.floor() as i32;
        if arena.is_wall(tx, ty) {
            return ProjectileOutcome::HitWall { at: (self.x, self.y), tile: (tx, ty) };
        }
        ProjectileOutcome::Alive
    }

    /// Reflect the velocity off the tile whose center is closest. We pick
    /// the axis with the larger approach component and invert it — cheap
    /// axis-aligned bounce without needing surface normals.
    pub fn reflect_off(&mut self, tile: (i32, i32)) {
        let tile_cx = tile.0 as f32 + 0.5;
        let tile_cy = tile.1 as f32 + 0.5;
        let dx = self.x - tile_cx;
        let dy = self.y - tile_cy;
        if dx.abs() > dy.abs() {
            self.vx = -self.vx;
            // Nudge outside the tile so the next step doesn't re-collide.
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
        match camera.mip_level() {
            0 => {
                let speed = (self.vx * self.vx + self.vy * self.vy).sqrt().max(0.001);
                let ux = self.vx / speed;
                let uy = self.vy / speed;
                sprite::render_projectile_screen(fb, sx, sy, ux, uy, camera.zoom);
            }
            _ => {
                sprite::render_dot(
                    fb,
                    (sx.round() as i32, sy.round() as i32),
                    Pixel::rgb(255, 245, 150),
                );
            }
        }
    }
}
