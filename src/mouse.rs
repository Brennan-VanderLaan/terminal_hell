//! Mouse state. Terminal cells are twice as tall as they are wide relative to
//! our half-block pixels, so cell (col, row) maps to pixel (col, row*2 + 0.5)
//! — we pick the center of the cell vertically so aim doesn't snap to
//! top-of-cell as the cursor moves.

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
        let px = self.col as f32 - ox as f32;
        let py = self.row as f32 * 2.0 + 0.5 - oy as f32;
        (px, py)
    }
}
