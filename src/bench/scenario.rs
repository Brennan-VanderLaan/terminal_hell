//! Declarative scenario data. A `Scenario` is immutable config;
//! execution lives in `runner.rs`.

use crate::enemy::Archetype;
use crate::bench::scripts::PlayerScript;
use std::time::Duration;

/// One benchmark test case. Runtime: fresh `Game` → place players
/// → apply spawns (possibly time-gated) → tick for `duration` →
/// collect a `ScenarioReport` → tear down.
pub struct Scenario {
    /// Lookup key for `--scenario NAME` on the CLI. Also printed in
    /// the report header.
    pub name: &'static str,
    /// One-line description for the catalogue listing.
    pub summary: &'static str,
    /// Arena size in world units. Smaller arenas stress spatial
    /// queries differently than the full 1600×800 playfield.
    pub arena: (u16, u16),
    /// Seed for `Arena::generate`. Fixed per scenario so runs are
    /// reproducible — use the same seed across commits to diff
    /// telemetry meaningfully.
    pub arena_seed: u64,
    /// Scripted players. Most scenarios use one; co-op stress
    /// scenarios can use several.
    pub players: Vec<ScriptedPlayer>,
    /// Queued spawns. Each entry fires once at its `at_secs` mark.
    /// Order in the Vec is irrelevant — runner schedules by time.
    pub spawns: Vec<ScenarioSpawn>,
    /// Wall-clock duration the scenario runs before teardown.
    pub duration: Duration,
    /// Optional early-exit: if all enemies die before `duration`,
    /// end the scenario and report. Useful for "how fast does this
    /// loadout clear N rushers" tests.
    pub stop_when_clear: bool,
    /// One-shot substance paints applied at scenario start. Acid
    /// lakes, oil slicks, fire sheets — exercises the ambient
    /// emitter + standing-hazard + fire-propagation passes.
    pub paints: Vec<ScenarioSubstancePaint>,
}

/// A scripted survivor at scenario start. The runner installs a
/// real `Player` at `pos` and drives its `PlayerInput` every tick
/// via `script`.
pub struct ScriptedPlayer {
    pub pos: (f32, f32),
    /// Pre-granted turret kits for scenarios that need the player
    /// to deploy defenses. `HoldWithTurrets` scripts will consume
    /// these on their deploy cadence.
    pub turret_kits: u8,
    pub script: PlayerScript,
    /// Optional HP override applied at scenario start. None keeps
    /// the player at PLAYER_MAX_HP (100). Spectacle scenarios bump
    /// this to several thousand so the player survives the full
    /// runtime and the operator can watch the whole show instead
    /// of getting cut off when the swarm closes.
    pub hp_override: Option<i32>,
}

/// A timed spawn batch. At `at_secs` into the scenario, the runner
/// places `count` instances of `archetype` according to `layout`.
pub struct ScenarioSpawn {
    pub archetype: Archetype,
    pub count: u32,
    pub layout: SpawnLayout,
    pub at_secs: f32,
}

/// A one-shot substance paint applied at scenario start, before the
/// sim begins ticking. Used by the substance-world bench scenarios
/// to set up acid lakes, oil slicks, flammable trails, etc. — the
/// runtime's usual paint paths (enemy-death reactions, Ignite hits,
/// acid primitives) wouldn't reproduce these layouts deterministically.
pub struct ScenarioSubstancePaint {
    /// Substance content-id ("fire", "acid_pool", "oil_pool", etc.)
    pub substance: &'static str,
    /// Initial state 0..=255. 0 = fresh; 255 = fully dried/expired.
    /// Fire substances should start at 0 so the scenario gets the
    /// full decay arc.
    pub state: u8,
    /// Spatial layout for the paint.
    pub layout: SubstanceLayout,
    /// Wall-clock time (scenario-relative) at which the paint
    /// applies. 0.0 means "at scenario start"; positive values
    /// stage the paint mid-run so you can choreograph cascading
    /// chaos (oil slick at t=0, ignition seed at t=3, second
    /// ignition at t=6, etc).
    pub at_secs: f32,
}

/// Spatial shapes for scenario substance painting.
pub enum SubstanceLayout {
    /// Single tile.
    Point(f32, f32),
    /// Axis-aligned rectangle filled with the substance. Half-extents
    /// in world tiles; the center tile is included.
    Rect {
        center: (f32, f32),
        half_w: f32,
        half_h: f32,
    },
    /// Filled disk of radius `radius` around `center`.
    Disk {
        center: (f32, f32),
        radius: f32,
    },
    /// Tile-thick line segment from `from` to `to`, stamped by
    /// sampling N evenly-spaced points along the segment.
    Line {
        from: (f32, f32),
        to: (f32, f32),
    },
    /// Scatter `count` tiles uniformly inside `rect`. Exercises the
    /// ambient emitter pass against sparse-tile distributions the
    /// camera viewport has to sample over.
    Scatter {
        center: (f32, f32),
        half_w: f32,
        half_h: f32,
        count: u32,
        seed: u64,
    },
}

/// How a spawn batch places its enemies spatially.
pub enum SpawnLayout {
    /// All at one spot — useful for "how does the separation pass
    /// resolve a 500-enemy stack?" stress tests.
    Point(f32, f32),
    /// Evenly distributed around a circle.
    Ring { center: (f32, f32), radius: f32 },
    /// Grid pattern centered on `center`, `spacing` units apart.
    Grid { center: (f32, f32), spacing: f32 },
    /// Scattered near the arena edges, biased into the outer band
    /// just like the wave director does. Matches real gameplay spawn
    /// flavor.
    Edges,
}
