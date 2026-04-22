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
    PlayerScript, RenderMode, Scenario, ScenarioReport, ScenarioSpawn, ScriptedPlayer,
    SpawnLayout, run_scenario,
};
use terminal_hell::enemy::Archetype;

/// Look up the living count of an archetype in the final census.
fn count_living(report: &ScenarioReport, name: &str) -> u32 {
    report
        .final_archetype_census
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, c)| *c)
        .unwrap_or(0)
}

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

// ===== B4.5 end-to-end dispatch verification =====

/// Headcrab leap-attach + phased Convert + Swap terminal, driven
/// end-to-end by the real sim. Spawns 25 Headcrabs next to 30
/// Rushers, runs 15 seconds (2.5s mounted + 3.5s deteriorating phases
/// = 6s per full gestation, so 15s allows multiple cohorts to
/// complete). Asserts:
///  - at least one Zombie exists at scenario end (conversion fired)
///  - at least one Rusher was converted (Rushers < initial 30)
///  - Headcrab count at end is less than initial (infectors died)
///
/// This is the load-bearing test that proves the generic Convert
/// dispatch, phase state machine, and Swap-mode terminal all work
/// together from a Headcrab spawning through to a Zombie standing
/// in a Rusher's grave.
#[test]
fn bench_headcrab_swarm_converts_rushers_to_zombies() {
    let arena = (500u16, 400u16);
    let center = c(arena.0, arena.1);
    let scenario = Scenario {
        name: "smoke_headcrab_swarm",
        summary: "",
        arena,
        arena_seed: 0x4EAD_C0AB,
        players: vec![ScriptedPlayer {
            pos: (center.0 - 200.0, center.1 - 150.0),
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: Some(5000),
        }],
        spawns: vec![
            ScenarioSpawn {
                archetype: Archetype::Rusher,
                count: 30,
                layout: SpawnLayout::Grid { center, spacing: 3.5 },
                at_secs: 0.0,
            },
            ScenarioSpawn {
                archetype: Archetype::Headcrab,
                count: 25,
                layout: SpawnLayout::Ring { center, radius: 14.0 },
                at_secs: 0.5,
            },
        ],
        duration: Duration::from_secs(15),
        stop_when_clear: false,
        paints: vec![],
    };
    let report = run_scenario(&scenario, RenderMode::Headless)
        .expect("smoke_headcrab_swarm should run clean");

    let zombies = count_living(&report, "zombie");
    let rushers = count_living(&report, "rusher");
    let headcrabs = count_living(&report, "headcrab");

    assert!(
        zombies > 0,
        "expected Zombies to exist after Headcrab gestation completes; census: {:?}",
        report.final_archetype_census,
    );
    assert!(
        rushers < 30,
        "expected some Rushers to have been converted; saw {rushers} remaining"
    );
    assert!(
        headcrabs < 25,
        "expected some Headcrabs to have died after landing conversions; saw {headcrabs} remaining"
    );
}

/// Rat Consume-growth chain, driven end-to-end. Rats bite Rushers
/// down with their Melee fallback, Rushers leave corpses, Rats
/// target and consume those corpses via the generic Consume pass,
/// and once a Rat's `consumed_hp` hits the 30 HP threshold it
/// self-swaps into a DireRat. Asserts:
///  - at least one DireRat exists at scenario end.
///
/// If this fails, the Consume growth chain (scan corpse → credit
/// hp_gain → cross threshold → apply_convert_swap) is broken
/// somewhere.
#[test]
fn bench_rat_feast_grows_into_dire_rats() {
    let arena = (400u16, 300u16);
    let center = c(arena.0, arena.1);
    // With enemy-on-enemy contact damage live, Rats bite Rushers to
    // death on their own. The player is parked out of combat purely
    // so the scenario measures the Rat↔Rusher interaction, not
    // player-fire-produced corpses.
    let scenario = Scenario {
        name: "smoke_rat_feast",
        summary: "",
        arena,
        arena_seed: 0x4A_7FEE_57,
        players: vec![ScriptedPlayer {
            pos: (center.0 - 150.0, center.1 - 120.0),
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: Some(5000),
        }],
        spawns: vec![
            ScenarioSpawn {
                archetype: Archetype::Rusher,
                count: 40,
                layout: SpawnLayout::Grid { center, spacing: 3.0 },
                at_secs: 0.0,
            },
            ScenarioSpawn {
                archetype: Archetype::Rat,
                count: 30,
                layout: SpawnLayout::Ring {
                    center: (center.0, center.1 + 60.0),
                    radius: 6.0,
                },
                at_secs: 0.5,
            },
        ],
        duration: Duration::from_secs(25),
        stop_when_clear: false,
        paints: vec![],
    };
    let report = run_scenario(&scenario, RenderMode::Headless)
        .expect("smoke_rat_feast should run clean");

    let dire_rats = count_living(&report, "dire_rat");
    assert!(
        dire_rats > 0,
        "expected DireRats to emerge from Rat Consume-growth; census: {:?}",
        report.final_archetype_census,
    );
}
