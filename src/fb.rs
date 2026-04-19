//! Half-block framebuffer.
//!
//! Each terminal cell displays two stacked logical pixels (top half / bottom
//! half) via the Unicode half-block glyphs. A frame is:
//!
//! 1. Clear the pixel buffer.
//! 2. Game writes pixels at half-block resolution (width × 2*height).
//! 3. Resolve into terminal cells (char + fg + bg).
//! 4. Diff against the previously-emitted frame; emit only changed cells.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

/// Half-block glyphs.
const UPPER: char = '▀';
// Lower and full unused for now — UPPER with fg=top, bg=bottom encodes every
// cell state we need without branching on char choice.

/// One logical pixel.
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
    fn to_color(self) -> Color {
        Color::Rgb { r: self.r, g: self.g, b: self.b }
    }
}

/// A resolved terminal cell.
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
    /// Pixel rows = 2 * terminal rows. Indexed row-major as [y * cols + x].
    pixels: Vec<Pixel>,
    /// Currently displayed cells (one per terminal cell). Used for diffing.
    displayed: Vec<Cell>,
    /// Scratch buffer for newly-resolved cells.
    resolved: Vec<Cell>,
    /// Set when the next blit must redraw every cell (first frame, resize).
    force_full: bool,
}

impl Framebuffer {
    pub fn new(cols: u16, rows: u16) -> Self {
        let pixel_count = (cols as usize) * (rows as usize) * 2;
        let cell_count = (cols as usize) * (rows as usize);
        Self {
            cols,
            rows,
            pixels: vec![Pixel::BLACK; pixel_count],
            displayed: vec![Cell::empty(); cell_count],
            resolved: vec![Cell::empty(); cell_count],
            force_full: true,
        }
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Logical pixel height (== 2 * terminal rows).
    pub fn pixel_height(&self) -> u16 {
        self.rows * 2
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        *self = Self::new(cols, rows);
    }

    pub fn clear(&mut self) {
        for p in &mut self.pixels {
            *p = Pixel::BLACK;
        }
    }

    #[inline]
    pub fn set(&mut self, x: u16, y: u16, color: Pixel) {
        if x >= self.cols || y >= self.pixel_height() {
            return;
        }
        let idx = (y as usize) * (self.cols as usize) + (x as usize);
        self.pixels[idx] = color;
    }

    /// Fill a rectangle of logical pixels with a color.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: Pixel) {
        let x1 = x.saturating_add(w).min(self.cols);
        let y1 = y.saturating_add(h).min(self.pixel_height());
        for yy in y..y1 {
            for xx in x..x1 {
                self.set(xx, yy, color);
            }
        }
    }

    /// Resolve pixel pairs into terminal cells.
    fn resolve(&mut self) {
        let cols = self.cols as usize;
        for row in 0..(self.rows as usize) {
            for col in 0..cols {
                let top = self.pixels[(row * 2) * cols + col];
                let bot = self.pixels[(row * 2 + 1) * cols + col];
                self.resolved[row * cols + col] = Cell { ch: UPPER, fg: top, bg: bot };
            }
        }
    }

    /// Emit changed cells to the writer. ANSI heavy; buffer before flushing.
    pub fn blit<W: Write>(&mut self, out: &mut W) -> Result<()> {
        self.resolve();

        let cols = self.cols as usize;
        let mut last_fg: Option<Pixel> = None;
        let mut last_bg: Option<Pixel> = None;

        for row in 0..(self.rows as usize) {
            let mut run_start: Option<usize> = None;
            for col in 0..cols {
                let idx = row * cols + col;
                let new_cell = self.resolved[idx];
                if !self.force_full && new_cell == self.displayed[idx] {
                    if run_start.is_some() {
                        run_start = None;
                    }
                    continue;
                }

                // Start or continue a run of changed cells in this row.
                if run_start.is_none() {
                    queue!(out, MoveTo(col as u16, row as u16))?;
                    run_start = Some(col);
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

    /// Force the next blit to redraw every cell (on resize or after leaving
    /// the alternate screen and returning).
    pub fn invalidate(&mut self) {
        self.force_full = true;
    }
}
