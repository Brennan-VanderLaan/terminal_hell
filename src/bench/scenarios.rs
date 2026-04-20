//! Starter scenario catalogue. Rust-native for now; TOML loader
//! arrives once the schema has settled.
//!
//! Naming convention: `<flavor>_<scale>` so diffs between commits
//! line up visually in the report table. Keep durations short (5–15s)
//! so the whole batch runs inside a CI minute; stress tests that
//! need longer can opt in explicitly.

use crate::bench::scenario::{Scenario, ScenarioSpawn, ScriptedPlayer, SpawnLayout};
use crate::bench::scripts::PlayerScript;
use crate::enemy::Archetype;
use std::time::Duration;

pub fn catalogue() -> Vec<Scenario> {
    vec![
        baseline_empty(),
        rusher_50(),
        rusher_500(),
        rusher_2000(),
        zerg_tide_10k(),
        turret_wall_vs_zerg(),
        map_scale_small(),
        map_scale_medium(),
        map_scale_large(),
        breacher_wall_stress(),
        sentinel_gauntlet(),
    ]
}

/// Arena center helper for the default 1600×800 world.
fn center(w: u16, h: u16) -> (f32, f32) {
    (w as f32 * 0.5, h as f32 * 0.5)
}

fn baseline_empty() -> Scenario {
    let arena = (800u16, 400u16);
    Scenario {
        name: "baseline_empty",
        summary: "Solo player, no enemies — measures idle sim cost.",
        arena,
        arena_seed: 0x0001,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::Stationary,
        }],
        spawns: vec![],
        duration: Duration::from_secs(5),
        stop_when_clear: false,
    }
}

fn rusher_50() -> Scenario {
    let arena = (400u16, 400u16);
    Scenario {
        name: "rusher_50",
        summary: "50 rushers from edges, shooting player — combat baseline.",
        arena,
        arena_seed: 0x0050,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 50,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: true,
    }
}

fn rusher_500() -> Scenario {
    let arena = (600u16, 400u16);
    Scenario {
        name: "rusher_500",
        summary: "500 rushers — mid-swarm stress, expect sub-16ms tick.",
        arena,
        arena_seed: 0x0500,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 500,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: false,
    }
}

fn rusher_2000() -> Scenario {
    let arena = (1200u16, 600u16);
    Scenario {
        name: "rusher_2000",
        summary: "2000 rushers — big-swarm stress on the full-width arena.",
        arena,
        arena_seed: 0x2000,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 2000,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: false,
    }
}

fn zerg_tide_10k() -> Scenario {
    let arena = (1600u16, 800u16);
    Scenario {
        name: "zerg_tide_10k",
        summary: "Ten thousand zerglings — the limit case. Expect pain.",
        arena,
        arena_seed: 0x1_0000,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::CircleStrafe {
                center: center(arena.0, arena.1),
                radius: 6.0,
                rate: 1.5,
            },
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Zergling,
            count: 10_000,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(15),
        stop_when_clear: false,
    }
}

fn turret_wall_vs_zerg() -> Scenario {
    let arena = (1200u16, 800u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "turret_wall_vs_zerg",
        summary: "Player ringed by 12 turrets, 10k zerglings rushing — the money shot.",
        arena,
        arena_seed: 0xA11E,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 12,
            script: PlayerScript::HoldAndDeploy { deploy_secs: 0.25 },
        }],
        spawns: vec![
            // Zerglings arrive 1s in — gives the player time to drop
            // the turret ring before the first wave closes.
            ScenarioSpawn {
                archetype: Archetype::Zergling,
                count: 10_000,
                layout: SpawnLayout::Edges,
                at_secs: 1.0,
            },
        ],
        duration: Duration::from_secs(20),
        stop_when_clear: false,
    }
}

/// Map-scale trio: same count + archetype, different arena sizes.
/// Diff the reports to see how arena dims affect sim cost (spatial
/// queries, pathfinding, cache behavior).
fn map_scale_small() -> Scenario {
    scale_template("map_scale_small", (200, 200), 0x51)
}
fn map_scale_medium() -> Scenario {
    scale_template("map_scale_medium", (800, 400), 0x52)
}
fn map_scale_large() -> Scenario {
    scale_template("map_scale_large", (1600, 800), 0x53)
}

fn scale_template(name: &'static str, arena: (u16, u16), seed: u64) -> Scenario {
    Scenario {
        name,
        summary: "Same swarm, varying arena — isolates map size effect.",
        arena,
        arena_seed: seed,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 500,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(8),
        stop_when_clear: false,
    }
}

fn breacher_wall_stress() -> Scenario {
    let arena = (400u16, 400u16);
    Scenario {
        name: "breacher_wall_stress",
        summary: "30 breachers A*-ing through walls — pathfinding + destruction cost.",
        arena,
        arena_seed: 0xB0B,
        players: vec![ScriptedPlayer {
            pos: center(arena.0, arena.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Breacher,
            count: 30,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(12),
        stop_when_clear: false,
    }
}

fn sentinel_gauntlet() -> Scenario {
    let arena = (800u16, 400u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "sentinel_gauntlet",
        summary: "20 sentinels in a ring around a circle-strafing player — ranged LoS + projectile cost.",
        arena,
        arena_seed: 0x5E7,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 0,
            script: PlayerScript::CircleStrafe {
                center: c,
                radius: 12.0,
                rate: 1.0,
            },
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Sentinel,
            count: 20,
            layout: SpawnLayout::Ring {
                center: c,
                radius: 60.0,
            },
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: false,
    }
}
