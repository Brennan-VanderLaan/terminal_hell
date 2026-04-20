//! Uniform-grid spatial hash for O(1) neighbor queries.
//!
//! Designed for per-tick rebuild: `clear()` + a stream of `insert`
//! calls at the start of each frame, then arbitrarily many
//! `for_each_near` queries during the frame. Fresh state every
//! tick avoids the maintenance cost of moving entries between
//! cells — profiling in a 10k-zergling scenario showed the rebuild
//! is cheap compared to the O(N²) it replaces.
//!
//! Cell sizing: set `cell_size` to roughly `2 × max_hit_radius` so
//! an overlap can only span the entry's own cell plus one ring of
//! neighbors. That caps per-query work at the 3×3 neighborhood
//! regardless of population density. The same grid is reused for
//! separation, chain arcs, gravity wells, and any other radius-
//! keyed query — one build, many queries.
//!
//! Intentionally not a quadtree: for the evenly-spread horde case
//! (enemies chasing a player) a flat grid is faster to build,
//! faster to query, and cache-friendlier. Quadtree wins for wildly
//! clumped distributions (stars in empty space), which isn't this
//! game's workload.

/// Flat grid cell payload: entry indices (typically enemy indices
/// into `Game.enemies`). u32 is plenty for any foreseeable entity
/// count; saves half the cache footprint vs `usize` on 64-bit.
pub type Entry = u32;

pub struct SpatialGrid {
    cell_size: f32,
    /// Number of cells along X / Y. Covers the whole world — any
    /// entry outside the arena is silently dropped by the insert
    /// path so out-of-bounds positions don't crash.
    cols: i32,
    rows: i32,
    /// Row-major: `cells[row * cols + col]`. Each cell stores a
    /// small Vec of entry indices. Arena areas without enemies
    /// cost one empty Vec — if that ever matters, swap to a
    /// `HashMap<(i32,i32), Vec<Entry>>` layout.
    cells: Vec<Vec<Entry>>,
}

impl SpatialGrid {
    pub fn new(arena_w: f32, arena_h: f32, cell_size: f32) -> Self {
        let cols = ((arena_w / cell_size).ceil() as i32).max(1);
        let rows = ((arena_h / cell_size).ceil() as i32).max(1);
        let cells = (0..(cols * rows)).map(|_| Vec::new()).collect();
        Self { cell_size, cols, rows, cells }
    }

    /// Drop every entry without freeing cell Vecs. Next frame's
    /// insert calls reuse the allocated capacity — steady-state
    /// rebuild cost is basically `N * push`.
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.clear();
        }
    }

    /// Insert `idx` at world position `(x, y)`. Out-of-bounds
    /// positions are silently ignored — callers don't need to clamp.
    pub fn insert(&mut self, idx: Entry, x: f32, y: f32) {
        let Some(i) = self.cell_index(x, y) else {
            return;
        };
        self.cells[i].push(idx);
    }

    /// Visit every entry index whose cell is within the 3×3 block
    /// centered on the cell containing `(x, y)`. The caller is
    /// responsible for applying the actual distance/radius check on
    /// each candidate — the grid only narrows the search.
    ///
    /// Why 3×3 (not a proper radius cover): callers size the grid
    /// so their largest query radius fits inside one cell width.
    /// A fixed neighborhood is cheaper than math to compute the
    /// exact cell span from a radius.
    pub fn for_each_near<F: FnMut(Entry)>(&self, x: f32, y: f32, mut f: F) {
        let (cx, cy) = match self.cell_coords(x, y) {
            Some(c) => c,
            None => return,
        };
        let x0 = (cx - 1).max(0);
        let y0 = (cy - 1).max(0);
        let x1 = (cx + 1).min(self.cols - 1);
        let y1 = (cy + 1).min(self.rows - 1);
        for yy in y0..=y1 {
            let row_base = (yy * self.cols) as usize;
            for xx in x0..=x1 {
                for &e in &self.cells[row_base + xx as usize] {
                    f(e);
                }
            }
        }
    }

    /// Visit candidates within a rectangular cell span large enough
    /// to cover `radius`. Use when the query radius exceeds the cell
    /// size (e.g. chain arcs at 12 tiles vs cell_size ≈ 13).
    pub fn for_each_in_radius<F: FnMut(Entry)>(
        &self,
        x: f32,
        y: f32,
        radius: f32,
        mut f: F,
    ) {
        let (cx, cy) = match self.cell_coords(x, y) {
            Some(c) => c,
            None => return,
        };
        let cells = ((radius / self.cell_size).ceil() as i32).max(1);
        let x0 = (cx - cells).max(0);
        let y0 = (cy - cells).max(0);
        let x1 = (cx + cells).min(self.cols - 1);
        let y1 = (cy + cells).min(self.rows - 1);
        for yy in y0..=y1 {
            let row_base = (yy * self.cols) as usize;
            for xx in x0..=x1 {
                for &e in &self.cells[row_base + xx as usize] {
                    f(e);
                }
            }
        }
    }

    fn cell_coords(&self, x: f32, y: f32) -> Option<(i32, i32)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let cx = (x / self.cell_size) as i32;
        let cy = (y / self.cell_size) as i32;
        if cx < 0 || cy < 0 || cx >= self.cols || cy >= self.rows {
            return None;
        }
        Some((cx, cy))
    }

    fn cell_index(&self, x: f32, y: f32) -> Option<usize> {
        let (cx, cy) = self.cell_coords(x, y)?;
        Some((cy * self.cols + cx) as usize)
    }

    #[cfg(test)]
    pub fn debug_cell_count(&self) -> (i32, i32, f32) {
        (self.cols, self.rows, self.cell_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query_finds_nearby_entry() {
        let mut grid = SpatialGrid::new(100.0, 100.0, 10.0);
        grid.insert(42, 25.0, 25.0);
        let mut found = Vec::new();
        grid.for_each_near(27.0, 27.0, |e| found.push(e));
        assert!(found.contains(&42));
    }

    #[test]
    fn query_skips_distant_cells() {
        let mut grid = SpatialGrid::new(100.0, 100.0, 10.0);
        grid.insert(1, 5.0, 5.0);
        grid.insert(2, 95.0, 95.0);
        let mut found = Vec::new();
        grid.for_each_near(5.0, 5.0, |e| found.push(e));
        assert!(found.contains(&1));
        assert!(!found.contains(&2));
    }

    #[test]
    fn out_of_bounds_insert_is_no_op() {
        let mut grid = SpatialGrid::new(100.0, 100.0, 10.0);
        grid.insert(7, -50.0, -50.0);
        grid.insert(8, 500.0, 500.0);
        let mut found = Vec::new();
        grid.for_each_near(0.0, 0.0, |e| found.push(e));
        grid.for_each_near(99.0, 99.0, |e| found.push(e));
        assert!(found.is_empty());
    }

    #[test]
    fn clear_resets() {
        let mut grid = SpatialGrid::new(100.0, 100.0, 10.0);
        grid.insert(1, 25.0, 25.0);
        grid.clear();
        let mut found = Vec::new();
        grid.for_each_near(25.0, 25.0, |e| found.push(e));
        assert!(found.is_empty());
    }

    #[test]
    fn radius_query_spans_multiple_cells() {
        let mut grid = SpatialGrid::new(100.0, 100.0, 10.0);
        grid.insert(1, 10.0, 10.0);
        grid.insert(2, 30.0, 10.0);
        grid.insert(3, 60.0, 10.0);
        let mut found = Vec::new();
        grid.for_each_in_radius(20.0, 10.0, 15.0, |e| found.push(e));
        assert!(found.contains(&1));
        assert!(found.contains(&2));
        assert!(!found.contains(&3));
    }
}
