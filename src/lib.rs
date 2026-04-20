use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::io::stdout;
use std::time::{Duration, Instant};

pub mod arena;
pub mod art;
pub mod audio;
pub mod behavior;
pub mod bench;
pub mod body_effect;
pub mod camera;
pub mod carcosa;
pub mod console;
pub mod content;
pub mod corpse;
pub mod enemy;
pub mod fb;
pub mod game;
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
use fb::Framebuffer;
use game::{Game, PlayerInput};
use input::Input;

const TARGET_FRAME: Duration = Duration::from_micros(16_667); // 60 fps
const SIM_DT: Duration = Duration::from_micros(33_333); // 30 Hz
const SIM_DT_SECS: f32 = 1.0 / 30.0;

pub fn run_solo() -> Result<()> {
    let _guard = terminal::TerminalGuard::enter()?;
    let mut stdout = stdout();

    let (cols, rows) = crossterm::terminal::size()?;
    let mut fb = Framebuffer::new(cols, rows);
    let (aw, ah) = net::client::world_size();
    let arena_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xDEAD_BEEF);
    tracing::info!(seed = arena_seed, "arena seed");
    let arena = Arena::generate(arena_seed, aw, ah);

    let content = content::ContentDb::load_core()?;
    tracing::info!(brand = %content.active_brand().id, "content pack loaded");
    let mut game = Game::new(arena, content, cols, rows);
    // Solo runs get a throwaway share code so the menu "Copy connect
    // string" item always has something to copy — useful if the user
    // later decides to `serve` from a similar environment.
    game.session_token = share::new_token().unwrap_or([0u8; 16]);
    game.share_code = Some(share::ShareCode::new(
        std::net::Ipv4Addr::LOCALHOST,
        4646,
        4647,
        game.session_token,
    ));
    let local_id = game.add_player();
    game.local_id = Some(local_id);
    let mut input = Input::default();
    let mut audio_overlay = audio::AudioOverlay::default();

    let mut tick_accum = Duration::ZERO;
    let mut last_instant = Instant::now();

    tracing::info!(cols, rows, aw, ah, "solo mode started");

    loop {
        let frame_start = Instant::now();
        tick_accum += last_instant.elapsed();
        last_instant = Instant::now();

        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(k) => {
                    let press = matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat);
                    let release = matches!(k.kind, KeyEventKind::Release);
                    // ESC always toggles the menu. When the menu is open it
                    // swallows the other game keys; Enter activates the
                    // selected item and may return us to gameplay or quit.
                    if let KeyCode::Esc = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.menu.toggle();
                        }
                        continue;
                    }
                    // Backtick toggles the log console. Pure overlay — it
                    // doesn't gate game input; player can keep playing
                    // while logs scroll.
                    if let KeyCode::Char('`') = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.console.toggle();
                        }
                        continue;
                    }
                    if let KeyCode::F(3) = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.perf_overlay.toggle();
                        }
                        continue;
                    }
                    if let KeyCode::F(4) = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.debug_spatial_grid = !game.debug_spatial_grid;
                        }
                        continue;
                    }
                    if let KeyCode::F(5) = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            audio_overlay.toggle();
                        }
                        continue;
                    }
                    if let KeyCode::Tab = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.inventory_open = !game.inventory_open;
                        }
                        continue;
                    }
                    if k.code == KeyCode::Char('c')
                        && press
                        && k.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Ok(());
                    }
                    if game.menu.open {
                        if !press || matches!(k.kind, KeyEventKind::Repeat) {
                            continue;
                        }
                        match k.code {
                            KeyCode::Up => game.menu.move_up(),
                            KeyCode::Down => {
                                let max = game.menu.items(game.is_authoritative).len();
                                game.menu.move_down(max);
                            }
                            KeyCode::Enter => {
                                let connect_cmd = game
                                    .share_code
                                    .as_ref()
                                    .map(|c| c.connect_command());
                                let install = game.install_one_liner.clone();
                                let action = game.menu.activate(
                                    game.is_authoritative,
                                    connect_cmd.as_deref(),
                                    install.as_deref(),
                                );
                                match action {
                                    menu::MenuAction::None => {}
                                    menu::MenuAction::Close => {}
                                    menu::MenuAction::Quit => return Ok(()),
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
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match k.code {
                        KeyCode::Char(' ') if press => game.mouse.lmb = true,
                        KeyCode::Char(' ') if release => game.mouse.lmb = false,
                        KeyCode::Char('e') | KeyCode::Char('E') if press => {
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                game.try_interact(local_id);
                            }
                        }
                        KeyCode::Char('q') if press => {
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                game.try_cycle_weapon(local_id);
                            }
                        }
                        KeyCode::Char('t') | KeyCode::Char('T') if press => {
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                game.try_deploy_turret(local_id);
                            }
                        }
                        KeyCode::Char('f') | KeyCode::Char('F') if press => {
                            // F for "flash" / traversal verb.
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                game.try_activate_traversal(local_id);
                            }
                        }
                        code if press => input.key_event(code, true),
                        code if release => input.key_event(code, false),
                        _ => {}
                    }
                }
                Event::Mouse(m) => {
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
                Event::Resize(w, h) => {
                    fb.resize(w, h);
                    game.resize_viewport(w, h);
                }
                _ => {}
            }
        }

        // Drive the host's local player from keyboard + mouse.
        let (mx, my) = input.move_vec();
        let aim = game.aim_target_for_local().unwrap_or((1.0, 0.0));
        game.set_input(
            local_id,
            PlayerInput {
                move_x: mx,
                move_y: my,
                aim_x: aim.0,
                aim_y: aim.1,
                firing: game.mouse.lmb,
            },
        );

        while tick_accum >= SIM_DT {
            // tick_authoritative short-circuits when game.paused is set,
            // so the menu's Pause toggle is the only thing gating sim now.
            game.tick_authoritative(SIM_DT_SECS);
            game.menu.tick(SIM_DT_SECS);
            tick_accum -= SIM_DT;
        }

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
        game.menu.render(&mut stdout, tc_z, tr_z, game.is_authoritative, game.paused)?;
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
        if let Some(phase) = &game.death_phase {
            let show_panel = matches!(
                phase,
                game::DeathPhase::Goldening { .. } | game::DeathPhase::Report { .. }
            );
            if show_panel {
                let (cols, rows) = crossterm::terminal::size()?;
                hud::draw_death_report(&mut stdout, cols, rows, &game)?;
            }
            // Accept a dismiss keypress once the Report phase has
            // had a moment to settle — this is the gate that stops
            // buffered WASD from skipping the cinematic.
            if game.death_report_accepts_input() {
                while event::poll(Duration::ZERO)? {
                    if let Event::Key(k) = event::read()? {
                        if matches!(k.kind, KeyEventKind::Press) {
                            return Ok(());
                        }
                    }
                }
            } else {
                // Drain + discard pending input during the cinematic
                // so buffered keys don't pile up and then all fire
                // on the first accepted frame.
                while event::poll(Duration::ZERO)? {
                    let _ = event::read()?;
                }
            }
        }

        let elapsed = frame_start.elapsed();
        if elapsed < TARGET_FRAME {
            std::thread::sleep(TARGET_FRAME - elapsed);
        }
    }
}
