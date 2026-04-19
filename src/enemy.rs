use crate::arena::Arena;
use crate::fb::{Framebuffer, Pixel};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Archetype {
    /// Small, fast, cheap melee zerg. Imp-ish.
    Rusher,
    /// Armored brute: slower, higher HP, harder-hitting melee. Pinkie-ish.
    Pinkie,
    /// Miniboss: huge HP, slow, big contact damage, drops a guaranteed Rare
    /// (handled by loot system once that ships). Cybermini.
    Miniboss,
}

impl Archetype {
    pub fn from_kind(kind: u8) -> Self {
        match kind {
            1 => Archetype::Pinkie,
            2 => Archetype::Miniboss,
            _ => Archetype::Rusher,
        }
    }
    pub fn to_kind(self) -> u8 {
        match self {
            Archetype::Rusher => 0,
            Archetype::Pinkie => 1,
            Archetype::Miniboss => 2,
        }
    }
}

pub struct Enemy {
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub archetype: Archetype,
    touch_cooldown: f32,
}

impl Enemy {
    pub fn spawn(archetype: Archetype, x: f32, y: f32) -> Self {
        Self { x, y, hp: starting_hp(archetype), archetype, touch_cooldown: 0.0 }
    }

    pub fn rusher(x: f32, y: f32) -> Self {
        Self::spawn(Archetype::Rusher, x, y)
    }

    pub fn speed(&self) -> f32 {
        match self.archetype {
            Archetype::Rusher => 22.0,
            Archetype::Pinkie => 15.0,
            Archetype::Miniboss => 9.0,
        }
    }

    pub fn touch_damage(&self) -> i32 {
        match self.archetype {
            Archetype::Rusher => 10,
            Archetype::Pinkie => 18,
            Archetype::Miniboss => 30,
        }
    }

    pub fn color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(255, 60, 60),
            Archetype::Pinkie => Pixel::rgb(255, 130, 170),
            Archetype::Miniboss => Pixel::rgb(255, 190, 60),
        }
    }

    pub fn gib_color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(180, 30, 40),
            Archetype::Pinkie => Pixel::rgb(190, 70, 120),
            Archetype::Miniboss => Pixel::rgb(200, 120, 40),
        }
    }

    pub fn update(&mut self, target: (f32, f32), arena: &Arena, dt: f32) {
        self.touch_cooldown = (self.touch_cooldown - dt).max(0.0);

        let dx = target.0 - self.x;
        let dy = target.1 - self.y;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-4 {
            return;
        }
        let inv = len2.sqrt().recip();
        let step_x = dx * inv * self.speed() * dt;
        let step_y = dy * inv * self.speed() * dt;

        let nx = self.x + step_x;
        if !collides(arena, nx, self.y) {
            self.x = nx;
        }
        let ny = self.y + step_y;
        if !collides(arena, self.x, ny) {
            self.y = ny;
        }
    }

    pub fn apply_damage(&mut self, dmg: i32) -> bool {
        self.hp -= dmg;
        self.hp <= 0
    }

    pub fn touch_player(&mut self, px: f32, py: f32) -> i32 {
        if self.touch_cooldown > 0.0 {
            return 0;
        }
        let dx = self.x - px;
        let dy = (self.y - py).abs().min((self.y + 1.0 - py).abs());
        let reach = match self.archetype {
            Archetype::Miniboss => 1.6,
            _ => 1.0,
        };
        if dx.abs() < reach && dy < reach {
            self.touch_cooldown = 0.5;
            return self.touch_damage();
        }
        0
    }

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32) {
        let px = ox + self.x.round() as i32;
        let py = oy + self.y.round() as i32;
        if px < 0 || py < 0 {
            return;
        }
        let c = self.color();
        fb.set(px as u16, py as u16, c);
        fb.set(px as u16, (py + 1) as u16, c);
        // Pinkies and minibosses are wider — paint a second column too.
        if matches!(self.archetype, Archetype::Pinkie | Archetype::Miniboss) {
            fb.set((px + 1) as u16, py as u16, c);
            fb.set((px + 1) as u16, (py + 1) as u16, c);
        }
    }
}

fn starting_hp(archetype: Archetype) -> i32 {
    match archetype {
        Archetype::Rusher => 60,
        Archetype::Pinkie => 140,
        Archetype::Miniboss => 550,
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}
