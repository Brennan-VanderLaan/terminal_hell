//! Arena: 3-layer tile stack, in logical-pixel units (1 tile = 1 pixel in
//! the framebuffer).
//!
//! **Ground layer** — floor and flat ground decoration: Floor, Carcosa
//! terrain, Rubble (post-destruction debris), blood pools, scorch marks.
//! Ground decals are cheap, drawn first, and don't block movement.
//!
//! **Object layer** — things sitting *on* the ground: debris piles,
//! corpses (as IDs pointing at entity-level corpse state), dropped loot in
//! the future. Objects render above ground and below structure.
//!
//! **Structure layer** — things that block movement and sightlines: walls,
//! low cover, doorways. Only structures gate collisions — objects are
//! walk-over-able.
//!
//! Breaking a wall destroys only the structure slot; the ground underneath
//! (including any Carcosa terrain or blood pool) persists. This makes
//! building interiors (walls forming rooms, Doorway openings to enter)
//! natural to express in a follow-on pass.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::substance::{SubstanceId, SubstanceRegistry};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// Ground-layer kind. Flat decoration; always passable. Replaced on
/// ground-targeted writes (e.g., a corpse collapsing into blood pool
/// overwrites whatever was underneath).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ground {
    Floor,
    Carcosa,
    Rubble { material: Material },
    /// Data-driven substance decal: `id` indexes the `SubstanceRegistry`
    /// in `ContentDb`, `state` is substance-specific state byte (shade
    /// for blood, intensity for scorch, etc). Replaces the old fixed
    /// `BloodPool` / `Scorch` variants so new substances can land as
    /// pure TOML.
    Substance {
        id: crate::substance::SubstanceId,
        state: u8,
    },
}

/// Object-layer kind. Sits above the ground; never blocks movement in v1
/// (players step over corpses and debris piles). Objects can block
/// projectiles if their variant says so — DebrisPile does, Corpse does
/// after a small hit radius is used during corpse-damage resolution.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    /// Loose debris — pile of broken concrete / bone / whatever. Partial
    /// LoS cover, no movement block.
    DebrisPile { material: Material, remaining: u8 },
    /// Handle to a full corpse entity (state lives on `Game.corpses`).
    /// Kept in the spatial grid so hit resolution is fast.
    Corpse { id: u32 },
}

/// Structure-layer kind. Opaque + blocking unless the variant opts out
/// (Doorway). The only layer that gates collisions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Structure {
    Wall { hp: u8, material: Material },
    /// Low cover — currently same collision as Wall (placeholder for
    /// future crouch mechanics).
    LowCover { hp: u8, material: Material },
    /// Doorway: part of a wall run but does *not* block movement or LoS.
    /// Makes enterable buildings a data-only construct.
    Doorway,
}

impl Structure {
    pub fn blocks_movement(self) -> bool {
        !matches!(self, Structure::Doorway)
    }
    pub fn blocks_los(self) -> bool {
        !matches!(self, Structure::Doorway)
    }
}

/// Full per-tile state.
#[derive(Clone, Copy, Debug)]
pub struct TileStack {
    pub ground: Ground,
    pub object: Option<Object>,
    pub structure: Option<Structure>,
}

impl TileStack {
    pub fn floor() -> Self {
        Self { ground: Ground::Floor, object: None, structure: None }
    }
}

/// Dominant-state view of a tile, preserved for legacy callers and the
/// net TileUpdate protocol. Blood / scorch decals are *not* surfaced
/// through this view — use `Arena::ground` directly for those.
#[derive(Clone, Copy, Debug)]
pub enum Tile {
    Floor,
    Wall { hp: u8, material: Material },
    Rubble { material: Material },
    Carcosa,
}

pub struct HitOutcome {
    pub destroyed: bool,
    pub material: Material,
}

pub struct Arena {
    pub width: u16,
    pub height: u16,
    stacks: Vec<TileStack>,
}

const WALL_HP: u8 = 2;

impl Arena {
    pub fn new_empty(width: u16, height: u16) -> Self {
        let n = (width as usize) * (height as usize);
        Self { width, height, stacks: vec![TileStack::floor(); n] }
    }

    /// Seed-driven procedural arena. Scales blob + breaker counts with
    /// arena area so a 10× world feels populated, not sparse. Layout:
    /// thick perimeter, an open central disc for player spawn + kiosk
    /// placement, and rectangular cover blobs + long sightline breakers
    /// scattered across the annulus.
    pub fn generate(seed: u64, width: u16, height: u16) -> Self {
        let mut a = Self::new_empty(width, height);
        let w = width as i32;
        let h = height as i32;
        let mut rng = SmallRng::seed_from_u64(seed);

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
        let core_radius = (w.min(h * 2) / 6).max(10);
        let core_r2 = core_radius * core_radius;

        let area = w.max(1) * h.max(1);
        let blob_count = (area / 4000).clamp(14, 600);
        let breaker_count = (area / 20000).clamp(3, 120);
        // Building count reduced since each building is 10× the
        // footprint of the old versions — ~1000 tiles avg instead of
        // ~100. Total coverage stays near 5-8% of the arena.
        let building_count = (area / 35000).clamp(4, 24);

        let margin = 5;
        for _ in 0..blob_count {
            let bw = rng.gen_range(3..=10) as i32;
            let bh = rng.gen_range(2..=6) as i32;
            if w <= margin * 2 + bw || h <= margin * 2 + bh {
                continue;
            }
            let x = rng.gen_range(margin..w - margin - bw);
            let y = rng.gen_range(margin..h - margin - bh);
            let dx = x + bw / 2 - cx;
            let dy = y + bh / 2 - cy;
            if dx * dx + dy * dy < core_r2 {
                continue;
            }
            place_block(&mut a, x, y, bw, bh);
        }

        for _ in 0..breaker_count {
            let horizontal = rng.gen_bool(0.5);
            if horizontal {
                let len = rng.gen_range(8..=(w / 6).max(10)) as i32;
                if w <= margin * 2 + len {
                    continue;
                }
                let x = rng.gen_range(margin..w - margin - len);
                let y = rng.gen_range(margin..h - margin - 2);
                let dx = x + len / 2 - cx;
                let dy = y - cy;
                if dx * dx + dy * dy < core_r2 {
                    continue;
                }
                place_block(&mut a, x, y, len, 2);
            } else {
                let len = rng.gen_range(6..=(h / 6).max(8)) as i32;
                if h <= margin * 2 + len {
                    continue;
                }
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

        // Buildings: real-size structures so players can actually
        // hide inside. Player sprite is ~8x11; a building needs to
        // be dozens of tiles across to read as "a building I enter"
        // instead of "a blob I stand next to." Size classes:
        //   small shack: 18-26 × 14-20 (single room, one door)
        //   mid room:    28-44 × 22-34 (one divider)
        //   compound:    55-80 × 40-65 (multi-room with 3+ dividers)
        for _ in 0..building_count {
            let roll: u8 = rng.gen_range(0..100);
            let (bw, bh) = match roll {
                0..=49 => {
                    let bw = rng.gen_range(18..=26) as i32;
                    let bh = rng.gen_range(14..=20) as i32;
                    (bw, bh)
                }
                50..=84 => {
                    let bw = rng.gen_range(28..=44) as i32;
                    let bh = rng.gen_range(22..=34) as i32;
                    (bw, bh)
                }
                _ => {
                    // Compound.
                    let bw = rng.gen_range(55..=80) as i32;
                    let bh = rng.gen_range(40..=65) as i32;
                    (bw, bh)
                }
            };
            if w <= margin * 2 + bw + 2 || h <= margin * 2 + bh + 2 {
                continue;
            }
            let x = rng.gen_range(margin + 1..w - margin - bw - 1);
            let y = rng.gen_range(margin + 1..h - margin - bh - 1);
            let dx = x + bw / 2 - cx;
            let dy = y + bh / 2 - cy;
            if dx * dx + dy * dy < core_r2 {
                continue;
            }
            let mut clear = true;
            for yy in (y + 1)..(y + bh - 1) {
                for xx in (x + 1)..(x + bw - 1) {
                    if a.is_wall(xx, yy) {
                        clear = false;
                        break;
                    }
                }
                if !clear {
                    break;
                }
            }
            if !clear {
                continue;
            }
            // Interior richness scales with size. Mid rooms get 1
            // divider; compounds get 3-5 dividers creating a
            // multi-room layout, plus extra exterior doorways.
            let divider_count = if bw * bh > 2500 {
                rng.gen_range(3..=5) as u8 // compound
            } else if bw * bh > 700 {
                1 // mid
            } else {
                0 // shack
            };
            let extra_doors = if divider_count > 1 { 2 } else { 0 };
            place_building(&mut a, &mut rng, x, y, bw, bh);
            for _ in 0..divider_count {
                add_building_interior(&mut a, &mut rng, x, y, bw, bh);
            }
            for _ in 0..extra_doors {
                add_exterior_doorway(&mut a, &mut rng, x, y, bw, bh);
            }
        }

        a
    }

    /// Hand-crafted layout used before the generator. Kept for tests.
    #[allow(dead_code)]
    pub fn hand_crafted(width: u16, height: u16) -> Self {
        let mut a = Self::new_empty(width, height);
        let w = width as i32;
        let h = height as i32;

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
        let u = (w / 28).max(3);

        place_block(&mut a, cx - 8 * u, cy - 3 * u, 2 * u, 3 * u);
        place_block(&mut a, cx + 6 * u, cy - 3 * u, 2 * u, 3 * u);
        place_block(&mut a, cx - 4 * u, cy - 5 * u, 8 * u, u.min(3));
        place_block(&mut a, cx - 4 * u, cy + 5 * u - u.min(3), 8 * u, u.min(3));
        place_block(&mut a, cx - 5 * u, cy, 3 * u, 2);
        place_block(&mut a, cx + 2 * u, cy, 3 * u, 2);

        a
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some((y as usize) * (self.width as usize) + (x as usize))
    }

    /// Full stack at (x, y). Returns a "floor" stack for OOB coords so
    /// callers don't have to special-case edges.
    #[inline]
    pub fn stack(&self, x: i32, y: i32) -> TileStack {
        match self.idx(x, y) {
            Some(i) => self.stacks[i],
            None => TileStack {
                ground: Ground::Floor,
                object: None,
                // OOB reads as an invincible wall so projectiles don't
                // leave the world.
                structure: Some(Structure::Wall {
                    hp: u8::MAX,
                    material: Material::Concrete,
                }),
            },
        }
    }

    /// Ground-layer decoration at (x, y). Prefer this over `tile()` when
    /// you care about blood / scorch / etc.
    #[inline]
    pub fn ground(&self, x: i32, y: i32) -> Ground {
        self.stack(x, y).ground
    }

    /// Object at (x, y), if any.
    #[inline]
    pub fn object(&self, x: i32, y: i32) -> Option<Object> {
        self.stack(x, y).object
    }

    /// Dominant-state view of (x, y). Used by the net TileUpdate encoder
    /// and by any legacy caller that just wants to know "is this a wall,
    /// a floor, or debris." Substance decals fall through to whatever
    /// structural state is present (or Floor if none).
    #[inline]
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        let s = self.stack(x, y);
        if let Some(Structure::Wall { hp, material }) = s.structure {
            return Tile::Wall { hp, material };
        }
        if let Some(Structure::LowCover { hp, material }) = s.structure {
            return Tile::Wall { hp, material };
        }
        match s.ground {
            Ground::Floor => Tile::Floor,
            Ground::Carcosa => Tile::Carcosa,
            Ground::Rubble { material } => Tile::Rubble { material },
            // Substance decals don't change dominant state — legacy
            // callers keep seeing Floor. Query `ground()` for detail.
            Ground::Substance { .. } => Tile::Floor,
        }
    }

    pub fn is_wall(&self, x: i32, y: i32) -> bool {
        matches!(
            self.stack(x, y).structure,
            Some(Structure::Wall { .. }) | Some(Structure::LowCover { .. })
        )
    }

    /// Does the structure at (x, y) block line of sight? True for walls,
    /// low cover, any non-Doorway structure. False for Doorways and
    /// open tiles.
    pub fn blocks_los(&self, x: i32, y: i32) -> bool {
        let s = self.stack(x, y);
        if let Some(st) = s.structure {
            if st.blocks_los() {
                return true;
            }
        }
        // Object-layer debris piles also block LoS — this is what
        // makes flying-debris chunks act as actual cover. Corpses
        // still don't (they're too short + flat).
        matches!(s.object, Some(Object::DebrisPile { .. }))
    }

    /// Amanatides–Woo grid traversal from `from` to `to` in world-pixel
    /// space. Walks every tile the segment passes through in order and
    /// returns the first tile whose `blocking(x, y)` returns true, with
    /// the parametric hit point along the segment. O(|dx| + |dy|) tile
    /// checks; no allocation.
    ///
    /// Fast enough for every projectile tick, every sniper/laser shot,
    /// every AI LoS query. Prefer this over hand-rolled step loops.
    pub fn raycast<F>(&self, from: (f32, f32), to: (f32, f32), mut blocking: F) -> Option<RayHit>
    where
        F: FnMut(&Arena, i32, i32) -> bool,
    {
        let dx = to.0 - from.0;
        let dy = to.1 - from.1;
        // Degenerate (no movement) — treat as a point test at `from`.
        if dx == 0.0 && dy == 0.0 {
            let ix = from.0.floor() as i32;
            let iy = from.1.floor() as i32;
            if blocking(self, ix, iy) {
                return Some(RayHit { point: from, tile: (ix, iy), t: 0.0, normal: (0, 0) });
            }
            return None;
        }

        let mut ix = from.0.floor() as i32;
        let mut iy = from.1.floor() as i32;

        // Check the starting cell first — if we spawn inside a blocker,
        // report an immediate hit so callers can snap out.
        if blocking(self, ix, iy) {
            return Some(RayHit { point: from, tile: (ix, iy), t: 0.0, normal: (0, 0) });
        }

        let step_x: i32 = if dx > 0.0 {
            1
        } else if dx < 0.0 {
            -1
        } else {
            0
        };
        let step_y: i32 = if dy > 0.0 {
            1
        } else if dy < 0.0 {
            -1
        } else {
            0
        };

        // Parametric distance along the segment to the next tile
        // boundary on each axis, and the parametric delta for crossing
        // one full tile along that axis.
        let inf = f32::INFINITY;
        let (mut t_max_x, t_delta_x) = if dx != 0.0 {
            let next_bx = if step_x > 0 { (ix + 1) as f32 } else { ix as f32 };
            ((next_bx - from.0) / dx, (1.0 / dx.abs()))
        } else {
            (inf, inf)
        };
        let (mut t_max_y, t_delta_y) = if dy != 0.0 {
            let next_by = if step_y > 0 { (iy + 1) as f32 } else { iy as f32 };
            ((next_by - from.1) / dy, (1.0 / dy.abs()))
        } else {
            (inf, inf)
        };

        // Hard cap in case of pathological input.
        let max_iters = (dx.abs() + dy.abs()).ceil() as i32 + 4;
        for _ in 0..max_iters {
            // Advance to whichever axis crosses its next boundary first.
            let (t, crossed_x) = if t_max_x < t_max_y {
                (t_max_x, true)
            } else {
                (t_max_y, false)
            };
            if t > 1.0 {
                return None;
            }
            if crossed_x {
                ix += step_x;
                t_max_x += t_delta_x;
            } else {
                iy += step_y;
                t_max_y += t_delta_y;
            }
            if blocking(self, ix, iy) {
                let px = from.0 + dx * t;
                let py = from.1 + dy * t;
                let normal = if crossed_x { (-step_x as i8, 0i8) } else { (0i8, -step_y as i8) };
                return Some(RayHit { point: (px, py), tile: (ix, iy), t, normal });
            }
        }
        None
    }

    /// Convenience: first wall-ish blocker along the segment.
    pub fn raycast_wall(&self, from: (f32, f32), to: (f32, f32)) -> Option<RayHit> {
        self.raycast(from, to, |a, x, y| a.is_wall(x, y))
    }

    /// Convenience: first LoS blocker along the segment. Used by AI
    /// ranged-attack LoS checks and any future sight logic.
    pub fn raycast_los(&self, from: (f32, f32), to: (f32, f32)) -> Option<RayHit> {
        self.raycast(from, to, |a, x, y| a.blocks_los(x, y))
    }

    /// True if no LoS blocker stands between `from` and `to`.
    pub fn has_line_of_sight(&self, from: (f32, f32), to: (f32, f32)) -> bool {
        self.raycast_los(from, to).is_none()
    }

    /// Does (x, y) stop a bullet? Like `blocks_los`, but broken
    /// cover (rubble piles) lets projectiles through. Matches the
    /// "I can walk through the rubble, I should be able to shoot
    /// through it too" mental model — debris piles still block
    /// *sight* (enemies behind them can't spot you), but once you
    /// spot them a round punches through the pile fine.
    pub fn blocks_projectile(&self, x: i32, y: i32) -> bool {
        let s = self.stack(x, y);
        if let Some(st) = s.structure {
            if st.blocks_los() {
                return true;
            }
        }
        // DebrisPile intentionally NOT included here (unlike
        // `blocks_los`) — rubble is walk-through-AND-shoot-through.
        false
    }

    /// Convenience: first projectile-blocker along the segment.
    /// Used by the projectile raycast — structural walls block
    /// rounds; loose rubble does not.
    pub fn raycast_projectile(&self, from: (f32, f32), to: (f32, f32)) -> Option<RayHit> {
        self.raycast(from, to, |a, x, y| a.blocks_projectile(x, y))
    }

    /// True if a player or projectile can move/pass through. Movement and
    /// LoS share this test today (structure walls block both; objects
    /// block neither).
    pub fn is_passable(&self, x: i32, y: i32) -> bool {
        match self.stack(x, y).structure {
            Some(s) => !s.blocks_movement(),
            None => true,
        }
    }

    pub fn is_carcosa(&self, x: i32, y: i32) -> bool {
        matches!(self.ground(x, y), Ground::Carcosa)
    }

    pub fn set_carcosa(&mut self, x: i32, y: i32) {
        if let Some(i) = self.idx(x, y) {
            self.stacks[i].structure = None;
            self.stacks[i].ground = Ground::Carcosa;
        }
    }

    pub fn set_wall(&mut self, x: i32, y: i32, material: Material, hp: u8) {
        if let Some(i) = self.idx(x, y) {
            self.stacks[i].structure = Some(Structure::Wall { hp, material });
        }
    }

    pub fn set_rubble(&mut self, x: i32, y: i32, material: Material) {
        if let Some(i) = self.idx(x, y) {
            self.stacks[i].structure = None;
            self.stacks[i].ground = Ground::Rubble { material };
        }
    }

    pub fn set_floor(&mut self, x: i32, y: i32) {
        if let Some(i) = self.idx(x, y) {
            self.stacks[i] = TileStack::floor();
        }
    }

    /// Paint a substance onto the ground layer, preserving whatever
    /// structure / object sits above. If the same substance is already
    /// there, the new state is combined via saturating add (so blood +
    /// blood pools into a bigger stain; scorch + scorch deepens). A
    /// different substance overwrites.
    pub fn set_substance(&mut self, x: i32, y: i32, id: SubstanceId, state: u8) {
        if let Some(i) = self.idx(x, y) {
            let cur = self.stacks[i].ground;
            let new_state = match cur {
                Ground::Substance { id: cur_id, state: cur_state } if cur_id == id => {
                    cur_state.saturating_add(state).min(255)
                }
                _ => state,
            };
            self.stacks[i].ground = Ground::Substance { id, state: new_state };
        }
    }

    /// Clear any substance from the ground layer, reverting to Floor.
    /// Structural state stays put.
    pub fn clear_substance(&mut self, x: i32, y: i32) {
        if let Some(i) = self.idx(x, y) {
            if matches!(self.stacks[i].ground, Ground::Substance { .. }) {
                self.stacks[i].ground = Ground::Floor;
            }
        }
    }

    /// Place or replace the object at (x, y).
    pub fn set_object(&mut self, x: i32, y: i32, obj: Object) {
        if let Some(i) = self.idx(x, y) {
            self.stacks[i].object = Some(obj);
        }
    }

    pub fn clear_object(&mut self, x: i32, y: i32) {
        if let Some(i) = self.idx(x, y) {
            self.stacks[i].object = None;
        }
    }

    /// Width × height accessor helpers — used by `diff_from_seed` callers
    /// that want to walk the grid from outside the crate.
    pub fn stacks_len(&self) -> usize {
        self.stacks.len()
    }

    /// Walk this arena against a freshly-regenerated reference arena
    /// (same seed + dimensions). Returns every tile that has diverged:
    /// structure deltas (wall damage / carcosa / rubble) and ground
    /// deltas (blood pools, scorch). Used by the host to catch a
    /// late-joining client up to the accumulated arena state so they
    /// walk into the chaos, not a pristine map.
    pub fn diff_from_seed(
        &self,
        seed: u64,
    ) -> (Vec<StructureDelta>, Vec<GroundDelta>) {
        let pristine = Arena::generate(seed, self.width, self.height);
        let mut structure_deltas = Vec::new();
        let mut ground_deltas = Vec::new();
        let w = self.width as i32;
        let h = self.height as i32;
        for y in 0..h {
            for x in 0..w {
                let i = (y as usize) * (self.width as usize) + (x as usize);
                let a = self.stacks[i];
                let b = pristine.stacks[i];
                if structure_differs(a.structure, b.structure) {
                    let (kind, hp) = encode_structure(a);
                    structure_deltas.push(StructureDelta {
                        x: x as u16,
                        y: y as u16,
                        kind,
                        hp,
                    });
                }
                if ground_differs(a.ground, b.ground) {
                    if let Ground::Substance { id, state } = a.ground {
                        ground_deltas.push(GroundDelta {
                            x: x as u16,
                            y: y as u16,
                            substance_id: id.0,
                            state,
                        });
                    }
                }
            }
        }
        (structure_deltas, ground_deltas)
    }

    /// Raw tile iteration for initial-snapshot serialization.
    /// Encodes the dominant-state Tile; ground decals (blood/scorch) and
    /// objects are NOT covered here — they sync via dedicated
    /// `GroundPaint` / `ObjectUpdate` messages in the decal + corpse
    /// pushes.
    pub fn encode_tiles(&self) -> Vec<u8> {
        // 2 bytes per tile: (kind, hp). kind: 0=Floor, 1=Wall, 2=Rubble, 3=Carcosa.
        let mut out = Vec::with_capacity(self.stacks.len() * 2);
        for s in &self.stacks {
            match (s.structure, s.ground) {
                (Some(Structure::Wall { hp, .. }), _)
                | (Some(Structure::LowCover { hp, .. }), _) => {
                    out.push(1);
                    out.push(hp);
                }
                (None, Ground::Rubble { .. }) => {
                    out.push(2);
                    out.push(0);
                }
                (None, Ground::Carcosa) => {
                    out.push(3);
                    out.push(0);
                }
                _ => {
                    // Floor / blood / scorch all encode as Floor for
                    // dominant-state sync; decals land via GroundPaint.
                    out.push(0);
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
        let mut stacks = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let kind = chunk[0];
            let hp = chunk[1];
            let stack = match kind {
                0 => TileStack::floor(),
                1 => TileStack {
                    ground: Ground::Floor,
                    object: None,
                    structure: Some(Structure::Wall { hp, material: Material::Concrete }),
                },
                2 => TileStack {
                    ground: Ground::Rubble { material: Material::Concrete },
                    object: None,
                    structure: None,
                },
                _ => TileStack {
                    ground: Ground::Carcosa,
                    object: None,
                    structure: None,
                },
            };
            stacks.push(stack);
        }
        Some(Self { width, height, stacks })
    }

    /// Apply 1 point of structural damage to a wall tile. Returns None if
    /// there was no wall there. On destroy, the underlying ground becomes
    /// Rubble unless Carcosa / blood / scorch was already there (those
    /// persist — a wall smashed on carcosa still reveals carcosa).
    pub fn damage_wall(&mut self, x: i32, y: i32) -> Option<HitOutcome> {
        let i = self.idx(x, y)?;
        let stack = self.stacks[i];
        match stack.structure {
            Some(Structure::Wall { hp, material }) => {
                if hp <= 1 {
                    self.stacks[i].structure = None;
                    // Keep existing ground decals (Carcosa / blood / scorch);
                    // otherwise drop a Rubble ground.
                    if matches!(stack.ground, Ground::Floor) {
                        self.stacks[i].ground = Ground::Rubble { material };
                    }
                    Some(HitOutcome { destroyed: true, material })
                } else {
                    self.stacks[i].structure =
                        Some(Structure::Wall { hp: hp - 1, material });
                    Some(HitOutcome { destroyed: false, material })
                }
            }
            _ => None,
        }
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera, subs: &SubstanceRegistry) {
        let (wx0, wy0, wx1, wy1) = camera.world_viewport();
        let ix0 = (wx0.floor() as i32).max(0);
        let iy0 = (wy0.floor() as i32).max(0);
        let ix1 = (wx1.ceil() as i32).min(self.width as i32);
        let iy1 = (wy1.ceil() as i32).min(self.height as i32);

        for wy in iy0..iy1 {
            for wx in ix0..ix1 {
                let stack = self.stack(wx, wy);

                // Layer 1: ground. Floor is intentionally left black
                // (so the braille layer can show through).
                let ground_color = ground_color_at(wx, wy, stack.ground, subs);
                if let Some(c) = ground_color {
                    fill_world_tile(fb, camera, wx, wy, c);
                }

                // Substance bloom: glowing decals bleed additive light
                // onto nearby pixels via the framebuffer bloom seeds.
                if let Ground::Substance { id, state } = stack.ground {
                    if let Some(def) = subs.get(id) {
                        if let Some(bloom) = def.bloom {
                            let (sx, sy) =
                                camera.world_to_screen((wx as f32 + 0.5, wy as f32 + 0.5));
                            let t = match def.state_curve {
                                crate::substance::StateCurve::FreshToDried => {
                                    1.0 - (state as f32 / 255.0)
                                }
                                crate::substance::StateCurve::FaintToIntense => {
                                    state as f32 / 255.0
                                }
                            };
                            let strength = bloom.strength * t.max(0.15);
                            fb.bloom_seed(sx, sy, bloom.color, bloom.radius, strength);
                        }
                    }
                }

                // Layer 2: object. Currently no objects are populated —
                // corpses and debris piles land in follow-on pushes.
                if let Some(obj) = stack.object {
                    if let Some(c) = object_color(obj) {
                        fill_world_tile(fb, camera, wx, wy, c);
                    }
                }

                // Layer 3: structure. Walls overwrite the other layers so
                // they read as solid silhouettes.
                if let Some(s) = stack.structure {
                    if let Some(c) = structure_color(s) {
                        fill_world_tile(fb, camera, wx, wy, c);
                    }
                }
            }
        }
    }
}

fn ground_color_at(wx: i32, wy: i32, g: Ground, subs: &SubstanceRegistry) -> Option<Pixel> {
    match g {
        Ground::Floor => None,
        Ground::Rubble { material } => Some(material.debris_color()),
        Ground::Carcosa => {
            let pattern = ((wx ^ wy) & 1) == 0;
            Some(if pattern {
                Pixel::rgb(180, 140, 20)
            } else {
                Pixel::rgb(110, 80, 10)
            })
        }
        Ground::Substance { id, state } => {
            let def = subs.get(id)?;
            let base = def.state_curve.resolve(state, def.fresh, def.dried);
            if def.jitter > 0 {
                let j = ((wx.wrapping_mul(37) ^ wy.wrapping_mul(91)) & 1) == 0;
                Some(if j {
                    base
                } else {
                    Pixel::rgb(
                        base.r.saturating_sub(def.jitter.saturating_mul(10)),
                        base.g,
                        base.b,
                    )
                })
            } else {
                Some(base)
            }
        }
    }
}

fn object_color(obj: Object) -> Option<Pixel> {
    match obj {
        Object::DebrisPile { material, .. } => Some(material.debris_color()),
        // Corpses render via their own Corpse::render pass (not via the
        // arena fill) so the per-corpse mutable sprite bitmap can draw.
        // The arena just carries the handle.
        Object::Corpse { .. } => None,
    }
}

fn structure_color(s: Structure) -> Option<Pixel> {
    match s {
        Structure::Wall { hp, material } | Structure::LowCover { hp, material } => {
            let base = material.intact_color();
            let ratio = (hp as f32 / WALL_HP as f32).clamp(0.35, 1.0);
            Some(Pixel::rgb(
                (base.r as f32 * ratio) as u8,
                (base.g as f32 * ratio) as u8,
                (base.b as f32 * ratio) as u8,
            ))
        }
        Structure::Doorway => None,
    }
}

fn fill_world_tile(fb: &mut Framebuffer, camera: &Camera, wx: i32, wy: i32, color: Pixel) {
    let (sx0, sy0) = camera.world_to_screen((wx as f32, wy as f32));
    let (sx1, sy1) = camera.world_to_screen(((wx + 1) as f32, (wy + 1) as f32));
    let x0 = (sx0.floor() as i32).max(0);
    let y0 = (sy0.floor() as i32).max(0);
    let x1 = (sx1.ceil() as i32).min(camera.viewport_w as i32);
    let y1 = (sy1.ceil() as i32).min(camera.viewport_h as i32);
    for py in y0..y1 {
        for px in x0..x1 {
            fb.set(px as u16, py as u16, color);
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

/// Add an interior divider wall + doorway. For bigger buildings the
/// divider lands at a random fraction across the structure (not just
/// the middle) so multiple dividers chain into irregular room
/// layouts, not a symmetric cross.
fn add_building_interior(a: &mut Arena, rng: &mut SmallRng, x: i32, y: i32, bw: i32, bh: i32) {
    let horizontal = rng.gen_bool(0.5);
    if horizontal {
        // Divider at 25-75% of height.
        let frac: f32 = rng.gen_range(0.25..0.75);
        let sy = y + (bh as f32 * frac) as i32;
        if sy <= y + 2 || sy >= y + bh - 2 {
            return;
        }
        for xx in (x + 1)..(x + bw - 1) {
            // Only overwrite if not already a doorway/wall (chain
            // dividers may cross other dividers).
            if !a.is_wall(xx, sy) {
                a.set_wall(xx, sy, Material::Concrete, WALL_HP);
            }
        }
        // 2-3 tile doorway in a random spot.
        let door_w = rng.gen_range(2..=3);
        let door_x = rng.gen_range((x + 2)..(x + bw - door_w - 2));
        for xx in door_x..door_x + door_w {
            if let Some(i) = a.idx(xx, sy) {
                a.stacks[i].structure = Some(Structure::Doorway);
            }
        }
    } else {
        let frac: f32 = rng.gen_range(0.25..0.75);
        let sx = x + (bw as f32 * frac) as i32;
        if sx <= x + 2 || sx >= x + bw - 2 {
            return;
        }
        for yy in (y + 1)..(y + bh - 1) {
            if !a.is_wall(sx, yy) {
                a.set_wall(sx, yy, Material::Concrete, WALL_HP);
            }
        }
        let door_h = rng.gen_range(2..=3);
        let door_y = rng.gen_range((y + 2)..(y + bh - door_h - 2));
        for yy in door_y..door_y + door_h {
            if let Some(i) = a.idx(sx, yy) {
                a.stacks[i].structure = Some(Structure::Doorway);
            }
        }
    }
}

/// Drop an extra 2-tile doorway on a random exterior wall — used by
/// big compounds so they have multiple approach vectors instead of
/// one chokepoint.
fn add_exterior_doorway(a: &mut Arena, rng: &mut SmallRng, x: i32, y: i32, bw: i32, bh: i32) {
    let side = rng.gen_range(0..4u8);
    let (dx, dy, run_axis_x) = match side {
        0 => (rng.gen_range(x + 2..x + bw - 3), y, true),
        1 => (rng.gen_range(x + 2..x + bw - 3), y + bh - 1, true),
        2 => (x, rng.gen_range(y + 2..y + bh - 3), false),
        _ => (x + bw - 1, rng.gen_range(y + 2..y + bh - 3), false),
    };
    for step in 0..2 {
        let (ex, ey) = if run_axis_x { (dx + step, dy) } else { (dx, dy + step) };
        if let Some(i) = a.idx(ex, ey) {
            a.stacks[i].structure = Some(Structure::Doorway);
        }
    }
}

/// Build a rectangular room with a doorway gap on one of the four
/// walls. The interior is left as Floor so players can walk inside;
/// the doorway tile is marked as `Structure::Doorway` (walkable,
/// non-blocking) so movement + LoS pass through cleanly.
fn place_building(a: &mut Arena, rng: &mut SmallRng, x: i32, y: i32, w: i32, h: i32) {
    // Perimeter walls.
    for xx in x..(x + w) {
        a.set_wall(xx, y, Material::Concrete, WALL_HP);
        a.set_wall(xx, y + h - 1, Material::Concrete, WALL_HP);
    }
    for yy in y..(y + h) {
        a.set_wall(x, yy, Material::Concrete, WALL_HP);
        a.set_wall(x + w - 1, yy, Material::Concrete, WALL_HP);
    }

    // Doorway on a random edge, 2 tiles wide.
    let side = rng.gen_range(0..4u8);
    let (dx, dy) = match side {
        0 => {
            let px = rng.gen_range((x + 2)..(x + w - 2));
            (px, y)
        }
        1 => {
            let px = rng.gen_range((x + 2)..(x + w - 2));
            (px, y + h - 1)
        }
        2 => {
            let py = rng.gen_range((y + 2)..(y + h - 2));
            (x, py)
        }
        _ => {
            let py = rng.gen_range((y + 2)..(y + h - 2));
            (x + w - 1, py)
        }
    };
    // 2-tile doorway.
    if let Some(i) = a.idx(dx, dy) {
        a.stacks[i].structure = Some(Structure::Doorway);
    }
    let (dx2, dy2) = match side {
        0 | 1 => (dx + 1, dy),
        _ => (dx, dy + 1),
    };
    if let Some(i) = a.idx(dx2, dy2) {
        a.stacks[i].structure = Some(Structure::Doorway);
    }
}

/// Result of a ray-cast along a world-space segment. `point` is the
/// contact position (entry into the first blocking tile), `tile` is the
/// blocker's integer coordinates, `t` is the parametric 0..=1 along the
/// segment, and `normal` is the tile-face normal (-1/0/1 per axis) used
/// by callers that want to reflect off the surface.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub point: (f32, f32),
    pub tile: (i32, i32),
    pub t: f32,
    pub normal: (i8, i8),
}

/// One tile's worth of structure delta, mirroring the TileUpdate wire
/// format. `kind`: 0=Floor, 1=Wall (hp in `hp`), 2=Rubble, 3=Carcosa.
#[derive(Clone, Copy, Debug)]
pub struct StructureDelta {
    pub x: u16,
    pub y: u16,
    pub kind: u8,
    pub hp: u8,
}

/// One tile's worth of ground-substance delta. `substance_id` indexes
/// the substance registry; `state` is the substance's state byte.
#[derive(Clone, Copy, Debug)]
pub struct GroundDelta {
    pub x: u16,
    pub y: u16,
    pub substance_id: u16,
    pub state: u8,
}

fn structure_differs(a: Option<Structure>, b: Option<Structure>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(x), Some(y)) => x != y,
        _ => true,
    }
}

fn ground_differs(a: Ground, b: Ground) -> bool {
    a != b
}

fn encode_structure(stack: TileStack) -> (u8, u8) {
    match stack.structure {
        Some(Structure::Wall { hp, .. }) | Some(Structure::LowCover { hp, .. }) => (1, hp),
        Some(Structure::Doorway) => (0, 0),
        None => match stack.ground {
            Ground::Rubble { .. } => (2, 0),
            Ground::Carcosa => (3, 0),
            _ => (0, 0),
        },
    }
}

