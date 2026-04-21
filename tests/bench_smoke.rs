//! Bench smoke tests. Runs a small set of fast, headless-safe
//! scenarios and asserts tick time stays under generous thresholds.
//! The purpose is regression detection, not performance validation —
//! if a future change makes the sim 10× slower, CI should fail loud.
//!
//! Thresholds are intentionally loose: CI hardware varies, and we'd
//! rather catch catastrophic regressions than chase false failures.
//! Tune downward as the sim stabilizes. Each scenario here runs
//! headless to avoid needing a TTY on CI runners.

use std::time::Duration;
use terminal_hell::bench::{
    PlayerScript, RenderMode, Scenario, ScenarioSpawn, ScriptedPlayer, SpawnLayout, run_scenario,
};
use terminal_hell::enemy::Archetype;

/// Convenience: center of an arena.
fn c(w: u16, h: u16) -> (f32, f32) {
    (w as f32 * 0.5, h as f32 * 0.5)
}

/// Short smoke-bench — ~2s idle scenario. Exercises the entire
/// runner plumbing without needing any enemies alive.
#[test]
fn bench_smoke_baseline() {
    let arena = (400u16, 200u16);
    let scenario = Scenario {
        name: "smoke_baseline",
        summary: "",
        arena,
        arena_seed: 0xCAFE,
        players: vec![ScriptedPlayer {
            pos: c(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_millis(1500),
        stop_when_clear: false,
        paints: vec![],
    };
    let report = run_scenario(&scenario, RenderMode::Headless).expect("smoke_baseline");
    assert!(report.frames > 10, "expected >10 frames, got {}", report.frames);
    // Idle sim should be tiny — well under 2ms even on a slow runner.
    assert!(
        report.tick_p95_us < 2_000,
        "baseline tick_p95 {}µs exceeded 2ms budget",
        report.tick_p95_us
    );
}

/// Mid-scale sim: 200 rushers from the edges. Tight budget: this is
/// the scenario that should stay fast through refactors. Bumping
/// the threshold should be a deliberate decision, not drift.
#[test]
fn bench_smoke_rusher_200() {
    let arena = (500u16, 400u16);
    let scenario = Scenario {
        name: "smoke_rusher_200",
        summary: "",
        arena,
        arena_seed: 0xBADD,
        players: vec![ScriptedPlayer {
            pos: c(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 200,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_millis(2000),
        stop_when_clear: false,
        paints: vec![],
    };
    let report = run_scenario(&scenario, RenderMode::Headless).expect("smoke_rusher_200");
    assert!(report.peak_enemies >= 200, "expected 200+ enemies, got {}", report.peak_enemies);
    assert!(
        report.tick_p95_us < 5_000,
        "rusher_200 tick_p95 {}µs exceeded 5ms budget — regression?",
        report.tick_p95_us
    );
}

/// Exercises the Ring layout + stationary sentinels. Keeps the
/// archetype-variety surface green.
#[test]
fn bench_smoke_sentinel_ring() {
    let arena = (400u16, 300u16);
    let center = c(arena.0, arena.1);
    let scenario = Scenario {
        name: "smoke_sentinel_ring",
        summary: "",
        arena,
        arena_seed: 0xFE75,
        players: vec![ScriptedPlayer {
            pos: center,
            turret_kits: 0,
            script: PlayerScript::CircleStrafe {
                center,
                radius: 8.0,
                rate: 1.0,
            },
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Sentinel,
            count: 8,
            layout: SpawnLayout::Ring { center, radius: 40.0 },
            at_secs: 0.0,
        }],
        duration: Duration::from_millis(1500),
        stop_when_clear: false,
        paints: vec![],
    };
    let report = run_scenario(&scenario, RenderMode::Headless).expect("smoke_sentinel_ring");
    assert_eq!(report.peak_enemies, 8);
    assert!(report.frames > 10);
}
