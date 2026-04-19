use crate::arena::Arena;
use crate::fb::Framebuffer;
use crate::input::Input;
use crate::sprite;

pub struct Player {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub speed: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub firing: bool,
    /// 0..100. Drains from Carcosa exposure, Hastur marks, and Sign flashes.
    /// Regens from kills in good form (for now, any kill nudges).
    pub sanity: f32,
}

impl Player {
    pub fn new(id: u32, x: f32, y: f32) -> Self {
        Self {
            id,
            x,
            y,
            hp: 100,
            speed: 38.0,
            aim_x: 1.0,
            aim_y: 0.0,
            firing: false,
            sanity: 100.0,
        }
    }

    pub fn apply_input_local(&mut self, input: &Input, arena: &Arena, dt: f32) {
        let (dx, dy) = input.move_vec();
        self.step(dx, dy, arena, dt);
    }

    pub fn apply_remote_input(
        &mut self,
        move_x: f32,
        move_y: f32,
        aim_x: f32,
        aim_y: f32,
        firing: bool,
        arena: &Arena,
        dt: f32,
    ) {
        self.aim_x = aim_x;
        self.aim_y = aim_y;
        self.firing = firing;
        self.step(move_x, move_y, arena, dt);
    }

    fn step(&mut self, dx: f32, dy: f32, arena: &Arena, dt: f32) {
        let step_x = dx * self.speed * dt;
        let step_y = dy * self.speed * dt;
        let target_x = self.x + step_x;
        if !collides(arena, target_x, self.y) {
            self.x = target_x;
        }
        let target_y = self.y + step_y;
        if !collides(arena, self.x, target_y) {
            self.y = target_y;
        }
    }

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32, _is_self: bool, marked: bool) {
        let cx = ox + self.x.round() as i32;
        let cy = oy + self.y.round() as i32;
        sprite::player_body().blit(fb, cx, cy);
        sprite::render_player_barrel(fb, cx as f32, cy as f32, self.aim_x, self.aim_y);
        if marked {
            sprite::render_hastur_mark(fb, cx, cy);
        }
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    // 1×2 collider in pixel space — movement still uses the old tile grid for
    // wall collision; sprite size is visual-only.
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}
