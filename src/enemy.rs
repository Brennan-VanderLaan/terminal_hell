use crate::arena::Arena;
use crate::content::ArchetypeStats;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::BurnStatus;
use crate::sprite;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Archetype {
    /// Small fast melee. Imp-ish.
    Rusher,
    /// Armored brute. Pinkie-ish.
    Pinkie,
    /// Fast armored rush variant. Charger — low HP for a brute, high speed.
    Charger,
    /// Ranged hitscan shooter. Slow on foot, telegraphs shots.
    Revenant,
    /// Every-5 miniboss.
    Miniboss,
}

impl Archetype {
    pub fn from_kind(kind: u8) -> Self {
        match kind {
            1 => Archetype::Pinkie,
            2 => Archetype::Miniboss,
            3 => Archetype::Charger,
            4 => Archetype::Revenant,
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
            Archetype::Miniboss => Pixel::rgb(255, 190, 60),
        }
    }

    pub fn gib_color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(180, 30, 40),
            Archetype::Pinkie => Pixel::rgb(190, 70, 120),
            Archetype::Charger => Pixel::rgb(170, 60, 40),
            Archetype::Revenant => Pixel::rgb(130, 160, 200),
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
        if !has_line_of_sight(arena, (self.x, self.y), player) {
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
            if has_line_of_sight(arena, (self.x, self.y), target) {
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
        matches!(self.archetype, Archetype::Revenant)
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

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32) {
        let cx = ox + self.x.round() as i32;
        let cy = oy + self.y.round() as i32;
        let mut s = sprite::enemy_sprite(self.archetype);
        if self.hit_flash > 0.0 {
            s.tint_toward(Pixel::rgb(255, 255, 255), self.hit_flash.min(0.75));
        } else if self.tell_timer > 0.0 {
            // Ranged charging tell — strong pale-white lean so players have
            // time to break line-of-sight.
            s.tint_toward(Pixel::rgb(255, 245, 230), 0.55);
        } else if self.burn.is_some() {
            s.tint_toward(Pixel::rgb(255, 140, 40), 0.35);
        }
        s.blit(fb, cx, cy);
    }
}

pub struct RangedShot {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub damage: i32,
}

fn initial_attack_cooldown(archetype: Archetype) -> f32 {
    match archetype {
        Archetype::Revenant => 1.2, // small delay before the first shot
        _ => 0.0,
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}

/// DDA-style line walk between two world positions. Returns false if any
/// wall tile sits along the line. Used by Revenant LoS checks.
fn has_line_of_sight(arena: &Arena, from: (f32, f32), to: (f32, f32)) -> bool {
    let steps = ((to.0 - from.0).abs().max((to.1 - from.1).abs()).ceil() as i32).max(1);
    for i in 1..steps {
        let t = i as f32 / steps as f32;
        let x = from.0 + (to.0 - from.0) * t;
        let y = from.1 + (to.1 - from.1) * t;
        if arena.is_wall(x.floor() as i32, y.floor() as i32) {
            return false;
        }
    }
    true
}
