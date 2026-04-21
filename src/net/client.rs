use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEventKind, MouseButton, MouseEventKind};
use renet::{ConnectionConfig, DefaultChannel, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};
use std::io::stdout;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime};

use crate::arena::Arena;
use crate::audio;
use crate::bindings;
use crate::bindings::router::{
    InputEvent, InputRouter, LastInput, dispatch_menu_nav, dispatch_overlay, resolve_frame_input,
};
use crate::bindings::wizard::{RebindWizard, WizardDevice, WizardOutcome};
use crate::enemy::Enemy;
use crate::fb::Framebuffer;
use crate::game::{DestructionBlast, Game, InputMode};
use crate::gamepad;
use crate::hud;
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

    let loaded_bindings = bindings::Bindings::load().unwrap_or_else(|e| {
        tracing::warn!(error = ?e, "failed to load bindings; using defaults");
        bindings::Bindings::defaults()
    });
    let mut router = InputRouter::new(loaded_bindings);
    let mut wizard: Option<RebindWizard> = None;
    let mut audio_overlay = audio::AudioOverlay::default();
    gamepad::ensure_init();
    let mut last_pad_aim: Option<(f32, f32)> = None;
    let mut last_instant = Instant::now();
    let mut input_accum = Duration::ZERO;
    let mut welcomed = false;
    let mut run_ended_summary: Option<(u32, u32, u64)> = None;
    // Per-client rumble state. We watch the snapshot stream for drops
    // in OUR local player's hp (server is authoritative, so damage is
    // applied server-side and reaches us via snapshot). Fire rumble
    // fires on the press-edge of our own input intent — the server
    // doesn't need to broadcast "you fired" back, since the player's
    // finger already pulled the trigger.
    let mut last_local_hp: Option<i32> = None;
    let mut last_firing: bool = false;

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

        // Damage rumble: diff local player HP between snapshots. This
        // only ever reacts to THIS player's hp — nothing to do with
        // remote teammates, so four connected clients will each rumble
        // only on their own hits without any cross-talk.
        if let Some(p) = game.local_player() {
            if let Some(prev) = last_local_hp {
                let dropped = prev - p.hp;
                if dropped > 0 {
                    gamepad::rumble(gamepad::Rumble::Damage { dmg: dropped });
                }
            }
            last_local_hp = Some(p.hp);
        }

        // Local input events, through the shared bindings router.
        while event::poll(Duration::ZERO)? {
            router.push_crossterm(event::read()?);
        }
        for ev in gamepad::drain_events() {
            router.push_gamepad(ev);
        }
        let pad = gamepad::snapshot();
        router.observe_gamepad_analog(&pad);

        game.input_mode = match router.last_input() {
            LastInput::MouseKeyboard => InputMode::MouseKeyboard,
            LastInput::Gamepad => InputMode::Gamepad,
        };

        if dispatch_client_events(
            &mut router,
            &mut game,
            &mut audio_overlay,
            &mut wizard,
            &mut fb,
            &mut client,
        )? {
            return Ok(());
        }

        // Fire rumble: press-edge of local firing intent. Server is
        // authoritative for whether the shot actually lands / uses
        // ammo, but the player's trigger-pull is local — rumbling on
        // their intent feels tight even with 80ms of network latency.
        // Wizard suppresses rumble so capture buttons can't trigger
        // a buzz mid-rebind.
        let firing_now = wizard.is_none() && (router.firing() || game.mouse.lmb);
        if firing_now && !last_firing {
            gamepad::rumble(gamepad::Rumble::Fire);
        }
        last_firing = firing_now;

        // Tick client-side particles (pure visual) + menu feedback timer.
        game.tick_client(dt.as_secs_f32());
        game.menu.tick(dt.as_secs_f32());

        // Send input ~60 Hz.
        if client.is_connected() && welcomed && input_accum >= INPUT_PERIOD {
            input_accum = Duration::ZERO;
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
            let firing = if wizard.is_some() {
                false
            } else {
                router.firing() || game.mouse.lmb
            };
            let inp = ClientInput {
                move_x: if wizard.is_some() { 0.0 } else { mx },
                move_y: if wizard.is_some() { 0.0 } else { my },
                aim_x: aim.0,
                aim_y: aim.1,
                firing,
            };
            client.send_message(
                DefaultChannel::Unreliable,
                proto::encode(&ClientMsg::Input(inp)),
            );
        }

        transport.send_packets(&mut client).ok();

        // Render. AimZoom hold eases here.
        let aim_held = router.is_held(bindings::Action::AimZoom);
        game.camera.tick_zoom(dt.as_secs_f32().min(0.05), aim_held);
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
            game.menu.render(
                &mut out,
                tc,
                tr,
                game.is_authoritative,
                game.paused,
                game.input_mode == crate::game::InputMode::Gamepad,
            )?;
            if let Some(w) = wizard.as_ref() {
                w.render(&mut out, tc, tr)?;
            }
        } else {
            fb.blit(&mut out)?;
            hud::draw_connecting(&mut out)?;
        }
        // Single flush at end of render pass — HUD overlays used to
        // flush individually which caused mid-frame repaints /
        // flicker. One flush = one atomic terminal update.
        use std::io::Write as _;
        out.flush()?;

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
        ServerMsg::SubstancePaint { x, y, substance_id, state } => {
            game.apply_substance_paint(x, y, substance_id, state);
        }
        ServerMsg::WorldSync { tile_deltas, ground_deltas } => {
            for d in tile_deltas {
                game.apply_tile_update(d.x as i32, d.y as i32, d.kind, d.hp);
            }
            for d in ground_deltas {
                game.apply_substance_paint(d.x as i32, d.y as i32, d.substance_id, d.state);
            }
        }
        ServerMsg::CorpseHit { id, seed, dir_x, dir_y } => {
            game.apply_corpse_hit(id, seed, dir_x, dir_y);
        }
        ServerMsg::BodyReaction { name: _, x: _, y: _, seed: _ } => {
            // Body reactions are host-only today — their effects flow
            // through SubstancePaint / Blast / snapshots. Kept in the
            // proto for future use when a reaction needs a dedicated
            // wire path.
        }
        ServerMsg::PlayerJoined { .. } | ServerMsg::PlayerLeft { .. } => {
            // Player list is driven by the snapshot; nothing to do here yet.
        }
        ServerMsg::RunEnded { wave, kills, elapsed_secs } => {
            *run_ended_summary = Some((wave, kills, elapsed_secs as u64));
        }
        ServerMsg::PickupToast { player_id, text, color } => {
            // Only surface the toast when it's for *this* client's
            // local player — other survivors' grabs aren't ours.
            if game.local_id == Some(player_id) {
                game.push_toast(
                    text,
                    crate::fb::Pixel::rgb(color[0], color[1], color[2]),
                );
            }
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
            // Corpse positions come from the snap; hole state is
            // preserved across merges. Late joiners get the full corpse
            // list in their first snapshot and see any subsequent hits
            // via reliable CorpseHit events.
            game.merge_corpse_snapshot(&s.corpses);
            game.projectiles.clear();
            for ps in s.projectiles {
                game.projectiles.push(Projectile {
                    x: ps.x,
                    y: ps.y,
                    vx: ps.vx,
                    vy: ps.vy,
                    ttl: 0.1,
                    damage: 0,
                    owner_id: 0,
                    primitives: Vec::new(),
                    bounces_left: 0,
                    pierces_left: 0,
                    kind: crate::projectile::ProjectileKind::from_byte(ps.kind),
                    from_ai: false,
                    trail_phase: 0.0,
                });
            }
            game.pickups.clear();
            for ps in s.pickups {
                game.pickups.push(Pickup::new_kind(ps.id, ps.x, ps.y, ps.kind));
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
            // Mirror the authoritative clearing countdown so the
            // "next wave in Xs" strip renders identically on clients.
            game.director.clear_timer = s.clear_timer;
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
                gore_tier: b.gore_tier,
                dir_x: b.dir_x,
                dir_y: b.dir_y,
            };
            game.apply_blast(&blast);
        }
        _ => {}
    }
}

/// Client-side event dispatch. Gameplay verbs (Interact, CycleWeapon,
/// DeployTurret, Traversal) marshal to network packets instead of
/// local method calls because the server owns player state.
fn dispatch_client_events(
    router: &mut InputRouter,
    game: &mut Game,
    audio_overlay: &mut audio::AudioOverlay,
    wizard: &mut Option<RebindWizard>,
    fb: &mut Framebuffer,
    client: &mut RenetClient,
) -> Result<bool> {
    use crate::bindings::Action;
    for ev in router.drain() {
        match ev {
            InputEvent::Quit => return Ok(true),
            InputEvent::MenuToggle => {
                if wizard.is_some() {
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
                        game.camera.adjust_zoom(crate::camera::ZOOM_STEP);
                    }
                    MouseEventKind::ScrollDown => {
                        game.camera.adjust_zoom(1.0 / crate::camera::ZOOM_STEP);
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
                            game.menu.open = true;
                        }
                        WizardOutcome::Cancel => {
                            router.set_capture(None);
                            *wizard = None;
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
                            game.menu.open = true;
                        }
                        WizardOutcome::Cancel => {
                            router.set_capture(None);
                            *wizard = None;
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
                    let menu_action = dispatch_menu_nav(game, action);
                    match menu_action {
                        menu::MenuAction::None | menu::MenuAction::Close => {}
                        menu::MenuAction::Quit => return Ok(true),
                        menu::MenuAction::TogglePause => {
                            // Clients don't own pause — item should
                            // be hidden since is_authoritative is
                            // false, but if the map is stale, warn.
                            game.menu.set_feedback("only the host can pause", 1.5);
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
                                game.menu.set_feedback(
                                    "bindings reset to defaults",
                                    2.0,
                                );
                            }
                        }
                    }
                    continue;
                }
                // Gameplay: gameplay verbs → network packets.
                match action {
                    Action::Interact => client.send_message(
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&ClientMsg::Interact),
                    ),
                    Action::CycleWeapon => client.send_message(
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&ClientMsg::CycleWeapon),
                    ),
                    Action::DeployTurret => client.send_message(
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&ClientMsg::DeployTurret),
                    ),
                    Action::Traversal => client.send_message(
                        DefaultChannel::ReliableOrdered,
                        proto::encode(&ClientMsg::ActivateTraversal),
                    ),
                    _ => {
                        // Overlays are local-only, regardless of net mode.
                        dispatch_overlay(game, audio_overlay, action);
                    }
                }
            }
            InputEvent::ActionRelease(_) => {}
        }
    }
    Ok(false)
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
