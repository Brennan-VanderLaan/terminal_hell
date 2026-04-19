use crate::projectile::Projectile;

/// Baseline starter weapon: auto-fire pulse rifle. One fire mode, no
/// primitives yet — slots get wired in during the primitive-bus milestone.
pub struct Weapon {
    cooldown: f32,
    fire_rate: f32,
    projectile_speed: f32,
    damage: i32,
    projectile_ttl: f32,
}

impl Weapon {
    pub fn pulse() -> Self {
        Self {
            cooldown: 0.0,
            fire_rate: 10.0,
            projectile_speed: 140.0,
            damage: 25,
            projectile_ttl: 0.9,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
    }

    /// Attempt to fire from `origin` toward `target` (both in arena pixel
    /// coordinates). Returns a projectile if the weapon was ready.
    pub fn try_fire(&mut self, origin: (f32, f32), target: (f32, f32)) -> Option<Projectile> {
        if self.cooldown > 0.0 {
            return None;
        }
        let (fx, fy) = origin;
        let (tx, ty) = target;
        let dx = tx - fx;
        let dy = ty - fy;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-4 {
            return None;
        }
        let inv = len2.sqrt().recip();
        let vx = dx * inv * self.projectile_speed;
        let vy = dy * inv * self.projectile_speed;
        self.cooldown = 1.0 / self.fire_rate;
        Some(Projectile { x: fx, y: fy, vx, vy, ttl: self.projectile_ttl, damage: self.damage })
    }
}
