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
use crate::menu;
use crate::net::proto::{
    self, Blast, ClientMsg, CorpseSnap, EnemySnap, GroundDeltaMsg, HitscanSnap, KioskSnap,
    PickupSnap, PlayerSnap, ProjSnap, ServerMsg, SignSnap, Snapshot, TileDeltaMsg, WeaponLoadout,
    WeaponSnap, Welcome,
};
use crate::share::{self, ShareCode};
use crate::terminal;

const TARGET_FRAME: Duration = Duration::from_micros(16_667);
const SIM_DT: Duration = Duration::from_micros(33_333);
const SIM_DT_SECS: f32 = 1.0 / 30.0;
const SNAPSHOT_PERIOD: Duration = Duration::from_millis(50); // 20 Hz

pub fn run_serve(port: u16) -> Result<()> {
    let bind_addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let public_addr = bind_addr;
    let http_port = port + 1;

    let session_token = share::new_token()?;
    let lan_ip = detect_lan_ipv4();

    // Phase 4: opportunistic UPnP. Try to open both ports on the LAN
    // gateway before we STUN — so the mapping is in place when a peer
    // probes us. If UPnP refuses / there's no gateway, print clear fall
    // back guidance but keep going.
    let upnp_report = crate::upnp::try_open(lan_ip, port, http_port);
    match (&upnp_report.error, upnp_report.game_opened, upnp_report.http_opened) {
        (_, true, true) => {
            let msg = format!(
                "[upnp] auto-opened UDP {port} + TCP {http_port} on your \
                 router. Friends should be able to connect directly."
            );
            eprintln!("{msg}");
            tracing::info!("{msg}");
        }
        (None, true, false) => {
            let msg = format!(
                "[upnp] opened UDP {port} but not TCP {http_port}. \
                 Direct-connect works; auto-install won't."
            );
            eprintln!("{msg}");
            tracing::warn!("{msg}");
        }
        (None, false, true) => {
            let msg = format!(
                "[upnp] opened TCP {http_port} but not UDP {port}. \
                 Auto-install works; direct-connect needs manual forward."
            );
            eprintln!("{msg}");
            tracing::warn!("{msg}");
        }
        (err, _, _) => {
            let e = err.as_deref().unwrap_or("unknown");
            let msg = format!(
                "[upnp] couldn't auto-forward ports ({e}). Options: \
                 (a) forward UDP {port} + TCP {http_port} manually in \
                 your router, (b) install Tailscale on both ends, or \
                 (c) play LAN-only."
            );
            eprintln!("{msg}");
            tracing::warn!("{msg}");
        }
    }

    // Phase 2: probe for our public endpoint via Google STUN. We bind our
    // probe socket to the same port our game server is about to use so
    // the NAT mapping the STUN server reports matches the one friends
    // will actually reach. If STUN fails or reports CG-NAT, fall back to
    // LAN IP and print a clear warning.
    let public_ip = match crate::stun::public_endpoint(
        crate::stun::GOOGLE_STUN,
        0, // ephemeral; game socket binds separately below
        std::time::Duration::from_secs(3),
    ) {
        Ok(endpoint) => {
            let ip = match endpoint.ip() {
                std::net::IpAddr::V4(ip) => ip,
                _ => {
                    tracing::warn!("STUN returned non-IPv4 address; using LAN IP");
                    eprintln!(
                        "[nat] STUN returned a non-IPv4 endpoint; using LAN IP {lan_ip}."
                    );
                    lan_ip
                }
            };
            if crate::stun::is_cgnat(ip) {
                let msg = format!(
                    "[nat] Your ISP uses carrier-grade NAT (public IP {ip}). \
                     Direct connections won't work; use Tailscale or play \
                     LAN-only. Share code will carry LAN IP {lan_ip}."
                );
                eprintln!("{msg}");
                tracing::warn!("{msg}");
                lan_ip
            } else {
                let msg = format!(
                    "[nat] STUN reports public IP {ip}. Friends can paste \
                     the share code from the ESC menu. Port {port} (UDP) \
                     must be forwarded on your router."
                );
                eprintln!("{msg}");
                tracing::info!("{msg}");
                ip
            }
        }
        Err(e) => {
            let msg = format!(
                "[nat] STUN probe failed ({e}). Share code will carry LAN \
                 IP {lan_ip}; only LAN / Tailscale peers will reach you."
            );
            eprintln!("{msg}");
            tracing::warn!("{msg}");
            lan_ip
        }
    };

    let share_code = ShareCode::new(public_ip, port, http_port, session_token);
    let share_msg = format!("[share] {}", share_code.connect_command());
    eprintln!("{share_msg}");
    tracing::info!("{share_msg}");

    // Phase 3: HTTP install server for friends without the binary. Binds
    // the same http_port we advertise in the share code. Runs on its own
    // thread until the process exits.
    let install = match crate::install_server::start(http_port, session_token, &share_code) {
        Ok(s) => {
            let m1 = format!(
                "[install] serving auto-install scripts on http://{}:{}",
                share_code.ip, http_port
            );
            let m2 = format!("[install] bash:       {}", s.bash_one_liner);
            let m3 = format!("[install] powershell: {}", s.pwsh_one_liner);
            eprintln!("{m1}");
            eprintln!("{m2}");
            eprintln!("{m3}");
            tracing::info!("{m1}");
            tracing::info!("{m2}");
            tracing::info!("{m3}");
            Some(s)
        }
        Err(e) => {
            let msg = format!(
                "[install] couldn't start HTTP install server on port \
                 {http_port}: {e}. Direct-connect share strings still work."
            );
            eprintln!("{msg}");
            tracing::warn!("{msg}");
            None
        }
    };

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
    let (aw, ah) = super::client::world_size();
    let arena_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xDEAD_BEEF);
    tracing::info!(seed = arena_seed, "arena seed");
    let arena = Arena::generate(arena_seed, aw, ah);
    let content = crate::content::ContentDb::load_core()?;
    tracing::info!(brand = %content.active_brand().id, "content pack loaded");
    let mut game = Game::new(arena, content, cols, rows);
    game.session_token = session_token;
    game.share_code = Some(share_code.clone());
    if let Some(srv) = install.as_ref() {
        game.install_one_liner = Some(format!(
            "# Bash / zsh:\n{}\n\n# PowerShell:\n{}",
            srv.bash_one_liner, srv.pwsh_one_liner
        ));
    }

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
                    // Validate session token from client user_data. If it
                    // doesn't match, immediately disconnect — blocks
                    // strangers who might guess our address but can't
                    // possibly have the token.
                    let user_data = transport.user_data(client_id);
                    let ok = match user_data {
                        Some(d) => d[..16] == session_token,
                        None => false,
                    };
                    if !ok {
                        tracing::warn!(%client_id, "rejected: bad session token");
                        server.disconnect(client_id);
                        continue;
                    }
                    let pid = game.add_player();
                    client_to_player.insert(client_id, pid);
                    let welcome = ServerMsg::Welcome(Welcome {
                        your_id: pid,
                        arena_w: game.arena.width,
                        arena_h: game.arena.height,
                        arena_seed,
                    });
                    server.send_message(
                        client_id,
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&welcome),
                    );
                    // World-state catch-up: every tile that's diverged
                    // from the seed-regenerated pristine arena. Keeps
                    // late joiners in sync with the accumulated damage
                    // + ground decals the other survivors have made.
                    let (structs, grounds) = game.arena.diff_from_seed(arena_seed);
                    let tile_deltas: Vec<TileDeltaMsg> = structs
                        .into_iter()
                        .map(|d| TileDeltaMsg { x: d.x, y: d.y, kind: d.kind, hp: d.hp })
                        .collect();
                    let ground_deltas: Vec<GroundDeltaMsg> = grounds
                        .into_iter()
                        .map(|d| GroundDeltaMsg {
                            x: d.x,
                            y: d.y,
                            substance_id: d.substance_id,
                            state: d.state,
                        })
                        .collect();
                    tracing::info!(
                        %client_id,
                        tile = tile_deltas.len(),
                        ground = ground_deltas.len(),
                        "WorldSync"
                    );
                    server.send_message(
                        client_id,
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&ServerMsg::WorldSync { tile_deltas, ground_deltas }),
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
                    Some(ClientMsg::DeployTurret) => game.try_deploy_turret(pid),
                    Some(ClientMsg::ActivateTraversal) => game.try_activate_traversal(pid),
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
                    if let KeyCode::F(3) = k.code {
                        if press && !matches!(k.kind, KeyEventKind::Repeat) {
                            game.perf_overlay.toggle();
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
                                        let msg = if game.paused { "paused (all peers frozen)" } else { "resumed" };
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

        // Sim ticks. Menu UI is overlay-only; host sim keeps running so
        // remote players don't freeze just because the host hit Esc.
        while tick_accum >= SIM_DT {
            game.tick_authoritative(SIM_DT_SECS);
            game.menu.tick(SIM_DT_SECS);
            tick_accum -= SIM_DT;

            // Reliable events generated this tick.
            for &(x, y, kind, hp) in &game.tick_tile_updates {
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    proto::encode(&ServerMsg::TileUpdate { x, y, kind, hp }),
                );
            }
            for &(x, y, substance_id, state) in &game.tick_ground_paints {
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    proto::encode(&ServerMsg::SubstancePaint { x, y, substance_id, state }),
                );
            }
            for &(id, seed, dir_x, dir_y) in &game.tick_corpse_hits {
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    proto::encode(&ServerMsg::CorpseHit { id, seed, dir_x, dir_y }),
                );
            }
            for (name, x, y, seed) in &game.tick_body_reactions {
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    proto::encode(&ServerMsg::BodyReaction {
                        name: name.clone(),
                        x: *x,
                        y: *y,
                        seed: *seed,
                    }),
                );
            }
            // Toast events — route each to its owning client so the
            // HUD's pickup notifications are per-player. Host's own
            // player toasts are already pushed directly to
            // `game.toasts` in `consume_pickup`; skip those here.
            for (player_id, text, color) in &game.tick_toast_events {
                let host_pid = game.local_id;
                if host_pid == Some(*player_id) {
                    continue;
                }
                let Some(&cid) = client_to_player
                    .iter()
                    .find(|(_, pid)| **pid == *player_id)
                    .map(|(cid, _)| cid)
                else {
                    continue;
                };
                server.send_message(
                    cid,
                    DefaultChannel::ReliableOrdered,
                    proto::encode(&ServerMsg::PickupToast {
                        player_id: *player_id,
                        text: text.clone(),
                        color: [color.r, color.g, color.b],
                    }),
                );
            }
            for b in &game.tick_blasts {
                let blast = Blast {
                    x: b.x,
                    y: b.y,
                    color: b.color_rgb,
                    seed: b.seed,
                    intensity: b.intensity,
                    gore_tier: b.gore_tier,
                    dir_x: b.dir_x,
                    dir_y: b.dir_y,
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
        game.update_camera_follow();
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
        hud::draw_active_brands(&mut out, &game.active_brands)?;
        if let Some(p) = game.local_player() {
            hud::draw_perks(&mut out, &p.perks)?;
        }
        hud::draw_kiosk_labels(&mut out, &game)?;
        let (tc, tr) = crossterm::terminal::size()?;
        hud::draw_zoom_indicator(&mut out, tc, game.camera.zoom)?;
        hud::draw_wave_banner(
            &mut out, tc, tr, game.director.wave, game.director.banner_ttl,
        )?;
        hud::draw_clear_countdown(&mut out, tc, game.director.clear_timer)?;
        hud::draw_pickup_labels(&mut out, &game)?;
        hud::draw_pickup_toasts(&mut out, tc, &game.toasts)?;
        if game.paused {
            hud::draw_paused_banner(&mut out, tc, tr, game.is_authoritative)?;
        }
        if game.alive
            && game.local_player().map(|p| p.hp <= 0).unwrap_or(false)
        {
            hud::draw_dead_banner(&mut out, tc, tr)?;
        }
        game.perf_overlay.render(&mut out, tc, tr, &game.perf)?;
        if game.inventory_open {
            if let Some(p) = game.local_player() {
                let loadout = game.local_loadout();
                hud::draw_inventory(&mut out, tc, tr, p, loadout.as_ref())?;
            }
        }
        game.console.render(&mut out, tc, tr)?;
        game.menu.render(&mut out, tc, tr, game.is_authoritative, game.paused)?;

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

/// Best-effort LAN IPv4 discovery: opens a UDP socket aimed at a public
/// address to let the OS pick the outbound source IP, then returns that.
/// Never actually sends a packet. Falls back to 127.0.0.1 if detection
/// fails — phase 2 STUN replaces this for internet share codes.
fn detect_lan_ipv4() -> std::net::Ipv4Addr {
    use std::net::{SocketAddr, UdpSocket};
    let probe = || -> Option<std::net::Ipv4Addr> {
        let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
        sock.connect("8.8.8.8:80").ok()?;
        match sock.local_addr().ok()? {
            SocketAddr::V4(v4) => Some(*v4.ip()),
            _ => None,
        }
    };
    probe().unwrap_or(std::net::Ipv4Addr::LOCALHOST)
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
        corpses: game
            .corpses
            .iter()
            .map(|c| CorpseSnap {
                id: c.id,
                x: c.x,
                y: c.y,
                kind: c.archetype.to_kind(),
                hp: c.hp,
            })
            .collect(),
        projectiles: game
            .projectiles
            .iter()
            .map(|p| ProjSnap {
                x: p.x,
                y: p.y,
                vx: p.vx,
                vy: p.vy,
                kind: p.kind.to_byte(),
            })
            .collect(),
        pickups: game
            .pickups
            .iter()
            .map(|pk| PickupSnap {
                id: pk.id,
                x: pk.x,
                y: pk.y,
                kind: pk.kind.clone(),
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
                    mode: w.mode,
                }),
                slot1: rt.weapons[1].as_ref().map(|w| WeaponLoadout {
                    rarity: w.rarity,
                    primitives: w.slots.clone(),
                    mode: w.mode,
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
        clear_timer: game.director.clear_timer,
        corruption: game.corruption,
        marked_player_id: game.marked_player_id.unwrap_or(0),
        yellow_signs: game
            .yellow_signs
            .iter()
            .map(|s| SignSnap { id: s.id, x: s.x, y: s.y, ttl: s.ttl, ttl_max: s.ttl_max })
            .collect(),
        paused: game.paused,
    }
}
