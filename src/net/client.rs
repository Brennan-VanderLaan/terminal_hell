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
use crate::menu;
use crate::net::proto::{self, ClientInput, ClientMsg, ServerMsg};
use crate::pickup::Pickup;
use crate::player::Player;
use crate::projectile::Projectile;
use crate::terminal;

const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const INPUT_PERIOD: Duration = Duration::from_micros(16_667); // ~60 Hz

pub fn run_connect(addr: String) -> Result<()> {
    // Accept either a raw `ip:port` (LAN / manual) or a `TH01...` share
    // code. Share codes additionally carry the session token which the
    // host uses to validate the connection.
    let (server_addr, user_data): (SocketAddr, Option<[u8; 256]>) =
        if addr.starts_with(crate::share::MAGIC) {
            let code = crate::share::ShareCode::decode(&addr)?;
            let server_addr =
                SocketAddr::from((code.ip, code.game_port));
            let mut data = [0u8; 256];
            data[..16].copy_from_slice(&code.token);
            (server_addr, Some(data))
        } else {
            (parse_server_addr(&addr)?, None)
        };
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let client_id = current_time.as_nanos() as u64;
    let auth = ClientAuthentication::Unsecure {
        server_addr,
        client_id,
        user_data,
        protocol_id: super::PROTOCOL_ID,
    };
    let mut transport = NetcodeClientTransport::new(current_time, auth, socket)?;
    let mut client = RenetClient::new(ConnectionConfig::default());

    tracing::info!(%server_addr, "connecting");

    let _guard = terminal::TerminalGuard::enter()?;
    let mut out = stdout();

    let (cols, rows) = crossterm::terminal::size()?;
    let mut fb = Framebuffer::new(cols, rows);

    // Start with a placeholder arena — replaced by Welcome with the real
    // seed (client regenerates locally to save bandwidth on the 1600×800
    // world).
    let (aw, ah) = world_size();
    let content = crate::content::ContentDb::load_core()?;
    let mut game = Game::new(Arena::generate(0, aw, ah), content, cols, rows);
    // Clients don't own the authoritative sim — this flag hides host-only
    // menu items (notably the pause toggle) and makes toggle_pause a no-op.
    game.is_authoritative = false;
    // Client already knows the share code it was invoked with; stash it
    // so the menu can offer "copy connect string" for re-sharing.
    if let Ok(code) = crate::share::ShareCode::decode(&addr) {
        game.share_code = Some(code);
    }

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
                    if let KeyCode::Esc = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.menu.toggle();
                        }
                        continue;
                    }
                    if let KeyCode::Char('`') = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.console.toggle();
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
                                        // Clients don't own pause — item should be hidden
                                        // anyway since is_authoritative == false.
                                        game.menu.set_feedback("only the host can pause", 1.5);
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
                                client.send_message(
                                    DefaultChannel::ReliableOrdered,
                                    proto::encode(&ClientMsg::Interact),
                                );
                            }
                        }
                        KeyCode::Char('q') if press => {
                            if !matches!(k.kind, KeyEventKind::Repeat) {
                                client.send_message(
                                    DefaultChannel::ReliableOrdered,
                                    proto::encode(&ClientMsg::CycleWeapon),
                                );
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
                            game.camera.adjust_zoom(crate::camera::ZOOM_STEP);
                        }
                        MouseEventKind::ScrollDown => {
                            game.camera.adjust_zoom(1.0 / crate::camera::ZOOM_STEP);
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

        // Tick client-side particles (pure visual) + menu feedback timer.
        game.tick_client(dt.as_secs_f32());
        game.menu.tick(dt.as_secs_f32());

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
        game.update_camera_follow();
        fb.clear();
        if welcomed {
            let (tint_color, tint_amount) = game.corruption_tint();
            fb.set_tint(tint_color, tint_amount);
            game.render(&mut fb);
            fb.blit(&mut out)?;
            let hp = game.local_player().map(|p| p.hp).unwrap_or(0);
            let local = game.local_player();
            let sanity = local.map(|p| p.sanity).unwrap_or(100.0);
            let marked = local.map(|p| Some(p.id) == game.marked_player_id).unwrap_or(false);
            hud::draw_hud(
                &mut out,
                game.director.wave,
                hp,
                game.kills,
                game.corruption,
                sanity,
                marked,
            )?;
            if let Some(loadout) = game.local_loadout() {
                hud::draw_loadout(&mut out, &loadout)?;
            }
            hud::draw_intermission(&mut out, &game)?;
            hud::draw_kiosk_labels(&mut out, &game)?;
            let (tc, tr) = crossterm::terminal::size()?;
            hud::draw_zoom_indicator(&mut out, tc, game.camera.zoom)?;
            hud::draw_wave_banner(
                &mut out, tc, tr, game.director.wave, game.director.banner_ttl,
            )?;
            if game.paused {
                hud::draw_paused_banner(&mut out, tc, tr, game.is_authoritative)?;
            }
            game.console.render(&mut out, tc, tr)?;
            game.menu.render(&mut out, tc, tr, game.is_authoritative, game.paused)?;
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
            // Rebuild arena from the server-sent seed; all peers run the
            // same generator so layouts match without shipping the raw
            // tile blob over UDP.
            game.arena = crate::arena::Arena::generate(w.arena_seed, w.arena_w, w.arena_h);
            // Recenter the camera on the arena so it's not stuck at (0,0)
            // until the first snapshot arrives.
            game.camera.center = (w.arena_w as f32 / 2.0, w.arena_h as f32 / 2.0);
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
                p.sanity = ps.sanity;
                game.players.push(p);
            }
            game.marked_player_id = if s.marked_player_id == 0 {
                None
            } else {
                Some(s.marked_player_id)
            };
            game.yellow_signs.clear();
            for sn in s.yellow_signs {
                game.yellow_signs.push(crate::carcosa::YellowSign {
                    id: sn.id,
                    x: sn.x,
                    y: sn.y,
                    ttl: sn.ttl,
                    ttl_max: sn.ttl_max,
                });
            }
            // Mirror host pause state — stops local particle/phantom ticks.
            game.paused = s.paused;

            // Rebuild enemies + projectiles. Authoring stays on the host; the
            // client just draws what it's told.
            game.enemies.clear();
            for es in s.enemies {
                let archetype = crate::enemy::Archetype::from_kind(es.kind);
                let stats = game.content.stats(archetype);
                let mut e = Enemy::spawn(archetype, stats, es.x, es.y);
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
                    owner_id: 0,
                    primitives: Vec::new(),
                    bounces_left: 0,
                    pierces_left: 0,
                });
            }
            game.pickups.clear();
            for ps in s.pickups {
                game.pickups.push(Pickup::new(ps.id, ps.x, ps.y, ps.rarity, ps.primitives));
            }
            game.hitscans.clear();
            for hs in s.hitscans {
                game.hitscans.push(crate::game::HitscanTrace {
                    from: (hs.from_x, hs.from_y),
                    to: (hs.to_x, hs.to_y),
                    ttl: hs.ttl,
                });
            }
            game.kiosks.clear();
            for ks in s.kiosks {
                let color = crate::fb::Pixel::rgb(ks.color[0], ks.color[1], ks.color[2]);
                let mut k = crate::vote::VoteKiosk::new(
                    ks.id, ks.x, ks.y, ks.brand_id, ks.brand_name, color,
                );
                k.votes = ks.votes;
                game.kiosks.push(k);
            }
            game.active_brands = s.active_brands;
            game.client_phase = decode_phase(s.intermission_phase);
            game.client_phase_timer = s.phase_timer;
            game.corruption = s.corruption;
            game.remote_weapons = s.weapons;
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

fn decode_phase(p: u8) -> Option<crate::waves::IntermissionPhase> {
    use crate::waves::IntermissionPhase::*;
    match p {
        0 => Some(Breathe),
        1 => Some(Vote),
        2 => Some(Stock),
        3 => Some(Warning),
        _ => None,
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

/// Fixed world size (independent of terminal). 10× the previous ~160×80
/// arena so players have somewhere to explore and the camera is doing
/// something interesting.
pub const WORLD_W: u16 = 1600;
pub const WORLD_H: u16 = 800;

pub fn world_size() -> (u16, u16) {
    (WORLD_W, WORLD_H)
}
