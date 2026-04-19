use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use renet::{ClientId, ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use std::collections::HashMap;
use std::io::stdout;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime};

use crate::arena::Arena;
use crate::fb::Framebuffer;
use crate::game::{Game, PlayerInput};
use crate::hud;
use crate::input::Input;
use crate::net::proto::{
    self, Blast, ClientMsg, EnemySnap, HitscanSnap, KioskSnap, PickupSnap, PlayerSnap, ProjSnap,
    ServerMsg, SignSnap, Snapshot, WeaponLoadout, WeaponSnap, Welcome,
};
use crate::terminal;

const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const SIM_DT: Duration = Duration::from_micros(33_333);
const SIM_DT_SECS: f32 = 1.0 / 30.0;
const SNAPSHOT_PERIOD: Duration = Duration::from_millis(50); // 20 Hz

pub fn run_serve(port: u16) -> Result<()> {
    let bind_addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let public_addr = bind_addr;

    let socket = UdpSocket::bind(bind_addr)?;
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    let server_config = ServerConfig {
        current_time,
        max_clients: 8,
        protocol_id: super::PROTOCOL_ID,
        public_addresses: vec![public_addr],
        authentication: ServerAuthentication::Unsecure,
    };
    let mut transport = NetcodeServerTransport::new(server_config, socket)?;
    let mut server = RenetServer::new(ConnectionConfig::default());

    tracing::info!(%bind_addr, "serve listening");

    let _guard = terminal::TerminalGuard::enter()?;
    let mut out = stdout();

    let (cols, rows) = crossterm::terminal::size()?;
    let mut fb = Framebuffer::new(cols, rows);
    let (aw, ah) = super::client::arena_size(cols, rows);
    let arena_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xDEAD_BEEF);
    tracing::info!(seed = arena_seed, "arena seed");
    let arena = Arena::generate(arena_seed, aw, ah);
    let content = crate::content::ContentDb::load_core()?;
    tracing::info!(brand = %content.active_brand().id, "content pack loaded");
    let mut game = Game::new(
        arena,
        content,
        super::client::center_offset(cols, rows, aw, ah),
    );

    // The host plays too — add the local player immediately.
    let local_id = game.add_player();
    game.local_id = Some(local_id);

    // Map RenetClient IDs (u64) to in-game player IDs (u32).
    let mut client_to_player: HashMap<ClientId, u32> = HashMap::new();

    let mut input = Input::default();
    let mut last_instant = Instant::now();
    let mut tick_accum = Duration::ZERO;
    let mut snapshot_accum = Duration::ZERO;

    loop {
        let frame_start = Instant::now();
        let dt = last_instant.elapsed();
        last_instant = Instant::now();
        tick_accum += dt;
        snapshot_accum += dt;

        // Transport & protocol update.
        server.update(dt);
        transport.update(dt, &mut server)?;

        // Connection events.
        while let Some(ev) = server.get_event() {
            match ev {
                ServerEvent::ClientConnected { client_id } => {
                    let pid = game.add_player();
                    client_to_player.insert(client_id, pid);
                    let welcome = ServerMsg::Welcome(Welcome {
                        your_id: pid,
                        arena_w: game.arena.width,
                        arena_h: game.arena.height,
                        arena_tiles: game.arena.encode_tiles(),
                        arena_seed,
                    });
                    server.send_message(
                        client_id,
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&welcome),
                    );
                    server.broadcast_message_except(
                        client_id,
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&ServerMsg::PlayerJoined { id: pid }),
                    );
                    tracing::info!(%client_id, pid, "client joined");
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    if let Some(pid) = client_to_player.remove(&client_id) {
                        game.remove_player(pid);
                        server.broadcast_message(
                            DefaultChannel::ReliableOrdered,
                            proto::encode(&ServerMsg::PlayerLeft { id: pid }),
                        );
                    }
                    tracing::info!(%client_id, %reason, "client left");
                }
            }
        }

        // Drain incoming client inputs (Unreliable: movement/aim) and
        // action events (Reliable: interact / cycle weapon).
        let client_ids: Vec<_> = server.clients_id();
        for cid in client_ids.iter().copied() {
            while let Some(msg) = server.receive_message(cid, DefaultChannel::Unreliable) {
                if let Some(ClientMsg::Input(inp)) = proto::decode::<ClientMsg>(&msg) {
                    if let Some(&pid) = client_to_player.get(&cid) {
                        game.set_input(
                            pid,
                            PlayerInput {
                                move_x: inp.move_x,
                                move_y: inp.move_y,
                                aim_x: inp.aim_x,
                                aim_y: inp.aim_y,
                                firing: inp.firing,
                            },
                        );
                    }
                }
            }
            while let Some(msg) = server.receive_message(cid, DefaultChannel::ReliableOrdered) {
                let Some(pid) = client_to_player.get(&cid).copied() else { continue };
                match proto::decode::<ClientMsg>(&msg) {
                    Some(ClientMsg::Interact) => game.try_interact(pid),
                    Some(ClientMsg::CycleWeapon) => game.try_cycle_weapon(pid),
                    _ => {}
                }
            }
        }

        // Local input events (host's keyboard + mouse).
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(k) => {
                    let press =
                        matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat);
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
                    let (aw2, ah2) = super::client::arena_size(w, h);
                    game.set_origin(super::client::center_offset(w, h, aw2, ah2));
                }
                _ => {}
            }
        }

        // Apply host's local input to its own player.
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

        // Sim ticks.
        while tick_accum >= SIM_DT {
            game.tick_authoritative(SIM_DT_SECS);
            tick_accum -= SIM_DT;

            // Reliable events generated this tick.
            for &(x, y, kind, hp) in &game.tick_tile_updates {
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    proto::encode(&ServerMsg::TileUpdate { x, y, kind, hp }),
                );
            }
            for b in &game.tick_blasts {
                let blast = Blast {
                    x: b.x,
                    y: b.y,
                    color: b.color_rgb,
                    seed: b.seed,
                    intensity: b.intensity,
                };
                server.broadcast_message(
                    DefaultChannel::Unreliable,
                    proto::encode(&ServerMsg::Blast(blast)),
                );
            }
        }

        // Snapshot @ 20Hz.
        if snapshot_accum >= SNAPSHOT_PERIOD {
            snapshot_accum = Duration::ZERO;
            let snap = build_snapshot(&game);
            server.broadcast_message(
                DefaultChannel::Unreliable,
                proto::encode(&ServerMsg::Snapshot(snap)),
            );
        }

        transport.send_packets(&mut server);

        // Render the host's own view.
        fb.clear();
        {
            let (tint_color, tint_amount) = game.corruption_tint();
            fb.set_tint(tint_color, tint_amount);
        }
        game.render(&mut fb);
        fb.blit(&mut out)?;
        let hp = game
            .local_player()
            .map(|p| p.hp)
            .unwrap_or(0);
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
        let (tc, tr) = crossterm::terminal::size()?;
        hud::draw_wave_banner(
            &mut out, tc, tr, game.director.wave, game.director.banner_ttl,
        )?;

        if !game.alive {
            let (c2, r2) = crossterm::terminal::size()?;
            hud::draw_gameover(
                &mut out, c2, r2, game.director.wave, game.kills, game.elapsed_secs as u64,
            )?;
            server.broadcast_message(
                DefaultChannel::ReliableOrdered,
                proto::encode(&ServerMsg::RunEnded {
                    wave: game.director.wave,
                    kills: game.kills,
                    elapsed_secs: game.elapsed_secs,
                }),
            );
            transport.send_packets(&mut server);
            super::client::wait_for_any_key()?;
            return Ok(());
        }

        let elapsed = frame_start.elapsed();
        if elapsed < TARGET_FRAME {
            std::thread::sleep(TARGET_FRAME - elapsed);
        }
    }
}

fn build_snapshot(game: &Game) -> Snapshot {
    Snapshot {
        wave: game.director.wave,
        kills: game.kills,
        alive: game.alive,
        elapsed_secs: game.elapsed_secs,
        players: game
            .players
            .iter()
            .map(|p| PlayerSnap {
                id: p.id,
                x: p.x,
                y: p.y,
                hp: p.hp,
                aim_x: p.aim_x,
                aim_y: p.aim_y,
                sanity: p.sanity,
            })
            .collect(),
        enemies: game
            .enemies
            .iter()
            .map(|e| EnemySnap { x: e.x, y: e.y, hp: e.hp, kind: e.archetype.to_kind() })
            .collect(),
        projectiles: game
            .projectiles
            .iter()
            .map(|p| ProjSnap { x: p.x, y: p.y })
            .collect(),
        pickups: game
            .pickups
            .iter()
            .map(|pk| PickupSnap {
                id: pk.id,
                x: pk.x,
                y: pk.y,
                rarity: pk.rarity,
                primitives: pk.primitives.clone(),
            })
            .collect(),
        weapons: game
            .players
            .iter()
            .zip(game.runtimes.iter())
            .map(|(pl, rt)| WeaponSnap {
                player_id: pl.id,
                active_slot: rt.active_slot,
                slot0: rt.weapons[0].as_ref().map(|w| WeaponLoadout {
                    rarity: w.rarity,
                    primitives: w.slots.clone(),
                }),
                slot1: rt.weapons[1].as_ref().map(|w| WeaponLoadout {
                    rarity: w.rarity,
                    primitives: w.slots.clone(),
                }),
            })
            .collect(),
        hitscans: game
            .hitscans
            .iter()
            .map(|h| HitscanSnap {
                from_x: h.from.0,
                from_y: h.from.1,
                to_x: h.to.0,
                to_y: h.to.1,
                ttl: h.ttl,
            })
            .collect(),
        kiosks: game
            .kiosks
            .iter()
            .map(|k| KioskSnap {
                id: k.id,
                x: k.x,
                y: k.y,
                brand_id: k.brand_id.clone(),
                brand_name: k.brand_name.clone(),
                color: [k.brand_color.r, k.brand_color.g, k.brand_color.b],
                votes: k.votes,
            })
            .collect(),
        active_brands: game.active_brands.clone(),
        intermission_phase: match game.director.current_phase() {
            Some(crate::waves::IntermissionPhase::Breathe) => 0,
            Some(crate::waves::IntermissionPhase::Vote) => 1,
            Some(crate::waves::IntermissionPhase::Stock) => 2,
            Some(crate::waves::IntermissionPhase::Warning) => 3,
            None => u8::MAX,
        },
        phase_timer: game.director.phase_timer,
        corruption: game.corruption,
        marked_player_id: game.marked_player_id.unwrap_or(0),
        yellow_signs: game
            .yellow_signs
            .iter()
            .map(|s| SignSnap { id: s.id, x: s.x, y: s.y, ttl: s.ttl, ttl_max: s.ttl_max })
            .collect(),
    }
}
