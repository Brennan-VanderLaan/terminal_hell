//! Scenario-driven benchmark mode.
//!
//! A sibling to `run_solo` / `run_serve` / `run_connect`. Reuses the
//! main `Game` engine — same tick, same rendering, same content packs
//! — but drives it with scripted inputs + direct enemy spawns instead
//! of a keyboard + wave director. Each scenario runs for a fixed
//! duration, gathers telemetry, wipes the world, and hands off to the
//! next scenario in the batch.
//!
//! Two render modes share the runner:
//!
//! - `watch`   — full framebuffer compose + terminal emission at
//!   60 fps, so the operator can literally watch 10k zerglings
//!   stampede a ring of turrets. Measures total runtime cost.
//! - `headless` — no framebuffer, no terminal guard, no stdout
//!   writes. Sim + particle tick still run at wall-clock cadence.
//!   CI-friendly: runs without a TTY, cheap enough for every PR.
//!
//! The delta between the two modes isolates rendering cost from sim
//! cost for a given scenario — which is the whole reason both ship.
//!
//! Authoring stays in Rust for v1 (type-safe, no parsing overhead,
//! refactors with the rest of the codebase). TOML scenario packs
//! arrive later once the schema has settled.

pub mod report;
pub mod runner;
pub mod scenario;
pub mod scenarios;
pub mod scripts;

pub use report::ScenarioReport;
pub use runner::{RenderMode, run_scenario, run_scenario_with_interrupt};
pub use scenario::{Scenario, ScenarioSpawn, ScriptedPlayer, SpawnLayout};
pub use scripts::PlayerScript;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use std::path::PathBuf;

/// Entry point for `terminal_hell bench`. Resolves the requested
/// scenario set (from `--scenario` or `--playlist`), runs each in
/// sequence, optionally repeats while `--loop` + interrupt allow,
/// and writes structured JSON to `output` if requested.
///
/// `looping` turns the bench into a screensaver soundstage: between
/// iterations it asks the audio engine to rescan its sample pools so
/// files produced by the recorder land in-context on the next loop.
pub fn run_bench(
    scenario_name: Option<String>,
    playlist: Option<String>,
    render_mode: RenderMode,
    debug_grid: bool,
    output: Option<PathBuf>,
    looping: bool,
) -> Result<()> {
    let catalogue = scenarios::catalogue();

    // Resolve the selected scenario set. Playlist wins over scenario
    // if both are given (strictly more specific).
    let selected: Vec<&Scenario> = if let Some(list) = playlist.as_ref() {
        let mut out = Vec::new();
        for name in list.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            match catalogue.iter().find(|s| s.name == name) {
                Some(s) => out.push(s),
                None => {
                    eprintln!("unknown scenario `{name}` in playlist. Available:");
                    for s in &catalogue {
                        eprintln!("  - {} ({})", s.name, s.summary);
                    }
                    return Ok(());
                }
            }
        }
        if out.is_empty() {
            eprintln!("playlist is empty after parsing; exiting");
            return Ok(());
        }
        out
    } else if let Some(name) = scenario_name.as_ref() {
        match catalogue.iter().find(|s| s.name == name) {
            Some(s) => vec![s],
            None => {
                eprintln!("unknown scenario `{name}`. Available:");
                for s in &catalogue {
                    eprintln!("  - {} ({})", s.name, s.summary);
                }
                return Ok(());
            }
        }
    } else {
        catalogue.iter().collect()
    };

    // Shared interrupt flag. Ctrl+C (via the OS signal handler) and
    // Esc / Ctrl+C in Watch mode's key-poll loop both flip this.
    let interrupt = Arc::new(AtomicBool::new(false));
    {
        let flag = interrupt.clone();
        let _ = ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    let content_root = crate::audio::default_content_root();
    let mut reports: Vec<ScenarioReport> = Vec::new();
    let mut iteration: u32 = 0;

    'outer: loop {
        iteration += 1;
        if looping {
            eprintln!("\n[bench] loop iteration {iteration}");
            // Rescan between iterations so hot-reloaded samples land
            // cleanly at the start of each scenario rather than
            // mid-playback.
            let _ = crate::audio::scan(&content_root);
        }

        for scenario in &selected {
            if interrupt.load(Ordering::SeqCst) {
                eprintln!("\n[bench] interrupted — skipping remaining scenarios");
                break 'outer;
            }
            let report = run_scenario_with_interrupt(
                scenario,
                render_mode,
                debug_grid,
                &interrupt,
            )?;
            // Log to stderr so piping JSON to a file stays clean.
            eprintln!("\n{}", report.format_pretty());
            reports.push(report);
        }

        if !looping {
            break;
        }
    }

    if let Some(path) = output {
        let json = report::reports_to_json(&reports);
        std::fs::write(&path, json)?;
        eprintln!("\nwrote {} scenario reports to {}", reports.len(), path.display());
    }

    Ok(())
}
