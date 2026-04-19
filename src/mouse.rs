//! Mouse state. With the sextant framebuffer, each terminal cell covers a
//! 2×3 logical pixel area: cell (col, row) → pixels `(col*2..col*2+2, row*3..row*3+3)`.
//! We pick the cell center (col*2 + 1, row*3 + 1.5) for aim so the cursor
//! doesn't snap to a corner.

#[derive(Default)]
pub struct Mouse {
    /// Last known cursor position, in terminal cells.
    pub col: u16,
    pub row: u16,
    pub lmb: bool,
}

impl Mouse {
    /// Aim target in arena pixel coordinates, given the arena's screen origin.
    pub fn aim_target(&self, origin: (i32, i32)) -> (f32, f32) {
        let (ox, oy) = origin;
        let px = self.col as f32 * 2.0 + 1.0 - ox as f32;
        let py = self.row as f32 * 3.0 + 1.5 - oy as f32;
        (px, py)
    }
}
