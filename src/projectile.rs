use crate::arena::Arena;
use crate::fb::Framebuffer;
use crate::sprite;

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
        sprite::render_projectile(
            fb,
            ox as f32 + self.x,
            oy as f32 + self.y,
            self.vx,
            self.vy,
        );
    }
}
