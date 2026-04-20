//! Scenario execution. Builds a fresh `Game`, applies scripted
//! inputs + timed spawns, ticks at wall-clock cadence, captures
//! telemetry, and returns a `ScenarioReport`.

use anyhow::Result;
use std::io::stdout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::arena::Arena;
use crate::bench::report::{ScenarioReport, SubsystemSample, percentile_us};
use crate::bench::scenario::{Scenario, ScenarioSpawn, SpawnLayout};
use crate::bench::scripts::should_deploy_turret;
use crate::content::ContentDb;
use crate::enemy::Enemy;
use crate::fb::Framebuffer;
use crate::game::Game;
use crate::hud;
use crate::terminal;

/// Sim cadence — matches the main game loop. Kept in sync with
/// `lib.rs::SIM_DT`. If that changes, update here too (the two
/// values should stay locked so benchmark results translate
/// directly to live gameplay).
const SIM_DT: Duration = Duration::from_micros(33_333);
const SIM_DT_SECS: f32 = 1.0 / 30.0;
const TARGET_FRAME: Duration = Duration::from_micros(16_667); // 60 fps

/// Which rendering path the runner drives. See module docs for the
/// motivation — in short, `Watch` includes terminal emission,
/// `Headless` strips it so CI can run benches without a TTY.
#[derive(Clone, Copy, Debug)]
pub enum RenderMode {
    /// Full pipeline: TerminalGuard, framebuffer compose, stdout
    /// emission. "Real" cost, fun to watch.
    Watch,
    /// Sim + particle tick only. No framebuffer, no stdout writes.
    /// The delta vs `Watch` attributes cost to rendering.
    Headless,
}

impl RenderMode {
    fn label(self) -> &'static str {
        match self {
            RenderMode::Watch => "watch",
            RenderMode::Headless => "headless",
        }
    }
}

/// Run one scenario end-to-end. Returns a structured report; prints
/// nothing — caller decides where the report goes.
///
/// Thin wrapper over `run_scenario_with_interrupt` for callers that
/// don't need interrupt handling (e.g. integration tests, which run
/// short scenarios non-interactively).
pub fn run_scenario(scenario: &Scenario, render_mode: RenderMode) -> Result<ScenarioReport> {
    let flag = Arc::new(AtomicBool::new(false));
    run_scenario_with_interrupt(scenario, render_mode, false, &flag)
}

/// Run one scenario with an interrupt flag. When `interrupt` is
/// tripped mid-run the loop exits cleanly and the report reflects
/// whatever data was captured. Used by the CLI so Ctrl+C / Esc
/// don't have to wait for the scenario to finish naturally.
pub fn run_scenario_with_interrupt(
    scenario: &Scenario,
    render_mode: RenderMode,
    debug_grid: bool,
    interrupt: &Arc<AtomicBool>,
) -> Result<ScenarioReport> {
    // Only enter the raw-mode terminal guard when we actually need
    // the TTY. Headless runs work fine piped through CI.
    let terminal_guard = match render_mode {
        RenderMode::Watch => Some(terminal::TerminalGuard::enter()?),
        RenderMode::Headless => None,
    };
    let mut stdout = stdout();

    // Viewport size: fetch from the real terminal in Watch mode;
    // use a dummy 80×24 in Headless so Game::new has something to
    // initialize its camera with. (Camera dims don't affect sim.)
    let (cols, rows) = match render_mode {
        RenderMode::Watch => crossterm::terminal::size().unwrap_or((80, 24)),
        RenderMode::Headless => (80, 24),
    };
    let mut fb = Framebuffer::new(cols, rows);

    let (aw, ah) = scenario.arena;
    let arena = Arena::generate(scenario.arena_seed, aw, ah);
    let content = ContentDb::load_core()?;
    let mut game = Game::new(arena, content, cols, rows);

    // Scripted players. Each spawned at its declared pos; local_id
    // points at the first so HUD code paths still resolve cleanly.
    let mut player_ids: Vec<u32> = Vec::with_capacity(scenario.players.len());
    let mut deploy_timers: Vec<f32> = vec![0.0; scenario.players.len()];
    for sp in &scenario.players {
        let pid = game.add_player();
        if let Some(i) = game.players.iter().position(|p| p.id == pid) {
            game.players[i].x = sp.pos.0;
            game.players[i].y = sp.pos.1;
            game.players[i].turret_kits = sp.turret_kits;
            // Optional HP boost so spectacle scenarios can outlast
            // the swarm. Default 100 is fine for combat baselines;
            // multi-stage shows want a few thousand HP so the
            // operator gets to see the climax.
            if let Some(hp) = sp.hp_override {
                game.players[i].hp = hp;
            }
        }
        player_ids.push(pid);
    }
    if let Some(&id) = player_ids.first() {
        game.local_id = Some(id);
    }
    game.debug_spatial_grid = debug_grid;
    let mut audio_overlay = crate::audio::AudioOverlay::default();

    // Camera follows first player for Watch mode; no-op for headless.
    game.update_camera_follow();

    // Substance paints: apply zero-time paints up front so the
    // first frame already sees them; defer the rest into a sorted
    // queue dispatched by `at_secs` alongside the spawn queue.
    let mut pending_paints: Vec<&crate::bench::scenario::ScenarioSubstancePaint> =
        scenario.paints.iter().filter(|p| p.at_secs > 0.0).collect();
    pending_paints.sort_by(|a, b| a.at_secs.partial_cmp(&b.at_secs).unwrap());
    let mut next_paint_idx: usize = 0;
    for paint in scenario.paints.iter().filter(|p| p.at_secs <= 0.0) {
        apply_substance_paint(&mut game, paint);
    }

    // Queue spawns by time. Copy + sort to make dispatch O(n log n).
    let mut pending_spawns: Vec<&ScenarioSpawn> = scenario.spawns.iter().collect();
    pending_spawns.sort_by(|a, b| a.at_secs.partial_cmp(&b.at_secs).unwrap());
    let mut next_spawn_idx: usize = 0;

    // Per-frame samples. Pre-reserved to avoid reallocation noise in
    // the hot loop — sized for the expected frame count at 60fps.
    let expected_frames = (scenario.duration.as_secs_f32() * 60.0).ceil() as usize + 16;
    let mut tick_samples: Vec<Duration> = Vec::with_capacity(expected_frames);
    let mut frame_samples: Vec<Duration> = Vec::with_capacity(expected_frames);

    let mut peak_enemies: u32 = 0;
    let mut peak_particles: u32 = 0;
    let mut peak_projectiles: u32 = 0;
    let mut peak_corpses: u32 = 0;

    let scenario_start = Instant::now();
    let mut last_instant = Instant::now();
    let mut tick_accum = Duration::ZERO;
    let mut cleared_early = false;

    let mut interrupted = false;
    while scenario_start.elapsed() < scenario.duration {
        // OS-level Ctrl+C handler may have flipped the flag between
        // frames; bail immediately in either render mode.
        if interrupt.load(Ordering::SeqCst) {
            interrupted = true;
            break;
        }
        // Watch mode: poll crossterm for Esc / Ctrl+C since raw-mode
        // swallows the native signal. Non-blocking — costs ~1µs per
        // frame when no event is pending.
        if matches!(render_mode, RenderMode::Watch) {
            match poll_watch_events()? {
                WatchEvent::None => {}
                WatchEvent::Interrupt => {
                    interrupt.store(true, Ordering::SeqCst);
                    interrupted = true;
                    break;
                }
                WatchEvent::ToggleDebugGrid => {
                    game.debug_spatial_grid = !game.debug_spatial_grid;
                }
                WatchEvent::ToggleAudioOverlay => {
                    audio_overlay.toggle();
                }
            }
        }
        let frame_start = Instant::now();
        let elapsed_secs = scenario_start.elapsed().as_secs_f32();
        tick_accum += last_instant.elapsed();
        last_instant = Instant::now();

        // Fire any spawn batches whose `at_secs` has arrived.
        while next_spawn_idx < pending_spawns.len()
            && pending_spawns[next_spawn_idx].at_secs <= elapsed_secs
        {
            let batch = pending_spawns[next_spawn_idx];
            apply_spawn(&mut game, batch);
            next_spawn_idx += 1;
        }
        // Same for staged substance paints — fire seeds, secondary
        // oil drops, blood splash drops, etc. Lets a scenario
        // choreograph cascading chaos.
        while next_paint_idx < pending_paints.len()
            && pending_paints[next_paint_idx].at_secs <= elapsed_secs
        {
            let paint = pending_paints[next_paint_idx];
            apply_substance_paint(&mut game, paint);
            next_paint_idx += 1;
        }

        // Script-driven inputs + turret-deploy cadence.
        for (i, sp) in scenario.players.iter().enumerate() {
            let pid = player_ids[i];
            let (px, py) = game
                .players
                .iter()
                .find(|p| p.id == pid)
                .map(|p| (p.x, p.y))
                .unwrap_or(sp.pos);
            let input = sp.script.tick(elapsed_secs, (px, py), &game);
            game.set_input(pid, input);
            if should_deploy_turret(sp, deploy_timers[i], elapsed_secs) {
                game.try_deploy_turret(pid);
                deploy_timers[i] = elapsed_secs;
            }
        }

        // Sim tick(s). Same fixed-step ladder as the main loop so
        // behavior matches live gameplay.
        let tick_start = Instant::now();
        while tick_accum >= SIM_DT {
            game.tick_authoritative(SIM_DT_SECS);
            tick_accum -= SIM_DT;
        }
        // Particle + client-side state advances every frame even on
        // host (this is how the main solo loop works). Keeps gore
        // counts realistic during benches.
        game.tick_client(SIM_DT_SECS);
        let tick_dur = tick_start.elapsed();
        tick_samples.push(tick_dur);
        // Roll per-frame perf accumulators. In Watch mode the render
        // call would fold this automatically; in Headless we have to
        // do it ourselves or `take_totals` at scenario end will only
        // see the last frame's scopes.
        game.perf.end_frame();

        // Track peak populations.
        peak_enemies = peak_enemies.max(game.enemies.len() as u32);
        peak_particles = peak_particles.max(game.particles.len() as u32);
        peak_projectiles = peak_projectiles.max(game.projectiles.len() as u32);
        peak_corpses = peak_corpses.max(game.corpses.len() as u32);

        // Watch path: full render + stdout. Headless: skip both.
        if matches!(render_mode, RenderMode::Watch) {
            game.update_camera_follow();
            fb.clear();
            let (tint_color, tint_amount) = game.corruption_tint();
            fb.set_tint(tint_color, tint_amount);
            game.render(&mut fb);
            fb.blit(&mut stdout)?;
            // Minimal HUD — a bench overlay showing scenario + time.
            let _ = hud::draw_hud(
                &mut stdout,
                game.director.wave,
                game.local_player().map(|p| p.hp).unwrap_or(100),
                game.kills,
                game.corruption,
                game.local_player().map(|p| p.sanity).unwrap_or(100.0),
                false,
            );
            let _ = draw_bench_banner(
                &mut stdout,
                scenario.name,
                elapsed_secs,
                scenario.duration.as_secs_f32(),
                game.enemies.len() as u32,
            );
            let (tc, tr) = crossterm::terminal::size().unwrap_or((cols, rows));
            let _ = audio_overlay.render(&mut stdout, tc, tr);
        }

        // Early-out if the scenario asked for it and nothing hostile
        // remains. Matches the wave director's clear check.
        if scenario.stop_when_clear
            && next_spawn_idx >= pending_spawns.len()
            && game.enemies.is_empty()
        {
            cleared_early = true;
            break;
        }

        let frame_dur = frame_start.elapsed();
        frame_samples.push(frame_dur);
        if matches!(render_mode, RenderMode::Watch) && frame_dur < TARGET_FRAME {
            std::thread::sleep(TARGET_FRAME - frame_dur);
        } else if matches!(render_mode, RenderMode::Headless) && frame_dur < SIM_DT {
            // Headless still paces to sim cadence — we want real
            // wall-clock timing for telemetry parity.
            std::thread::sleep(SIM_DT - frame_dur);
        }
    }

    drop(terminal_guard);

    if interrupted {
        eprintln!(
            "[bench] interrupted mid-scenario `{}` at {:.1}s — partial report follows",
            scenario.name,
            scenario_start.elapsed().as_secs_f32(),
        );
    }

    let frames = tick_samples.len() as u32;
    let mut tick_sorted = tick_samples.clone();
    tick_sorted.sort();
    let mut frame_sorted = frame_samples.clone();
    frame_sorted.sort();
    let tick_max = tick_sorted.iter().copied().max().unwrap_or_default();
    let frame_max = frame_sorted.iter().copied().max().unwrap_or_default();

    // Subsystem breakdown comes from FrameProfile's accumulators.
    // The 1s window reports are lossy for whole-scenario analysis;
    // we want per-scenario totals. See FrameProfile::take_totals.
    let subsystems = game
        .perf
        .take_totals()
        .into_iter()
        .map(|(label, total, calls)| {
            let avg_us = if frames > 0 {
                (total.as_micros() as u64) / (frames as u64)
            } else {
                0
            };
            SubsystemSample {
                label: label.to_string(),
                avg_us,
                total_calls: calls,
            }
        })
        .collect::<Vec<_>>();

    let elapsed = scenario_start.elapsed().as_secs_f32();
    let fps = if elapsed > 0.0 {
        frames as f32 / elapsed
    } else {
        0.0
    };

    Ok(ScenarioReport {
        name: scenario.name.to_string(),
        render_mode: render_mode.label(),
        arena: scenario.arena,
        duration_secs: elapsed,
        frames,
        tick_p50_us: percentile_us(&tick_sorted, 0.50),
        tick_p95_us: percentile_us(&tick_sorted, 0.95),
        tick_p99_us: percentile_us(&tick_sorted, 0.99),
        tick_max_us: tick_max.as_micros() as u64,
        frame_p50_us: percentile_us(&frame_sorted, 0.50),
        frame_p95_us: percentile_us(&frame_sorted, 0.95),
        frame_p99_us: percentile_us(&frame_sorted, 0.99),
        frame_max_us: frame_max.as_micros() as u64,
        fps,
        peak_enemies,
        peak_particles,
        peak_projectiles,
        peak_corpses,
        subsystems,
        cleared_early,
    })
}

/// Push a spawn batch into `game.enemies` according to its layout.
/// Bypasses the wave director — benches want control over exact
/// placement, not director pacing.
/// Stamp a ScenarioSubstancePaint onto the arena ground layer.
/// Tiles outside the arena bounds or on non-floor structures are
/// skipped silently. Uses `Arena::set_substance` (which replaces
/// any existing substance at that tile).
fn apply_substance_paint(game: &mut Game, paint: &crate::bench::scenario::ScenarioSubstancePaint) {
    use crate::bench::scenario::SubstanceLayout;
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    let Some(id) = game.content.substances.by_name(paint.substance) else {
        tracing::warn!(substance = paint.substance, "scenario paint: unknown substance");
        return;
    };
    let w = game.arena.width as i32;
    let h = game.arena.height as i32;
    let in_bounds = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h;
    let mut stamp = |g: &mut Game, x: i32, y: i32| {
        if in_bounds(x, y) && g.arena.is_passable(x, y) {
            g.arena.set_substance(x, y, id, paint.state);
        }
    };
    match paint.layout {
        SubstanceLayout::Point(x, y) => stamp(game, x as i32, y as i32),
        SubstanceLayout::Rect { center, half_w, half_h } => {
            let x0 = (center.0 - half_w) as i32;
            let x1 = (center.0 + half_w) as i32;
            let y0 = (center.1 - half_h) as i32;
            let y1 = (center.1 + half_h) as i32;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    stamp(game, x, y);
                }
            }
        }
        SubstanceLayout::Disk { center, radius } => {
            let r2 = radius * radius;
            let x0 = (center.0 - radius) as i32;
            let x1 = (center.0 + radius) as i32;
            let y0 = (center.1 - radius) as i32;
            let y1 = (center.1 + radius) as i32;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dx = x as f32 + 0.5 - center.0;
                    let dy = y as f32 + 0.5 - center.1;
                    if dx * dx + dy * dy <= r2 {
                        stamp(game, x, y);
                    }
                }
            }
        }
        SubstanceLayout::Line { from, to } => {
            let dx = to.0 - from.0;
            let dy = to.1 - from.1;
            let steps = (dx * dx + dy * dy).sqrt().ceil().max(1.0) as i32;
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let px = from.0 + dx * t;
                let py = from.1 + dy * t;
                stamp(game, px as i32, py as i32);
            }
        }
        SubstanceLayout::Scatter { center, half_w, half_h, count, seed } => {
            let mut rng = SmallRng::seed_from_u64(seed);
            for _ in 0..count {
                let x = (center.0 + rng.gen_range(-half_w..=half_w)) as i32;
                let y = (center.1 + rng.gen_range(-half_h..=half_h)) as i32;
                stamp(game, x, y);
            }
        }
    }
}

fn apply_spawn(game: &mut Game, batch: &ScenarioSpawn) {
    let stats = game.content.stats(batch.archetype).clone();
    let arena_w = game.arena.width as f32;
    let arena_h = game.arena.height as f32;

    let points = layout_points(&batch.layout, batch.count, arena_w, arena_h);
    let audio_id = batch.archetype.audio_id();
    for (x, y) in points {
        // Clamp to arena interior so a bad layout spec doesn't
        // shove enemies through walls. A separate pass would be
        // nicer, but clamping is cheap + sufficient for benches.
        let cx = x.clamp(2.0, arena_w - 2.0);
        let cy = y.clamp(2.0, arena_h - 2.0);
        game.enemies.push(Enemy::spawn(batch.archetype, &stats, cx, cy));
        // Fire the spawn sound for this archetype. No-op until
        // audio::init() has run; safe to call every spawn regardless.
        crate::audio::emit(audio_id, "spawn", Some((cx, cy)));
    }
    tracing::debug!(
        arch = ?batch.archetype,
        count = batch.count,
        "bench spawn batch applied"
    );
}

fn layout_points(
    layout: &SpawnLayout,
    count: u32,
    arena_w: f32,
    arena_h: f32,
) -> Vec<(f32, f32)> {
    use std::f32::consts::TAU;
    let mut out = Vec::with_capacity(count as usize);
    match *layout {
        SpawnLayout::Point(x, y) => {
            // Tiny jitter so the separation pass has something to
            // spread; perfectly co-located enemies get a golden-
            // ratio stir, but a 500-wide stack is visually clearer
            // when we seed it with a small random offset first.
            for i in 0..count {
                let a = (i as f32) * 0.61803 * TAU;
                let r = 0.25 + (i as f32 % 8.0) * 0.05;
                out.push((x + a.cos() * r, y + a.sin() * r));
            }
        }
        SpawnLayout::Ring { center, radius } => {
            for i in 0..count {
                let theta = (i as f32 / count.max(1) as f32) * TAU;
                out.push((center.0 + theta.cos() * radius, center.1 + theta.sin() * radius));
            }
        }
        SpawnLayout::Grid { center, spacing } => {
            let side = (count as f32).sqrt().ceil() as u32;
            let half = (side as f32) * spacing * 0.5;
            for i in 0..count {
                let gx = (i % side) as f32 * spacing - half;
                let gy = (i / side) as f32 * spacing - half;
                out.push((center.0 + gx, center.1 + gy));
            }
        }
        SpawnLayout::Edges => {
            // Biased along the outer rim; mixes all four edges so
            // the pressure mirrors the director's "from every
            // direction" feel. Deterministic offsets so re-runs
            // match.
            for i in 0..count {
                let edge = i % 4;
                let along = (i as f32 / (count / 4).max(1) as f32) % 1.0;
                let margin = 6.0;
                let (x, y) = match edge {
                    0 => (along * (arena_w - 2.0 * margin) + margin, margin),
                    1 => (along * (arena_w - 2.0 * margin) + margin, arena_h - margin),
                    2 => (margin, along * (arena_h - 2.0 * margin) + margin),
                    _ => (arena_w - margin, along * (arena_h - 2.0 * margin) + margin),
                };
                out.push((x, y));
            }
        }
    }
    out
}

enum WatchEvent {
    None,
    Interrupt,
    ToggleDebugGrid,
    ToggleAudioOverlay,
}

/// Non-blocking key poll for Watch mode. Picks up Esc / Ctrl+C for
/// clean abort, F4 for the spatial-grid overlay, and F5 for the
/// audio-engine overlay. Raw-mode swallows native signals, so we
/// read everything from the event stream.
fn poll_watch_events() -> Result<WatchEvent> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    if !event::poll(Duration::from_millis(0))? {
        return Ok(WatchEvent::None);
    }
    let evt = event::read()?;
    if let Event::Key(k) = evt {
        if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(WatchEvent::None);
        }
        if let KeyCode::Esc = k.code {
            return Ok(WatchEvent::Interrupt);
        }
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(WatchEvent::Interrupt);
        }
        if let KeyCode::F(4) = k.code {
            return Ok(WatchEvent::ToggleDebugGrid);
        }
        if let KeyCode::F(5) = k.code {
            return Ok(WatchEvent::ToggleAudioOverlay);
        }
    }
    Ok(WatchEvent::None)
}

fn draw_bench_banner<W: std::io::Write>(
    out: &mut W,
    name: &str,
    elapsed: f32,
    duration: f32,
    enemies: u32,
) -> Result<()> {
    use crossterm::{
        cursor::MoveTo,
        queue,
        style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    };
    let text = format!(
        " BENCH · {name} · {elapsed:>5.1}/{duration:>4.1}s · {enemies:>5} enemies ",
    );
    queue!(
        out,
        MoveTo(0, 2),
        SetForegroundColor(Color::Rgb { r: 160, g: 255, b: 200 }),
        SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
        Print(&text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}
