//! Arena: tile grid in logical-pixel units (1 tile = 1 pixel in the
//! framebuffer). Walls have HP and a material; destruction reports back
//! which color to paint the glyph-confetti with.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Concrete,
}

impl Material {
    pub fn intact_color(self) -> Pixel {
        match self {
            // Neon magenta synthwave concrete, for now.
            Material::Concrete => Pixel::rgb(240, 70, 200),
        }
    }
    pub fn debris_color(self) -> Pixel {
        match self {
            Material::Concrete => Pixel::rgb(130, 40, 110),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Tile {
    Floor,
    Wall { hp: u8, material: Material },
    Rubble { material: Material },
    /// Carcosa-terrain tile. Drains sanity from players standing on it;
    /// renders yellow-ochre. Non-destructible in v0.3.
    Carcosa,
}

pub struct HitOutcome {
    pub destroyed: bool,
    pub material: Material,
}

pub struct Arena {
    pub width: u16,
    pub height: u16,
    tiles: Vec<Tile>,
}

const WALL_HP: u8 = 2;

impl Arena {
    pub fn new_empty(width: u16, height: u16) -> Self {
        let n = (width as usize) * (height as usize);
        Self { width, height, tiles: vec![Tile::Floor; n] }
    }

    /// Seed-driven procedural arena. Scales blob + breaker counts with
    /// arena area so a 10× world feels populated, not sparse. Layout:
    /// thick perimeter, an open central disc for player spawn + kiosk
    /// placement, and rectangular cover blobs + long sightline breakers
    /// scattered across the annulus.
    pub fn generate(seed: u64, width: u16, height: u16) -> Self {
        let mut a = Self::new_empty(width, height);
        let w = width as i32;
        let h = height as i32;
        let mut rng = SmallRng::seed_from_u64(seed);

        for t in 0..3 {
            for x in 0..w {
                a.set_wall(x, t, Material::Concrete, WALL_HP * 3);
                a.set_wall(x, h - 1 - t, Material::Concrete, WALL_HP * 3);
            }
            for y in 0..h {
                a.set_wall(t, y, Material::Concrete, WALL_HP * 3);
                a.set_wall(w - 1 - t, y, Material::Concrete, WALL_HP * 3);
            }
        }

        let cx = w / 2;
        let cy = h / 2;
        let core_radius = (w.min(h * 2) / 6).max(10);
        let core_r2 = core_radius * core_radius;

        // Density-driven counts: roughly one blob per 4000 world pixels,
        // one breaker per 20000.
        let area = w.max(1) * h.max(1);
        let blob_count = (area / 4000).clamp(14, 600);
        let breaker_count = (area / 20000).clamp(3, 120);

        let margin = 5;
        for _ in 0..blob_count {
            let bw = rng.gen_range(3..=10) as i32;
            let bh = rng.gen_range(2..=6) as i32;
            if w <= margin * 2 + bw || h <= margin * 2 + bh {
                continue;
            }
            let x = rng.gen_range(margin..w - margin - bw);
            let y = rng.gen_range(margin..h - margin - bh);
            let dx = x + bw / 2 - cx;
            let dy = y + bh / 2 - cy;
            if dx * dx + dy * dy < core_r2 {
                continue;
            }
            place_block(&mut a, x, y, bw, bh);
        }

        for _ in 0..breaker_count {
            let horizontal = rng.gen_bool(0.5);
            if horizontal {
                let len = rng.gen_range(8..=(w / 6).max(10)) as i32;
                if w <= margin * 2 + len {
                    continue;
                }
                let x = rng.gen_range(margin..w - margin - len);
                let y = rng.gen_range(margin..h - margin - 2);
                let dx = x + len / 2 - cx;
                let dy = y - cy;
                if dx * dx + dy * dy < core_r2 {
                    continue;
                }
                place_block(&mut a, x, y, len, 2);
            } else {
                let len = rng.gen_range(6..=(h / 6).max(8)) as i32;
                if h <= margin * 2 + len {
                    continue;
                }
                let x = rng.gen_range(margin..w - margin - 2);
                let y = rng.gen_range(margin..h - margin - len);
                let dx = x - cx;
                let dy = y + len / 2 - cy;
                if dx * dx + dy * dy < core_r2 {
                    continue;
                }
                place_block(&mut a, x, y, 2, len);
            }
        }

        a
    }

    /// Backwards-compat hand-crafted layout used before the generator. Kept
    /// for tests; live code should prefer `generate`.
    pub fn hand_crafted(width: u16, height: u16) -> Self {
        let mut a = Self::new_empty(width, height);
        let w = width as i32;
        let h = height as i32;

        // Thick perimeter (3 pixels) for visual weight. Outer ring is
        // effectively indestructible via triple HP.
        for t in 0..3 {
            for x in 0..w {
                a.set_wall(x, t, Material::Concrete, WALL_HP * 3);
                a.set_wall(x, h - 1 - t, Material::Concrete, WALL_HP * 3);
            }
            for y in 0..h {
                a.set_wall(t, y, Material::Concrete, WALL_HP * 3);
                a.set_wall(w - 1 - t, y, Material::Concrete, WALL_HP * 3);
            }
        }

        // Interior cover — scales with arena width so layouts stay readable
        // whether the terminal is 120 cols or 300 cols.
        let cx = w / 2;
        let cy = h / 2;
        let u = (w / 28).max(3); // "unit" — scales cover blocks proportionally

        place_block(&mut a, cx - 8 * u, cy - 3 * u, 2 * u, 3 * u);
        place_block(&mut a, cx + 6 * u, cy - 3 * u, 2 * u, 3 * u);
        place_block(&mut a, cx - 4 * u, cy - 5 * u, 8 * u, u.min(3));
        place_block(&mut a, cx - 4 * u, cy + 5 * u - u.min(3), 8 * u, u.min(3));
        place_block(&mut a, cx - 5 * u, cy, 3 * u, u.min(2).max(2));
        place_block(&mut a, cx + 2 * u, cy, 3 * u, u.min(2).max(2));

        a
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some((y as usize) * (self.width as usize) + (x as usize))
    }

    #[inline]
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        match self.idx(x, y) {
            Some(i) => self.tiles[i],
            None => Tile::Wall { hp: u8::MAX, material: Material::Concrete },
        }
    }

    pub fn is_wall(&self, x: i32, y: i32) -> bool {
        matches!(self.tile(x, y), Tile::Wall { .. })
    }

    pub fn is_passable(&self, x: i32, y: i32) -> bool {
        matches!(self.tile(x, y), Tile::Floor | Tile::Rubble { .. } | Tile::Carcosa)
    }

    pub fn is_carcosa(&self, x: i32, y: i32) -> bool {
        matches!(self.tile(x, y), Tile::Carcosa)
    }

    pub fn set_carcosa(&mut self, x: i32, y: i32) {
        if let Some(i) = self.idx(x, y) {
            self.tiles[i] = Tile::Carcosa;
        }
    }

    pub fn set_wall(&mut self, x: i32, y: i32, material: Material, hp: u8) {
        if let Some(i) = self.idx(x, y) {
            self.tiles[i] = Tile::Wall { hp, material };
        }
    }

    pub fn set_rubble(&mut self, x: i32, y: i32, material: Material) {
        if let Some(i) = self.idx(x, y) {
            self.tiles[i] = Tile::Rubble { material };
        }
    }

    pub fn set_floor(&mut self, x: i32, y: i32) {
        if let Some(i) = self.idx(x, y) {
            self.tiles[i] = Tile::Floor;
        }
    }

    /// Raw tile iteration for initial-snapshot serialization.
    pub fn encode_tiles(&self) -> Vec<u8> {
        // 2 bytes per tile: (kind, hp). kind: 0=Floor, 1=Wall, 2=Rubble, 3=Carcosa.
        let mut out = Vec::with_capacity(self.tiles.len() * 2);
        for t in &self.tiles {
            match *t {
                Tile::Floor => {
                    out.push(0);
                    out.push(0);
                }
                Tile::Wall { hp, .. } => {
                    out.push(1);
                    out.push(hp);
                }
                Tile::Rubble { .. } => {
                    out.push(2);
                    out.push(0);
                }
                Tile::Carcosa => {
                    out.push(3);
                    out.push(0);
                }
            }
        }
        out
    }

    pub fn decode_tiles(width: u16, height: u16, bytes: &[u8]) -> Option<Self> {
        let expected = (width as usize) * (height as usize) * 2;
        if bytes.len() != expected {
            return None;
        }
        let mut tiles = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let kind = chunk[0];
            let hp = chunk[1];
            tiles.push(match kind {
                0 => Tile::Floor,
                1 => Tile::Wall { hp, material: Material::Concrete },
                2 => Tile::Rubble { material: Material::Concrete },
                _ => Tile::Carcosa,
            });
        }
        Some(Self { width, height, tiles })
    }

    /// Apply 1 point of structural damage to a wall tile. Returns None if
    /// there was no wall there (projectile should still despawn on impact —
    /// the caller may treat that as "missed" vs "absorbed" as needed).
    pub fn damage_wall(&mut self, x: i32, y: i32) -> Option<HitOutcome> {
        let i = self.idx(x, y)?;
        let tile = self.tiles[i];
        match tile {
            Tile::Wall { hp, material } => {
                if hp <= 1 {
                    self.tiles[i] = Tile::Rubble { material };
                    Some(HitOutcome { destroyed: true, material })
                } else {
                    self.tiles[i] = Tile::Wall { hp: hp - 1, material };
                    Some(HitOutcome { destroyed: false, material })
                }
            }
            _ => None,
        }
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (wx0, wy0, wx1, wy1) = camera.world_viewport();
        let ix0 = (wx0.floor() as i32).max(0);
        let iy0 = (wy0.floor() as i32).max(0);
        let ix1 = (wx1.ceil() as i32).min(self.width as i32);
        let iy1 = (wy1.ceil() as i32).min(self.height as i32);

        for wy in iy0..iy1 {
            for wx in ix0..ix1 {
                let color = match self.tile(wx, wy) {
                    Tile::Floor => continue,
                    Tile::Wall { material, hp } => {
                        let base = material.intact_color();
                        let ratio = (hp as f32 / WALL_HP as f32).clamp(0.35, 1.0);
                        Pixel::rgb(
                            (base.r as f32 * ratio) as u8,
                            (base.g as f32 * ratio) as u8,
                            (base.b as f32 * ratio) as u8,
                        )
                    }
                    Tile::Rubble { material } => material.debris_color(),
                    Tile::Carcosa => {
                        let pattern = ((wx ^ wy) & 1) == 0;
                        if pattern {
                            Pixel::rgb(180, 140, 20)
                        } else {
                            Pixel::rgb(110, 80, 10)
                        }
                    }
                };

                // Screen rect for this world tile. Nearest-neighbor fill:
                // at zoom > 1 we upscale; at zoom < 1 multiple tiles map to
                // the same screen pixel and last-write wins (acceptable).
                let (sx0, sy0) = camera.world_to_screen((wx as f32, wy as f32));
                let (sx1, sy1) =
                    camera.world_to_screen(((wx + 1) as f32, (wy + 1) as f32));
                let x0 = (sx0.floor() as i32).max(0);
                let y0 = (sy0.floor() as i32).max(0);
                let x1 = (sx1.ceil() as i32).min(camera.viewport_w as i32);
                let y1 = (sy1.ceil() as i32).min(camera.viewport_h as i32);
                for py in y0..y1 {
                    for px in x0..x1 {
                        fb.set(px as u16, py as u16, color);
                    }
                }
            }
        }
    }
}

fn place_block(a: &mut Arena, x: i32, y: i32, w: i32, h: i32) {
    for yy in y..(y + h) {
        for xx in x..(x + w) {
            a.set_wall(xx, yy, Material::Concrete, WALL_HP);
        }
    }
}
