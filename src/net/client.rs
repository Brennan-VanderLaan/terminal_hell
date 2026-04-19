use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use renet::{ConnectionConfig, DefaultChannel, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};
use std::io::stdout;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime};

use crate::arena::Arena;
use crate::enemy::Enemy;
use crate::fb::Framebuffer;
use crate::game::{DestructionBlast, Game};
use crate::hud;
use crate::input::Input;
use crate::net::proto::{self, ClientInput, ClientMsg, ServerMsg};
use crate::player::Player;
use crate::projectile::Projectile;
use crate::terminal;

const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const INPUT_PERIOD: Duration = Duration::from_micros(16_667); // ~60 Hz

pub fn run_connect(addr: String) -> Result<()> {
    let server_addr: SocketAddr = parse_server_addr(&addr)?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let client_id = current_time.as_nanos() as u64;
    let auth = ClientAuthentication::Unsecure {
        server_addr,
        client_id,
        user_data: None,
        protocol_id: super::PROTOCOL_ID,
    };
    let mut transport = NetcodeClientTransport::new(current_time, auth, socket)?;
    let mut client = RenetClient::new(ConnectionConfig::default());

    tracing::info!(%server_addr, "connecting");

    let _guard = terminal::TerminalGuard::enter()?;
    let mut out = stdout();

    let (cols, rows) = crossterm::terminal::size()?;
    let mut fb = Framebuffer::new(cols, rows);

    // Start with a placeholder arena — replaced by Welcome.
    let (aw, ah) = arena_size(cols, rows);
    let mut game = Game::new(Arena::hand_crafted(aw, ah), center_offset(cols, rows, aw, ah));

    let mut input = Input::default();
    let mut last_instant = Instant::now();
    let mut input_accum = Duration::ZERO;
    let mut welcomed = false;
    let mut run_ended_summary: Option<(u32, u32, u64)> = None;

    loop {
        let frame_start = Instant::now();
        let dt = last_instant.elapsed();
        last_instant = Instant::now();
        input_accum += dt;

        client.update(dt);
        transport.update(dt, &mut client)?;

        // Drain incoming server messages.
        while let Some(bytes) = client.receive_message(DefaultChannel::ReliableOrdered) {
            if let Some(msg) = proto::decode::<ServerMsg>(&bytes) {
                handle_reliable(msg, &mut game, &mut welcomed, &mut run_ended_summary);
            }
        }
        while let Some(bytes) = client.receive_message(DefaultChannel::Unreliable) {
            if let Some(msg) = proto::decode::<ServerMsg>(&bytes) {
                handle_unreliable(msg, &mut game);
            }
        }

        // Local input events.
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(k) => {
                    let press =
                        matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat);
                    let release = matches!(k.kind, KeyEventKind::Release);
                    match k.code {
                        KeyCode::Esc => return Ok(()),
                        KeyCode::Char('q') if press => return Ok(()),
                        KeyCode::Char('c')
                            if press && k.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(());
                        }
                        KeyCode::Char(' ') if press => game.mouse.lmb = true,
                        KeyCode::Char(' ') if release => game.mouse.lmb = false,
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
                    let (aw2, ah2) = arena_size(w, h);
                    game.set_origin(center_offset(w, h, aw2, ah2));
                }
                _ => {}
            }
        }

        // Tick client-side particles (pure visual).
        game.tick_client(dt.as_secs_f32());

        // Send input ~60 Hz.
        if client.is_connected() && welcomed && input_accum >= INPUT_PERIOD {
            input_accum = Duration::ZERO;
            let (mx, my) = input.move_vec();
            let aim = game.aim_target_for_local().unwrap_or((1.0, 0.0));
            let inp = ClientInput {
                move_x: mx,
                move_y: my,
                aim_x: aim.0,
                aim_y: aim.1,
                firing: game.mouse.lmb,
            };
            client.send_message(
                DefaultChannel::Unreliable,
                proto::encode(&ClientMsg::Input(inp)),
            );
        }

        transport.send_packets(&mut client).ok();

        // Render.
        fb.clear();
        if welcomed {
            game.render(&mut fb);
            fb.blit(&mut out)?;
            let hp = game.local_player().map(|p| p.hp).unwrap_or(0);
            hud::draw_hud(&mut out, game.director.wave, hp, game.kills)?;
            let (tc, tr) = crossterm::terminal::size()?;
            hud::draw_wave_banner(
                &mut out, tc, tr, game.director.wave, game.director.banner_ttl,
            )?;
        } else {
            fb.blit(&mut out)?;
            hud::draw_connecting(&mut out)?;
        }

        if let Some((wave, kills, elapsed)) = run_ended_summary {
            let (c2, r2) = crossterm::terminal::size()?;
            hud::draw_gameover(&mut out, c2, r2, wave, kills, elapsed)?;
            wait_for_any_key()?;
            return Ok(());
        }

        let elapsed = frame_start.elapsed();
        if elapsed < TARGET_FRAME {
            std::thread::sleep(TARGET_FRAME - elapsed);
        }
    }
}

fn handle_reliable(
    msg: ServerMsg,
    game: &mut Game,
    welcomed: &mut bool,
    run_ended_summary: &mut Option<(u32, u32, u64)>,
) {
    match msg {
        ServerMsg::Welcome(w) => {
            if let Some(new_arena) =
                Arena::decode_tiles(w.arena_w, w.arena_h, &w.arena_tiles)
            {
                game.arena = new_arena;
            }
            game.local_id = Some(w.your_id);
            *welcomed = true;
        }
        ServerMsg::TileUpdate { x, y, kind, hp } => {
            game.apply_tile_update(x, y, kind, hp);
        }
        ServerMsg::PlayerJoined { .. } | ServerMsg::PlayerLeft { .. } => {
            // Player list is driven by the snapshot; nothing to do here yet.
        }
        ServerMsg::RunEnded { wave, kills, elapsed_secs } => {
            *run_ended_summary = Some((wave, kills, elapsed_secs as u64));
        }
        _ => {}
    }
}

fn handle_unreliable(msg: ServerMsg, game: &mut Game) {
    match msg {
        ServerMsg::Snapshot(s) => {
            game.director.wave = s.wave;
            game.kills = s.kills;
            game.alive = s.alive;
            game.elapsed_secs = s.elapsed_secs;

            // Rebuild players from snapshot.
            game.players.clear();
            for ps in s.players {
                let mut p = Player::new(ps.id, ps.x, ps.y);
                p.hp = ps.hp;
                p.aim_x = ps.aim_x;
                p.aim_y = ps.aim_y;
                game.players.push(p);
            }

            // Rebuild enemies + projectiles. Authoring stays on the host; the
            // client just draws what it's told.
            game.enemies.clear();
            for es in s.enemies {
                let archetype = crate::enemy::Archetype::from_kind(es.kind);
                let mut e = Enemy::spawn(archetype, es.x, es.y);
                e.hp = es.hp;
                game.enemies.push(e);
            }
            game.projectiles.clear();
            for ps in s.projectiles {
                game.projectiles.push(Projectile {
                    x: ps.x,
                    y: ps.y,
                    vx: 0.0,
                    vy: 0.0,
                    ttl: 0.1,
                    damage: 0,
                });
            }
        }
        ServerMsg::Blast(b) => {
            let blast = DestructionBlast {
                x: b.x,
                y: b.y,
                color_rgb: b.color,
                seed: b.seed,
                intensity: b.intensity,
            };
            game.apply_blast(&blast);
        }
        _ => {}
    }
}

fn parse_server_addr(s: &str) -> Result<SocketAddr> {
    if s.contains(':') {
        s.parse().context("parse server addr")
    } else {
        format!("{s}:{}", super::DEFAULT_PORT).parse().context("parse server addr")
    }
}

pub fn wait_for_any_key() -> Result<()> {
    loop {
        match event::read()? {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                return Ok(());
            }
            Event::Mouse(m) if matches!(m.kind, MouseEventKind::Down(_)) => return Ok(()),
            _ => {}
        }
    }
}

pub fn arena_size(cols: u16, rows: u16) -> (u16, u16) {
    let pixel_h = rows.saturating_mul(2);
    let w = cols.min(160).max(40);
    let h = pixel_h.min(80).max(30);
    (w, h)
}

pub fn center_offset(cols: u16, rows: u16, aw: u16, ah: u16) -> (i32, i32) {
    let ox = (cols as i32 - aw as i32) / 2;
    let oy = (rows as i32 * 2 - ah as i32) / 2;
    (ox.max(0), oy.max(0))
}
