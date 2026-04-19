use crate::arena::Arena;
use crate::fb::{Framebuffer, Pixel};

pub enum ProjectileOutcome {
    Alive,
    Expired,
    HitWall { at: (f32, f32), tile: (i32, i32) },
}

pub struct Projectile {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub ttl: f32,
    pub damage: i32,
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

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32) {
        let px = ox + self.x.round() as i32;
        let py = oy + self.y.round() as i32;
        if px < 0 || py < 0 {
            return;
        }
        fb.set(px as u16, py as u16, Pixel::rgb(255, 240, 140));
    }
}
