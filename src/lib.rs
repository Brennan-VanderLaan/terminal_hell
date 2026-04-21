use anyhow::Result;
use crossterm::event::{self, MouseButton, MouseEventKind};
use std::io::stdout;
use std::time::{Duration, Instant};

pub mod arena;
pub mod art;
pub mod audio;
pub mod behavior;
pub mod bench;
pub mod bindings;
pub mod body_effect;
pub mod camera;
pub mod carcosa;
pub mod console;
pub mod content;
pub mod corpse;
pub mod enemy;
pub mod fb;
pub mod game;
pub mod gamepad;
#[cfg(target_os = "linux")]
pub mod gamepad_linux_ff;
pub mod gore;
pub mod hud;
pub mod input;
pub mod interaction;
pub mod mip_hint;
pub mod install_server;
pub mod log_buf;
pub mod menu;
pub mod mouse;
pub mod net;
pub mod particle;
pub mod pathfind;
pub mod perf_overlay;
pub mod perk;
pub mod pickup;
pub mod platform;
pub mod player;
pub mod primitive;
pub mod projectile;
pub mod share;
pub mod spatial;
pub mod sprite;
pub mod stun;
pub mod substance;
pub mod tag;
pub mod telemetry;
pub mod terminal;
pub mod traversal;
pub mod upnp;
pub mod vote;
pub mod waves;
pub mod weapon;

use arena::Arena;
use bindings::router::{
    InputEvent, InputRouter, LastInput, dispatch_local_verb, dispatch_menu_nav, dispatch_overlay,
    resolve_frame_input,
};
use bindings::wizard::{RebindWizard, WizardDevice, WizardOutcome};
use fb::Framebuffer;
use game::{Game, InputMode, PlayerInput};

const TARGET_FRAME: Duration = Duration::from_micros(16_667); // 60 fps
const SIM_DT: Duration = Duration::from_micros(33_333); // 30 Hz
const SIM_DT_SECS: f32 = 1.0 / 30.0;

pub fn run_solo() -> Result<()> {
    let _guard = terminal::TerminalGuard::enter()?;
    // Bump Windows scheduler resolution to 1ms for the duration of the
    // session so thread::sleep honors sub-15ms durations (gamepad
    // worker poll + main-loop frame pacing).
    let _timer_guard = platform::TimerResolutionGuard::millisecond();
    let mut stdout = stdout();

    let (cols, rows) = crossterm::terminal::size()?;
    let mut fb = Framebuffer::new(cols, rows);
    let (aw, ah) = net::client::world_size();

    let loaded_bindings = bindings::Bindings::load().unwrap_or_else(|e| {
        tracing::warn!(error = ?e, "failed to load bindings; using defaults");
        bindings::Bindings::defaults()
    });
    let mut router = InputRouter::new(loaded_bindings);
    let mut audio_overlay = audio::AudioOverlay::default();
    // Optional Xbox-style controller. No-op if none attached or gilrs
    // can't initialize (missing udev, etc.) — same contract as audio.
    gamepad::ensure_init();

    // Outer loop: each iteration is one full run. `continue` after a
    // death-screen dismiss restarts without re-launching the binary;
    // `break` returns to the caller (binary exits).
    'session: loop {
        // ContentDb isn't Clone-friendly (its substance registry uses
        // leaked string interning), so we reload it each iteration.
        // Cheap: everything's embedded via `include_dir!`, no disk I/O.
        let content = content::ContentDb::load_core()?;
        tracing::info!(brand = %content.active_brand().id, "content pack loaded");

        let arena_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xDEAD_BEEF);
        tracing::info!(seed = arena_seed, "arena seed");
        let arena = Arena::generate(arena_seed, aw, ah);

        let mut game = Game::new(arena, content, cols, rows);
        // Solo runs get a throwaway share code so the menu "Copy
        // connect string" item always has something to copy —
        // useful if the user later decides to `serve` from a similar
        // environment.
        game.session_token = share::new_token().unwrap_or([0u8; 16]);
        game.share_code = Some(share::ShareCode::new(
            std::net::Ipv4Addr::LOCALHOST,
            4646,
            4647,
            game.session_token,
        ));
        let local_id = game.add_player();
        game.local_id = Some(local_id);

        // Remembered gamepad aim so light stick-flicks don't instantly
        // snap the aim back to mouse direction mid-burst.
        let mut last_pad_aim: Option<(f32, f32)> = None;
        let mut wizard: Option<RebindWizard> = None;

        let mut tick_accum = Duration::ZERO;
        let mut last_instant = Instant::now();

        tracing::info!(cols, rows, aw, ah, "solo run started");

        loop {
        let frame_start = Instant::now();
        let frame_dt = last_instant.elapsed();
        tick_accum += frame_dt;
        last_instant = Instant::now();

        // Stage 1: pump raw events into the router. It translates
        // crossterm key/mouse events + gamepad button events into a
        // unified `InputEvent` stream while also tracking held
        // actions for movement + fire.
        while event::poll(Duration::ZERO)? {
            router.push_crossterm(event::read()?);
        }
        let pad_events = gamepad::drain_events();
        game.perf.set_count("gpad_events_frame", pad_events.len());
        for ev in pad_events {
            router.push_gamepad(ev);
        }
        // Pure analog stick/trigger movement (no button edges) also
        // counts as "I'm on the controller now".
        let pad = gamepad::snapshot();
        router.observe_gamepad_analog(&pad);
        telemetry::ingest_gamepad(&mut game.perf);

        // Stage 2: reflect the router's device-activity tracker into
        // the UI mode flag.
        game.input_mode = match router.last_input() {
            LastInput::MouseKeyboard => InputMode::MouseKeyboard,
            LastInput::Gamepad => InputMode::Gamepad,
        };

        // Stage 3: dispatch events. Wizard (if open) eats everything;
        // otherwise menu-open state gates gameplay. During the death
        // cinematic's accept-input window, dispatch intercepts
        // restart/quit intents and reports which one the player chose.
        match dispatch_events(
            &mut router,
            &mut game,
            &mut audio_overlay,
            &mut wizard,
            &mut fb,
            local_id,
        )? {
            SessionFlow::Continue => {}
            SessionFlow::Quit => break 'session,
            SessionFlow::Restart => continue 'session,
        }

        // Stage 4: resolve per-frame inputs (movement, aim, firing).
        let prefer_gamepad = game.input_mode == InputMode::Gamepad;
        let frame_input = resolve_frame_input(&router, prefer_gamepad, &mut last_pad_aim);
        let (mx, my) = frame_input.move_vec;
        let aim = match game.input_mode {
            InputMode::Gamepad => frame_input.aim.unwrap_or((1.0, 0.0)),
            InputMode::MouseKeyboard => {
                last_pad_aim = None;
                game.aim_target_for_local().unwrap_or((1.0, 0.0))
            }
        };
        game.gamepad_aim = if game.input_mode == InputMode::Gamepad {
            last_pad_aim
        } else {
            None
        };
        // Firing sources: router's Fire-held state (Space or RT via
        // bindings) plus mouse LMB. Wizard suppresses all gameplay
        // input so a capture key doesn't also fire a weapon.
        let firing = if wizard.is_some() {
            false
        } else {
            router.firing() || game.mouse.lmb
        };
        game.set_input(
            local_id,
            PlayerInput {
                move_x: if wizard.is_some() { 0.0 } else { mx },
                move_y: if wizard.is_some() { 0.0 } else { my },
                aim_x: aim.0,
                aim_y: aim.1,
                firing,
            },
        );

        while tick_accum >= SIM_DT {
            // Snapshot hp before the tick so we can rumble on any damage
            // path (melee / projectile / hazard / bleed) without wrapping
            // every call site in player.rs.
            let hp_before = game.local_player().map(|p| p.hp).unwrap_or(0);
            // tick_authoritative short-circuits when game.paused is set,
            // so the menu's Pause toggle is the only thing gating sim now.
            game.tick_authoritative(SIM_DT_SECS);
            game.menu.tick(SIM_DT_SECS);
            let hp_after = game.local_player().map(|p| p.hp).unwrap_or(0);
            let dropped = hp_before - hp_after;
            if dropped > 0 {
                gamepad::rumble(gamepad::Rumble::Damage { dmg: dropped });
            }
            tick_accum -= SIM_DT;
        }

        // AimZoom is held-style: ease toward the hold target each
        // render frame so release snaps back to the player's chosen
        // base zoom. Cap dt so a huge frame pause doesn't snap the
        // ease to target instantly.
        let aim_held = router.is_held(bindings::Action::AimZoom);
        game.camera.tick_zoom(frame_dt.as_secs_f32().min(0.05), aim_held);
        game.update_camera_follow();
        fb.clear();
        {
            let (tint_color, tint_amount) = game.corruption_tint();
            fb.set_tint(tint_color, tint_amount);
        }
        game.render(&mut fb);
        fb.blit(&mut stdout)?;
        let hp = game.local_player().map(|p| p.hp).unwrap_or(0);
        let local = game.local_player();
        let sanity = local.map(|p| p.sanity).unwrap_or(100.0);
        let marked = local.map(|p| Some(p.id) == game.marked_player_id).unwrap_or(false);
        hud::draw_hud(
            &mut stdout,
            game.director.wave,
            hp,
            game.kills,
            game.corruption,
            sanity,
            marked,
        )?;
        if let Some(loadout) = game.local_loadout() {
            hud::draw_loadout(&mut stdout, &loadout)?;
        }
        hud::draw_intermission(&mut stdout, &game)?;
        hud::draw_active_brands(&mut stdout, &game.active_brands)?;
        if let Some(p) = game.local_player() {
            hud::draw_perks(&mut stdout, &p.perks)?;
        }
        hud::draw_kiosk_labels(&mut stdout, &game)?;
        let (tc_z, tr_z) = crossterm::terminal::size()?;
        hud::draw_zoom_indicator(&mut stdout, tc_z, game.camera.zoom)?;
        if game.paused {
            hud::draw_paused_banner(&mut stdout, tc_z, tr_z, game.is_authoritative)?;
        }
        // Dead banner: local player is down but the team plays on.
        if game.alive
            && game.local_player().map(|p| p.hp <= 0).unwrap_or(false)
        {
            hud::draw_dead_banner(&mut stdout, tc_z, tr_z)?;
        }
        // Console renders below the menu so the menu stays modal on top.
        game.perf_overlay.render(&mut stdout, tc_z, tr_z, &game.perf)?;
        audio_overlay.render(&mut stdout, tc_z, tr_z)?;
        if game.inventory_open {
            if let Some(p) = game.local_player() {
                let loadout = game.local_loadout();
                hud::draw_inventory(&mut stdout, tc_z, tr_z, p, loadout.as_ref())?;
            }
        }
        game.console.render(&mut stdout, tc_z, tr_z)?;
        game.menu.render(
            &mut stdout,
            tc_z,
            tr_z,
            game.is_authoritative,
            game.paused,
            game.input_mode == InputMode::Gamepad,
        )?;
        if let Some(w) = wizard.as_ref() {
            w.render(&mut stdout, tc_z, tr_z)?;
        }
        let (tc, tr) = crossterm::terminal::size()?;
        hud::draw_wave_banner(
            &mut stdout,
            tc,
            tr,
            game.director.wave,
            game.director.banner_ttl,
        )?;
        hud::draw_clear_countdown(&mut stdout, tc, game.director.clear_timer)?;
        hud::draw_pickup_labels(&mut stdout, &game)?;
        hud::draw_pickup_toasts(&mut stdout, tc, &game.toasts)?;
        hud::draw_kill_feed(&mut stdout, tc, &game.kill_feed)?;
        // Single flush after every HUD overlay has queued its bytes
        // — one flush per frame keeps the terminal from repainting
        // mid-pass, which is the source of the flicker we saw when
        // every draw_* function flushed independently.
        use std::io::Write as _;
        stdout.flush()?;

        // Death cinematic — render the report overlay during any
        // DeathPhase. The inner state machine runs through Dying →
        // Praise → Goldening → Report; we only render the report
        // panel during Goldening + Report so the first two phases
        // still show the in-world action (bodies + gore + frozen
        // horde) uncovered.
        // Death cinematic overlay. Dismiss keys (restart / quit) flow
        // through the same `dispatch_events` path that handles every
        // other input — see the early-return in that function.
        if let Some(phase) = &game.death_phase {
            let show_panel = matches!(
                phase,
                game::DeathPhase::Goldening { .. } | game::DeathPhase::Report { .. }
            );
            if show_panel {
                let (cols, rows) = crossterm::terminal::size()?;
                hud::draw_death_report(&mut stdout, cols, rows, &game)?;
                hud::draw_restart_prompt(
                    &mut stdout,
                    cols,
                    rows,
                    game.input_mode == InputMode::Gamepad,
                )?;
                stdout.flush()?;
            }
        }

        let elapsed = frame_start.elapsed();
        if elapsed < TARGET_FRAME {
            std::thread::sleep(TARGET_FRAME - elapsed);
        }
        }
    }
    Ok(())
}

/// Control-flow decision produced by a single frame's input dispatch.
enum SessionFlow {
    /// Keep running this session.
    Continue,
    /// Exit the binary.
    Quit,
    /// End the current game and start a fresh one — death-screen
    /// dismiss path.
    Restart,
}

/// Drain the router's pending events and dispatch them.
///
/// Wizard-open state swallows all Action events and routes raw-key /
/// raw-button captures into the wizard. Menu-open state swallows
/// gameplay verbs and overlay toggles, passing only MenuUp/Down/Activate
/// through. When the death cinematic is accepting input, gameplay /
/// menu dispatch is bypassed and the frame's events are scanned for
/// restart (Space/Enter/Fire/Interact/MenuActivate) or quit
/// (Esc/MenuToggle/Ctrl+C).
fn dispatch_events(
    router: &mut InputRouter,
    game: &mut Game,
    audio_overlay: &mut audio::AudioOverlay,
    wizard: &mut Option<RebindWizard>,
    fb: &mut Framebuffer,
    _local_id: u32,
) -> Result<SessionFlow> {
    let local_id = _local_id;
    // Death-screen dismiss. Once the report accepts input, intercept
    // the event stream before the usual menu/gameplay dispatch so
    // Fire/Interact/Space/Enter restart, Esc/Menu quits, and
    // everything else is swallowed. This runs in the same stream the
    // router already drained at the top of the frame, so every input
    // source (keyboard, gamepad, mouse) is covered uniformly.
    if game.death_phase.is_some() && game.death_report_accepts_input() {
        for ev in router.drain() {
            match ev {
                InputEvent::Quit => return Ok(SessionFlow::Quit),
                InputEvent::MenuToggle => return Ok(SessionFlow::Quit),
                InputEvent::ActionPress(action) => match action {
                    bindings::Action::Fire
                    | bindings::Action::Interact
                    | bindings::Action::MenuActivate => return Ok(SessionFlow::Restart),
                    _ => {}
                },
                _ => {}
            }
        }
        return Ok(SessionFlow::Continue);
    }
    for ev in router.drain() {
        match ev {
            InputEvent::Quit => return Ok(SessionFlow::Quit),
            InputEvent::MenuToggle => {
                if wizard.is_some() {
                    // ESC / Start cancels the wizard without saving.
                    *wizard = None;
                    router.set_capture(None);
                } else {
                    game.menu.toggle();
                }
            }
            InputEvent::Resize(w, h) => {
                fb.resize(w, h);
                game.resize_viewport(w, h);
            }
            InputEvent::Mouse(m) => {
                game.mouse.col = m.column;
                game.mouse.row = m.row;
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => game.mouse.lmb = true,
                    MouseEventKind::Up(MouseButton::Left) => game.mouse.lmb = false,
                    MouseEventKind::ScrollUp => {
                        game.camera.adjust_zoom(camera::ZOOM_STEP);
                    }
                    MouseEventKind::ScrollDown => {
                        game.camera.adjust_zoom(1.0 / camera::ZOOM_STEP);
                    }
                    _ => {}
                }
            }
            InputEvent::RawKeyboardCapture(code) => {
                if let Some(w) = wizard.as_mut() {
                    match w.handle_key_capture(code) {
                        WizardOutcome::Continue => {}
                        WizardOutcome::Save(new) => {
                            if let Err(e) = new.save() {
                                tracing::warn!(error = ?e, "save bindings");
                            }
                            router.set_bindings(new);
                            router.set_capture(None);
                            *wizard = None;
                            // Re-open the menu so the player lands
                            // back where they launched the wizard
                            // instead of being dumped into gameplay
                            // with no indication of what happened.
                            game.menu.open = true;
                        }
                        WizardOutcome::Cancel => {
                            router.set_capture(None);
                            *wizard = None;
                            // Re-open the menu so the player lands
                            // back where they launched the wizard
                            // instead of being dumped into gameplay
                            // with no indication of what happened.
                            game.menu.open = true;
                        }
                    }
                }
            }
            InputEvent::RawGamepadCapture(btn) => {
                if let Some(w) = wizard.as_mut() {
                    match w.handle_gamepad_capture(btn) {
                        WizardOutcome::Continue => {}
                        WizardOutcome::Save(new) => {
                            if let Err(e) = new.save() {
                                tracing::warn!(error = ?e, "save bindings");
                            }
                            router.set_bindings(new);
                            router.set_capture(None);
                            *wizard = None;
                            // Re-open the menu so the player lands
                            // back where they launched the wizard
                            // instead of being dumped into gameplay
                            // with no indication of what happened.
                            game.menu.open = true;
                        }
                        WizardOutcome::Cancel => {
                            router.set_capture(None);
                            *wizard = None;
                            // Re-open the menu so the player lands
                            // back where they launched the wizard
                            // instead of being dumped into gameplay
                            // with no indication of what happened.
                            game.menu.open = true;
                        }
                    }
                }
            }
            InputEvent::ActionPress(action) => {
                if wizard.is_some() {
                    continue;
                }
                if game.menu.open {
                    // When menu is open, only nav + wizard-launch
                    // items from the menu pass through. Activate can
                    // spawn a wizard via [`handle_menu_action`].
                    let menu_action = dispatch_menu_nav(game, action);
                    if handle_menu_action(menu_action, game, router, wizard)? {
                        return Ok(SessionFlow::Quit);
                    }
                    continue;
                }
                // Gameplay mode: verbs first, then overlays.
                if dispatch_local_verb(game, local_id, action) {
                    continue;
                }
                dispatch_overlay(game, audio_overlay, action);
            }
            InputEvent::ActionRelease(_) => {
                // Held-state is maintained inside the router; no
                // discrete action needed on release today.
            }
        }
    }
    Ok(SessionFlow::Continue)
}

/// Apply a [`MenuAction`] produced by Enter / MenuActivate while the
/// menu is open. Returns `true` when the main loop should exit.
fn handle_menu_action(
    action: menu::MenuAction,
    game: &mut Game,
    router: &mut InputRouter,
    wizard: &mut Option<RebindWizard>,
) -> Result<bool> {
    match action {
        menu::MenuAction::None | menu::MenuAction::Close => {}
        menu::MenuAction::Quit => return Ok(true),
        menu::MenuAction::TogglePause => {
            game.toggle_pause();
            let msg = if game.paused { "paused" } else { "resumed" };
            game.menu.set_feedback(msg, 1.5);
        }
        menu::MenuAction::Copy(text) => {
            let feedback = menu::copy_to_clipboard(&text);
            eprintln!("\n{}", text);
            game.menu.set_feedback(feedback, 2.0);
        }
        menu::MenuAction::RebindKeyboard => {
            *wizard = Some(RebindWizard::start(
                WizardDevice::Keyboard,
                router.bindings.clone(),
            ));
            router.set_capture(Some(bindings::router::CaptureMode::Keyboard));
            game.menu.open = false;
        }
        menu::MenuAction::RebindGamepad => {
            *wizard = Some(RebindWizard::start(
                WizardDevice::Gamepad,
                router.bindings.clone(),
            ));
            router.set_capture(Some(bindings::router::CaptureMode::Gamepad));
            game.menu.open = false;
        }
        menu::MenuAction::ResetBindings => {
            let defaults = bindings::Bindings::defaults();
            if let Err(e) = defaults.save() {
                tracing::warn!(error = ?e, "save bindings");
                game.menu.set_feedback("reset failed — see console", 2.0);
            } else {
                router.set_bindings(defaults);
                game.menu.set_feedback("bindings reset to defaults", 2.0);
            }
        }
    }
    Ok(false)
}
