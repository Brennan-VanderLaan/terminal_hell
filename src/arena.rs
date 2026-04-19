//! Arena: tile grid in logical-pixel units (1 tile = 1 pixel in the
//! framebuffer). Walls have HP and a material; destruction reports back
//! which color to paint the glyph-confetti with.

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

    /// Seed-driven procedural arena. Deterministic given `(seed, width,
    /// height)`. Layout: thick perimeter, an open central disc for player
    /// spawn + kiosk placement, and 14–22 cover blobs scattered in the
    /// surrounding annulus. Each blob rolls a random aspect ratio so the
    /// arena doesn't feel tiled.
    pub fn generate(seed: u64, width: u16, height: u16) -> Self {
        let mut a = Self::new_empty(width, height);
        let w = width as i32;
        let h = height as i32;
        let mut rng = SmallRng::seed_from_u64(seed);

        // Thick perimeter.
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
        // Keep a central disc open for spawns, kiosks, and initial movement.
        let core_radius = (w.min(h * 2) / 6).max(10);
        let core_r2 = core_radius * core_radius;

        // Scatter cover blobs.
        let blob_count = rng.gen_range(14..=22);
        let margin = 5;
        for _ in 0..blob_count {
            let bw = rng.gen_range(3..=10) as i32;
            let bh = rng.gen_range(2..=6) as i32;
            let x = rng.gen_range(margin..w - margin - bw);
            let y = rng.gen_range(margin..h - margin - bh);
            // Skip blobs overlapping the central disc.
            let dx = x + bw / 2 - cx;
            let dy = y + bh / 2 - cy;
            if dx * dx + dy * dy < core_r2 {
                continue;
            }
            place_block(&mut a, x, y, bw, bh);
        }

        // A handful of long sightline-breakers.
        for _ in 0..rng.gen_range(3..=6) {
            let horizontal = rng.gen_bool(0.5);
            if horizontal {
                let len = rng.gen_range(8..=(w / 3).max(10)) as i32;
                let x = rng.gen_range(margin..w - margin - len);
                let y = rng.gen_range(margin..h - margin - 2);
                let dx = x + len / 2 - cx;
                let dy = y - cy;
                if dx * dx + dy * dy < core_r2 {
                    continue;
                }
                place_block(&mut a, x, y, len, 2);
            } else {
                let len = rng.gen_range(6..=(h / 3).max(8)) as i32;
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

    pub fn render(&self, fb: &mut Framebuffer, ox: i32, oy: i32) {
        // Floor renders as nothing — black terminal cells. Walls and rubble
        // pop against the dark; braille overlays (bullet trails, particles)
        // can show through on empty tiles.
        for y in 0..self.height {
            for x in 0..self.width {
                let px = ox + x as i32;
                let py = oy + y as i32;
                if px < 0 || py < 0 {
                    continue;
                }
                match self.tile(x as i32, y as i32) {
                    Tile::Floor => {}
                    Tile::Wall { material, hp } => {
                        let base = material.intact_color();
                        let ratio = (hp as f32 / WALL_HP as f32).clamp(0.35, 1.0);
                        let color = Pixel::rgb(
                            (base.r as f32 * ratio) as u8,
                            (base.g as f32 * ratio) as u8,
                            (base.b as f32 * ratio) as u8,
                        );
                        fb.set(px as u16, py as u16, color);
                    }
                    Tile::Rubble { material } => {
                        fb.set(px as u16, py as u16, material.debris_color());
                    }
                    Tile::Carcosa => {
                        // Yellow-ochre Carcosa tile; subtle two-color pattern
                        // so cells read as "wrong" without being blinding.
                        let pattern = ((x ^ y) & 1) == 0;
                        let color = if pattern {
                            Pixel::rgb(180, 140, 20)
                        } else {
                            Pixel::rgb(110, 80, 10)
                        };
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
