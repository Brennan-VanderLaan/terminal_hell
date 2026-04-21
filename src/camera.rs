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

/// How far the tactical-view hold pulls the camera out when held,
/// multiplicative on top of `base_zoom`. 0.6 widens the visible
/// arena enough to see incoming threats without pushing sprites past
/// the Blob tier. Below 1.0 because this is a top-down shooter where
/// "aim" means seeing the battlefield, not iron-sighting one target.
pub const AIM_ZOOM_MULT: f32 = 0.6;
/// Seconds from 0 → full tactical ease (and back) when the hold
/// toggles. Short enough to feel responsive, long enough to avoid
/// flickering mip tiers on borderline zoom values.
pub const AIM_EASE_SECS: f32 = 0.14;

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub center: (f32, f32),
    /// Effective zoom used for render. Equals `base_zoom` scaled by
    /// the current aim-hold ease factor, clamped to [ZOOM_MIN, ZOOM_MAX].
    pub zoom: f32,
    /// Persistent zoom the player has chosen via ZoomIn/ZoomOut (and
    /// the mouse wheel). The AimZoom hold temporarily multiplies on
    /// top of this but never modifies it, so releasing aim snaps back
    /// to exactly the level the player set.
    pub base_zoom: f32,
    /// 0..1 ease factor for the aim-zoom hold. Driven by [`tick_zoom`].
    pub aim_ease: f32,
    pub viewport_w: u16, // screen pixels (2 × terminal cols)
    pub viewport_h: u16, // screen pixels (3 × terminal rows)
}

impl Camera {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            center: (0.0, 0.0),
            zoom: 1.0,
            base_zoom: 1.0,
            aim_ease: 0.0,
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

    /// Scale the persistent zoom. Mouse wheel + ZoomIn/ZoomOut
    /// actions both flow through here, so aim-hold releases snap
    /// back to whatever the player chose.
    pub fn adjust_zoom(&mut self, factor: f32) {
        self.base_zoom = (self.base_zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        self.recompute_effective_zoom();
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.base_zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.recompute_effective_zoom();
    }

    /// Ease the aim-zoom toward the target (1.0 while held, 0.0 while
    /// released) and recompute the effective zoom. Call once per
    /// render frame with the frame delta.
    pub fn tick_zoom(&mut self, dt: f32, aim_held: bool) {
        let target: f32 = if aim_held { 1.0 } else { 0.0 };
        let step = if AIM_EASE_SECS > 0.0 {
            dt / AIM_EASE_SECS
        } else {
            1.0
        };
        if (self.aim_ease - target).abs() <= step {
            self.aim_ease = target;
        } else if self.aim_ease < target {
            self.aim_ease += step;
        } else {
            self.aim_ease -= step;
        }
        self.recompute_effective_zoom();
    }

    fn recompute_effective_zoom(&mut self) {
        let mult = 1.0 + (AIM_ZOOM_MULT - 1.0) * self.aim_ease.clamp(0.0, 1.0);
        self.zoom = (self.base_zoom * mult).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Recompute `center` so the camera tracks `target` with optional edge
    /// nudging toward the mouse. `mouse_screen` is in screen-pixel space.
    pub fn follow(&mut self, target: (f32, f32), mouse_screen: (f32, f32)) {
        let cx = self.viewport_w as f32 / 2.0;
        let cy = self.viewport_h as f32 / 2.0;
        let fx = ((mouse_screen.0 - cx) / cx).clamp(-1.0, 1.0);
        let fy = ((mouse_screen.1 - cy) / cy).clamp(-1.0, 1.0);
        self.apply_follow(target, fx, fy);
    }

    /// Gamepad-equivalent of [`follow`] driven by a unit-length aim
    /// direction instead of a mouse cursor. Full-stick push nudges the
    /// camera the same amount as mouse-at-screen-edge. Values outside
    /// [-1, 1] are clamped.
    pub fn follow_dir(&mut self, target: (f32, f32), aim_dir: (f32, f32)) {
        let fx = aim_dir.0.clamp(-1.0, 1.0);
        let fy = aim_dir.1.clamp(-1.0, 1.0);
        self.apply_follow(target, fx, fy);
    }

    fn apply_follow(&mut self, target: (f32, f32), fx: f32, fy: f32) {
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
