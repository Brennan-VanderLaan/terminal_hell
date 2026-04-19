//! Rendering camera. The simulation always works in world coordinates; the
//! camera is a pure render concept that transforms world positions onto
//! the sextant framebuffer grid.
//!
//! `center` is the world position that maps to the middle of the visible
//! viewport. `zoom` is screen-pixels per world-pixel: 1.0 is identity,
//! 2.0 draws everything 2× larger, 0.5 draws everything 2× smaller.
//!
//! The camera also derives a **mip level** from the current zoom: full
//! sprites at L0, simplified blobs at L1, single dots at L2. Renderers
//! query `mip_level()` and pick their own drawing strategy.

pub const ZOOM_MIN: f32 = 0.2;
pub const ZOOM_MAX: f32 = 3.0;
pub const ZOOM_STEP: f32 = 1.25;

pub const MIP_L0_MIN_ZOOM: f32 = 1.0;
pub const MIP_L1_MIN_ZOOM: f32 = 0.35;

const EDGE_NUDGE_ZONE: f32 = 0.30;
const EDGE_NUDGE_MAX_WORLD: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub center: (f32, f32),
    pub zoom: f32,
    pub viewport_w: u16, // screen pixels (2 × terminal cols)
    pub viewport_h: u16, // screen pixels (3 × terminal rows)
}

impl Camera {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            center: (0.0, 0.0),
            zoom: 1.0,
            viewport_w: cols.saturating_mul(2),
            viewport_h: rows.saturating_mul(3),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.viewport_w = cols.saturating_mul(2);
        self.viewport_h = rows.saturating_mul(3);
    }

    /// World position (x, y) → screen pixel position (x, y).
    pub fn world_to_screen(&self, world: (f32, f32)) -> (f32, f32) {
        let cx = self.viewport_w as f32 / 2.0;
        let cy = self.viewport_h as f32 / 2.0;
        let dx = world.0 - self.center.0;
        let dy = world.1 - self.center.1;
        (cx + dx * self.zoom, cy + dy * self.zoom)
    }

    pub fn screen_to_world(&self, screen: (f32, f32)) -> (f32, f32) {
        let cx = self.viewport_w as f32 / 2.0;
        let cy = self.viewport_h as f32 / 2.0;
        (
            (screen.0 - cx) / self.zoom + self.center.0,
            (screen.1 - cy) / self.zoom + self.center.1,
        )
    }

    /// Axis-aligned bounds of the visible world, padded slightly so tiles
    /// whose centers are just past the edge still draw.
    pub fn world_viewport(&self) -> (f32, f32, f32, f32) {
        let tl = self.screen_to_world((0.0, 0.0));
        let br = self.screen_to_world((self.viewport_w as f32, self.viewport_h as f32));
        (tl.0 - 1.0, tl.1 - 1.0, br.0 + 1.0, br.1 + 1.0)
    }

    pub fn mip_level(&self) -> u8 {
        if self.zoom >= MIP_L0_MIN_ZOOM {
            0
        } else if self.zoom >= MIP_L1_MIN_ZOOM {
            1
        } else {
            2
        }
    }

    pub fn adjust_zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Recompute `center` so the camera tracks `target` with optional edge
    /// nudging toward the mouse. `mouse_screen` is in screen-pixel space.
    pub fn follow(&mut self, target: (f32, f32), mouse_screen: (f32, f32)) {
        let cx = self.viewport_w as f32 / 2.0;
        let cy = self.viewport_h as f32 / 2.0;
        let fx = ((mouse_screen.0 - cx) / cx).clamp(-1.0, 1.0);
        let fy = ((mouse_screen.1 - cy) / cy).clamp(-1.0, 1.0);

        let nudge_axis = |f: f32| -> f32 {
            let a = f.abs();
            if a > EDGE_NUDGE_ZONE {
                let t = (a - EDGE_NUDGE_ZONE) / (1.0 - EDGE_NUDGE_ZONE);
                f.signum() * t * EDGE_NUDGE_MAX_WORLD
            } else {
                0.0
            }
        };

        // Divide nudge by zoom so the visual "look-ahead" distance in
        // *screen* pixels stays roughly constant regardless of zoom level.
        let nz = self.zoom.max(0.01);
        self.center = (
            target.0 + nudge_axis(fx) / nz,
            target.1 + nudge_axis(fy) / nz,
        );
    }

    /// Director mode (or any free-camera scenario): parks the camera at
    /// a fixed world position with a fixed zoom. Kept for future use.
    #[allow(dead_code)]
    pub fn set_fixed(&mut self, center: (f32, f32), zoom: f32) {
        self.center = center;
        self.set_zoom(zoom);
    }
}
