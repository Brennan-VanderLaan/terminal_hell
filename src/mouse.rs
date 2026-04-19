//! Mouse state. With the sextant framebuffer, each terminal cell covers a
//! 2×3 logical pixel area: cell (col, row) → pixels `(col*2..col*2+2, row*3..row*3+3)`.
//! The cursor's reported position is converted to a screen-pixel coord and
//! inverse-transformed through the active camera to produce a world-space
//! aim target.

use crate::camera::Camera;

#[derive(Default)]
pub struct Mouse {
    /// Last known cursor position, in terminal cells.
    pub col: u16,
    pub row: u16,
    pub lmb: bool,
}

impl Mouse {
    pub fn screen_pos(&self) -> (f32, f32) {
        (self.col as f32 * 2.0 + 1.0, self.row as f32 * 3.0 + 1.5)
    }

    /// Aim target in arena world coordinates. Inverse-transforms the cursor
    /// through the camera so aim works at any zoom / camera center.
    pub fn aim_target(&self, camera: &Camera) -> (f32, f32) {
        camera.screen_to_world(self.screen_pos())
    }
}
