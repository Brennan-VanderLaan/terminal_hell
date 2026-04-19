//! Hybrid framebuffer: **sextants** (2×3 per terminal cell) for solid fills,
//! **braille** (2×4 per cell) for dotted overlays.
//!
//! A frame:
//! 1. Clear both layers.
//! 2. Game writes solid pixels into the sextant grid; optional fine details
//!    into the braille grid.
//! 3. Resolve per terminal cell:
//!    - If any sextant pixel is lit → pick a 2-color (FG/BG) sextant glyph.
//!    - Else if any braille dot is lit → emit a braille glyph with its fg.
//!    - Else → empty space.
//! 4. Diff against the previously-displayed cell buffer; emit only changes.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Pixel {
    pub const BLACK: Pixel = Pixel { r: 0, g: 0, b: 0 };
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub fn is_black(self) -> bool {
        self.r == 0 && self.g == 0 && self.b == 0
    }
    fn to_color(self) -> Color {
        Color::Rgb { r: self.r, g: self.g, b: self.b }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Cell {
    ch: char,
    fg: Pixel,
    bg: Pixel,
}

impl Cell {
    fn empty() -> Self {
        Self { ch: ' ', fg: Pixel::BLACK, bg: Pixel::BLACK }
    }
}

pub struct Framebuffer {
    cols: u16,
    rows: u16,
    /// Sextant pixel grid: (2*cols) wide × (3*rows) tall, row-major.
    sex_pixels: Vec<Pixel>,
    /// Braille dot bitmaps, one byte per terminal cell (8 dots).
    braille_dots: Vec<u8>,
    /// One color per terminal cell, applied when that cell renders as braille.
    braille_fg: Vec<Pixel>,
    /// Additive bloom contributions, pixel-resolution. Flushed at resolve
    /// time by adding into lit pixels and brightening adjacent dark
    /// pixels. Sparse set: `bloom_seeds` records the origins so the
    /// accumulate step only touches pixels near a seed.
    bloom_accum: Vec<(u16, u16, u16)>, // (r, g, b) clamped at resolve
    bloom_seeds: Vec<BloomSeed>,
    /// Optional ambient tint applied to every lit pixel at blit time.
    /// `(color, amount)` where amount is 0..1 (0 = no tint, 1 = replace).
    /// Used by the Corruption curve to amber-wash the arena.
    tint: Option<(Pixel, f32)>,
    /// Previously-emitted cells for diffing.
    displayed: Vec<Cell>,
    resolved: Vec<Cell>,
    force_full: bool,
}

/// A bloom source: pixel-resolution position, color, radius, strength.
/// Radius is clamped to `BLOOM_MAX_RADIUS` at accumulation time.
#[derive(Clone, Copy, Debug)]
pub struct BloomSeed {
    pub x: f32,
    pub y: f32,
    pub color: Pixel,
    pub radius: u8,
    pub strength: f32,
}

pub const BLOOM_MAX_RADIUS: u8 = 6;

impl Framebuffer {
    pub fn new(cols: u16, rows: u16) -> Self {
        let cells = (cols as usize) * (rows as usize);
        let sex = (cols as usize) * 2 * (rows as usize) * 3;
        Self {
            cols,
            rows,
            sex_pixels: vec![Pixel::BLACK; sex],
            braille_dots: vec![0u8; cells],
            braille_fg: vec![Pixel::BLACK; cells],
            bloom_accum: vec![(0u16, 0u16, 0u16); sex],
            bloom_seeds: Vec::new(),
            displayed: vec![Cell::empty(); cells],
            resolved: vec![Cell::empty(); cells],
            tint: None,
            force_full: true,
        }
    }

    /// Record a bloom seed for this frame. Applied additively during
    /// resolve — brightens the seed pixel and nearby pixels within
    /// `radius` with a falloff dictated by distance.
    pub fn bloom_seed(&mut self, x: f32, y: f32, color: Pixel, radius: u8, strength: f32) {
        if strength <= 0.0 {
            return;
        }
        self.bloom_seeds.push(BloomSeed {
            x,
            y,
            color,
            radius: radius.min(BLOOM_MAX_RADIUS),
            strength: strength.clamp(0.0, 2.0),
        });
    }

    /// Enable an ambient post-resolve tint. Passing `None` or `amount <= 0`
    /// disables it. The amount is clamped to 0..1.
    pub fn set_tint(&mut self, color: Pixel, amount: f32) {
        let a = amount.clamp(0.0, 1.0);
        if a <= 0.0 {
            if self.tint.is_some() {
                self.tint = None;
                self.force_full = true;
            }
        } else {
            let new = Some((color, a));
            if self.tint != new {
                self.tint = new;
                self.force_full = true;
            }
        }
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }
    pub fn rows(&self) -> u16 {
        self.rows
    }
    pub fn pixel_width(&self) -> u16 {
        self.cols * 2
    }
    pub fn pixel_height(&self) -> u16 {
        self.rows * 3
    }
    pub fn braille_height(&self) -> u16 {
        self.rows * 4
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        // New Framebuffer includes a fresh bloom_accum sized to the
        // new pixel grid, so a simple replace is correct.
        *self = Self::new(cols, rows);
    }

    pub fn clear(&mut self) {
        for p in &mut self.sex_pixels {
            *p = Pixel::BLACK;
        }
        for d in &mut self.braille_dots {
            *d = 0;
        }
        for c in &mut self.braille_fg {
            *c = Pixel::BLACK;
        }
        for b in &mut self.bloom_accum {
            *b = (0, 0, 0);
        }
        self.bloom_seeds.clear();
    }

    /// Accumulate every recorded bloom seed into `bloom_accum`. Called
    /// from `resolve` right before the tint+cell-resolve pass so the
    /// bloom contributions survive into the final output. O(seeds × r²)
    /// — cheap given the BLOOM_MAX_RADIUS cap.
    fn accumulate_bloom(&mut self) {
        let pw = self.pixel_width() as i32;
        let ph = self.pixel_height() as i32;
        for seed in &self.bloom_seeds {
            let r = seed.radius as i32;
            if r == 0 {
                continue;
            }
            let cx = seed.x as i32;
            let cy = seed.y as i32;
            let r2 = (r * r) as f32;
            for dy in -r..=r {
                let py = cy + dy;
                if py < 0 || py >= ph {
                    continue;
                }
                for dx in -r..=r {
                    let px = cx + dx;
                    if px < 0 || px >= pw {
                        continue;
                    }
                    let d2 = (dx * dx + dy * dy) as f32;
                    if d2 > r2 {
                        continue;
                    }
                    // Linear falloff by distance, times the seed's
                    // strength.
                    let falloff = 1.0 - (d2 / r2).sqrt();
                    let amount = falloff * seed.strength;
                    let idx = (py as usize) * (pw as usize) + (px as usize);
                    let acc = &mut self.bloom_accum[idx];
                    acc.0 = (acc.0 as u32
                        + (seed.color.r as u32 * (amount * 255.0) as u32 / 255))
                        .min(u16::MAX as u32) as u16;
                    acc.1 = (acc.1 as u32
                        + (seed.color.g as u32 * (amount * 255.0) as u32 / 255))
                        .min(u16::MAX as u32) as u16;
                    acc.2 = (acc.2 as u32
                        + (seed.color.b as u32 * (amount * 255.0) as u32 / 255))
                        .min(u16::MAX as u32) as u16;
                }
            }
        }
    }

    #[inline]
    pub fn set(&mut self, x: u16, y: u16, color: Pixel) {
        let pw = self.pixel_width();
        let ph = self.pixel_height();
        if x >= pw || y >= ph {
            return;
        }
        self.sex_pixels[(y as usize) * (pw as usize) + (x as usize)] = color;
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: Pixel) {
        let x1 = x.saturating_add(w).min(self.pixel_width());
        let y1 = y.saturating_add(h).min(self.pixel_height());
        for yy in y..y1 {
            for xx in x..x1 {
                self.set(xx, yy, color);
            }
        }
    }

    /// Set a braille dot. Coordinates are in dot-space: (2*cols) × (4*rows).
    #[inline]
    pub fn braille_set(&mut self, x: u16, y: u16, color: Pixel) {
        if x >= self.cols * 2 || y >= self.rows * 4 {
            return;
        }
        let col = (x / 2) as usize;
        let row = (y / 4) as usize;
        let sx = (x % 2) as u8;
        let sy = (y % 4) as u8;
        let cell_idx = row * (self.cols as usize) + col;
        self.braille_dots[cell_idx] |= 1 << braille_bit(sx, sy);
        // Latest write wins for the cell color.
        self.braille_fg[cell_idx] = color;
    }

    fn resolve(&mut self) {
        // Accumulate bloom first so it appears behind / alongside the
        // other rendered pixels but before the tint pass.
        if !self.bloom_seeds.is_empty() {
            self.accumulate_bloom();
        }
        let cols = self.cols as usize;
        let pw = self.pixel_width() as usize;
        let tint = self.tint;
        for row in 0..(self.rows as usize) {
            for col in 0..cols {
                let base_x = col * 2;
                let base_y = row * 3;
                let bloom = !self.bloom_seeds.is_empty();
                let mut p00 = self.sex_pixels[base_y * pw + base_x];
                let mut p10 = self.sex_pixels[base_y * pw + base_x + 1];
                let mut p01 = self.sex_pixels[(base_y + 1) * pw + base_x];
                let mut p11 = self.sex_pixels[(base_y + 1) * pw + base_x + 1];
                let mut p02 = self.sex_pixels[(base_y + 2) * pw + base_x];
                let mut p12 = self.sex_pixels[(base_y + 2) * pw + base_x + 1];
                if bloom {
                    apply_bloom(&mut p00, self.bloom_accum[base_y * pw + base_x]);
                    apply_bloom(&mut p10, self.bloom_accum[base_y * pw + base_x + 1]);
                    apply_bloom(&mut p01, self.bloom_accum[(base_y + 1) * pw + base_x]);
                    apply_bloom(&mut p11, self.bloom_accum[(base_y + 1) * pw + base_x + 1]);
                    apply_bloom(&mut p02, self.bloom_accum[(base_y + 2) * pw + base_x]);
                    apply_bloom(&mut p12, self.bloom_accum[(base_y + 2) * pw + base_x + 1]);
                }
                if let Some((tc, a)) = tint {
                    apply_tint(&mut p00, tc, a);
                    apply_tint(&mut p10, tc, a);
                    apply_tint(&mut p01, tc, a);
                    apply_tint(&mut p11, tc, a);
                    apply_tint(&mut p02, tc, a);
                    apply_tint(&mut p12, tc, a);
                }
                let pixels = [p00, p10, p01, p11, p02, p12];

                let cell_idx = row * cols + col;
                let any_sex = pixels.iter().any(|p| !p.is_black());
                if any_sex {
                    let (ch, fg, bg) = resolve_sextant(&pixels);
                    self.resolved[cell_idx] = Cell { ch, fg, bg };
                } else {
                    let dots = self.braille_dots[cell_idx];
                    if dots != 0 {
                        let ch = char::from_u32(0x2800 + dots as u32).unwrap_or('⠀');
                        let mut fg = self.braille_fg[cell_idx];
                        if let Some((tc, a)) = tint {
                            apply_tint(&mut fg, tc, a);
                        }
                        self.resolved[cell_idx] =
                            Cell { ch, fg, bg: Pixel::BLACK };
                    } else {
                        self.resolved[cell_idx] = Cell::empty();
                    }
                }
            }
        }
    }

    pub fn blit<W: Write>(&mut self, out: &mut W) -> Result<()> {
        self.resolve();

        let cols = self.cols as usize;
        let mut last_fg: Option<Pixel> = None;
        let mut last_bg: Option<Pixel> = None;

        for row in 0..(self.rows as usize) {
            let mut run_active = false;
            for col in 0..cols {
                let idx = row * cols + col;
                let new_cell = self.resolved[idx];
                if !self.force_full && new_cell == self.displayed[idx] {
                    run_active = false;
                    continue;
                }
                if !run_active {
                    queue!(out, MoveTo(col as u16, row as u16))?;
                    run_active = true;
                }
                if last_fg != Some(new_cell.fg) {
                    queue!(out, SetForegroundColor(new_cell.fg.to_color()))?;
                    last_fg = Some(new_cell.fg);
                }
                if last_bg != Some(new_cell.bg) {
                    queue!(out, SetBackgroundColor(new_cell.bg.to_color()))?;
                    last_bg = Some(new_cell.bg);
                }
                queue!(out, Print(new_cell.ch))?;
                self.displayed[idx] = new_cell;
            }
        }

        queue!(out, ResetColor)?;
        out.flush()?;
        self.force_full = false;
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.force_full = true;
    }
}

/// Sextant codepoint for a 6-bit pattern. Bit `i` = pixel at
/// (sx = i & 1, sy = i >> 1). Bit 0 is top-left, bit 5 is bottom-right.
fn sextant_glyph(pattern: u8) -> char {
    let p = pattern & 0x3F;
    match p {
        0 => ' ',
        0b010101 => '\u{258C}', // ▌ left half column
        0b101010 => '\u{2590}', // ▐ right half column
        0b111111 => '\u{2588}', // █ full block
        _ => {
            // U+1FB00..U+1FB3B holds 60 sextants, skipping the 4 reused
            // patterns above. Count skipped patterns below `p`.
            let mut skipped: u32 = 1; // 0 is always below
            if p > 21 {
                skipped += 1;
            }
            if p > 42 {
                skipped += 1;
            }
            char::from_u32(0x1FB00 + (p as u32) - skipped).unwrap_or('?')
        }
    }
}

/// Map (sx in 0..2, sy in 0..4) to braille bit index per Unicode spec:
/// columns are left=1/2/3/7, right=4/5/6/8; bits 0..5 follow the visual
/// top-to-bottom, left-to-right order, and bits 6/7 are the extra bottom row.
fn braille_bit(sx: u8, sy: u8) -> u8 {
    match (sx, sy) {
        (0, 0) => 0,
        (0, 1) => 1,
        (0, 2) => 2,
        (0, 3) => 6,
        (1, 0) => 3,
        (1, 1) => 4,
        (1, 2) => 5,
        (1, 3) => 7,
        _ => 0,
    }
}

/// Choose a (char, fg, bg) for a 6-pixel sextant cell. Strategy: the most
/// common non-black color becomes FG; the next-most-common color (often
/// black) becomes BG. Pattern bits mark FG-colored pixels.
fn apply_tint(p: &mut Pixel, tint: Pixel, amount: f32) {
    if p.is_black() {
        return; // don't light up empty space
    }
    let a = amount;
    let inv = 1.0 - a;
    p.r = ((p.r as f32) * inv + (tint.r as f32) * a).round() as u8;
    p.g = ((p.g as f32) * inv + (tint.g as f32) * a).round() as u8;
    p.b = ((p.b as f32) * inv + (tint.b as f32) * a).round() as u8;
}

/// Additive bloom blend. Adds the accumulated bloom contribution onto
/// a pixel with saturating clamp, *including* pixels that started black —
/// that's how a glowing substance lights up adjacent empty space.
fn apply_bloom(p: &mut Pixel, bloom: (u16, u16, u16)) {
    if bloom.0 == 0 && bloom.1 == 0 && bloom.2 == 0 {
        return;
    }
    p.r = (p.r as u16).saturating_add(bloom.0).min(255) as u8;
    p.g = (p.g as u16).saturating_add(bloom.1).min(255) as u8;
    p.b = (p.b as u16).saturating_add(bloom.2).min(255) as u8;
}

fn resolve_sextant(pixels: &[Pixel; 6]) -> (char, Pixel, Pixel) {
    // Small-count bucketing — max 6 distinct colors so linear scan is fine.
    let mut counts: [(Pixel, u8); 6] = [(Pixel::BLACK, 0); 6];
    let mut n = 0usize;
    for &p in pixels {
        let mut found = false;
        for entry in counts.iter_mut().take(n) {
            if entry.0 == p {
                entry.1 += 1;
                found = true;
                break;
            }
        }
        if !found && n < 6 {
            counts[n] = (p, 1);
            n += 1;
        }
    }
    let buckets = &mut counts[..n];
    // Sort descending by count.
    buckets.sort_by(|a, b| b.1.cmp(&a.1));

    // Prefer the most common non-black color as FG. If everything is black,
    // emit a space.
    let fg = buckets.iter().find(|(c, _)| !c.is_black()).map(|(c, _)| *c);
    let Some(fg) = fg else {
        return (' ', Pixel::BLACK, Pixel::BLACK);
    };

    // BG is the most common color that isn't FG. Falls back to black when
    // everything matches FG — full block.
    let bg = buckets.iter().find(|(c, _)| *c != fg).map(|(c, _)| *c).unwrap_or(Pixel::BLACK);

    let mut pattern = 0u8;
    for (i, p) in pixels.iter().enumerate() {
        if *p == fg {
            pattern |= 1 << i;
        }
    }
    (sextant_glyph(pattern), fg, bg)
}
