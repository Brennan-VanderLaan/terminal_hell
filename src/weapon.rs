use crate::primitive::{Primitive, Rarity};
use crate::projectile::Projectile;

/// Runtime instance of a weapon. `slots` is the primitive list rolled when
/// the weapon was spawned; slot count comes from the weapon's rarity. The
/// `overdrive_armed` flag is state used by the Overdrive primitive.
#[derive(Clone, Debug)]
pub struct Weapon {
    pub cooldown: f32,
    pub fire_rate: f32,
    pub projectile_speed: f32,
    pub damage: i32,
    pub projectile_ttl: f32,
    pub rarity: Rarity,
    pub slots: Vec<Primitive>,
    pub overdrive_armed: bool,
}

impl Weapon {
    pub fn pulse() -> Self {
        Self::new_basic()
    }

    pub fn new_basic() -> Self {
        Self {
            cooldown: 0.0,
            fire_rate: 10.0,
            projectile_speed: 140.0,
            damage: 25,
            projectile_ttl: 0.9,
            rarity: Rarity::Common,
            slots: Vec::new(),
            overdrive_armed: false,
        }
    }

    /// Build a weapon with specific rarity + primitive loadout — used by
    /// loot drops.
    pub fn with_slots(rarity: Rarity, slots: Vec<Primitive>) -> Self {
        let mut w = Self::new_basic();
        w.rarity = rarity;
        w.slots = slots;
        w
    }

    pub fn tick(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
    }

    pub fn try_fire(
        &mut self,
        origin: (f32, f32),
        target: (f32, f32),
        owner_id: u32,
    ) -> Option<Projectile> {
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

        // Overdrive: if armed, double this shot's damage and consume the charge.
        let mut damage = self.damage;
        if self.overdrive_armed {
            damage *= 2;
            self.overdrive_armed = false;
        }

        // Pierce + Ricochet default counts. More copies of the primitive →
        // more bounces/pierces (gives stacking meaning even for MVP).
        let ricochet = self.slots.iter().filter(|p| **p == Primitive::Ricochet).count() as u8;
        let pierce = self.slots.iter().filter(|p| **p == Primitive::Pierce).count() as u8;

        Some(Projectile {
            x: fx,
            y: fy,
            vx,
            vy,
            ttl: self.projectile_ttl,
            damage,
            owner_id,
            primitives: self.slots.clone(),
            bounces_left: ricochet,
            pierces_left: pierce,
        })
    }

    /// Called by the game when this weapon's owner gets a kill. Primes
    /// Overdrive for the next shot if the weapon has the primitive.
    pub fn note_kill(&mut self) {
        if self.slots.contains(&Primitive::Overdrive) {
            self.overdrive_armed = true;
        }
    }
}
