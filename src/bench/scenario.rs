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
}

/// A timed spawn batch. At `at_secs` into the scenario, the runner
/// places `count` instances of `archetype` according to `layout`.
pub struct ScenarioSpawn {
    pub archetype: Archetype,
    pub count: u32,
    pub layout: SpawnLayout,
    pub at_secs: f32,
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
