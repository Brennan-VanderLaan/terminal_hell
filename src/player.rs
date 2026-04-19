use crate::arena::Arena;
use crate::fb::{Framebuffer, Pixel};
use crate::input::Input;

pub struct Player {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub speed: f32,
    /// Aim direction normalized, in pixel-space. Sent by clients; used by the
    /// host to fan projectiles when the player fires.
    pub aim_x: f32,
    pub aim_y: f32,
    pub firing: bool,
}

impl Player {
    pub fn new(id: u32, x: f32, y: f32) -> Self {
        Self { id, x, y, hp: 100, speed: 38.0, aim_x: 1.0, aim_y: 0.0, firing: false }
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

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32, is_self: bool) {
        let px = ox + self.x.round() as i32;
        let py = oy + self.y.round() as i32;
        if px < 0 || py < 0 {
            return;
        }
        // Self is bright cyan, remotes are bright green — fast recognition.
        let color = if is_self {
            Pixel::rgb(80, 255, 220)
        } else {
            Pixel::rgb(140, 255, 100)
        };
        fb.set(px as u16, py as u16, color);
        fb.set(px as u16, (py + 1) as u16, color);
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}
