use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::io::stdout;
use std::time::{Duration, Instant};

pub mod arena;
pub mod content;
pub mod enemy;
pub mod fb;
pub mod game;
pub mod hud;
pub mod input;
pub mod mouse;
pub mod net;
pub mod particle;
pub mod pickup;
pub mod player;
pub mod primitive;
pub mod projectile;
pub mod sprite;
pub mod terminal;
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
    let (aw, ah) = net::client::arena_size(cols, rows);
    let arena = Arena::hand_crafted(aw, ah);

    let origin = net::client::center_offset(cols, rows, aw, ah);
    let content = content::ContentDb::load_core()?;
    tracing::info!(brand = %content.active_brand().id, "content pack loaded");
    let mut game = Game::new(arena, content, origin);
    let local_id = game.add_player();
    game.local_id = Some(local_id);
    let mut input = Input::default();

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
                    match k.code {
                        KeyCode::Esc => return Ok(()),
                        KeyCode::Char('c')
                            if press && k.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(());
                        }
                        KeyCode::Char(' ') if press => game.mouse.lmb = true,
                        KeyCode::Char(' ') if release => game.mouse.lmb = false,
                        KeyCode::Char('e') | KeyCode::Char('E') if press => {
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                game.try_interact(local_id);
                            }
                        }
                        KeyCode::Char('q') if press => {
                            // In solo mode Q toggles weapon slot, not quit.
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                game.try_cycle_weapon(local_id);
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
                        _ => {}
                    }
                }
                Event::Resize(w, h) => {
                    fb.resize(w, h);
                    let (aw2, ah2) = net::client::arena_size(w, h);
                    game.set_origin(net::client::center_offset(w, h, aw2, ah2));
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
            game.tick_authoritative(SIM_DT_SECS);
            tick_accum -= SIM_DT;
        }

        fb.clear();
        game.render(&mut fb);
        fb.blit(&mut stdout)?;
        let hp = game.local_player().map(|p| p.hp).unwrap_or(0);
        hud::draw_hud(&mut stdout, game.director.wave, hp, game.kills)?;
        if let Some(loadout) = game.local_loadout() {
            hud::draw_loadout(&mut stdout, &loadout)?;
        }
        let (tc, tr) = crossterm::terminal::size()?;
        hud::draw_wave_banner(
            &mut stdout,
            tc,
            tr,
            game.director.wave,
            game.director.banner_ttl,
        )?;

        if !game.alive {
            let (cols, rows) = crossterm::terminal::size()?;
            hud::draw_gameover(
                &mut stdout,
                cols,
                rows,
                game.director.wave,
                game.kills,
                game.elapsed_secs as u64,
            )?;
            net::client::wait_for_any_key()?;
            return Ok(());
        }

        let elapsed = frame_start.elapsed();
        if elapsed < TARGET_FRAME {
            std::thread::sleep(TARGET_FRAME - elapsed);
        }
    }
}
