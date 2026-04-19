//! Rendering camera. The simulation always works in world coordinates; the
//! camera is a pure render concept that transforms world positions onto
//! the sextant framebuffer grid.
//!
//! `center` is the world position that maps to the middle of the visible
//! viewport. `zoom` is screen-pixels per world-pixel: 1.0 is identity,
//! 2.0 draws everything 2× larger, 0.5 draws everything 2× smaller.
//!
//! The camera also derives a **mip level** from the current zoom.
//! Renderers query `mip_level()` and pick their own drawing strategy.
//! The 5 tiers, from most to least detail:
//!
//!   Hero   — zoom ≥ 4.0 — full sprite + HIGH-detail overlay (muzzle flash,
//!            wound decals, blood stains, burn soot)
//!   Close  — zoom ≥ 2.0 — full sprite + MID-detail overlay
//!   Normal — zoom ≥ 1.0 — base sprite, no overlay
//!   Blob   — zoom ≥ 0.35 — 3×3 colored blob
//!   Dot    — zoom <  0.35 — single screen pixel

pub const ZOOM_MIN: f32 = 0.2;
pub const ZOOM_MAX: f32 = 6.0;
pub const ZOOM_STEP: f32 = 1.25;

pub const MIP_HERO_MIN_ZOOM: f32 = 4.0;
pub const MIP_CLOSE_MIN_ZOOM: f32 = 2.0;
pub const MIP_NORMAL_MIN_ZOOM: f32 = 1.0;
pub const MIP_BLOB_MIN_ZOOM: f32 = 0.35;

const EDGE_NUDGE_ZONE: f32 = 0.30;
const EDGE_NUDGE_MAX_WORLD: f32 = 40.0;

/// Which MIP tier a given zoom falls into. Ordered from most detailed to
/// least so `level as u8` can be used for comparisons if needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MipLevel {
    Hero,
    Close,
    Normal,
    Blob,
    Dot,
}

impl MipLevel {
    /// True if renderers should draw a full procedural sprite rather than
    /// a blob/dot.
    pub fn shows_sprite(self) -> bool {
        matches!(self, MipLevel::Hero | MipLevel::Close | MipLevel::Normal)
    }
    /// True if sprite-local overlays (muzzle flash, wound decals, blood
    /// splatter, burn soot) should be composited on top of the base sprite.
    pub fn shows_overlay(self) -> bool {
        matches!(self, MipLevel::Hero | MipLevel::Close)
    }
    /// True for the most detailed tier where HIGH-intensity overlay elements
    /// (extra trail pixels, muzzle flash sparkle, barrel highlight) kick in.
    pub fn shows_hi_overlay(self) -> bool {
        matches!(self, MipLevel::Hero)
    }
}

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

    pub fn mip_level(&self) -> MipLevel {
        if self.zoom >= MIP_HERO_MIN_ZOOM {
            MipLevel::Hero
        } else if self.zoom >= MIP_CLOSE_MIN_ZOOM {
            MipLevel::Close
        } else if self.zoom >= MIP_NORMAL_MIN_ZOOM {
            MipLevel::Normal
        } else if self.zoom >= MIP_BLOB_MIN_ZOOM {
            MipLevel::Blob
        } else {
            MipLevel::Dot
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
