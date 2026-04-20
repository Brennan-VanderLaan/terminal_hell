//! A* pathfinding on the arena tile grid. 8-connected, Chebyshev
//! heuristic, with a hard expansion cap so a pathological request
//! (goal surrounded by walls, huge open arena) doesn't chew the sim.
//!
//! Passability is `arena.is_passable` — structures block; object
//! corpses are walk-over. Breachers call this once per `recompute_ms`
//! with a hard cap; if the cap is exhausted the partial best-so-far
//! is returned (so the Breacher at least moves toward the player
//! through the nearest-accessible tile).
//!
//! A* here returns a path in *tile-center* world coordinates so
//! callers can feed them directly into movement target logic.

use crate::arena::Arena;
use std::collections::{BinaryHeap, HashMap};

/// Maximum nodes the open set will expand before we bail out. Tuned
/// so a full-arena path (1600×800) stays under 1 ms. Breacher's
/// recompute interval + this cap keep the sim budget bounded.
pub const ASTAR_MAX_EXPANSIONS: usize = 4096;

/// Compute an 8-connected A* path from `start` to `goal`. Both are
/// tile coords. Returns a path INCLUDING the start and goal if
/// reachable, or the best-so-far path if the expansion cap is hit.
/// Returns None only if start == goal or start itself is not
/// walkable.
pub fn astar(arena: &Arena, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    if start == goal {
        return Some(vec![start]);
    }
    // Starting inside a wall shouldn't happen for a live enemy; bail
    // rather than pretending we can path out.
    if arena.is_wall(start.0, start.1) {
        return None;
    }

    let mut open: BinaryHeap<Node> = BinaryHeap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_score: HashMap<(i32, i32), u32> = HashMap::new();
    let mut best_seen: (i32, i32) = start;
    let mut best_h: u32 = chebyshev(start, goal);

    g_score.insert(start, 0);
    open.push(Node {
        pos: start,
        f: best_h,
        g: 0,
    });

    let mut expansions = 0usize;

    while let Some(cur) = open.pop() {
        if cur.pos == goal {
            return Some(reconstruct(&came_from, goal, start));
        }
        expansions += 1;
        if expansions > ASTAR_MAX_EXPANSIONS {
            // Fallback: return the best approximation we've seen so
            // the Breacher at least walks in the right direction.
            return Some(reconstruct(&came_from, best_seen, start));
        }

        let cur_g = cur.g;
        // Skip stale entries (we push duplicates; the heap may have
        // an older worse copy).
        if let Some(&known) = g_score.get(&cur.pos) {
            if cur_g > known {
                continue;
            }
        }
        for &(dx, dy) in &NEIGHBORS {
            let nx = cur.pos.0 + dx;
            let ny = cur.pos.1 + dy;
            if arena.is_wall(nx, ny) {
                continue;
            }
            // Diagonal movement refuses to cut corners through walls —
            // if either cardinal neighbor is blocked, skip the
            // diagonal so agents don't tunnel through 1-tile gaps.
            if dx != 0 && dy != 0
                && (arena.is_wall(cur.pos.0 + dx, cur.pos.1)
                    || arena.is_wall(cur.pos.0, cur.pos.1 + dy))
            {
                continue;
            }
            let step_cost = if dx != 0 && dy != 0 { 14 } else { 10 };
            let tentative_g = cur_g.saturating_add(step_cost);
            let neighbor = (nx, ny);
            let known_g = g_score.get(&neighbor).copied().unwrap_or(u32::MAX);
            if tentative_g < known_g {
                came_from.insert(neighbor, cur.pos);
                g_score.insert(neighbor, tentative_g);
                let h = chebyshev(neighbor, goal);
                let f = tentative_g.saturating_add(h);
                if h < best_h {
                    best_h = h;
                    best_seen = neighbor;
                }
                open.push(Node { pos: neighbor, f, g: tentative_g });
            }
        }
    }
    None
}

fn reconstruct(
    came_from: &HashMap<(i32, i32), (i32, i32)>,
    mut cur: (i32, i32),
    start: (i32, i32),
) -> Vec<(i32, i32)> {
    let mut out = vec![cur];
    while cur != start {
        match came_from.get(&cur) {
            Some(&prev) => {
                out.push(prev);
                cur = prev;
            }
            None => break,
        }
    }
    out.reverse();
    out
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> u32 {
    let dx = (a.0 - b.0).abs() as u32;
    let dy = (a.1 - b.1).abs() as u32;
    // Mirrors the step cost: diagonal = 14, cardinal = 10.
    let diag = dx.min(dy);
    let straight = dx.max(dy) - diag;
    diag * 14 + straight * 10
}

const NEIGHBORS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// BinaryHeap is a max-heap; we want lowest f at the top, so invert
/// Ord manually.
#[derive(Clone, Copy, Eq, PartialEq)]
struct Node {
    pos: (i32, i32),
    f: u32,
    g: u32,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.f.cmp(&self.f).then_with(|| other.g.cmp(&self.g))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
