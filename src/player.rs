use crate::arena::Arena;
use crate::camera::{Camera, MipLevel};
use crate::fb::{Framebuffer, Pixel};
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

    pub fn render(
        &self,
        fb: &mut Framebuffer,
        camera: &Camera,
        is_self: bool,
        marked: bool,
    ) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let center = (sx.round() as i32, sy.round() as i32);
        let self_color = Pixel::rgb(80, 255, 220);
        let other_color = Pixel::rgb(140, 255, 100);
        let tint = if is_self { self_color } else { other_color };

        let mip = camera.mip_level();
        if mip.shows_sprite() {
            let body = sprite::player_body();
            body.blit_scaled(fb, center, camera.zoom);
            let hi = matches!(mip, MipLevel::Hero);
            sprite::render_player_barrel(
                fb,
                sx,
                sy,
                self.aim_x,
                self.aim_y,
                camera.zoom,
                hi,
            );
            // Muzzle flash only at Close/Hero tiers — keeps the Normal tier
            // reading clean and matches where the barrel detail lives.
            if self.firing && mip.shows_overlay() {
                let intensity = if hi { 1.0 } else { 0.7 };
                sprite::render_muzzle_flash(
                    fb,
                    sx,
                    sy,
                    self.aim_x,
                    self.aim_y,
                    camera.zoom,
                    intensity,
                );
            }
            if marked {
                sprite::render_hastur_mark(fb, center.0, center.1);
            }
        } else if matches!(mip, MipLevel::Blob) {
            sprite::render_blob(fb, center, tint);
            if marked {
                sprite::render_hastur_mark(fb, center.0, center.1);
            }
        } else {
            sprite::render_dot(fb, center, tint);
            // Even at max zoom-out, marked survivors keep a tiny crown
            // so the team can locate them on the overview.
            if marked && center.1 >= 2 {
                fb.set(center.0 as u16, (center.1 - 2) as u16, Pixel::rgb(255, 220, 80));
            }
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
