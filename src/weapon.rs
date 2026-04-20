//! Runtime weapon instance. Drives per-shot damage, fire cadence, and
//! the primitive slots that modify each projectile. Also the single
//! source of truth for fire-mode behavior variety (pulse / auto /
//! burst / pump / rail).

use crate::primitive::{Primitive, Rarity};
use crate::projectile::Projectile;
use serde::{Deserialize, Serialize};

/// Fire-mode discriminant — controls how a single `try_fire` call
/// emits projectiles. Each mode brings a distinct feel:
///   * Pulse  — one fast round per click, original MVP behavior.
///   * Auto   — faster cadence, lower per-shot damage.
///   * Burst  — 3 rounds per click with tight spacing.
///   * Pump   — 5-pellet shotgun cone, single click, long cooldown.
///   * Rail   — 1 high-damage pierce line, long cooldown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FireMode {
    #[default]
    Pulse,
    Auto,
    Burst,
    Pump,
    Rail,
}

impl FireMode {
    pub fn glyph(self) -> char {
        match self {
            FireMode::Pulse => '·',
            FireMode::Auto => '»',
            FireMode::Burst => '⋯',
            FireMode::Pump => '◊',
            FireMode::Rail => '═',
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FireMode::Pulse => "pulse",
            FireMode::Auto => "auto",
            FireMode::Burst => "burst",
            FireMode::Pump => "pump",
            FireMode::Rail => "rail",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "pulse" => FireMode::Pulse,
            "auto" => FireMode::Auto,
            "burst" => FireMode::Burst,
            "pump" => FireMode::Pump,
            "rail" => FireMode::Rail,
            _ => return None,
        })
    }
}

/// Runtime instance of a weapon. `slots` is the primitive list rolled
/// when the weapon was spawned; slot count comes from the weapon's
/// rarity. `mode` drives the per-click firing behavior.
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
    pub mode: FireMode,
    /// Burst mode: rounds left to fire in the current burst sequence.
    pub burst_remaining: u8,
    /// Internal burst-spacing timer (time between rounds in a burst).
    pub burst_spacing: f32,
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
            mode: FireMode::Pulse,
            burst_remaining: 0,
            burst_spacing: 0.0,
        }
    }

    pub fn with_slots(rarity: Rarity, slots: Vec<Primitive>) -> Self {
        let mut w = Self::new_basic();
        w.rarity = rarity;
        w.slots = slots;
        w
    }

    pub fn with_mode(mut self, mode: FireMode) -> Self {
        self.mode = mode;
        // Baseline tuning per mode.
        match mode {
            FireMode::Pulse => {}
            FireMode::Auto => {
                self.fire_rate = 16.0;
                self.damage = 18;
            }
            FireMode::Burst => {
                self.fire_rate = 2.2; // 2.2 bursts / sec
                self.damage = 20;
            }
            FireMode::Pump => {
                self.fire_rate = 1.3;
                self.damage = 14;
                self.projectile_ttl = 0.5;
            }
            FireMode::Rail => {
                self.fire_rate = 0.9;
                self.damage = 80;
                self.projectile_speed = 260.0;
                self.projectile_ttl = 1.4;
            }
        }
        self
    }

    pub fn tick(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        if self.burst_spacing > 0.0 {
            self.burst_spacing = (self.burst_spacing - dt).max(0.0);
        }
    }

    /// Attempt a fire. Each mode dispatches to its own sub-routine
    /// and returns the (possibly multiple) projectiles to spawn.
    pub fn try_fire(
        &mut self,
        origin: (f32, f32),
        target: (f32, f32),
        owner_id: u32,
        damage_mul: f32,
    ) -> Vec<Projectile> {
        let dx = target.0 - origin.0;
        let dy = target.1 - origin.1;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-4 {
            return Vec::new();
        }
        let inv = len2.sqrt().recip();
        let ux = dx * inv;
        let uy = dy * inv;

        match self.mode {
            FireMode::Pulse | FireMode::Auto => {
                self.fire_single(origin, ux, uy, owner_id, damage_mul)
            }
            FireMode::Burst => self.fire_burst(origin, ux, uy, owner_id, damage_mul),
            FireMode::Pump => self.fire_pump(origin, ux, uy, owner_id, damage_mul),
            FireMode::Rail => self.fire_rail(origin, ux, uy, owner_id, damage_mul),
        }
    }

    fn fire_single(
        &mut self,
        origin: (f32, f32),
        ux: f32,
        uy: f32,
        owner_id: u32,
        damage_mul: f32,
    ) -> Vec<Projectile> {
        if self.cooldown > 0.0 {
            return Vec::new();
        }
        self.cooldown = 1.0 / self.fire_rate;
        vec![self.build_projectile(origin, ux, uy, owner_id, damage_mul, 1.0, self.projectile_ttl)]
    }

    fn fire_burst(
        &mut self,
        origin: (f32, f32),
        ux: f32,
        uy: f32,
        owner_id: u32,
        damage_mul: f32,
    ) -> Vec<Projectile> {
        // Burst arms 3 rounds on click; each round waits 0.07s.
        if self.burst_remaining == 0 {
            if self.cooldown > 0.0 {
                return Vec::new();
            }
            self.burst_remaining = 3;
            self.cooldown = 1.0 / self.fire_rate;
        }
        if self.burst_spacing > 0.0 {
            return Vec::new();
        }
        self.burst_remaining -= 1;
        self.burst_spacing = 0.07;
        vec![self.build_projectile(origin, ux, uy, owner_id, damage_mul, 1.0, self.projectile_ttl)]
    }

    fn fire_pump(
        &mut self,
        origin: (f32, f32),
        ux: f32,
        uy: f32,
        owner_id: u32,
        damage_mul: f32,
    ) -> Vec<Projectile> {
        if self.cooldown > 0.0 {
            return Vec::new();
        }
        self.cooldown = 1.0 / self.fire_rate;
        // 5 pellets across a ~25° cone. Each pellet is a Pulse-kind
        // projectile with slightly-randomized ttl so the cone arrives
        // as a wave, not a single frame.
        let mut out = Vec::with_capacity(5);
        let cone = 0.22; // radians half-angle (~12.5°).
        for i in 0..5 {
            let t = if i == 0 { 0.0 } else {
                // Evenly spread pellets across the cone.
                let norm = (i as f32 / 2.0) - 1.0; // -1 .. 1
                norm * cone
            };
            let cos = t.cos();
            let sin = t.sin();
            let rx = ux * cos - uy * sin;
            let ry = ux * sin + uy * cos;
            out.push(self.build_projectile(
                origin,
                rx,
                ry,
                owner_id,
                damage_mul,
                1.0,
                self.projectile_ttl,
            ));
        }
        out
    }

    fn fire_rail(
        &mut self,
        origin: (f32, f32),
        ux: f32,
        uy: f32,
        owner_id: u32,
        damage_mul: f32,
    ) -> Vec<Projectile> {
        if self.cooldown > 0.0 {
            return Vec::new();
        }
        self.cooldown = 1.0 / self.fire_rate;
        // Rail shoots a single projectile with heavy pierce + higher
        // speed. Stacks on top of any Pierce slots in the loadout.
        let mut p = self.build_projectile(
            origin,
            ux,
            uy,
            owner_id,
            damage_mul,
            1.0,
            self.projectile_ttl,
        );
        p.pierces_left = p.pierces_left.saturating_add(4);
        vec![p]
    }

    fn build_projectile(
        &self,
        origin: (f32, f32),
        ux: f32,
        uy: f32,
        owner_id: u32,
        damage_mul: f32,
        damage_scale: f32,
        ttl: f32,
    ) -> Projectile {
        let vx = ux * self.projectile_speed;
        let vy = uy * self.projectile_speed;

        let mut damage = self.damage;
        if self.overdrive_armed {
            damage *= 2;
        }
        damage = ((damage as f32) * damage_mul * damage_scale).round() as i32;

        let ricochet = self.slots.iter().filter(|p| **p == Primitive::Ricochet).count() as u8;
        let pierce = self.slots.iter().filter(|p| **p == Primitive::Pierce).count() as u8;

        Projectile {
            x: origin.0,
            y: origin.1,
            vx,
            vy,
            ttl,
            damage,
            owner_id,
            primitives: self.slots.clone(),
            bounces_left: ricochet,
            pierces_left: pierce,
            kind: crate::projectile::ProjectileKind::Pulse,
            from_ai: false,
            trail_phase: 0.0,
        }
    }

    /// Consume the Overdrive charge if armed. Call after the firing
    /// call completes so the buff applies to whatever burst / salvo
    /// got spawned this click.
    pub fn consume_overdrive(&mut self) {
        if self.overdrive_armed {
            self.overdrive_armed = false;
        }
    }

    pub fn note_kill(&mut self) {
        if self.slots.contains(&Primitive::Overdrive) {
            self.overdrive_armed = true;
        }
    }
}
