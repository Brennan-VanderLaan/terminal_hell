//! Starter scenario catalogue. Rust-native for now; TOML loader
//! arrives once the schema has settled.
//!
//! Naming convention: `<flavor>_<scale>` so diffs between commits
//! line up visually in the report table. Keep durations short (5–15s)
//! so the whole batch runs inside a CI minute; stress tests that
//! need longer can opt in explicitly.

use crate::bench::scenario::{
    Scenario, ScenarioSpawn, ScenarioSubstancePaint, ScriptedPlayer, SpawnLayout,
    SubstanceLayout,
};
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
        // Substance-world bench suite. Each isolates one axis of
        // the new mechanics so regressions attribute cleanly to
        // the `substance_world` perf scope.
        ambient_floor_paint(),
        acid_lake(),
        fire_sheet_2000(),
        oil_slick_inferno(),
        burning_swarm(),
        uranium_field(),
        flammable_blood_chain(),
        death_pool_cascade(),
        full_stack_chaos(),
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
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_secs(5),
        stop_when_clear: false,
        paints: vec![],
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
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 50,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: true,
        paints: vec![],
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
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 500,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: false,
        paints: vec![],
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
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 2000,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(10),
        stop_when_clear: false,
        paints: vec![],
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
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Zergling,
            count: 10_000,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(15),
        stop_when_clear: false,
        paints: vec![],
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
            hp_override: None,
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
        paints: vec![],
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
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 500,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(8),
        stop_when_clear: false,
        paints: vec![],
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
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Breacher,
            count: 30,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(12),
        stop_when_clear: false,
        paints: vec![],
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
            hp_override: None,
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
        paints: vec![],
    }
}

// ───────────────────────────────────────────────────────────────────
// Substance-world scenarios — exercise the ambient emitter,
// standing-hazard, and fire-propagation passes added in the
// substance overhaul. Each isolates one axis so the
// `substance_world` perf scope reads cleanly per scenario.
// ───────────────────────────────────────────────────────────────────

fn ambient_floor_paint() -> Scenario {
    // Bigger arena + scorch field for the post-rescale baseline.
    // Half-extents 100×70 = ~28 000 tiles of scorch, the kind of
    // footprint a long miniboss-cleared room actually accumulates.
    let arena = (500u16, 350u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "ambient_floor_paint",
        summary: "Massive scorch field, idle player — ambient emitter pass cost at scale.",
        arena,
        arena_seed: 0xA001,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_secs(6),
        stop_when_clear: false,
        paints: vec![ScenarioSubstancePaint {
            substance: "scorch",
            state: 30,
            layout: SubstanceLayout::Rect {
                center: c,
                half_w: 100.0,
                half_h: 70.0,
            },
            at_secs: 0.0,
        }],
    }
}

fn acid_lake() -> Scenario {
    // Disk r=40 ≈ 5000 acid tiles — what an Acid-primitive cleanup
    // of a 50-rusher pack would actually leave on the floor. Player
    // strafes the lake edge at r=38 so they dip in/out and the
    // hazard pass attribution lights up cleanly.
    let arena = (500u16, 500u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "acid_lake",
        summary: "Player strafes the rim of a 5000-tile acid lake — DPS + emitter at scale.",
        arena,
        arena_seed: 0xAC1D,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 0,
            script: PlayerScript::CircleStrafe {
                center: c,
                radius: 38.0,
                rate: 0.4,
            },
            hp_override: Some(2000),
        }],
        spawns: vec![],
        duration: Duration::from_secs(8),
        stop_when_clear: false,
        paints: vec![ScenarioSubstancePaint {
            substance: "acid_pool",
            state: 0,
            layout: SubstanceLayout::Disk {
                center: c,
                radius: 40.0,
            },
            at_secs: 0.0,
        }],
    }
}

fn fire_sheet_2000() -> Scenario {
    // ~2000-tile fire sheet (50×40 rect). Stresses propagation +
    // decay at the scale a multi-Ignite combat actually generates.
    let arena = (500u16, 400u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "fire_sheet_2000",
        summary: "2000-tile pre-painted fire sheet — propagation + decay at combat scale.",
        arena,
        arena_seed: 0xF15E,
        players: vec![ScriptedPlayer {
            pos: (c.0 + 120.0, c.1),
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_secs(8),
        stop_when_clear: false,
        paints: vec![ScenarioSubstancePaint {
            substance: "fire",
            state: 0,
            layout: SubstanceLayout::Rect {
                center: c,
                half_w: 25.0,
                half_h: 20.0,
            },
            at_secs: 0.0,
        }],
    }
}

fn oil_slick_inferno() -> Scenario {
    // Big slick — 60×40 rect = ~5000 tiles, the kind of leak a few
    // mech kills would produce. Three staggered fire seeds along
    // one side so the cascade fans out instead of just blooming
    // from one point.
    let arena = (700u16, 500u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "oil_slick_inferno",
        summary: "5000-tile oil slick + 3 staged fire seeds — cascade across a big flammable.",
        arena,
        arena_seed: 0x011E,
        players: vec![ScriptedPlayer {
            pos: (c.0 + 160.0, c.1),
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_secs(14),
        stop_when_clear: false,
        paints: vec![
            ScenarioSubstancePaint {
                substance: "oil_pool",
                state: 0,
                layout: SubstanceLayout::Rect {
                    center: c,
                    half_w: 60.0,
                    half_h: 40.0,
                },
                at_secs: 0.0,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Disk {
                    center: (c.0 - 55.0, c.1 - 30.0),
                    radius: 3.0,
                },
                at_secs: 0.5,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Disk {
                    center: (c.0 - 55.0, c.1),
                    radius: 3.0,
                },
                at_secs: 2.0,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Disk {
                    center: (c.0 - 55.0, c.1 + 30.0),
                    radius: 3.0,
                },
                at_secs: 3.5,
            },
        ],
    }
}

fn burning_swarm() -> Scenario {
    // Bigger arena (700×500) so a 5000-tile fire field fits with
    // edge-spawn breathing room. 400 enemies (was 200) match the
    // "everything's bigger now" theme.
    let arena = (700u16, 500u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "burning_swarm",
        summary: "400 rushers cross a 5000-tile fire field — hazard pass × entity count at scale.",
        arena,
        arena_seed: 0xB14E,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
            hp_override: None,
        }],
        spawns: vec![ScenarioSpawn {
            archetype: Archetype::Rusher,
            count: 400,
            layout: SpawnLayout::Edges,
            at_secs: 0.0,
        }],
        duration: Duration::from_secs(12),
        stop_when_clear: false,
        paints: vec![ScenarioSubstancePaint {
            substance: "fire",
            state: 0,
            // ~5000-tile fire field. Combined with 400 rushers
            // dying inside it (each painting blood/ember/etc),
            // this is the worst case for the hazard pass.
            layout: SubstanceLayout::Rect {
                center: c,
                half_w: 60.0,
                half_h: 40.0,
            },
            at_secs: 0.0,
        }],
    }
}

fn uranium_field() -> Scenario {
    let arena = (400u16, 300u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "uranium_field",
        summary: "Sparse uranium scatter — sparse-tile iterator + sanity/DPS hazards.",
        arena,
        arena_seed: 0xDEAD,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 0,
            script: PlayerScript::CircleStrafe {
                center: c,
                radius: 14.0,
                rate: 0.8,
            },
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_secs(8),
        stop_when_clear: false,
        paints: vec![ScenarioSubstancePaint {
            substance: "uranium_slag",
            state: 0,
            layout: SubstanceLayout::Scatter {
                center: c,
                half_w: 50.0,
                half_h: 30.0,
                count: 80,
                seed: 0xDEAD_C0DE,
            },
            at_secs: 0.0,
        }],
    }
}

fn flammable_blood_chain() -> Scenario {
    // Long blood corridor — 300 tiles end-to-end. Width = 5
    // (multiple parallel lines) so the fire propagates as a
    // proper wave-front rather than a single 1-tile fuse.
    let arena = (500u16, 200u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "flammable_blood_chain",
        summary: "Wide blood corridor + ignition seed — fire wavefront over 300 tiles of flammable.",
        arena,
        arena_seed: 0xB100,
        players: vec![ScriptedPlayer {
            pos: (c.0 - 200.0, c.1),
            turret_kits: 0,
            script: PlayerScript::Stationary,
            hp_override: None,
        }],
        spawns: vec![],
        duration: Duration::from_secs(20),
        stop_when_clear: false,
        paints: vec![
            ScenarioSubstancePaint {
                substance: "blood_pool",
                state: 0,
                layout: SubstanceLayout::Rect {
                    center: c,
                    half_w: 150.0,
                    half_h: 3.0,
                },
                at_secs: 0.0,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Point(c.0 - 150.0, c.1),
                at_secs: 0.5,
            },
        ],
    }
}

/// FULL STACK CHAOS — multi-stage spectacle for watch mode. Big
/// arena, player ringed by 16 turrets so they survive long enough
/// to see the whole show, mixed enemy waves arriving over 28
/// seconds, and a choreographed fire cascade across an enormous
/// oil slick. Stages choreographed via timed paints + spawns:
///
///   t=0   600 rushers from edges; oil disk r=38, blood ring r=56,
///         ember scatter further out
///   t=4   400 leapers arrive; first fire seed at slick edge
///   t=7   second fire seed at opposite edge — pincer
///   t=9   6 rocketeers in a ring lobbing standoff explosives
///   t=11  third fire seed dead-center — full cascade trigger
///   t=14  1500 zergling tide
///   t=15  flood ichor splash — green palette mix at climax
///   t=20  miniboss spawns — extreme gore profile fires
fn full_stack_chaos() -> Scenario {
    let arena = (1000u16, 700u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "full_stack_chaos",
        summary: "Multi-stage inferno: oil + 3 staged ignitions + mixed waves to 1500 + miniboss. Watch mode money shot.",
        arena,
        arena_seed: 0xCA05,
        players: vec![ScriptedPlayer {
            pos: c,
            turret_kits: 16,
            script: PlayerScript::HoldAndDeploy { deploy_secs: 0.2 },
            hp_override: Some(5000),
        }],
        spawns: vec![
            ScenarioSpawn {
                archetype: Archetype::Rusher,
                count: 600,
                layout: SpawnLayout::Edges,
                at_secs: 0.0,
            },
            ScenarioSpawn {
                archetype: Archetype::Leaper,
                count: 400,
                layout: SpawnLayout::Edges,
                at_secs: 4.0,
            },
            ScenarioSpawn {
                archetype: Archetype::Rocketeer,
                count: 6,
                layout: SpawnLayout::Ring {
                    center: c,
                    radius: 180.0,
                },
                at_secs: 9.0,
            },
            ScenarioSpawn {
                archetype: Archetype::Zergling,
                count: 1500,
                layout: SpawnLayout::Edges,
                at_secs: 14.0,
            },
            ScenarioSpawn {
                archetype: Archetype::Miniboss,
                count: 1,
                layout: SpawnLayout::Point(c.0 + 60.0, c.1 + 40.0),
                at_secs: 20.0,
            },
        ],
        duration: Duration::from_secs(28),
        stop_when_clear: false,
        paints: vec![
            ScenarioSubstancePaint {
                substance: "oil_pool",
                state: 0,
                layout: SubstanceLayout::Disk {
                    center: c,
                    radius: 38.0,
                },
                at_secs: 0.0,
            },
            ScenarioSubstancePaint {
                substance: "blood_pool",
                state: 0,
                layout: SubstanceLayout::Disk {
                    center: c,
                    radius: 56.0,
                },
                at_secs: 0.0,
            },
            ScenarioSubstancePaint {
                substance: "ember_scatter",
                state: 0,
                layout: SubstanceLayout::Scatter {
                    center: c,
                    half_w: 90.0,
                    half_h: 60.0,
                    count: 40,
                    seed: 0xEEEE_BBBB,
                },
                at_secs: 0.0,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Point(c.0 - 36.0, c.1),
                at_secs: 4.0,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Point(c.0 + 36.0, c.1),
                at_secs: 7.0,
            },
            ScenarioSubstancePaint {
                substance: "fire",
                state: 0,
                layout: SubstanceLayout::Disk {
                    center: c,
                    radius: 4.0,
                },
                at_secs: 11.0,
            },
            ScenarioSubstancePaint {
                substance: "flood_ichor",
                state: 0,
                layout: SubstanceLayout::Scatter {
                    center: c,
                    half_w: 80.0,
                    half_h: 50.0,
                    count: 60,
                    seed: 0xFEED_BEEF,
                },
                at_secs: 15.0,
            },
        ],
    }
}

fn death_pool_cascade() -> Scenario {
    // Specifically tests overlapping death-paint pools. Spawn a
    // grid of mixed archetypes (Pmc, Sentinel, Flood, Phaser,
    // Swarmling) packed tight, then a player firing into the
    // cluster. Each kill paints its faction substance — oil for
    // mechs, ichor for Flood, ember+ignite for swarmlings, etc.
    // Pools overlap immediately, ambient particles saturate, and
    // the substance_world pass has to scan a heavily-painted
    // viewport every frame.
    let arena = (500u16, 400u16);
    let c = center(arena.0, arena.1);
    Scenario {
        name: "death_pool_cascade",
        summary: "Mixed-faction death pile — overlapping pools + ambient saturation worst case.",
        arena,
        arena_seed: 0xD0DE,
        players: vec![ScriptedPlayer {
            pos: (c.0 - 80.0, c.1),
            turret_kits: 0,
            script: PlayerScript::ShootNearest,
            hp_override: Some(3000),
        }],
        spawns: vec![
            // Tight grid of mech wrecks-in-waiting.
            ScenarioSpawn {
                archetype: Archetype::Sentinel,
                count: 20,
                layout: SpawnLayout::Grid {
                    center: (c.0 + 30.0, c.1 - 40.0),
                    spacing: 6.0,
                },
                at_secs: 0.0,
            },
            // PMCs (oil paint via heavy-blood + bigger gore).
            ScenarioSpawn {
                archetype: Archetype::Pmc,
                count: 30,
                layout: SpawnLayout::Grid {
                    center: (c.0 + 30.0, c.1),
                    spacing: 5.0,
                },
                at_secs: 0.0,
            },
            // Flood — green ichor on death.
            ScenarioSpawn {
                archetype: Archetype::Flood,
                count: 30,
                layout: SpawnLayout::Grid {
                    center: (c.0 + 30.0, c.1 + 40.0),
                    spacing: 5.0,
                },
                at_secs: 0.0,
            },
            // Swarmlings — ignite_aura on death (fire painting).
            ScenarioSpawn {
                archetype: Archetype::Swarmling,
                count: 60,
                layout: SpawnLayout::Grid {
                    center: (c.0 + 100.0, c.1),
                    spacing: 4.0,
                },
                at_secs: 0.0,
            },
            // Phasers — psychic residue on death (violet).
            ScenarioSpawn {
                archetype: Archetype::Phaser,
                count: 15,
                layout: SpawnLayout::Grid {
                    center: (c.0 + 80.0, c.1 - 60.0),
                    spacing: 5.0,
                },
                at_secs: 0.0,
            },
        ],
        duration: Duration::from_secs(15),
        stop_when_clear: false,
        paints: vec![],
    }
}
