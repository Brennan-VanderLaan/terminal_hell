use crate::arena::{Arena, Material, Object, Tile};
use crate::body_effect::ReactionRegistry;
use crate::camera::Camera;
use crate::carcosa::{Phantom, YellowSign};
use crate::content::ContentDb;
use crate::corpse::Corpse;
use crate::enemy::{Archetype, Enemy};
use crate::fb::{Framebuffer, Pixel};
use crate::interaction::Reaction;
use crate::mouse::Mouse;
use crate::particle::{self, Particle};
use crate::pickup::Pickup;
use crate::player::Player;
use crate::primitive::{Primitive, Rarity};
use crate::projectile::{Projectile, ProjectileOutcome};
use crate::substance::SubstanceId;
use crate::tag::Tag;
use crate::vote::VoteKiosk;
use crate::waves::{WaveDirector, WaveEvent};
use crate::weapon::{FireMode, Weapon};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

/// Last-used input device for the local player. Flips on any
/// mouse/keyboard or gamepad activity. Drives UI glyph selection
/// (menu footer, pickup hints) and the camera look-ahead source.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    MouseKeyboard,
    Gamepad,
}

/// Per-player state the host tracks to drive sim from incoming inputs.
pub struct PlayerInput {
    pub move_x: f32,
    pub move_y: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub firing: bool,
}

pub struct PlayerRuntime {
    pub weapons: [Option<Weapon>; 2],
    pub active_slot: u8,
    pub input: PlayerInput,
}

impl PlayerRuntime {
    pub fn active_weapon(&self) -> Option<&Weapon> {
        self.weapons[self.active_slot as usize].as_ref()
    }
    pub fn active_weapon_mut(&mut self) -> Option<&mut Weapon> {
        self.weapons[self.active_slot as usize].as_mut()
    }
    pub fn install_active(&mut self, w: Weapon) {
        self.weapons[self.active_slot as usize] = Some(w);
    }
    /// Rotate active_slot to the next non-empty slot. No-op if no other slot
    /// carries a weapon.
    pub fn cycle(&mut self) {
        let count = self.weapons.len() as u8;
        for _ in 0..count {
            self.active_slot = (self.active_slot + 1) % count;
            if self.weapons[self.active_slot as usize].is_some() {
                return;
            }
        }
    }
}

pub struct DestructionBlast {
    pub x: f32,
    pub y: f32,
    pub color_rgb: [u8; 3],
    pub seed: u64,
    pub intensity: u8, // 0=small hit, 1=wall destroyed, 2=enemy killed
    /// Gore tier: 0 standard, 1 heavy, 2 extreme, 3 corpse_hit. Zero
    /// for wall destruction / non-gore blasts. Enemy-kill paths set
    /// this from the dying archetype's `gore_profile`.
    pub gore_tier: u8,
    /// Impact direction (unit vector). When non-zero, the gore
    /// spawner biases particles into a forward cone along this
    /// direction — used for corpse-hit bullet spray. Zero for
    /// omnidirectional bursts (enemy deaths, wall breaks).
    pub dir_x: f32,
    pub dir_y: f32,
}

/// Four-phase cinematic run-end sequence. Begins the frame the last
/// living player drops and plays out over ~9 seconds before the
/// death-report overlay accepts input. Keeps buffered keys from
/// accidentally dismissing the "you lost" moment.
///
/// Every phase tickers down through `tick_death_phase` which rolls
/// it forward on timer expiry. `Report` has no auto-advance; the
/// outer loop watches for a fresh key press (with a minimum elapsed
/// time since entry) before exiting the run.
#[derive(Clone, Debug)]
pub enum DeathPhase {
    /// Enemies keep doing what they were doing — including attacking
    /// the player's body. Corpse-hit system fires normally; a big
    /// gore burst lands on entry; small pops continue on a cadence.
    Dying { ttl: f32 },
    /// All enemies freeze in place and "praise Hastur" — aim
    /// upward, occasional gold particle pops. The moment the horde
    /// realizes it won.
    PraiseHastur { ttl: f32 },
    /// World tint ramps from the current corruption value to full
    /// saturated gold. Enemies stay frozen. The yellow becomes the
    /// whole screen, solidifying.
    Goldening { ttl: f32 },
    /// Waiting on a keypress to exit. `input_armed_at` is the
    /// scenario-elapsed time at which keys will start being
    /// accepted — gives the screen time to settle before the
    /// earliest "any key" press counts.
    Report { input_armed: bool },
}

impl DeathPhase {
    pub fn label(&self) -> &'static str {
        match self {
            DeathPhase::Dying { .. } => "dying",
            DeathPhase::PraiseHastur { .. } => "praise",
            DeathPhase::Goldening { .. } => "gold",
            DeathPhase::Report { .. } => "report",
        }
    }

    /// True if gameplay should be frozen (enemies + projectiles +
    /// AI all paused). The cinematic phases after Dying are static
    /// tableau; Report is fully paused.
    pub fn freezes_sim(&self) -> bool {
        matches!(
            self,
            DeathPhase::PraiseHastur { .. }
                | DeathPhase::Goldening { .. }
                | DeathPhase::Report { .. }
        )
    }
}

/// Short-lived pickup notification. Newest entries slide in from
/// the top and decay over `ttl_max` seconds. Drawn as a HUD stack
/// above the perk/loadout strips. Purely client-visual — never
/// synced over the wire.
pub struct PickupToast {
    pub text: String,
    pub color: Pixel,
    pub ttl: f32,
    pub ttl_max: f32,
}

/// One line of the Counter-Strike-style kill feed rendered top-right.
/// `<killer>` did something to `<victim>` with `<weapon>`. When either
/// side is the local player, the renderer tints the line so it stands
/// out from teammates' kills in multiplayer.
#[derive(Clone, Debug)]
pub struct KillFeedEntry {
    pub killer: String,
    pub weapon: String,
    pub victim: String,
    pub ttl: f32,
    pub ttl_max: f32,
    /// True when the local player is the killer — used by the
    /// renderer to highlight your own kills.
    pub local_killer: bool,
    /// True when the local player is the victim — highlights in the
    /// "you got hit" color.
    pub local_victim: bool,
}

impl KillFeedEntry {
    /// Default TTL — long enough to read at a glance, short enough
    /// that the feed doesn't pile up in a busy wave.
    pub const DEFAULT_TTL: f32 = 5.0;
    /// Cap on entries kept in the list. Older ones get trimmed when
    /// a new kill lands to avoid unbounded growth during a horde.
    pub const MAX_ENTRIES: usize = 6;
}

/// Flat HUD-facing view of a player's weapon inventory.
pub struct LocalLoadoutView {
    pub active_slot: u8,
    pub slots: [Option<(Rarity, Vec<Primitive>, FireMode)>; 2],
}

/// Transient tracer for enemy hitscan shots. Lives ~0.12s as visual feedback.
#[derive(Clone, Debug)]
pub struct HitscanTrace {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub ttl: f32,
}

/// Audio-memory event: a loud thing happened at this spot. Enemies
/// without a player target use these to investigate — they walk
/// toward the noise for `ttl` seconds or until it expires. Decays
/// each tick; never replayed.
#[derive(Clone, Copy, Debug)]
pub struct NoiseEvent {
    pub x: f32,
    pub y: f32,
    /// Hearing radius — enemies within this distance "hear" it.
    pub radius: f32,
    pub ttl: f32,
}

pub struct Game {
    pub arena: Arena,
    pub content: ContentDb,
    pub players: Vec<Player>,
    pub runtimes: Vec<PlayerRuntime>,
    pub projectiles: Vec<Projectile>,
    pub particles: Vec<Particle>,
    pub enemies: Vec<Enemy>,
    pub pickups: Vec<Pickup>,
    pub kiosks: Vec<VoteKiosk>,
    /// Brand IDs currently bleeding into the wave pool, in vote order.
    pub active_brands: Vec<String>,
    pub director: WaveDirector,
    pub mouse: Mouse,
    pub kills: u32,
    pub alive: bool,
    pub elapsed_secs: f32,
    /// Arena-wide Carcosa encroachment. 0..100+, never regresses past peak.
    pub corruption: f32,
    pub peak_corruption: f32,
    /// Hastur's gaze. Corruption ≥ 50 rotates the mark through living
    /// players every 45s — marked players take +25% damage and draw AI
    /// priority.
    pub marked_player_id: Option<u32>,
    pub mark_rotate_timer: f32,
    /// Visible Yellow Sign manifestations; server-authoritative.
    pub yellow_signs: Vec<YellowSign>,
    /// Hastur Daemon: countdown until the next notice event is scheduled.
    pub daemon_timer: f32,
    /// Incrementing ID for newly-manifested signs.
    next_sign_id: u32,
    /// Client-side phantom enemies (pure hallucination; never synced).
    pub phantoms: Vec<Phantom>,
    /// Rendering camera owned by Game so every render path shares it. Sim
    /// never reads from this.
    pub camera: Camera,
    /// Session token used for netcode user_data validation and (phase 3)
    /// HMAC binary signing. Random per `serve`, blank in solo.
    pub session_token: [u8; 16],
    /// Share-code advertised to friends. None until the serve loop (or
    /// STUN probe in phase 2) fills it in.
    pub share_code: Option<crate::share::ShareCode>,
    /// Install one-liner ready for clipboard (populated when the HTTP
    /// install server is up — phase 3). None until then.
    pub install_one_liner: Option<String>,
    pub menu: crate::menu::Menu,
    pub console: crate::console::Console,
    pub perf_overlay: crate::perf_overlay::PerfOverlay,
    /// Tab toggles the inventory overlay (perks + loadout + counts).
    pub inventory_open: bool,
    /// F4 toggles a spatial-grid overlay on top of the arena. Cells
    /// with >0 enemies get a faint border; color scales with cell
    /// population so clusters pop visually. Pure diagnostic — helps
    /// verify cell_size sanity + spot hot spots in the separation
    /// pass.
    pub debug_spatial_grid: bool,
    /// Death-cinematic state machine. `None` during normal play;
    /// ratchets through Dying → PraiseHastur → Goldening → Report
    /// once every living player has dropped. See `DeathPhase` for
    /// per-phase semantics.
    pub death_phase: Option<DeathPhase>,
    /// Wall-clock seconds at which the current `DeathPhase` started.
    /// Drives the Report phase's input-gate — we only accept the
    /// "dismiss" keypress once the cinematic has had time to play.
    pub death_started_at: f32,
    /// Seconds accumulated inside the Report phase. Ticks even when
    /// `elapsed_secs` is frozen (which it is, once the cinematic
    /// begins) so the input gate still arms after a real 1.2s.
    pub death_report_elapsed: f32,
    /// Transient "you just grabbed X" toasts. Pushed by the pickup
    /// consume path; ticked every frame, drawn as a HUD strip.
    pub toasts: Vec<PickupToast>,
    /// Counter-Strike-style kill feed, stacked top-right. Each entry
    /// is rendered as `<killer> [weapon] <victim>` for the duration
    /// of its `ttl`. Pushed on player kills (in the kills_this_tick
    /// loop) and on player deaths (in the melee damage site that
    /// flips `player.hp` to zero).
    pub kill_feed: Vec<KillFeedEntry>,
    /// Accumulator for the fire-propagation tick. Fire doesn't need
    /// sim-rate propagation — every 0.3s is plenty for the cascade
    /// to read as "the oil caught." This accumulates dt between
    /// frames and fires the spread pass on crossover.
    pub fire_tick_accum: f32,
    /// True when this process owns the authoritative sim (solo or host).
    /// Clients of a remote server run with this false; the menu uses it
    /// to hide host-only controls (like the pause toggle) from clients.
    pub is_authoritative: bool,
    /// When true, authoritative ticks + client-side particle ticks are
    /// skipped. Synced via snapshot so clients freeze in step with the
    /// host.
    pub paused: bool,
    pub local_id: Option<u32>,
    /// Which input device was most-recently used by the local player.
    /// Drives UI glyph selection and the camera look-ahead source
    /// (mouse cursor vs. right-stick aim).
    pub input_mode: InputMode,
    /// Last-known gamepad aim direction (unit vector). Populated by
    /// the solo loop each frame when `input_mode == Gamepad`; used by
    /// the camera to nudge look-ahead and to set the player's aim
    /// when the right stick is centered (coast).
    pub gamepad_aim: Option<(f32, f32)>,
    seed_counter: u64,
    next_player_id: u32,
    next_pickup_id: u32,
    next_kiosk_id: u32,
    loot_rng: SmallRng,
    kiosk_rng: SmallRng,
    /// Per-loop profiler. Instrument hot sections via
    /// `self.perf.begin/end`; once per second the rolling window
    /// dumps to tracing so the backtick log console shows what's
    /// eating frame time.
    pub perf: crate::telemetry::FrameProfile,
    pub tick_tile_updates: Vec<(i32, i32, u8, u8)>,
    /// Substance paint events generated this tick: (x, y, substance_id,
    /// state). Broadcast reliable. See ServerMsg::SubstancePaint.
    pub tick_ground_paints: Vec<(i32, i32, u16, u8)>,
    /// Corpse hits generated this tick: (corpse_id, seed). Broadcast reliable.
    /// Corpse hits queued this tick for reliable broadcast:
    /// `(corpse_id, seed, dir_x, dir_y)`. Direction is the bullet's
    /// unit motion vector — clients apply a matching forward-cone
    /// spray so the gore looks the same on every peer.
    pub tick_corpse_hits: Vec<(u32, u64, f32, f32)>,
    pub tick_blasts: Vec<DestructionBlast>,
    pub hitscans: Vec<HitscanTrace>,
    /// Active noise events drawing enemy attention. Populated on
    /// loud world events (wall break, rocket detonation, player
    /// gunfire); decayed each tick.
    pub noise_events: Vec<NoiseEvent>,
    /// Persistent corpse entities. Host owns hp + hole state; clients
    /// mirror positions from snapshots and synthesize holes from
    /// reliable CorpseHit events.
    pub corpses: Vec<Corpse>,
    next_corpse_id: u32,
    /// Body-on-death + body-on-interaction reaction lookup. Built-ins
    /// install at startup; future pack / plugin code can append.
    pub reactions: ReactionRegistry,
    /// Tag-driven interaction rules (OnDeath, OnContact, ...). Any
    /// module can append rules; firing a body event queries this book
    /// for every tag the body carries.
    pub interactions: crate::interaction::InteractionBook,
    /// Body-effect events generated this tick for reliable broadcast.
    /// `(reaction_name, x, y, seed)`. Currently host-only; the wire
    /// path stays reserved for future use.
    pub tick_body_reactions: Vec<(String, f32, f32, u64)>,
    /// Per-player pickup events this tick. Server drains these and
    /// either pushes to its own `toasts` (if the target is the host
    /// player) or relays them over the reliable-ordered channel to
    /// the right client. Format: `(player_id, text, color)`.
    pub tick_toast_events: Vec<(u32, String, Pixel)>,
    /// True until the first successful vote resolution. While true,
    /// the vote *replaces* the initial default brand instead of
    /// stacking on top of it — otherwise the default would persist
    /// forever beneath whatever the players voted for.
    pub default_brand_active: bool,
    /// Set by the client from incoming snapshots so the HUD can render the
    /// local player's loadout even though the client doesn't run the
    /// authoritative weapon sim.
    pub remote_weapons: Vec<crate::net::proto::WeaponSnap>,
    /// Mirror of the server's intermission state on the client side; used
    /// for HUD rendering without running the wave director locally.
    pub client_phase: Option<crate::waves::IntermissionPhase>,
    pub client_phase_timer: f32,
}

impl Game {
    pub fn new(arena: Arena, content: ContentDb, cols: u16, rows: u16) -> Self {
        let default_brand = content.default_brand.clone();
        let mut camera = Camera::new(cols, rows);
        camera.center = (arena.width as f32 / 2.0, arena.height as f32 / 2.0);
        Self {
            arena,
            content,
            players: Vec::new(),
            runtimes: Vec::new(),
            projectiles: Vec::with_capacity(128),
            particles: Vec::with_capacity(1024),
            enemies: Vec::with_capacity(64),
            pickups: Vec::with_capacity(16),
            kiosks: Vec::new(),
            active_brands: vec![default_brand],
            director: WaveDirector::new(0xC0FFEE),
            mouse: Mouse::default(),
            kills: 0,
            alive: true,
            elapsed_secs: 0.0,
            corruption: 0.0,
            peak_corruption: 0.0,
            marked_player_id: None,
            mark_rotate_timer: 0.0,
            yellow_signs: Vec::new(),
            daemon_timer: 30.0,
            next_sign_id: 1,
            phantoms: Vec::new(),
            camera,
            session_token: [0u8; 16],
            share_code: None,
            install_one_liner: None,
            menu: crate::menu::Menu::default(),
            console: crate::console::Console::default(),
            perf_overlay: crate::perf_overlay::PerfOverlay::default(),
            inventory_open: false,
            debug_spatial_grid: false,
            death_phase: None,
            death_started_at: 0.0,
            death_report_elapsed: 0.0,
            toasts: Vec::new(),
            kill_feed: Vec::new(),
            fire_tick_accum: 0.0,
            is_authoritative: true,
            paused: false,
            local_id: None,
            input_mode: InputMode::MouseKeyboard,
            gamepad_aim: None,
            seed_counter: 0x9E3779B97F4A7C15,
            next_player_id: 1,
            next_pickup_id: 1,
            next_kiosk_id: 1,
            loot_rng: SmallRng::seed_from_u64(0xBAD_F00D),
            kiosk_rng: SmallRng::seed_from_u64(0xCAFE_BABE),
            perf: crate::telemetry::FrameProfile::new(),
            tick_tile_updates: Vec::new(),
            tick_ground_paints: Vec::new(),
            tick_corpse_hits: Vec::new(),
            tick_blasts: Vec::new(),
            hitscans: Vec::new(),
            noise_events: Vec::new(),
            corpses: Vec::new(),
            next_corpse_id: 1,
            reactions: ReactionRegistry::with_builtins(),
            interactions: crate::interaction::InteractionBook::with_builtins(),
            tick_body_reactions: Vec::new(),
            tick_toast_events: Vec::new(),
            default_brand_active: true,
            remote_weapons: Vec::new(),
            client_phase: None,
            client_phase_timer: 0.0,
        }
    }

    /// Intermission state for HUD display. Host pulls from director; client
    /// pulls from its last snapshot.
    pub fn display_phase(&self) -> (Option<crate::waves::IntermissionPhase>, f32) {
        if let Some(p) = self.director.current_phase() {
            (Some(p), self.director.phase_timer)
        } else if self.client_phase.is_some() {
            (self.client_phase, self.client_phase_timer)
        } else {
            (None, 0.0)
        }
    }

    /// Build a HUD-ready loadout line for the local player. For the host
    /// this reads the authoritative runtime; for a client it reads the most
    /// recent snapshot.
    pub fn local_loadout(&self) -> Option<LocalLoadoutView> {
        let id = self.local_id?;
        // Prefer host-side runtime when it exists.
        if let Some(i) = self.players.iter().position(|p| p.id == id) {
            if let Some(rt) = self.runtimes.get(i) {
                if !rt.weapons.iter().all(|w| w.is_none()) {
                    return Some(LocalLoadoutView {
                        active_slot: rt.active_slot,
                        slots: [
                            rt.weapons[0].as_ref().map(|w| (w.rarity, w.slots.clone(), w.mode)),
                            rt.weapons[1].as_ref().map(|w| (w.rarity, w.slots.clone(), w.mode)),
                        ],
                    });
                }
            }
        }
        // Fall back to snapshot data on client-only peers.
        let snap = self.remote_weapons.iter().find(|w| w.player_id == id)?;
        Some(LocalLoadoutView {
            active_slot: snap.active_slot,
            slots: [
                snap.slot0.as_ref().map(|l| (l.rarity, l.primitives.clone(), l.mode)),
                snap.slot1.as_ref().map(|l| (l.rarity, l.primitives.clone(), l.mode)),
            ],
        })
    }

    pub fn resize_viewport(&mut self, cols: u16, rows: u16) {
        self.camera.resize(cols, rows);
    }

    pub fn add_player(&mut self) -> u32 {
        let id = self.next_player_id;
        self.next_player_id += 1;
        let x = self.arena.width as f32 / 2.0;
        let y = self.arena.height as f32 / 2.0;
        self.players.push(Player::new(id, x, y));
        self.runtimes.push(PlayerRuntime {
            weapons: [Some(Weapon::pulse()), None],
            active_slot: 0,
            input: PlayerInput { move_x: 0.0, move_y: 0.0, aim_x: 1.0, aim_y: 0.0, firing: false },
        });
        id
    }

    /// Attempt to grab the nearest pickup OR vote at the nearest kiosk.
    /// Called on interact keypress (E) events. Picks the closer of the two
    /// when both are in range.
    pub fn try_interact(&mut self, player_id: u32) {
        let Some(pi) = self.player_index(player_id) else {
            return;
        };
        let (px, py) = (self.players[pi].x, self.players[pi].y);

        let pickup_r2 = Pickup::pickup_radius() * Pickup::pickup_radius();
        let mut best_pickup: Option<(usize, f32)> = None;
        for (i, pk) in self.pickups.iter().enumerate() {
            let dx = pk.x - px;
            let dy = pk.y - py;
            let d2 = dx * dx + dy * dy;
            if d2 <= pickup_r2 && best_pickup.map_or(true, |(_, b)| d2 < b) {
                best_pickup = Some((i, d2));
            }
        }

        let kiosk_r2 = VoteKiosk::interact_radius() * VoteKiosk::interact_radius();
        let mut best_kiosk: Option<(usize, f32)> = None;
        for (i, k) in self.kiosks.iter().enumerate() {
            let dx = k.x - px;
            let dy = k.y - py;
            let d2 = dx * dx + dy * dy;
            if d2 <= kiosk_r2 && best_kiosk.map_or(true, |(_, b)| d2 < b) {
                best_kiosk = Some((i, d2));
            }
        }

        match (best_pickup, best_kiosk) {
            (Some((pi_idx, pd)), Some((_, kd))) if pd <= kd => {
                self.consume_pickup(pi, pi_idx);
            }
            (Some((pi_idx, _)), None) => {
                self.consume_pickup(pi, pi_idx);
            }
            (_, Some((ki_idx, _))) => {
                self.register_vote(player_id, ki_idx);
            }
            _ => {}
        }
    }

    fn consume_pickup(&mut self, pi: usize, pickup_idx: usize) {
        let pickup = self.pickups.swap_remove(pickup_idx);
        let toast_text = pickup.kind.label();
        let toast_color = pickup.kind.halo_color();
        // Only push toasts for the local player — remotes shouldn't
        // see everyone else's pickup spam on their own HUD.
        let is_local = self.local_id == Some(self.players[pi].id);
        match &pickup.kind {
            crate::pickup::PickupKind::Weapon { .. } => {
                if let Some(weapon) = pickup.into_weapon() {
                    self.runtimes[pi].install_active(weapon);
                }
            }
            crate::pickup::PickupKind::MedKit { heal } => {
                let p = &mut self.players[pi];
                p.hp = (p.hp + heal).min(crate::player::PLAYER_MAX_HP);
            }
            crate::pickup::PickupKind::SanityDose { restore } => {
                let p = &mut self.players[pi];
                p.sanity = (p.sanity + restore).min(100.0);
            }
            crate::pickup::PickupKind::ArmorPlate { absorb } => {
                let p = &mut self.players[pi];
                p.armor = (p.armor + absorb).min(p.max_armor());
            }
            crate::pickup::PickupKind::TurretKit => {
                let p = &mut self.players[pi];
                p.turret_kits = p.turret_kits.saturating_add(1);
            }
            crate::pickup::PickupKind::Perk { perk } => {
                let p = &mut self.players[pi];
                if !p.perks.contains(perk) {
                    p.perks.push(*perk);
                    tracing::info!(perk = ?perk, player = p.id, "perk acquired");
                }
            }
            crate::pickup::PickupKind::Traversal { verb } => {
                let p = &mut self.players[pi];
                p.traversal = Some(*verb);
                p.traversal_cooldown = 0.0;
                tracing::info!(verb = ?verb, player = p.id, "traversal verb acquired");
            }
        }
        let pid = self.players[pi].id;
        if is_local {
            self.push_toast(toast_text.clone(), toast_color);
        }
        // Queue a per-player toast event so the server loop can
        // relay it to the matching client (or drop it if the pickup
        // belongs to the host's own player — already handled above).
        self.tick_toast_events
            .push((pid, toast_text, toast_color));
    }

    /// Auto-pickup sweep — called once per tick. Any pickup whose
    /// kind isn't "equipment" gets consumed when a living player
    /// overlaps it. Weapons + turret kits still require an explicit
    /// `try_interact` (E key) so players don't accidentally swap a
    /// rolled rare loadout by running past the wrong crate.
    fn auto_pickup_consumables(&mut self) {
        let pickup_r2 = Pickup::pickup_radius() * Pickup::pickup_radius();
        // Collect matches into a batch so we can mutate `pickups`
        // safely after releasing the read borrow. swap_remove
        // invalidates indices, so consume back-to-front.
        let mut to_consume: Vec<(usize, usize)> = Vec::new(); // (player_idx, pickup_idx)
        for (pi, player) in self.players.iter().enumerate() {
            if player.hp <= 0 {
                continue;
            }
            for (pk_idx, pk) in self.pickups.iter().enumerate() {
                if pk.kind.requires_interact() {
                    continue;
                }
                if to_consume.iter().any(|(_, idx)| *idx == pk_idx) {
                    continue;
                }
                let dx = pk.x - player.x;
                let dy = pk.y - player.y;
                if dx * dx + dy * dy <= pickup_r2 {
                    to_consume.push((pi, pk_idx));
                }
            }
        }
        to_consume.sort_by(|a, b| b.1.cmp(&a.1));
        for (pi, pk_idx) in to_consume {
            self.consume_pickup(pi, pk_idx);
        }
    }

    /// Queue a new toast on the top of the stack. Keeps the list
    /// capped at 5 so a rapid loot shower doesn't blow out the HUD
    /// column.
    pub fn push_toast(&mut self, text: String, color: Pixel) {
        const TTL: f32 = 3.5;
        self.toasts.insert(
            0,
            PickupToast { text, color, ttl: TTL, ttl_max: TTL },
        );
        if self.toasts.len() > 5 {
            self.toasts.truncate(5);
        }
    }

    /// Drain expired toasts. Called once per render-tick so the
    /// HUD animates out whether we're host or client.
    pub fn tick_toasts(&mut self, dt: f32) {
        self.toasts.retain_mut(|t| {
            t.ttl -= dt;
            t.ttl > 0.0
        });
    }

    /// Insert a new kill-feed line at the top of the stack, trimming
    /// any overflow off the bottom so a busy wave can't fill the
    /// viewport. Newest at index 0 so the renderer can walk the feed
    /// top-down.
    pub fn push_kill_feed(&mut self, entry: KillFeedEntry) {
        self.kill_feed.insert(0, entry);
        if self.kill_feed.len() > KillFeedEntry::MAX_ENTRIES {
            self.kill_feed.truncate(KillFeedEntry::MAX_ENTRIES);
        }
    }

    /// Fade out the kill feed. Called once per render-tick so the
    /// feed animates the same on solo, host, and client.
    pub fn tick_kill_feed(&mut self, dt: f32) {
        self.kill_feed.retain_mut(|e| {
            e.ttl -= dt;
            e.ttl > 0.0
        });
    }

    pub fn try_activate_traversal(&mut self, player_id: u32) {
        let Some(pi) = self.player_index(player_id) else { return };
        if self.players[pi].hp <= 0 {
            return;
        }
        // Scoped borrow so we can reach arena immutably.
        let _ = self.players[pi].activate_traversal(&self.arena);
    }

    fn register_vote(&mut self, player_id: u32, kiosk_idx: usize) {
        // Remove the voter from any other kiosk first so a player's vote
        // moves cleanly between kiosks instead of stacking.
        for (i, k) in self.kiosks.iter_mut().enumerate() {
            if i == kiosk_idx {
                continue;
            }
            if k.voter_ids.remove(&player_id) {
                k.votes = k.votes.saturating_sub(1);
            }
        }
        let Some(k) = self.kiosks.get_mut(kiosk_idx) else { return };
        if k.voter_ids.insert(player_id) {
            k.votes += 1;
        }
    }

    /// Cycle the active weapon slot.
    /// Consume one TurretKit from the player's inventory and spawn
    /// a friendly turret at the player's feet. No-op if the player
    /// has no kits or if the spawn tile is blocked.
    pub fn try_deploy_turret(&mut self, player_id: u32) {
        let Some(pi) = self.player_index(player_id) else {
            return;
        };
        if self.players[pi].turret_kits == 0 || self.players[pi].hp <= 0 {
            return;
        }
        let (px, py) = (self.players[pi].x, self.players[pi].y);
        let aim_x = self.players[pi].aim_x;
        let aim_y = self.players[pi].aim_y;

        // Try a ladder of spawn candidates so the deploy never
        // silently fails because the ideal spot is a wall or
        // out-of-bounds. First try: 3 tiles forward along aim; then
        // 1.5 tiles forward; then the player's own tile; then the
        // four cardinal neighbors. We ONLY decrement the kit when
        // one of these lands in a passable tile — otherwise the
        // player keeps their kit + gets visible feedback (no
        // silent eat).
        const CANDIDATES: &[(f32, f32, f32)] = &[
            (3.0, 3.0, 0.0),   // (aim_mul_x, aim_mul_y, perp_mul)
            (1.5, 1.5, 0.0),
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 1.5),
            (0.0, 0.0, -1.5),
        ];
        let mut chosen: Option<(f32, f32)> = None;
        for &(ax, ay, perp) in CANDIDATES {
            let sx = px + aim_x * ax + (-aim_y) * perp;
            let sy = py + aim_y * ay + aim_x * perp;
            if self.arena.is_passable(sx as i32, sy as i32) {
                chosen = Some((sx, sy));
                break;
            }
        }
        let Some((sx, sy)) = chosen else {
            tracing::info!(player = player_id, "turret deploy blocked — no passable neighbor");
            return;
        };

        self.players[pi].turret_kits -= 1;
        let stats = self.content.stats(Archetype::PlayerTurret).clone();
        self.enemies.push(Enemy::spawn(
            Archetype::PlayerTurret,
            &stats,
            sx,
            sy,
        ));
    }

    pub fn try_cycle_weapon(&mut self, player_id: u32) {
        if let Some(i) = self.player_index(player_id) {
            self.runtimes[i].cycle();
        }
    }

    fn drop_miniboss_loot(&mut self, arch: Archetype, x: f32, y: f32) {
        // Miniboss always drops a rare weapon + one guaranteed
        // consumable + one perk (subject to the player-has-it check).
        let rarity = Rarity::Rare;
        let (slots, mode) = self.roll_signature_weapon(arch, rarity);
        let id = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups
            .push(Pickup::new_weapon_with_mode(id, x, y, rarity, slots, mode));
        // Plus a random consumable next to it.
        let consumable = self.roll_consumable();
        let cid = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups
            .push(Pickup::new_kind(cid, x + 1.5, y + 1.5, consumable));
        // Perk drop — picks a perk no survivor already owns; brand's
        // `signature_perks` bias the roll toward thematic picks.
        self.maybe_drop_perk(x - 1.5, y + 1.5);
        // 1-in-3 miniboss kills drop a traversal verb.
        if self.loot_rng.r#gen::<f32>() < 0.33 {
            self.drop_traversal_verb(x - 3.0, y);
        }
    }

    /// Roll a weapon's slot list + fire mode biased by the archetype's
    /// signature pool. Archetypes without a signature fall back to the
    /// global primitive roll + a random fire mode, so any missing
    /// archetype row stays playable.
    fn roll_signature_weapon(
        &mut self,
        arch: Archetype,
        rarity: Rarity,
    ) -> (Vec<Primitive>, FireMode) {
        use rand::seq::SliceRandom;
        let pool = self.content.archetype_signature_primitives(arch);
        let slot_count = rarity.slot_count();
        let mut slots = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            if let Some(p) = pool.choose(&mut self.loot_rng).copied() {
                slots.push(p);
            } else {
                slots.push(self.roll_primitive());
            }
        }
        let mode = self
            .content
            .archetype_fire_mode(arch)
            .unwrap_or_else(|| {
                const MODES: &[FireMode] = &[
                    FireMode::Pulse,
                    FireMode::Auto,
                    FireMode::Burst,
                    FireMode::Pump,
                    FireMode::Rail,
                ];
                *MODES.choose(&mut self.loot_rng).unwrap_or(&FireMode::Pulse)
            });
        (slots, mode)
    }

    /// Rare chance for a regular enemy kill to drop a signature
    /// weapon. Rarity scales up with wave to keep the ramp visible.
    fn maybe_drop_signature_weapon(&mut self, arch: Archetype, x: f32, y: f32) {
        // Only archetypes with an authored signature can drop weapons
        // this way — otherwise regular kills flood the floor with
        // random junk rolls.
        if self.content.archetype_signature_primitives(arch).is_empty() {
            return;
        }
        let butcher = self
            .players
            .iter()
            .any(|p| p.hp > 0 && p.has_perk(crate::perk::Perk::Butcher));
        let chance = if butcher { 0.030 } else { 0.015 };
        if self.loot_rng.r#gen::<f32>() > chance {
            return;
        }
        // Wave-based rarity: early waves lean Common, late waves push
        // Uncommon/Rare so the progression curve keeps climbing.
        let wave = self.director.wave;
        let roll: f32 = self.loot_rng.r#gen();
        let rarity = if wave >= 15 && roll < 0.25 {
            Rarity::Rare
        } else if wave >= 8 && roll < 0.55 {
            Rarity::Uncommon
        } else {
            Rarity::Common
        };
        let (slots, mode) = self.roll_signature_weapon(arch, rarity);
        let id = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups.push(Pickup::new_weapon_with_mode(
            id, x, y, rarity, slots, mode,
        ));
    }

    fn drop_traversal_verb(&mut self, x: f32, y: f32) {
        // Brand-signature verb if set, else roll from the full pool.
        let brand_id = self.content.default_brand.clone();
        let verb = self
            .content
            .brand_traversal(&brand_id)
            .unwrap_or_else(|| crate::traversal::roll_verb(&mut self.loot_rng));
        let id = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups.push(Pickup::new_kind(
            id,
            x,
            y,
            crate::pickup::PickupKind::Traversal { verb },
        ));
    }

    /// Spawn a perk pickup at (x, y) using a perk no player already
    /// has. No-op if everyone already has every perk. Brand's
    /// `signature_perks` bias the roll: we try those first, fall back
    /// to the full unowned set if every signature perk is already
    /// claimed.
    fn maybe_drop_perk(&mut self, x: f32, y: f32) {
        use rand::seq::SliceRandom;
        // Merge every player's perk list — perks are duplicated per
        // player inventory but the drop is shared, so skip any perk
        // anyone has already.
        let mut owned: Vec<crate::perk::Perk> = Vec::new();
        for p in &self.players {
            for &perk in &p.perks {
                if !owned.contains(&perk) {
                    owned.push(perk);
                }
            }
        }
        let brand_id = self.content.default_brand.clone();
        let brand_pool = self.content.brand_perks(&brand_id);
        let perk = {
            let candidates: Vec<crate::perk::Perk> = brand_pool
                .into_iter()
                .filter(|p| !owned.contains(p))
                .collect();
            if let Some(choice) = candidates.choose(&mut self.loot_rng) {
                Some(*choice)
            } else {
                crate::perk::roll_new_perk(&owned, &mut self.loot_rng)
            }
        };
        let Some(perk) = perk else {
            return;
        };
        let id = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups.push(Pickup::new_kind(
            id,
            x,
            y,
            crate::pickup::PickupKind::Perk { perk },
        ));
    }

    /// Small chance to drop a consumable from a regular kill. Keeps
    /// the loot flow consistent — no more "only miniboss drops
    /// anything" dead spots between wave 5 intervals.
    fn maybe_drop_consumable_loot(&mut self, x: f32, y: f32, chance: f32) {
        if self.loot_rng.r#gen::<f32>() > chance {
            return;
        }
        let kind = self.roll_consumable();
        let id = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups.push(Pickup::new_kind(id, x, y, kind));
    }

    fn roll_consumable(&mut self) -> crate::pickup::PickupKind {
        let roll: u8 = self.loot_rng.gen_range(0..100);
        match roll {
            // Medkit — common.
            0..=44 => crate::pickup::PickupKind::MedKit { heal: 35 },
            // Sanity dose — common.
            45..=74 => crate::pickup::PickupKind::SanityDose { restore: 40.0 },
            // Armor plate — uncommon.
            75..=92 => crate::pickup::PickupKind::ArmorPlate { absorb: 50 },
            // Turret kit — rare.
            _ => crate::pickup::PickupKind::TurretKit,
        }
    }

    /// Phase durations (seconds). Tuned to land the whole cinematic
    /// at ~9 seconds before the dismiss prompt — long enough to sell
    /// the "you died horribly" beat, short enough that nobody feels
    /// held hostage.
    const DEATH_DYING_SECS: f32 = 3.0;
    const DEATH_PRAISE_SECS: f32 = 3.5;
    const DEATH_GOLDEN_SECS: f32 = 2.2;
    /// Minimum seconds the Report phase must be visible before
    /// accepting the dismiss keypress. Longer than the worst typical
    /// input-buffer drain (a few hundred ms of queued WASD from the
    /// frame the player actually died).
    const DEATH_REPORT_INPUT_ARM_SECS: f32 = 1.2;

    /// Enter the death cinematic. Spawns a heavy gore burst at every
    /// dead player's position + drops a persistent "body" corpse so
    /// enemies continue to shoot/walk over something visible. Also
    /// applies the Carcosa corruption spike.
    fn enter_death_phase(&mut self) {
        let down_count = self.players.iter().filter(|p| p.hp <= 0).count() as f32;
        self.add_corruption(7.0 * down_count);
        // Collect positions up front so we don't borrow-conflict
        // with spawn_corpse / emit_gore_burst inside the loop.
        let bodies: Vec<(f32, f32)> = self
            .players
            .iter()
            .filter(|p| p.hp <= 0)
            .map(|p| (p.x, p.y))
            .collect();
        for (bx, by) in bodies {
            let seed = self.next_seed((bx as i32, by as i32));
            // Heavy gib burst — the whole "you died horribly" moment
            // in one frame. Uses a human-tone palette that reads as
            // a player death rather than enemy gibs.
            self.emit_gore_burst(
                (bx, by),
                Pixel::rgb(255, 40, 40),
                Archetype::Pmc,
                seed,
            );
            // A persistent corpse so enemy attacks + corpse-hit gore
            // continue to paint the scene during `Dying`. Archetype
            // is the closest stand-in we have for a humanoid player
            // silhouette (§17 notes a proper PlayerBody archetype
            // as a future polish pass).
            self.spawn_corpse(Archetype::Pmc, bx, by);
        }
        self.death_phase = Some(DeathPhase::Dying {
            ttl: Self::DEATH_DYING_SECS,
        });
        self.death_started_at = self.elapsed_secs;
        tracing::info!("death cinematic begins");
    }

    /// Advance the current death phase. Handles transitions and
    /// re-enters `death_started_at` on every phase boundary.
    fn tick_death_phase(&mut self, dt: f32) {
        let Some(phase) = self.death_phase.as_mut() else {
            return;
        };
        match phase {
            DeathPhase::Dying { ttl } => {
                *ttl -= dt;
                if *ttl <= 0.0 {
                    self.death_phase = Some(DeathPhase::PraiseHastur {
                        ttl: Self::DEATH_PRAISE_SECS,
                    });
                    self.death_started_at = self.elapsed_secs;
                }
            }
            DeathPhase::PraiseHastur { ttl } => {
                *ttl -= dt;
                if *ttl <= 0.0 {
                    self.death_phase = Some(DeathPhase::Goldening {
                        ttl: Self::DEATH_GOLDEN_SECS,
                    });
                    self.death_started_at = self.elapsed_secs;
                }
            }
            DeathPhase::Goldening { ttl } => {
                *ttl -= dt;
                if *ttl <= 0.0 {
                    self.death_phase = Some(DeathPhase::Report { input_armed: false });
                    self.death_started_at = self.elapsed_secs;
                    self.death_report_elapsed = 0.0;
                }
            }
            DeathPhase::Report { input_armed } => {
                // Uses the dedicated cinematic clock, not
                // `elapsed_secs` — the latter is frozen at time-of-
                // death so the "time survived" stat stays honest.
                self.death_report_elapsed += dt;
                if !*input_armed
                    && self.death_report_elapsed >= Self::DEATH_REPORT_INPUT_ARM_SECS
                {
                    *input_armed = true;
                }
            }
        }
    }

    /// `true` when the outer run loop should accept a dismiss key
    /// and exit the run. Until this flips, buffered keys are
    /// swallowed — they don't count as a dismiss.
    pub fn death_report_accepts_input(&self) -> bool {
        matches!(
            self.death_phase,
            Some(DeathPhase::Report { input_armed: true })
        )
    }

    /// During frozen death phases, emit small gold particle pops
    /// from a random subset of enemies so the horde reads as "dancing
    /// in praise" rather than a still image. Cadence is slow enough
    /// to be legible even with 10k enemies on screen.
    fn tick_praise_hastur_pops(&mut self, _dt: f32) {
        // Only emit during Praise / Goldening — Report is a static
        // screen waiting on input, any motion there is noise.
        let emit = matches!(
            self.death_phase,
            Some(DeathPhase::PraiseHastur { .. }) | Some(DeathPhase::Goldening { .. })
        );
        if !emit || self.enemies.is_empty() {
            return;
        }
        use rand::Rng;
        // Pick ~1% of the horde this tick. Scales sublinearly with
        // N so a 10k horde doesn't drown the screen in particles.
        let pick_count = ((self.enemies.len() as f32 * 0.01) as usize)
            .max(1)
            .min(60);
        let gold = Pixel::rgb(255, 220, 80);
        let pale = Pixel::rgb(255, 240, 180);
        for _ in 0..pick_count {
            let idx = self.kiosk_rng.gen_range(0..self.enemies.len());
            let (ex, ey) = {
                let e = &self.enemies[idx];
                if e.hp <= 0 {
                    continue;
                }
                (e.x, e.y)
            };
            // Two-color sparkle above the enemy — looks like incense
            // / devotion smoke curling upward. Short TTL so it
            // doesn't pile up into a fog.
            let angle = self.kiosk_rng.r#gen::<f32>() * std::f32::consts::TAU;
            let speed = self.kiosk_rng.gen_range(4.0..14.0);
            let ttl = self.kiosk_rng.gen_range(0.35..0.7);
            let color = if self.kiosk_rng.r#gen::<bool>() { gold } else { pale };
            self.particles.push(crate::particle::Particle {
                x: ex,
                y: ey - 1.0,
                vx: angle.cos() * speed * 0.4,
                vy: -speed,
                ttl,
                ttl_max: ttl,
                color,
                size: 1,
            });
        }
    }

    /// Gold blend factor 0..1 across the Goldening + Report phases.
    /// Dying + PraiseHastur return 0. Used by the render tint
    /// override so the world gilds over the Goldening window and
    /// stays fully gold during the Report.
    pub fn death_gold_blend(&self) -> f32 {
        match self.death_phase {
            Some(DeathPhase::Goldening { ttl }) => {
                1.0 - (ttl / Self::DEATH_GOLDEN_SECS).clamp(0.0, 1.0)
            }
            Some(DeathPhase::Report { .. }) => 1.0,
            _ => 0.0,
        }
    }

    fn apply_wave_event(&mut self, event: WaveEvent) {
        match event {
            WaveEvent::EnterVote => self.spawn_vote_kiosks(),
            WaveEvent::ExitVote => self.resolve_vote(),
            WaveEvent::WaveStart(w) => {
                self.add_corruption(2.0);
                // Refresh Survivor perk's per-wave +10 armor stipend.
                for p in &mut self.players {
                    p.on_wave_start();
                }
                // Milestone perk drops: wave 5, 10, 15, 20... drop a
                // guaranteed perk near a random living player so
                // late runs stay progression-rich even without
                // miniboss kills.
                if w > 0 && w % 5 == 0 {
                    let living: Vec<(f32, f32)> = self
                        .players
                        .iter()
                        .filter(|p| p.hp > 0)
                        .map(|p| (p.x, p.y))
                        .collect();
                    if !living.is_empty() {
                        let idx = (w as usize) % living.len();
                        let (px, py) = living[idx];
                        self.maybe_drop_perk(px + 2.0, py);
                    }
                }
            }
        }
    }

    pub fn add_corruption(&mut self, amount: f32) {
        self.corruption = (self.corruption + amount).max(self.peak_corruption);
        self.peak_corruption = self.peak_corruption.max(self.corruption);
    }

    /// Every so often, plant a fresh Carcosa tile in a passable area past
    /// the 25% threshold. More tiles spawn at higher corruption.
    fn tick_carcosa(&mut self, dt: f32) {
        if self.corruption < 25.0 {
            return;
        }
        // Spawn rate scales with corruption: ~1 tile / 8s at 25%, 1/1.5s at 100%.
        let rate = (self.corruption / 100.0).powf(1.2) * 0.65 + 0.12;
        if self.kiosk_rng.r#gen::<f32>() > rate * dt {
            return;
        }
        // Pick a random passable tile near a player so the corruption bites
        // where the action is, not in some corner they'll never visit.
        if self.players.is_empty() {
            return;
        }
        let anchor = {
            let pi = self.kiosk_rng.gen_range(0..self.players.len());
            (self.players[pi].x, self.players[pi].y)
        };
        for _ in 0..12 {
            let dx = self.kiosk_rng.gen_range(-18..=18);
            let dy = self.kiosk_rng.gen_range(-12..=12);
            let tx = (anchor.0 as i32 + dx).max(2);
            let ty = (anchor.1 as i32 + dy).max(2);
            if tx as u16 >= self.arena.width - 2 || ty as u16 >= self.arena.height - 2 {
                continue;
            }
            if !self.arena.is_passable(tx, ty) {
                continue;
            }
            if self.arena.is_carcosa(tx, ty) {
                continue;
            }
            self.arena.set_carcosa(tx, ty);
            self.tick_tile_updates.push((tx, ty, 3, 0));
            return;
        }
    }

    /// Sanity drain from Carcosa terrain + mark proximity + baseline regen.
    fn tick_sanity(&mut self, dt: f32) {
        let marked_id = self.marked_player_id;
        for player in &mut self.players {
            if player.hp <= 0 {
                continue;
            }
            // Drain per tick.
            let tx = player.x.floor() as i32;
            let ty = player.y.floor() as i32;
            let on_carcosa = self.arena.is_carcosa(tx, ty)
                || self.arena.is_carcosa(tx, ty + 1);
            if on_carcosa {
                // GloomShroud perk: Carcosa drains sanity 60% slower.
                let mul = if player.has_perk(crate::perk::Perk::GloomShroud) {
                    0.4
                } else {
                    1.0
                };
                player.sanity -= 18.0 * dt * mul;
            }
            if Some(player.id) == marked_id {
                player.sanity -= 4.0 * dt;
            }
            // Very slow passive recovery so survivors who escape Carcosa
            // feel the trickle coming back.
            if !on_carcosa && Some(player.id) != marked_id {
                player.sanity += 1.5 * dt;
            }
            player.sanity = player.sanity.clamp(0.0, 100.0);
        }
    }

    fn tick_marks(&mut self, dt: f32) {
        // No marks until Corruption 50%. At 100%+ marks rotate twice as fast.
        if self.corruption < 50.0 || self.players.iter().all(|p| p.hp <= 0) {
            self.marked_player_id = None;
            self.mark_rotate_timer = 0.0;
            return;
        }
        self.mark_rotate_timer -= dt;
        if self.marked_player_id.is_none()
            || !self.players.iter().any(|p| Some(p.id) == self.marked_player_id && p.hp > 0)
            || self.mark_rotate_timer <= 0.0
        {
            // Pick a living player at random to mark.
            let living: Vec<u32> =
                self.players.iter().filter(|p| p.hp > 0).map(|p| p.id).collect();
            if !living.is_empty() {
                let idx = self.kiosk_rng.gen_range(0..living.len());
                self.marked_player_id = Some(living[idx]);
                let interval = if self.corruption >= 100.0 { 22.0 } else { 45.0 };
                self.mark_rotate_timer = interval;
            }
        }
    }

    /// Return the damage scalar applied when an enemy hits this player.
    /// Marked players take +25% damage.
    pub fn damage_scalar_for(&self, player_id: u32) -> f32 {
        if Some(player_id) == self.marked_player_id {
            1.25
        } else {
            1.0
        }
    }

    /// The Hastur Daemon. Schedules periodic Yellow Sign appearances once
    /// Corruption reaches 25%. At higher corruption the Daemon fires more
    /// often.
    fn tick_daemon(&mut self, dt: f32) {
        if self.corruption < 25.0 {
            self.daemon_timer = 25.0;
            return;
        }
        self.daemon_timer -= dt;
        if self.daemon_timer > 0.0 {
            return;
        }
        // Schedule next event — faster at higher corruption.
        let base = 30.0 - (self.corruption / 100.0) * 18.0; // 30s → 12s
        self.daemon_timer = base.max(10.0);

        // Spawn a Yellow Sign near a random living player so the player has
        // to react, not just notice from afar.
        let anchor = self
            .players
            .iter()
            .filter(|p| p.hp > 0)
            .nth(self.kiosk_rng.gen_range(0..self.players.iter().filter(|p| p.hp > 0).count().max(1)))
            .map(|p| (p.x, p.y))
            .unwrap_or_else(|| (self.arena.width as f32 / 2.0, self.arena.height as f32 / 2.0));

        // Offset into a visible-but-not-overlapping position.
        let dx: f32 = (self.kiosk_rng.gen_range(-24..=24)) as f32;
        let dy: f32 = (self.kiosk_rng.gen_range(-16..=16)) as f32;
        let x = (anchor.0 + dx).clamp(6.0, self.arena.width as f32 - 6.0);
        let y = (anchor.1 + dy).clamp(6.0, self.arena.height as f32 - 6.0);

        let id = self.next_sign_id;
        self.next_sign_id += 1;
        self.yellow_signs.push(YellowSign::new(id, x, y));
        // Notice events also give corruption a small nudge.
        self.add_corruption(1.2);
    }

    fn tick_signs(&mut self, dt: f32) {
        // Compute total sanity drain from all visible signs — drains every
        // living player while any sign is up.
        let mut drain_per_sec = 0.0f32;
        for s in &self.yellow_signs {
            drain_per_sec += s.sanity_drain_per_sec();
        }
        if drain_per_sec > 0.0 {
            for player in &mut self.players {
                if player.hp > 0 {
                    player.sanity = (player.sanity - drain_per_sec * dt).clamp(0.0, 100.0);
                }
            }
        }
        self.yellow_signs.retain_mut(|s| s.tick(dt));
    }

    /// Client-only: roll phantom spawns near the local player when sanity
    /// drops below the hallucination threshold. No-op on the host.
    pub fn tick_phantoms(&mut self, dt: f32) {
        self.phantoms.retain_mut(|p| p.tick(dt));

        // Snapshot the local player's sanity/position so we can release the
        // immutable borrow before rolling RNG.
        let (px, py, sanity) = match self.local_player() {
            Some(p) => (p.x, p.y, p.sanity),
            None => return,
        };
        if sanity > 50.0 {
            return;
        }
        let severity = 1.0 - (sanity / 50.0).clamp(0.0, 1.0);
        let rate = severity * 0.6;
        if self.kiosk_rng.r#gen::<f32>() > rate * dt {
            return;
        }

        let dx = self.kiosk_rng.gen_range(-12..=12) as f32;
        let dy = self.kiosk_rng.gen_range(-10..=10) as f32;
        if dx.abs() < 4.0 && dy.abs() < 4.0 {
            return;
        }
        let x = (px + dx).clamp(4.0, self.arena.width as f32 - 4.0);
        let y = (py + dy).clamp(4.0, self.arena.height as f32 - 4.0);
        let kind = self
            .enemies
            .first()
            .map(|e| e.archetype.to_kind())
            .unwrap_or(0);
        self.phantoms.push(Phantom::new(x, y, kind));
    }

    /// Corruption tint for the framebuffer: colour + amount based on the
    /// current level. Returns (color, amount) where amount is 0..0.5.
    pub fn corruption_tint(&self) -> (Pixel, f32) {
        // Death cinematic overrides the normal corruption drift —
        // during Goldening + Report phases the world gilds into
        // saturated gold (Hastur's color), blending from the
        // corruption tint at entry to full gold over the phase.
        let gold_blend = self.death_gold_blend();
        if gold_blend > 0.0 {
            // Gold target: bright yellow-gold, heavy tint amount so
            // the scene visibly solidifies. Blend from base amber.
            let base = Pixel::rgb(255, 180, 60);
            let gold = Pixel::rgb(255, 215, 0);
            let t = gold_blend;
            let r = (base.r as f32 * (1.0 - t) + gold.r as f32 * t) as u8;
            let g = (base.g as f32 * (1.0 - t) + gold.g as f32 * t) as u8;
            let b = (base.b as f32 * (1.0 - t) + gold.b as f32 * t) as u8;
            let base_amount = {
                let ct = (self.corruption / 100.0).clamp(0.0, 1.2);
                (ct * 0.45).min(0.5)
            };
            // End state: 0.85 tint amount — legible but unmistakably
            // golden. Eases from the current corruption tint.
            let amount = base_amount * (1.0 - t) + 0.85 * t;
            return (Pixel::rgb(r, g, b), amount);
        }
        // Amber-ochre drift; stronger with more corruption. Cap at 0.5 so
        // pixels remain legible even at very high corruption.
        let t = (self.corruption / 100.0).clamp(0.0, 1.2);
        let amount = (t * 0.45).min(0.5);
        (Pixel::rgb(255, 180, 60), amount)
    }

    fn spawn_vote_kiosks(&mut self) {
        self.kiosks.clear();
        // Eligible brands: anything in content that isn't already active.
        let mut eligible: Vec<String> = self
            .content
            .brands
            .keys()
            .filter(|id| !self.active_brands.contains(id))
            .cloned()
            .collect();
        if eligible.is_empty() {
            return;
        }
        // Shuffle and take up to 3.
        for i in (1..eligible.len()).rev() {
            let j = self.kiosk_rng.gen_range(0..=i);
            eligible.swap(i, j);
        }
        eligible.truncate(3);

        // Spread kiosk positions around the arena center.
        let cx = self.arena.width as f32 / 2.0;
        let cy = self.arena.height as f32 / 2.0;
        let radius = (self.arena.width.min(self.arena.height) as f32) * 0.28;
        let count = eligible.len();
        for (i, brand_id) in eligible.iter().enumerate() {
            let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
            let mut x = cx + angle.cos() * radius;
            let mut y = cy + angle.sin() * radius;
            // Nudge out of walls toward center.
            for _ in 0..8 {
                if !self.arena.is_wall(x.floor() as i32, y.floor() as i32) {
                    break;
                }
                x = cx + (x - cx) * 0.8;
                y = cy + (y - cy) * 0.8;
            }
            let (name, color) = match self.content.brand(brand_id) {
                Some(b) => (b.name.clone(), brand_color(&b.id)),
                None => (brand_id.clone(), Pixel::rgb(255, 255, 255)),
            };
            let id = self.next_kiosk_id;
            self.next_kiosk_id += 1;
            self.kiosks
                .push(VoteKiosk::new(id, x, y, brand_id.clone(), name, color));
        }
    }

    fn resolve_vote(&mut self) {
        if self.kiosks.is_empty() {
            return;
        }
        // Pick the kiosk with the most votes. If nobody voted at all,
        // the Hastur Daemon picks one of the ACTUALLY-PRESENTED
        // kiosks at random — not a random brand from all content —
        // so the player's vote terminal choices always matter.
        let top_votes = self.kiosks.iter().map(|k| k.votes).max().unwrap_or(0);
        let picked_brand = if top_votes == 0 {
            // Abstention — Daemon picks one of the kiosks offered.
            let i = self.kiosk_rng.gen_range(0..self.kiosks.len());
            self.kiosks[i].brand_id.clone()
        } else {
            // Tie-break: first kiosk with the max vote count wins.
            self.kiosks
                .iter()
                .find(|k| k.votes == top_votes)
                .map(|k| k.brand_id.clone())
                .unwrap_or_default()
        };
        if !picked_brand.is_empty() && !self.active_brands.contains(&picked_brand) {
            // On the first vote we REPLACE the default brand instead
            // of stacking on top of it — otherwise players would
            // always see the default sneaking into waves they didn't
            // vote for.
            if self.default_brand_active {
                self.active_brands.clear();
                self.default_brand_active = false;
            }
            self.active_brands.push(picked_brand);
        }
        self.kiosks.clear();
    }

    fn roll_primitive(&mut self) -> Primitive {
        const POOL: &[Primitive] = &[
            Primitive::Ignite,
            Primitive::Breach,
            Primitive::Ricochet,
            Primitive::Chain,
            Primitive::Pierce,
            Primitive::Overdrive,
            Primitive::Acid,
            Primitive::Cryo,
            Primitive::Contagion,
            Primitive::GravityWell,
            Primitive::Siphon,
            Primitive::ShieldBreak,
        ];
        POOL[self.loot_rng.gen_range(0..POOL.len())]
    }

    pub fn remove_player(&mut self, id: u32) {
        if let Some(i) = self.players.iter().position(|p| p.id == id) {
            self.players.swap_remove(i);
            self.runtimes.swap_remove(i);
        }
    }

    pub fn player_index(&self, id: u32) -> Option<usize> {
        self.players.iter().position(|p| p.id == id)
    }

    pub fn set_input(&mut self, id: u32, input: PlayerInput) {
        if let Some(i) = self.player_index(id) {
            self.runtimes[i].input = input;
        }
    }

    pub fn tick_authoritative(&mut self, dt: f32) {
        self.perf.begin("tick_total");
        if !self.alive || self.paused {
            // Still clear per-tick outgoing event buffers even while paused
            // so a paused frame doesn't re-broadcast stale blasts.
            self.tick_tile_updates.clear();
            self.tick_blasts.clear();
            self.tick_ground_paints.clear();
            self.tick_corpse_hits.clear();
            self.tick_body_reactions.clear();
            self.tick_toast_events.clear();
            self.perf.end("tick_total");
            return;
        }
        self.tick_tile_updates.clear();
        self.tick_blasts.clear();
        self.tick_ground_paints.clear();
        self.tick_corpse_hits.clear();
        self.tick_toast_events.clear();
        // Run-clock: freezes the instant the death cinematic begins
        // so the "time survived" stat on the report reflects the
        // moment of death, not the moment the player dismissed the
        // overlay. Sub-frame death (same frame as the last kill) is
        // handled because enter_death_phase fires at the bottom of
        // this same tick — from the next frame onward the check
        // below short-circuits.
        if self.death_phase.is_none() {
            self.elapsed_secs += dt;
        }
        // Advance the death-cinematic state machine early so the
        // freeze gate below reflects this frame's phase. Trigger
        // (enter_death_phase) still fires at the bottom once every
        // hp-change this frame has settled.
        self.tick_death_phase(dt);
        if self.death_phase.as_ref().map_or(false, |p| p.freezes_sim()) {
            // Praise / Goldening / Report: sim is paused. Keep
            // corruption drift + death-cinematic cadence running
            // (particle pops land in tick_client); skip everything
            // else. Corpse + projectile + enemy state all freeze.
            self.tick_praise_hastur_pops(dt);
            self.perf.end("tick_total");
            return;
        }
        // Passive corruption tick; drivers below add more per event.
        self.add_corruption(dt * 0.15);
        self.perf.begin("tick_carcosa");
        self.tick_carcosa(dt);
        self.perf.end("tick_carcosa");
        self.tick_sanity(dt);
        self.tick_marks(dt);
        self.tick_daemon(dt);
        self.tick_signs(dt);
        self.tick_toasts(dt);
        self.tick_kill_feed(dt);
        // Substance world-interaction passes. Three tight loops that
        // tie the ground layer to the rest of the sim:
        //   • ambient emitters  — smoke/steam/bubbles/sparks from
        //     substance tiles visible to the camera.
        //   • standing hazards  — acid/fire/uranium damage + sanity
        //     drain for entities in-tile.
        //   • fire propagation  — fire tiles ignite flammable
        //     neighbors; fire tiles decay toward Floor on TTL.
        self.perf.begin("substance_world");
        self.tick_substance_ambient(dt);
        self.tick_substance_hazards(dt);
        self.tick_fire_propagation(dt);
        self.perf.end("substance_world");

        for (player, runtime) in self.players.iter_mut().zip(self.runtimes.iter_mut()) {
            // Dead players freeze in place — their last input is
            // stale and applying it would animate their corpse.
            // Weapons still tick cooldowns so the loadout the
            // Director (future) inherits isn't stuck mid-recovery.
            if player.hp > 0 {
                player.apply_remote_input(
                    runtime.input.move_x,
                    runtime.input.move_y,
                    runtime.input.aim_x,
                    runtime.input.aim_y,
                    runtime.input.firing,
                    &self.arena,
                    dt,
                );
                player.tick_perk_timers(dt);
                // Iron Will: bonus sanity regen, always-on.
                if player.has_perk(crate::perk::Perk::IronWill) {
                    player.sanity = (player.sanity + 1.5 * dt).min(100.0);
                }
                // Bloodhound: standing on a blood pool heals +2 HP/s
                // throttled to once every 0.5s to keep it manageable.
                if player.has_perk(crate::perk::Perk::Bloodhound)
                    && player.bloodhound_tick <= 0.0
                {
                    use crate::arena::Ground;
                    if matches!(
                        self.arena.ground(player.x as i32, player.y as i32),
                        Ground::Substance { .. }
                    ) {
                        // Only regen on blood pool specifically.
                        if let crate::arena::Ground::Substance { id, .. } =
                            self.arena.ground(player.x as i32, player.y as i32)
                        {
                            if id == self.content.blood_pool_id {
                                player.hp = (player.hp + 2)
                                    .min(crate::player::PLAYER_MAX_HP);
                                player.bloodhound_tick = 0.5;
                            }
                        }
                    }
                }
            } else {
                // Zero out firing so a dead player's held LMB doesn't
                // pretend they're shooting. Aim/move implicitly
                // irrelevant since we skip the move call.
                runtime.input.firing = false;
            }
            for w in runtime.weapons.iter_mut().flatten() {
                w.tick(dt * player.fire_rate_mul());
            }
        }

        // Player bleed FX — emit a small blood puff at each wounded
        // player's position whenever their cadence rolls over. Done
        // in a separate pass so the mutable player loop above
        // doesn't fight the particles borrow.
        let mut bleed_bursts: Vec<((f32, f32), u8)> = Vec::new();
        for player in self.players.iter_mut() {
            if let Some(intensity) = player.tick_bleed(dt) {
                bleed_bursts.push(((player.x, player.y), intensity));
            }
        }
        for ((bx, by), intensity) in bleed_bursts {
            let seed = self.next_seed((bx as i32, by as i32));
            self.emit_player_bleed(bx, by, intensity, seed);
        }

        // Consumables (medkit/sanity/armor/perk/traversal) auto-
        // pickup on collision; weapons + turret kits still need E.
        self.auto_pickup_consumables();

        let muzzle_offset = 7.5;
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        for (player, runtime) in self.players.iter().zip(self.runtimes.iter_mut()) {
            // Dead players don't fire — their weapons stay holstered
            // until a revive (none in MVP) or Director mode (future).
            if player.hp <= 0 || !runtime.input.firing {
                continue;
            }
            // Extract input into locals up front so the subsequent
            // `active_weapon_mut` borrow doesn't conflict with reading aim.
            let aim_x = runtime.input.aim_x;
            let aim_y = runtime.input.aim_y;
            let Some(weapon) = runtime.active_weapon_mut() else {
                continue;
            };
            let origin = (player.x + aim_x * muzzle_offset, player.y + aim_y * muzzle_offset);
            let target = (origin.0 + aim_x * 100.0, origin.1 + aim_y * 100.0);
            let damage_mul = player.outgoing_damage_mul();
            let salvo = weapon.try_fire(origin, target, player.id, damage_mul);
            if !salvo.is_empty() {
                weapon.consume_overdrive();
                // Emit a weapon-mode fire event per successful shot.
                // `FireMode::label` returns the snake-case id
                // ("pulse", "auto", etc) used as the audio pool key.
                crate::audio::emit(weapon.mode.label(), "fire", Some(origin));
                // Rumble only for the local player — other players'
                // shots are audible but shouldn't shake the host's pad.
                if Some(player.id) == self.local_id {
                    crate::gamepad::rumble(crate::gamepad::Rumble::Fire);
                }
            }
            new_projectiles.extend(salvo);
        }
        self.projectiles.append(&mut new_projectiles);

        // Tick pickups (TTL expiry).
        self.pickups.retain_mut(|p| p.tick(dt));

        let mut events: Vec<WaveEvent> = Vec::new();
        let anchors: Vec<(f32, f32)> = self
            .players
            .iter()
            .filter(|p| p.hp > 0)
            .map(|p| (p.x, p.y))
            .collect();
        self.director.tick(
            &self.arena,
            &mut self.enemies,
            &self.content,
            &self.active_brands,
            &mut events,
            &anchors,
            dt,
        );
        for kiosk in &mut self.kiosks {
            kiosk.tick(dt);
        }
        for event in events {
            self.apply_wave_event(event);
        }

        self.perf.begin("enemy_loop");
        // Step enemies + touch damage + ranged fire + status ticks.
        // Marked players (Hastur gaze) override target selection so AI
        // flocks toward them; all enemies prioritize the marked survivor if
        // they have one.
        // Only *living* players are valid enemy targets. Marked
        // players fall back to None once dead so AI stops flocking
        // to a corpse.
        let marked_pos = self.marked_player_id.and_then(|id| {
            self.players
                .iter()
                .find(|p| p.id == id && p.hp > 0)
                .map(|p| (p.id, p.x, p.y))
        });
        // During the Dying phase of the death cinematic, include
        // dead player bodies as valid targets so enemies keep
        // swarming the corpse — melee hits, ranged fire, movement
        // toward the body all resume. Normal play still filters
        // dead players out so a downed teammate isn't a pathing
        // magnet mid-run.
        let targeting_dying_bodies =
            matches!(self.death_phase, Some(DeathPhase::Dying { .. }));
        let positions: Vec<(u32, f32, f32)> = self
            .players
            .iter()
            .filter(|p| targeting_dying_bodies || p.hp > 0)
            .map(|p| (p.id, p.x, p.y))
            .collect();
        // Snapshot of corpse positions so Eaters can path toward them
        // without mutably borrowing `self.corpses` while the enemy loop
        // holds `self.enemies`.
        let corpse_positions: Vec<(u32, f32, f32)> =
            self.corpses.iter().map(|c| (c.id, c.x, c.y)).collect();
        // Snapshot of non-Flood enemy positions — used by Flood-faction
        // targeting so infected hosts chase regular horde + players.
        // All living enemies tagged with their team, so hostile-tag
        // scans can target them by team match. Generic: any new team
        // a content pack introduces flows through here unchanged.
        //
        // Bucketed by team tag up front: the old "flat Vec scanned
        // per enemy" path was O(N²) and dominated the frame at 10k
        // (each zergling re-checked every other zergling's team
        // before skipping it). With the bucket, an enemy only scans
        // the buckets matching its own hostile set — typically 1–2
        // small teams. Zerg-on-zerg becomes free because the horde
        // team isn't in any horde enemy's hostile list.
        use std::collections::HashMap;
        let mut enemies_by_team: HashMap<crate::tag::Tag, Vec<(usize, f32, f32)>> =
            HashMap::new();
        for (i, e) in self.enemies.iter().enumerate() {
            if e.hp <= 0 {
                continue;
            }
            enemies_by_team
                .entry(e.team)
                .or_default()
                .push((i, e.x, e.y));
        }
        let mut enemy_status_kills: Vec<usize> = Vec::new();
        let mut ranged_shots: Vec<(u32, crate::enemy::RangedShot)> = Vec::new();
        // Breacher wall smashes queued this tick — tile coords.
        let mut breacher_smashes: Vec<(i32, i32)> = Vec::new();
        // Rocketeer shots queued this tick — (from_x, from_y, to_x, to_y).
        let mut rocketeer_shots: Vec<(f32, f32, f32, f32)> = Vec::new();
        // Sapper detonations queued this tick — fuse ran out. Position.
        let mut sapper_detonations: Vec<(f32, f32, usize)> = Vec::new();
        // Juggernaut wall smashes queued this tick.
        let mut juggernaut_smashes: Vec<(i32, i32)> = Vec::new();
        // Howler shrieks — emit a large noise event at the Howler's pos.
        let mut howler_shrieks: Vec<(f32, f32)> = Vec::new();

        // Tick noise event TTLs + drop expired ones so enemies aren't
        // chasing ancient sounds.
        for n in &mut self.noise_events {
            n.ttl -= dt;
        }
        self.noise_events.retain(|n| n.ttl > 0.0);

        // Pre-pass: propagate noise events to enemies. Each enemy
        // without LoS to any player but within a noise's radius picks
        // up the noise as an investigation target. Enemies with a
        // direct target ignore noise.
        {
            let noises = self.noise_events.clone();
            for e in self.enemies.iter_mut() {
                if noises.is_empty() {
                    break;
                }
                // Nearest noise within range.
                let mut best: Option<(f32, f32, f32)> = None; // (x, y, d2)
                for n in &noises {
                    let dx = n.x - e.x;
                    let dy = n.y - e.y;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= n.radius * n.radius && best.map_or(true, |(_, _, bd)| d2 < bd) {
                        best = Some((n.x, n.y, d2));
                    }
                }
                if let Some((nx, ny, _)) = best {
                    e.noise_target = Some((nx, ny));
                    e.noise_ttl = 3.5;
                }
            }
        }
        // Kill-feed entries staged during the enemy loop (the `self`
        // borrow is locked to `self.enemies` inside, so we can't push
        // directly into `self.kill_feed` until the loop exits).
        let mut pending_player_deaths: Vec<(u32, Archetype, Option<crate::tag::Tag>)> = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            // Eater behavior: while hungry (consumed < THRESHOLD), seek
            // the nearest corpse instead of a player. Once fed, flips
            // to normal aggro and charges survivors.
            let eater_target = if e.archetype == Archetype::Eater
                && e.consumed < EATER_HUNGER_THRESHOLD
                && !corpse_positions.is_empty()
            {
                corpse_positions
                    .iter()
                    .min_by(|a, b| {
                        let da = (a.1 - e.x).powi(2) + (a.2 - e.y).powi(2);
                        let db = (b.1 - e.x).powi(2) + (b.2 - e.y).powi(2);
                        da.partial_cmp(&db).unwrap()
                    })
                    .copied()
            } else {
                None
            };

            // Director override: when a (future) Director command is
            // attached, it takes priority over intrinsic targeting.
            // Kept first in the chain so the RTS-style control path
            // drops in cleanly when wired.
            let override_target = if let Some(ov) = e.director_override {
                match ov.verb {
                    crate::behavior::CommandVerb::MoveTo
                    | crate::behavior::CommandVerb::AttackAt => Some((0u32, ov.point.0, ov.point.1)),
                    crate::behavior::CommandVerb::AttackEntity => ov.entity.and_then(|eid| {
                        positions
                            .iter()
                            .find(|p| p.0 == eid)
                            .copied()
                    }),
                    crate::behavior::CommandVerb::Guard => Some((0u32, e.x, e.y)),
                }
            } else {
                None
            };

            // Tick override TTL — expires and falls back to intrinsic
            // targeting once the timer runs out.
            if let Some(ov) = e.director_override.as_mut() {
                ov.ttl -= dt;
                if ov.ttl <= 0.0 {
                    e.director_override = None;
                }
            }

            // Cross-faction target scan: Horde can hunt survivor-team
            // PlayerTurrets + Flood can hunt the Horde. Bucketed by
            // team up front — this enemy only scans buckets matching
            // its own hostile tags, skipping entirely when no hostile
            // team exists on the map (the common zerg-on-zerg case).
            let survivor_tag = crate::tag::Tag::new(crate::enemy::TEAM_SURVIVOR);
            let cross_faction_target = {
                let mut best: Option<(f32, f32, f32)> = None;
                for hostile_tag in e.hostiles.iter() {
                    // Survivor is handled separately via the `positions`
                    // list (real players, not entity-side entries).
                    if hostile_tag == survivor_tag {
                        continue;
                    }
                    let Some(bucket) = enemies_by_team.get(&hostile_tag) else {
                        continue;
                    };
                    for (idx, hx, hy) in bucket {
                        if *idx == i {
                            continue;
                        }
                        let dx = hx - e.x;
                        let dy = hy - e.y;
                        let d2 = dx * dx + dy * dy;
                        if best.map_or(true, |(_, _, bd)| d2 < bd) {
                            best = Some((*hx, *hy, d2));
                        }
                    }
                }
                best.map(|(x, y, _)| (0u32, x, y))
            };

            // Target priority: Director override → cross-faction →
            // Eater corpse → marked player → nearest player.
            let hunts_survivors = e.hostiles.has(survivor_tag);
            let target = override_target
                .or(cross_faction_target)
                .or(eater_target)
                .or(marked_pos)
                .or_else(|| {
                    if !hunts_survivors {
                        return None;
                    }
                    positions
                        .iter()
                        .min_by(|a, b| {
                            let da = (a.1 - e.x).powi(2) + (a.2 - e.y).powi(2);
                            let db = (b.1 - e.x).powi(2) + (b.2 - e.y).powi(2);
                            da.partial_cmp(&db).unwrap()
                        })
                        .copied()
                });
            // Breacher pathfinding override. Compute A* toward the
            // current target; follow waypoints instead of straight-line
            // chase; if a waypoint is behind a wall, smash instead of
            // advancing.
            let mut breacher_target_override: Option<(f32, f32)> = None;
            if e.archetype == Archetype::Breacher {
                e.path_timer -= dt;
                if let Some((_pid, tx, ty)) = target {
                    if e.path.is_empty() || e.path_timer <= 0.0 {
                        let start = (e.x as i32, e.y as i32);
                        let goal = (tx as i32, ty as i32);
                        if let Some(path) = crate::pathfind::astar(&self.arena, start, goal)
                        {
                            e.path = path;
                            e.path_timer = 0.65;
                        }
                    }
                }
                // Drop waypoints we've already reached.
                while e.path.len() >= 2 {
                    let wp = e.path[1];
                    let dx = (wp.0 as f32 + 0.5) - e.x;
                    let dy = (wp.1 as f32 + 0.5) - e.y;
                    if dx * dx + dy * dy < 1.5 {
                        e.path.remove(0);
                    } else {
                        break;
                    }
                }
                if let Some(&wp) = e.path.get(1) {
                    if self.arena.is_wall(wp.0, wp.1) {
                        // Wind up telegraph, then smash on cooldown.
                        if e.attack_cooldown <= 0.0 {
                            if e.tell_timer <= 0.0 {
                                e.tell_timer = 0.55;
                            } else if e.tell_timer < 0.05 {
                                breacher_smashes.push(wp);
                                e.tell_timer = 0.0;
                                e.attack_cooldown = 1.4;
                            }
                        }
                        // Stay in place — the wall is "target".
                        breacher_target_override =
                            Some((wp.0 as f32 + 0.5, wp.1 as f32 + 0.5));
                    } else {
                        breacher_target_override =
                            Some((wp.0 as f32 + 0.5, wp.1 as f32 + 0.5));
                    }
                }
            }

            // Sapper: kamikaze. When adjacent to a wall or a player,
            // starts a fuse telegraph. Fuse ends → detonate (big
            // explosion + wall damage + self-death).
            if e.archetype == Archetype::Sapper && e.tell_timer <= 0.0 && e.attack_cooldown < 100.0 {
                let ix = e.x as i32;
                let iy = e.y as i32;
                let near_wall = self.arena.is_wall(ix + 1, iy)
                    || self.arena.is_wall(ix - 1, iy)
                    || self.arena.is_wall(ix, iy + 1)
                    || self.arena.is_wall(ix, iy - 1);
                let near_player = positions.iter().any(|(_, px, py)| {
                    let dx = *px - e.x;
                    let dy = *py - e.y;
                    dx * dx + dy * dy < 3.5 * 3.5
                });
                if near_wall || near_player {
                    e.tell_timer = 0.6;
                    // Lock out re-trigger (can't self-cancel mid-fuse).
                    e.attack_cooldown = 999.0;
                }
            }
            if e.archetype == Archetype::Sapper
                && e.attack_cooldown > 100.0
                && e.tell_timer > 0.0
                && e.tell_timer < 0.05
            {
                sapper_detonations.push((e.x, e.y, i));
                e.tell_timer = 0.0;
            }

            // Juggernaut: slow wall-smasher. When a wall sits 1–2 tiles
            // ahead along the move direction, telegraph + smash.
            if e.archetype == Archetype::Juggernaut && e.attack_cooldown <= 0.0 {
                if let Some((_pid, tx, ty)) = target {
                    let dx = tx - e.x;
                    let dy = ty - e.y;
                    let len = (dx * dx + dy * dy).sqrt().max(0.1);
                    let look_x = e.x + dx / len * 2.0;
                    let look_y = e.y + dy / len * 2.0;
                    let wall_ahead = self.arena.is_wall(look_x as i32, look_y as i32);
                    if wall_ahead {
                        if e.tell_timer <= 0.0 {
                            e.tell_timer = 0.4;
                        } else if e.tell_timer < 0.05 {
                            juggernaut_smashes.push((look_x as i32, look_y as i32));
                            e.tell_timer = 0.0;
                            e.attack_cooldown = 1.1;
                        }
                    }
                }
            }

            // Howler: emits a noise shriek on a cooldown. Never moves
            // (stationary override below keeps speed at 0 regardless
            // of its base speed).
            if e.archetype == Archetype::Howler && e.attack_cooldown <= 0.0 {
                howler_shrieks.push((e.x, e.y));
                crate::audio::emit("howler", "howl", Some((e.x, e.y)));
                e.attack_cooldown = 4.5;
            }

            // Charger behavior: cruise → wind up → sprint. When a
            // target is in LoS and at mid-range, spend 0.35s
            // telegraphing (visible eye glow + slower pace) then
            // sprint 1.6s at 2.5× toward the locked-in heading.
            // Window opened to 7–36 tiles so they commit earlier +
            // from further; cooldown dropped so a missed charge
            // isn't a free 4 seconds — they rewind fast + come again.
            if e.archetype == Archetype::Charger && e.sprint_timer <= 0.0 && e.sprint_cooldown <= 0.0 {
                if let Some((_pid, tx, ty)) = target {
                    let dx = tx - e.x;
                    let dy = ty - e.y;
                    let d2 = dx * dx + dy * dy;
                    let in_window = (7.0 * 7.0..=36.0 * 36.0).contains(&d2);
                    if in_window && self.arena.has_line_of_sight((e.x, e.y), (tx, ty)) {
                        if e.tell_timer <= 0.0 {
                            e.tell_timer = 0.35;
                            e.tell_target = (tx, ty);
                        } else if e.tell_timer < 0.05 {
                            // Tell just finished — lock in direction + sprint.
                            e.sprint_timer = 1.6;
                            e.sprint_cooldown = 2.2;
                            e.tell_timer = 0.0;
                            crate::audio::emit("charger", "charge_windup", Some((e.x, e.y)));
                        }
                    }
                }
            }

            // Leaper behavior: prowl → wind up → leap → recover.
            // Tuned for aggression — the whole point of a Leaper is
            // the "something just jumped at me" moment. Trigger range
            // widened to 5–26 tiles (was 6–20) so they leap from
            // further out, cadence tightened (windup 0.35s instead
            // of 0.45, recovery 0.9s instead of 1.2, cooldown 1.8s
            // instead of 3.0). Net cycle time ~0.35+0.55+0.9 ≈ 1.8s
            // + 1.8s cooldown = 3.6s between leaps — roughly twice
            // as often.
            if e.archetype == Archetype::Leaper {
                match e.leap_state {
                    0 => {
                        // Prowl. Trigger a leap when mid-range + LoS + no cooldown.
                        if let Some((_pid, tx, ty)) = target {
                            let dx = tx - e.x;
                            let dy = ty - e.y;
                            let d2 = dx * dx + dy * dy;
                            if (5.0f32 * 5.0..=26.0 * 26.0).contains(&d2)
                                && self.arena.has_line_of_sight((e.x, e.y), (tx, ty))
                                && e.sprint_cooldown <= 0.0
                            {
                                e.leap_state = 1;
                                e.leap_timer = 0.35;
                                e.tell_timer = 0.35;
                                let len = d2.sqrt().max(0.001);
                                e.leap_vec = (dx / len, dy / len);
                                e.tell_target = (tx, ty);
                                crate::audio::emit("leaper", "leap_windup", Some((e.x, e.y)));
                            }
                        }
                    }
                    1 => {
                        // Winding up — standing still, glowing.
                        if e.leap_timer <= 0.0 {
                            e.leap_state = 2;
                            e.leap_timer = 0.55; // leap duration
                        }
                    }
                    2 => {
                        // Leaping — enemy.rs speed mul applies; target is
                        // the projected landing spot for direction.
                        if e.leap_timer <= 0.0 {
                            e.leap_state = 3;
                            e.leap_timer = 0.9; // recovery window
                            e.sprint_cooldown = 1.8;
                            crate::audio::emit("leaper", "leap_land", Some((e.x, e.y)));
                        }
                    }
                    _ => {
                        // Recovering — move slowly, can't leap.
                        if e.leap_timer <= 0.0 {
                            e.leap_state = 0;
                        }
                    }
                }
            }

            // Phaser behavior: shimmer in place, then teleport 6–9
            // tiles toward the target. Reuses `sprint_cooldown` as
            // the phase-hop cooldown and `tell_timer` as the
            // shimmer. Direction locked at tell-end; the teleport
            // is instantaneous + clamped to passable tiles so the
            // Phaser doesn't land inside a wall.
            if e.archetype == Archetype::Phaser && e.sprint_cooldown <= 0.0 {
                if let Some((_pid, tx, ty)) = target {
                    let dx = tx - e.x;
                    let dy = ty - e.y;
                    let d2 = dx * dx + dy * dy;
                    // Only phase when more than a few tiles away,
                    // so once the Phaser is on top of the player it
                    // holds position and swings instead of flickering
                    // through them.
                    if (3.5f32 * 3.5..=36.0 * 36.0).contains(&d2)
                        && self.arena.has_line_of_sight((e.x, e.y), (tx, ty))
                    {
                        if e.tell_timer <= 0.0 {
                            // Start shimmer.
                            e.tell_timer = 0.35;
                            e.tell_target = (tx, ty);
                        } else if e.tell_timer < 0.05 {
                            // Tell finished — snap forward. Jump
                            // distance scales with range so a
                            // long-range Phaser closes more gap, but
                            // never overshoots the player.
                            let len = d2.sqrt().max(0.001);
                            let hop = 7.5_f32.min(len - 2.0).max(2.0);
                            let nx = e.x + (dx / len) * hop;
                            let ny = e.y + (dy / len) * hop;
                            // Clamp to passable tile; fall back to
                            // staying put if the destination is a
                            // wall (rare given LoS, but defensive).
                            if self.arena.is_passable(nx as i32, ny as i32) {
                                e.x = nx;
                                e.y = ny;
                            }
                            e.sprint_cooldown = 1.4;
                            e.tell_timer = 0.0;
                        }
                    }
                }
            }

            // Rocketeer firing override. If cooldown is ready + LoS
            // to target, queue a rocket toward the target position.
            // Widened firing window (14–110 tiles vs 16–60) so
            // rocketeers lob from genuine standoff distance instead
            // of creeping into close range first.
            if e.archetype == Archetype::Rocketeer {
                if let Some((_pid, tx, ty)) = target {
                    let dx = tx - e.x;
                    let dy = ty - e.y;
                    let d2 = dx * dx + dy * dy;
                    let in_range = (14.0 * 14.0..=110.0 * 110.0).contains(&d2);
                    if in_range
                        && e.attack_cooldown <= 0.0
                        && self.arena.has_line_of_sight((e.x, e.y), (tx, ty))
                    {
                        if e.tell_timer <= 0.0 {
                            // Winding up.
                            e.tell_timer = 0.7;
                        } else if e.tell_timer < 0.05 {
                            // Tell just finished — fire.
                            rocketeer_shots.push((e.x, e.y, tx, ty));
                            e.tell_timer = 0.0;
                            e.attack_cooldown = 3.5;
                            crate::audio::emit("rocketeer", "rocket_fire", Some((e.x, e.y)));
                        }
                    }
                }
            }

            // Noise investigation override: enemies without a clear
            // Director / marked target, when a noise event tagged them,
            // chase the noise. Loud actions (wall breaks, rocket
            // blasts, player gunfire) populate `e.noise_target` in a
            // pre-pass above the enemy loop.
            let noise_override = if override_target.is_none()
                && eater_target.is_none()
                && marked_pos.is_none()
            {
                e.noise_target.map(|(nx, ny)| (0u32, nx, ny))
            } else {
                None
            };

            // Sneak mover: Marksman + Rocketeer bias toward cover when
            // moving — stepping toward a wall-adjacent cell en route to
            // the target instead of marching straight across open floor.
            let sneak_override =
                if matches!(e.archetype, Archetype::Marksman | Archetype::Rocketeer) {
                    target.map(|(_, tx, ty)| sneak_cover_target(e, (tx, ty), &self.arena))
                } else {
                    None
                };

            // Stationary override for turret-class archetypes — they
            // don't chase, only fire. Howler stays put to shriek.
            let stationary_override = if matches!(
                e.archetype,
                Archetype::Howler | Archetype::Sentinel | Archetype::PlayerTurret
            ) {
                Some((e.x, e.y))
            } else {
                None
            };

            // Leap lock-in: while leaping, direction is fixed.
            let leap_override = if e.archetype == Archetype::Leaper && e.leap_state == 2 {
                // Project landing spot 12 tiles along the leap vector.
                let lx = e.x + e.leap_vec.0 * 12.0;
                let ly = e.y + e.leap_vec.1 * 12.0;
                Some((lx, ly))
            } else {
                None
            };

            if let Some((pid, tx, ty)) = target.or(noise_override) {
                // Keep the *original* target (live player) for ranged
                // fire — stationary_override would otherwise point
                // Sentinels at themselves so they never shot. The
                // movement target is still overridden per-archetype.
                let fire_target = (tx, ty);
                let (tx, ty) = stationary_override
                    .or(leap_override)
                    .or(breacher_target_override)
                    .or(sneak_override)
                    .unwrap_or((tx, ty));
                e.update((tx, ty), &self.arena, dt);
                // Apply touch damage to the nearest player (not the marked
                // one specifically — attacks trigger on contact).
                for player in &mut self.players {
                    if player.hp <= 0 {
                        continue;
                    }
                    let base = e.touch_player(player.x, player.y);
                    if base > 0 {
                        let scale = if Some(player.id) == marked_pos.map(|(id, _, _)| id) {
                            1.25
                        } else {
                            1.0
                        };
                        let dmg = ((base as f32) * scale).round() as i32;
                        let was_alive = player.hp > 0;
                        player.take_damage_from_branded(
                            dmg,
                            Some(e.archetype),
                            e.brand_id,
                            Some("melee"),
                        );
                        if was_alive && player.hp <= 0 {
                            // Defer the kill-feed push out of this
                            // mutable-player borrow — we need `self`
                            // for `enemy_display_name`. Tag the slot
                            // with the enemy's archetype + brand and
                            // flush after the enemies loop exits.
                            pending_player_deaths.push((player.id, e.archetype, e.brand_id));
                        }
                        // Melee contact connected — touch_player gates
                        // this by touch_cooldown, so emits match the
                        // actual strike cadence rather than firing
                        // every tick of contact.
                        crate::audio::emit(e.archetype.audio_id(), "attack", Some((e.x, e.y)));
                    }
                }
                if e.is_ranged() {
                    if let Some(shot) =
                        e.try_ranged_attack(fire_target, &self.arena)
                    {
                        ranged_shots.push((pid, shot));
                        crate::audio::emit(
                            e.archetype.audio_id(),
                            "fire",
                            Some((e.x, e.y)),
                        );
                    }
                }
            }
            let burn_damage = e.tick_statuses(dt);
            if burn_damage > 0 && e.apply_damage(burn_damage) {
                enemy_status_kills.push(i);
            }
        }
        self.perf.end("enemy_loop");

        // Flush player-death entries staged during the enemies loop.
        // We push them AFTER the loop so the kill-feed helper can
        // use `self.content` for brand-flavoured enemy names.
        for (pid, arch, brand) in pending_player_deaths {
            let local = self.local_id == Some(pid);
            let victim = if local {
                "You".to_string()
            } else {
                format!("P{pid}")
            };
            let killer =
                self.content.enemy_display_name(arch, brand.as_ref().map(|t| t.as_str()));
            self.push_kill_feed(KillFeedEntry {
                killer,
                weapon: "melee".to_string(),
                victim,
                ttl: KillFeedEntry::DEFAULT_TTL,
                ttl_max: KillFeedEntry::DEFAULT_TTL,
                local_killer: false,
                local_victim: local,
            });
        }

        // Projectile-dodge pass. Agile archetypes scan nearby
        // in-flight projectiles (player-owned AND ally rockets) and
        // sidestep when one is about to pass through them. Runs
        // before separation so the dodge displacement is absorbed
        // into the same collision resolution. Ignoring owner_id is
        // the point — a Leaper stepping out of a friendly
        // Rocketeer's firing line is exactly the "clearing a path
        // for the kill" behavior we want.
        self.perf.begin("enemy_dodge");
        self.tick_enemy_dodges(dt);
        self.perf.end("enemy_dodge");

        // Soft pairwise separation so mobs visibly spread out when
        // they stack on the same tile. Not a hard collision — just
        // a push-apart proportional to overlap depth so a cluster
        // bulges into a ring instead of sharing one pixel. Turns a
        // 30-zergling breakout from "one fat dot" into "a wall of
        // teeth" — the oh-shit factor.
        self.perf.begin("enemy_separation");
        self.resolve_enemy_separation(dt);
        self.perf.end("enemy_separation");

        // Resolve ranged shots as actual in-flight projectile rounds
        // (kind=Pulse, from_ai=true). The round travels through the
        // world and hits the player on swept-segment contact via the
        // existing AI-pulse collision path — no more instant-hit
        // hitscan "laser" feel.
        for (_pid, shot) in ranged_shots {
            let dx = shot.to.0 - shot.from.0;
            let dy = shot.to.1 - shot.from.1;
            let len = (dx * dx + dy * dy).sqrt().max(0.0001);
            // Base projectile speed ~180 world px/s — feels fast
            // enough to threaten but slow enough to strafe at close
            // range, unlike the old hitscan.
            let speed = 180.0;
            let vx = dx / len * speed;
            let vy = dy / len * speed;
            // Spawn just in front of the muzzle so the enemy's own
            // hitbox doesn't eat its shot.
            let spawn_x = shot.from.0 + dx / len * 1.5;
            let spawn_y = shot.from.1 + dy / len * 1.5;
            let mut proj = crate::projectile::Projectile::pulse(
                spawn_x,
                spawn_y,
                vx,
                vy,
                shot.damage,
                0,
                Vec::new(),
            );
            proj.from_ai = true;
            // Longer ttl so long-range sniper shots don't expire mid-flight.
            proj.ttl = 2.5;
            self.projectiles.push(proj);
        }
        // Status kills don't get removed here; they feed into the unified
        // end-of-tick kill cleanup so we only swap_remove once per enemy.
        // Emit kill blasts + miniboss loot drops for status kills now, while
        // positions are still valid.
        for &idx in &enemy_status_kills {
            let info = self.enemies.get(idx).map(|e| {
                (
                    (e.x as i32, e.y as i32),
                    e.color(),
                    (e.x, e.y),
                    e.archetype,
                    e.archetype == Archetype::Miniboss,
                )
            });
            if let Some((tile, color, at, arch, is_miniboss)) = info {
                let seed = self.next_seed(tile);
                self.emit_gore_burst(at, color, arch, seed);
                if is_miniboss {
                    self.drop_miniboss_loot(arch, at.0, at.1);
                }
            }
        }

        // Sapper detonations: big explosion + a 3×3 wall-damage
        // ring + self-death. The death path runs through the normal
        // kill cleanup so body_on_death (explode_big) fires too,
        // stacking the bang.
        for (x, y, idx) in sapper_detonations.drain(..) {
            let seed = self.next_seed((x as i32, y as i32));
            self.apply_explode(x, y, 5.5, 40, seed);
            // Breach ring — Sapper's defining feature.
            for dx in -2..=2_i32 {
                for dy in -2..=2_i32 {
                    if dx.abs() + dy.abs() > 3 {
                        continue;
                    }
                    let tpos = (x as i32 + dx, y as i32 + dy);
                    self.damage_wall_with_particles(
                        tpos,
                        (tpos.0 as f32 + 0.5, tpos.1 as f32 + 0.5),
                        &[],
                    );
                }
            }
            // Mark the sapper for removal through the normal kill
            // pipeline by setting hp ≤ 0.
            if let Some(e) = self.enemies.get_mut(idx) {
                e.hp = 0;
            }
        }

        // Juggernaut wall smashes. Slower + stronger than Breacher
        // per swing but no A* — one wall at a time.
        for tile in juggernaut_smashes.drain(..) {
            let at = (tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
            for _ in 0..4 {
                self.damage_wall_with_particles(tile, at, &[]);
            }
            crate::audio::emit("juggernaut", "wall_hit", Some(at));
        }

        // Howler shrieks — big noise pings that gather the horde.
        for (x, y) in howler_shrieks.drain(..) {
            self.emit_noise(x, y, 90.0, 4.0);
        }

        // Floodling conversion pass: each Floodling within touch
        // range of a Horde enemy converts that enemy to Flood (or
        // FloodCarrier on a 30% roll) and dies itself. Order doesn't
        // matter — processed with swap_remove.
        // Pre-compute all seeds we need so we don't overlap self
        // borrows inside the scan.
        let mut conversions: Vec<(usize, Archetype)> = Vec::new();
        let mut dying_floodlings: Vec<usize> = Vec::new();
        let enemies_len = self.enemies.len();
        for fi in 0..enemies_len {
            let (fx, fy, arch) = {
                let f = &self.enemies[fi];
                (f.x, f.y, f.archetype)
            };
            if arch != Archetype::Floodling {
                continue;
            }
            if dying_floodlings.contains(&fi) {
                continue;
            }
            let mut victim: Option<(usize, f32, f32)> = None;
            for vi in 0..enemies_len {
                if vi == fi {
                    continue;
                }
                let v = &self.enemies[vi];
                // Skip already-Flood hosts (one-shot conversion) and
                // other Floodlings, and skip dead victims. Team-tag
                // check — generic; any future "flood-family" team
                // works without archetype list edits.
                let is_already_infected = v.team == crate::tag::Tag::new(crate::enemy::TEAM_FLOOD);
                if is_already_infected
                    || v.archetype == Archetype::Floodling
                    || v.hp <= 0
                {
                    continue;
                }
                let dx = v.x - fx;
                let dy = v.y - fy;
                if dx * dx + dy * dy < 2.6 * 2.6 {
                    victim = Some((vi, v.x, v.y));
                    break;
                }
            }
            if let Some((vi, vx, vy)) = victim {
                let seed = self.next_seed((vx as i32, vy as i32));
                let new_kind = if (seed & 0xFF) < 80 {
                    Archetype::FloodCarrier
                } else {
                    Archetype::Flood
                };
                conversions.push((vi, new_kind));
                dying_floodlings.push(fi);
            }
        }
        // Apply conversions by rebuilding the victim enemy with the
        // new archetype but preserving position.
        for (vi, new_arch) in conversions {
            if vi >= self.enemies.len() {
                continue;
            }
            let (x, y) = (self.enemies[vi].x, self.enemies[vi].y);
            let stats = self.content.stats(new_arch).clone();
            self.enemies[vi] = Enemy::spawn(new_arch, &stats, x, y);
            // Puff of green gore to sell the transformation.
            let seed = self.next_seed((x as i32, y as i32));
            self.emit_blast((x, y), Pixel::rgb(150, 255, 140), seed, 1);
        }
        // Kill Floodlings that converted someone. Process in reverse
        // index order so swap_remove stays stable.
        dying_floodlings.sort();
        dying_floodlings.dedup();
        for idx in dying_floodlings.into_iter().rev() {
            if idx < self.enemies.len() {
                self.enemies[idx].hp = 0;
            }
        }

        // Apply Breacher wall smashes. Each smash is worth the
        // equivalent of 3 wall HP so a cheap wall falls in one hit;
        // a thick perimeter takes a handful. Emits a visual flash via
        // the existing wall-damage helper.
        for tile in breacher_smashes.drain(..) {
            let at = (tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
            for _ in 0..3 {
                self.damage_wall_with_particles(tile, at, &[]);
            }
            crate::audio::emit("breacher", "wall_hit", Some(at));
        }

        // Fire queued Rocketeer shots. Rockets are Projectile-kind
        // Rocket with `from_ai = true` so they pass through other
        // enemies (no friendly fire) but detonate on players/walls.
        for (ex, ey, tx, ty) in rocketeer_shots.drain(..) {
            let dx = tx - ex;
            let dy = ty - ey;
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            let speed = 55.0;
            let vx = dx / len * speed;
            let vy = dy / len * speed;
            // Spawn from slightly in front of the Rocketeer so their
            // own hitbox doesn't eat the shot.
            let spawn_x = ex + dx / len * 2.5;
            let spawn_y = ey + dy / len * 2.5;
            self.projectiles.push(crate::projectile::Projectile::rocket(
                spawn_x,
                spawn_y,
                vx,
                vy,
                Vec::new(), // baseline rocketeers fire unmodded; primitives come later via content
                true,
            ));
        }

        // Eater consume pass: collect (enemy_idx, corpse_idx) pairs
        // while we borrow immutably, then apply mutations afterward so
        // the borrow checker lets us touch `self` freely for blasts +
        // substance paints.
        let mut consume_plan: Vec<(usize, usize)> = Vec::new();
        let mut claimed_corpses: Vec<usize> = Vec::new();
        for (ei, e) in self.enemies.iter().enumerate() {
            if e.archetype != Archetype::Eater {
                continue;
            }
            let mut best: Option<(usize, f32)> = None;
            for (ci, c) in self.corpses.iter().enumerate() {
                if claimed_corpses.contains(&ci) {
                    continue;
                }
                let dx = c.x - e.x;
                let dy = c.y - e.y;
                let d2 = dx * dx + dy * dy;
                if d2 <= EATER_CONSUME_RADIUS * EATER_CONSUME_RADIUS
                    && best.map_or(true, |(_, bd)| d2 < bd)
                {
                    best = Some((ci, d2));
                }
            }
            if let Some((ci, _)) = best {
                claimed_corpses.push(ci);
                consume_plan.push((ei, ci));
            }
        }
        let blood_id = self.content.blood_pool_id;
        for (ei, ci) in consume_plan.iter().copied() {
            if ci >= self.corpses.len() || ei >= self.enemies.len() {
                continue;
            }
            let (cx, cy) = (self.corpses[ci].x, self.corpses[ci].y);
            let arch = self.corpses[ci].archetype;
            let gib = crate::corpse::gib_color(arch);
            {
                let e = &mut self.enemies[ei];
                e.hp += EATER_HP_PER_CONSUME;
                e.consumed = e.consumed.saturating_add(1);
            }
            let seed = self.next_seed((cx as i32, cy as i32));
            self.emit_blast((cx, cy), gib, seed, 2);
            self.paint_substance(cx as i32, cy as i32, blood_id, 0);
        }
        // Remove corpses in reverse-index order so swap_remove stays stable.
        let mut corpse_indices: Vec<usize> = consume_plan.iter().map(|(_, ci)| *ci).collect();
        corpse_indices.sort();
        corpse_indices.dedup();
        for idx in corpse_indices.into_iter().rev() {
            if idx < self.corpses.len() {
                self.corpses.swap_remove(idx);
            }
        }

        self.perf.begin("projectile_step");
        // Step projectiles + resolve impacts. `retain_mut` keeps piercing /
        // ricocheting projectiles alive; despawns others.
        //
        // Collision uses swept segment tests, not endpoint point-tests, so
        // fast projectiles can't tunnel through walls/enemies/corpses in
        // one tick. `p.update` runs a DDA ray-cast for walls; we follow
        // with segment-vs-circle sweeps for enemies and corpses and pick
        // the earliest contact (smallest `t`).
        let mut wall_hits: Vec<((f32, f32), (i32, i32), Vec<Primitive>)> = Vec::new();
        let mut enemy_hits: Vec<(usize, (f32, f32), i32, u32, Vec<Primitive>)> = Vec::new();
        // Corpse hits captured with the projectile's unit direction
        // so the gore spray can fire forward along the bullet path.
        let mut corpse_hits: Vec<(usize, (f32, f32), (f32, f32))> = Vec::new();
        // Rocket detonations queued this tick: (pos, primitives, from_ai).
        // Applied after the projectile retain loop so we can mutate
        // world state freely.
        let mut rocket_detonations: Vec<((f32, f32), Vec<Primitive>, bool)> = Vec::new();
        // AI pulse projectile hits queued: (player_idx, damage). Flat
        // damage application after the retain loop releases its
        // borrow.
        let mut ai_pulse_hits: Vec<(usize, i32)> = Vec::new();

        self.projectiles.retain_mut(|p| {
            let from = (p.x, p.y);
            let outcome = p.update(&self.arena, dt);
            if matches!(outcome, ProjectileOutcome::Expired) {
                // Grenades detonate on fuse expiry even if they
                // never hit anything — that's the "lobbed timed
                // explosive" part. Rockets just fizzle at TTL.
                if p.kind == crate::projectile::ProjectileKind::Grenade {
                    rocket_detonations.push(((p.x, p.y), p.primitives.clone(), p.from_ai));
                }
                return false;
            }
            // `to` is where the projectile actually ended this tick
            // (just before any wall it struck). Swept tests run on
            // [from..to]; anything past `to` is behind a wall.
            let to = (p.x, p.y);

            // AI-fired projectiles check player collision first.
            // Rockets + Grenades detonate on contact (explosion
            // damage); Pulse rounds apply flat damage and despawn.
            // Runs before enemy scan since player kill is the
            // intended path for AI fire.
            if p.from_ai {
                let mut best_player: Option<(usize, f32)> = None;
                for (i, pl) in self.players.iter().enumerate() {
                    if pl.hp <= 0 {
                        continue;
                    }
                    if let Some(t) =
                        segment_circle_first_t(from, to, (pl.x, pl.y), 2.2)
                    {
                        if best_player.map_or(true, |(_, bt)| t < bt) {
                            best_player = Some((i, t));
                        }
                    }
                }
                if let Some((idx, t)) = best_player {
                    let (px, py) = lerp_pt(from, to, t);
                    if p.kind.explodes_on_impact() {
                        rocket_detonations.push((
                            (px, py),
                            p.primitives.clone(),
                            p.from_ai,
                        ));
                    } else {
                        // Pulse: queue direct damage to this player.
                        ai_pulse_hits.push((idx, p.damage));
                    }
                    return false;
                }
            }

            // Nearest enemy along the segment. AI-fired projectiles
            // skip enemy-on-enemy collision so Rocketeers don't blow
            // up their own lineup.
            let mut best_enemy: Option<(usize, f32)> = None;
            if !p.from_ai {
                for (i, e) in self.enemies.iter().enumerate() {
                    if let Some(t) = segment_circle_first_t(from, to, (e.x, e.y), e.hit_radius()) {
                        if best_enemy.map_or(true, |(_, bt)| t < bt) {
                            best_enemy = Some((i, t));
                        }
                    }
                }
            }
            if let Some((idx, t)) = best_enemy {
                let (px, py) = lerp_pt(from, to, t);
                // Rockets detonate on enemy contact instead of doing
                // flat damage — the explosion does the real work and
                // will rip through anything in its radius.
                if p.kind.explodes_on_impact() {
                    rocket_detonations.push(((px, py), p.primitives.clone(), p.from_ai));
                    return false;
                }
                enemy_hits.push((idx, (px, py), p.damage, p.owner_id, p.primitives.clone()));
                if p.pierces_left > 0 {
                    p.pierces_left -= 1;
                    let r = self.enemies[idx].hit_radius();
                    nudge_past(p, (px, py), r);
                    return true;
                }
                return false;
            }

            // No enemy hit — check corpses next.
            let mut best_corpse: Option<(usize, f32)> = None;
            for (i, c) in self.corpses.iter().enumerate() {
                if let Some(t) = segment_circle_first_t(from, to, (c.x, c.y), Corpse::HIT_RADIUS) {
                    if best_corpse.map_or(true, |(_, bt)| t < bt) {
                        best_corpse = Some((i, t));
                    }
                }
            }
            if let Some((idx, t)) = best_corpse {
                let (px, py) = lerp_pt(from, to, t);
                if p.kind.explodes_on_impact() {
                    rocket_detonations.push(((px, py), p.primitives.clone(), p.from_ai));
                    return false;
                }
                // Capture the projectile's unit direction so the
                // gore sprays *forward* along the bullet path.
                let speed = (p.vx * p.vx + p.vy * p.vy).sqrt().max(0.001);
                let dir = (p.vx / speed, p.vy / speed);
                corpse_hits.push((idx, (px, py), dir));
                if p.pierces_left > 0 {
                    p.pierces_left -= 1;
                    nudge_past(p, (px, py), Corpse::HIT_RADIUS);
                    return true;
                }
                return false;
            }

            // Rocket kind detonates on ANY impact — even if no enemy/
            // corpse/wall hit this tick, we still let the pulse-style
            // fallthrough happen; rockets just explode differently.
            match outcome {
                ProjectileOutcome::HitWall { at, tile } => {
                    if p.kind.explodes_on_impact() && p.bounces_left == 0 {
                        // Rocket / Grenade detonation on wall: schedule an
                        // explosion via the queued-detonation list so we
                        // apply after the retain loop releases its borrow.
                        rocket_detonations.push((at, p.primitives.clone(), p.from_ai));
                        wall_hits.push((at, tile, p.primitives.clone()));
                        return false;
                    }
                    wall_hits.push((at, tile, p.primitives.clone()));
                    if p.bounces_left > 0 {
                        p.reflect_off(tile);
                        p.bounces_left -= 1;
                        return true;
                    }
                    false
                }
                _ => true,
            }
        });

        // Apply AI pulse projectile hits — flat damage to the
        // struck player, armor absorption handled inside take_damage.
        for (idx, damage) in ai_pulse_hits.drain(..) {
            if let Some(p) = self.players.get_mut(idx) {
                if p.hp > 0 {
                    p.take_damage(damage);
                }
            }
        }

        // Apply queued rocket detonations. Each one triggers the full
        // Explode reaction modulated by the rocket's primitive list
        // (Ignite adds burn; Chain adds shock arc; Breach widens wall
        // damage) and paints a cinematic gore burst.
        for (pos, primitives, from_ai) in rocket_detonations.drain(..) {
            let seed = self.next_seed((pos.0 as i32, pos.1 as i32));
            self.detonate_rocket(pos, &primitives, seed, from_ai);
        }

        // Apply wall damage + Breach ring.
        for (at, tile, primitives) in wall_hits {
            self.damage_wall_with_particles(tile, at, &primitives);
            if primitives.contains(&Primitive::Breach) {
                for dx in -1..=1_i32 {
                    for dy in -1..=1_i32 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let t = (tile.0 + dx, tile.1 + dy);
                        self.damage_wall_with_particles(
                            t,
                            (t.0 as f32 + 0.5, t.1 as f32 + 0.5),
                            &[],
                        );
                    }
                }
            }
        }

        // Apply enemy damage + Chain / Ignite triggers.
        let mut kills_this_tick: Vec<(usize, u32)> = Vec::new(); // (enemy_idx, killer_id)
        for (idx, at, dmg, owner_id, primitives) in enemy_hits.iter().rev() {
            // Compute modified damage up-front so primitives like
            // ShieldBreak can boost against armored targets.
            let raw_dmg = *dmg;
            let (died, color, tile, arch, is_miniboss, pos, was_burning, effective_dmg) = match self
                .enemies
                .get_mut(*idx)
            {
                Some(e) => {
                    if primitives.contains(&Primitive::Ignite) {
                        e.apply_burn(12.0, 2.5);
                    }
                    if primitives.contains(&Primitive::Cryo) {
                        e.apply_frost(3.0);
                    }
                    // ShieldBreak: +50% damage vs armored/heavy targets.
                    let armored = e.is_armored(&self.content);
                    let mut effective = raw_dmg;
                    if armored && primitives.contains(&Primitive::ShieldBreak) {
                        effective = ((effective as f32) * 1.5).round() as i32;
                    }
                    // Cryo-at-3-stacks: shatter crit +100%.
                    let shatter = matches!(&e.frost, Some(f) if f.stacks >= 3);
                    if shatter {
                        effective *= 2;
                    }
                    let was_burning = e.burn.is_some();
                    let died = e.apply_damage(effective);
                    let color = if died { e.color() } else { e.gib_color() };
                    let arch = e.archetype;
                    (
                        died,
                        color,
                        (e.x as i32, e.y as i32),
                        arch,
                        arch == Archetype::Miniboss,
                        (e.x, e.y),
                        was_burning,
                        effective,
                    )
                }
                None => continue,
            };
            // Credit the shooting player with the damage they landed.
            // owner_id == 0 is a house-fired projectile (chain arc,
            // gravity pull, or other indirect effect) — no player to
            // credit.
            if *owner_id != 0 && effective_dmg > 0 {
                if let Some(pi) = self.players.iter_mut().find(|p| p.id == *owner_id) {
                    pi.note_damage_dealt(effective_dmg.max(0) as u32);
                }
            }
            let seed = self.next_seed(tile);
            if died {
                self.emit_gore_burst(*at, color, arch, seed);
            } else {
                self.emit_blast(*at, color, seed, 0);
            }

            // Acid — paint a small acid pool around the hit point.
            // Sized to enemy footprint; multiple hits in the same
            // area still overlap into a proper corrosive lake.
            if primitives.contains(&Primitive::Acid) {
                if let Some(id) = self.content.substances.by_name("acid_pool") {
                    let s = seed.wrapping_mul(0xACAD_BEEF);
                    self.paint_substance_radius(at.0, at.1, id, 30, 2.5, s);
                }
            }

            // Ignite — paint a small fire disk at the hit. Just
            // big enough that the fire-propagation pass has a few
            // flammable neighbors to cascade into without the
            // single shot blanketing a room.
            if primitives.contains(&Primitive::Ignite) {
                let fire_id = self.content.fire_id;
                let s = seed.wrapping_mul(0x1F11E_F00D_u64);
                self.paint_substance_radius(at.0, at.1, fire_id, 0, 3.0, s);
            }

            // Contagion — if target was burning, burn two nearest
            // enemies too (short-range transmission).
            if primitives.contains(&Primitive::Contagion) && was_burning {
                self.contagion_spread(pos, 8.0, 2);
            }

            // GravityWell — pull 3 nearest enemies 1.5 tiles toward
            // the hit point.
            if primitives.contains(&Primitive::GravityWell) {
                self.gravity_pull(*at, 10.0, 3, 1.5);
            }

            if primitives.contains(&Primitive::Chain) {
                self.chain_from(*idx, (*dmg as f32 * 0.5) as i32, 12.0, 2, &mut kills_this_tick);
            }

            if died {
                kills_this_tick.push((*idx, *owner_id));
                if is_miniboss {
                    self.drop_miniboss_loot(arch, pos.0, pos.1);
                    self.add_corruption(5.0);
                }
                // Siphon — restore sanity on every credited kill.
                if primitives.contains(&Primitive::Siphon) && *owner_id != 0 {
                    if let Some(pi) = self.player_index(*owner_id) {
                        self.players[pi].sanity =
                            (self.players[pi].sanity + 8.0).min(100.0);
                    }
                }
            }
        }

        // Apply corpse hits: punch holes deterministically, spawn a
        // visible gore burst, splatter blood on the ground, and
        // collapse the corpse into a pool when its integrity runs
        // out. Uses the Heavy gore profile so each shot produces a
        // meaty reaction — shooting a body should look + feel like
        // violence, not a muzzle flicker.
        //
        // Sync: broadcast the hit via `tick_corpse_hits`; the
        // CorpseHit reliable msg carries the seed and client-side
        // `apply_corpse_hit` replays the identical gore + hole
        // pattern. Host does NOT also `emit_blast` here — that'd
        // double-broadcast a Blast + a CorpseHit for the same shot.
        let blood_id = self.content.blood_pool_id;
        for (idx, at, dir) in corpse_hits.iter().rev() {
            if *idx >= self.corpses.len() {
                continue;
            }
            let seed = self.next_seed((at.0 as i32, at.1 as i32));
            let (color, destroyed, pos) = {
                let c = &mut self.corpses[*idx];
                let color = c.apply_hit(seed);
                (color, c.is_destroyed(), (c.x, c.y))
            };
            self.tick_corpse_hits.push((
                self.corpses[*idx].id,
                seed,
                dir.0,
                dir.1,
            ));
            // Directional corpse-hit gore: small burst (~40 particles)
            // sprayed forward along the bullet path, not an
            // omnidirectional explosion. Much more restrained than
            // the enemy-death Heavy profile.
            let local_blast = DestructionBlast {
                x: at.0,
                y: at.1,
                color_rgb: [color.r, color.g, color.b],
                seed,
                intensity: 1,
                gore_tier: crate::gore::GoreTier::CorpseHit.to_byte(),
                dir_x: dir.0,
                dir_y: dir.1,
            };
            self.apply_blast(&local_blast);
            // Small blood splatter on the tile so the ground shows
            // accumulated damage even before the corpse collapses.
            self.paint_substance(at.0 as i32, at.1 as i32, blood_id, 140);
            if destroyed {
                // Collapse into a blood-pool decal on the ground, then
                // drop the corpse from the authoritative list.
                let (tx, ty) = (pos.0 as i32, pos.1 as i32);
                self.paint_substance(tx, ty, blood_id, 10);
                for (dx, dy) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if ((seed >> (dx + dy + 2)) & 1) == 1 {
                        self.paint_substance(tx + dx, ty + dy, blood_id, 40);
                    }
                }
                self.corpses.swap_remove(*idx);
            }
        }

        // Merge status kills into the unified stream (killer_id = 0: DoT has
        // no "firer" to credit Overdrive to).
        for idx in enemy_status_kills.iter().copied() {
            kills_this_tick.push((idx, 0));
        }
        // Cleanup kills (sort, reverse, swap_remove) once. Credit Overdrive
        // on each kill to the firing weapon while we still have the killer id.
        kills_this_tick.sort_by(|a, b| a.0.cmp(&b.0));
        kills_this_tick.dedup_by(|a, b| a.0 == b.0);
        for (idx, killer_id) in kills_this_tick.into_iter().rev() {
            if idx >= self.enemies.len() {
                continue;
            }
            // Kill-feed entry for this kill. Captured BEFORE we
            // swap_remove the enemy so we can name it. `killer_id == 0`
            // means DoT / bleed with no crediting weapon — we render
            // those with a dash as the weapon so the player knows it
            // was attrition, not a direct hit.
            let enemy_arch = self.enemies[idx].archetype;
            let enemy_brand = self.enemies[idx].brand_id;
            let victim = self.content.enemy_display_name(
                enemy_arch,
                enemy_brand.as_ref().map(|t| t.as_str()),
            );
            let (killer_name, weapon_label, local_killer) = if killer_id == 0 {
                ("—".to_string(), "bleed".to_string(), false)
            } else {
                let local = self.local_id == Some(killer_id);
                let killer_name = if local {
                    "You".to_string()
                } else {
                    format!("P{killer_id}")
                };
                let weapon = self
                    .player_index(killer_id)
                    .and_then(|i| self.runtimes[i].active_weapon().map(|w| w.mode.label()))
                    .unwrap_or("—")
                    .to_string();
                (killer_name, weapon, local)
            };
            self.push_kill_feed(KillFeedEntry {
                killer: killer_name,
                weapon: weapon_label,
                victim,
                ttl: KillFeedEntry::DEFAULT_TTL,
                ttl_max: KillFeedEntry::DEFAULT_TTL,
                local_killer,
                local_victim: false,
            });
            // Credit Overdrive to the killer's active weapon, if any.
            // Also notify the player so perk-side kill hooks
            // (CorpsePulse heal, Rampage chain) fire.
            if killer_id != 0 {
                if let Some(i) = self.player_index(killer_id) {
                    if let Some(w) = self.runtimes[i].active_weapon_mut() {
                        w.note_kill();
                    }
                    self.players[i].note_kill();
                }
            }
            // Small Corruption bump per kill; miniboss handled below via
            // the earlier is_miniboss path.
            self.add_corruption(0.2);
            // Spawn a persistent corpse before swap_remove so the body
            // stays in the world for players to walk over, shoot up,
            // etc. Carries the enemy's archetype for sprite lookup.
            let (cx, cy, arch) = {
                let e = &self.enemies[idx];
                (e.x, e.y, e.archetype)
            };
            self.spawn_corpse(arch, cx, cy);
            // Fire the death sample for this archetype; no-op if no
            // audio pool exists for it yet.
            crate::audio::emit(arch.audio_id(), "death", Some((cx, cy)));
            self.enemies.swap_remove(idx);
            self.kills += 1;
            // Random small consumable drop so the floor stays loot-
            // worthy between miniboss waves. 5% per normal kill;
            // miniboss + boss drops flow through `drop_miniboss_loot`
            // so they aren't doubled here.
            if !matches!(arch, Archetype::Miniboss | Archetype::Eater | Archetype::Killa) {
                // Butcher perk: any surviving player carrying it
                // doubles the drop chance. Team-wide benefit.
                let butcher = self
                    .players
                    .iter()
                    .any(|p| p.hp > 0 && p.has_perk(crate::perk::Perk::Butcher));
                let chance = if butcher { 0.10 } else { 0.05 };
                self.maybe_drop_consumable_loot(cx, cy, chance);
                // Rare archetype-signature weapon drop on top of the
                // consumable roll. Only archetypes with an authored
                // signature eligible — keeps floor-spam in check.
                self.maybe_drop_signature_weapon(arch, cx, cy);
            }
            // Death scream — every dying body makes some noise that
            // draws nearby allies. Miniboss deaths are deafening;
            // swarmling pops are barely audible.
            let scream_radius = match arch {
                Archetype::Miniboss | Archetype::Eater => 70.0,
                Archetype::Pinkie | Archetype::Breacher => 35.0,
                Archetype::Swarmling => 6.0,
                _ => 18.0,
            };
            self.emit_noise(cx, cy, scream_radius, 1.6);

            // Fire the archetype's body_on_death reaction (if any).
            // Content-driven: archetype TOML names a reaction registered
            // in the ReactionRegistry; the bind happens here.
            if let Some(name) = self.content.stats(arch).body_on_death.clone() {
                let seed = self.next_seed((cx as i32, cy as i32));
                self.fire_body_reaction(&name, cx, cy, seed);
            }
            // Also fire tag-driven OnDeath rules from the interaction
            // book — this is where emergent spawns (Corpse-Eater raise),
            // explosive volatility etc. live. Every archetype tag gets
            // checked; every matching rule rolls its chance.
            let tag_strs = self
                .content
                .stats(arch)
                .tags
                .clone()
                .unwrap_or_default();
            if !tag_strs.is_empty() {
                let mut rng = SmallRng::seed_from_u64(
                    self.next_seed((cx as i32, cy as i32)).wrapping_mul(0xBF58_476D),
                );
                for tag_str in &tag_strs {
                    let tag = Tag::new(tag_str);
                    let rules: Vec<crate::interaction::InteractionRule> = self
                        .interactions
                        .event_rules(tag, crate::interaction::Event::OnDeath)
                        .to_vec();
                    for rule in rules {
                        if rng.r#gen::<f32>() <= rule.chance {
                            let seed = self.next_seed((cx as i32, cy as i32));
                            self.apply_reaction(&rule.reaction, cx, cy, seed);
                        }
                    }
                }
            }
        }

        self.perf.end("projectile_step");

        self.perf.begin("particle_tick");
        self.particles.retain_mut(|p| p.update(dt));
        self.perf.end("particle_tick");
        self.hitscans.retain_mut(|h| {
            h.ttl -= dt;
            h.ttl > 0.0
        });

        // Death cinematic trigger. Every living player is down →
        // kick off the DeathPhase state machine (if not already
        // running). `alive` stays TRUE until the cinematic finishes
        // and the player dismisses the report — that way the outer
        // loop doesn't short-circuit into the old "any-key-exits"
        // path on frame one.
        if self.death_phase.is_none()
            && !self.players.is_empty()
            && self.players.iter().all(|p| p.hp <= 0)
        {
            self.enter_death_phase();
        }
        self.tick_death_phase(dt);

        // Population counters for the telemetry dump — helps pinpoint
        // "N particles alive" type slowdowns.
        self.perf.set_count("enemies", self.enemies.len());
        self.perf.set_count("corpses", self.corpses.len());
        self.perf.set_count("projectiles", self.projectiles.len());
        self.perf.set_count("particles", self.particles.len());
        self.perf.set_count("pickups", self.pickups.len());
        self.perf.end("tick_total");
    }

    /// Shared helper: damage a wall tile and emit the corresponding blast +
    /// tile-update event. Used by direct projectile hits and by Breach's
    /// adjacent-tile explosion.
    fn damage_wall_with_particles(
        &mut self,
        tile: (i32, i32),
        at: (f32, f32),
        extra_primitives: &[Primitive],
    ) {
        let Some(outcome) = self.arena.damage_wall(tile.0, tile.1) else {
            return;
        };
        let seed = self.next_seed(tile);
        let color = if outcome.destroyed {
            outcome.material.intact_color()
        } else {
            outcome.material.debris_color()
        };
        let mut intensity = if outcome.destroyed { 1 } else { 0 };
        if extra_primitives.contains(&Primitive::Ignite) {
            intensity = 1; // always-full on ignited wall hits, looks punchier
        }
        self.emit_blast(at, color, seed, intensity);
        self.emit_tile_update(tile);
        // Persistent scorch on destruction — the ground under a blown
        // wall carries the mark for the rest of the run so arena decay
        // reads as a visible story.
        if outcome.destroyed {
            let base = if extra_primitives.contains(&Primitive::Ignite) {
                160
            } else {
                90
            };
            let scorch_id = self.content.scorch_id;
            self.paint_substance(tile.0, tile.1, scorch_id, base);
            // Loud! Draws the horde.
            self.emit_noise(at.0, at.1, 35.0, 2.5);
            // Frostbite-feel flying debris: 1-3 chunks land in nearby
            // passable tiles and form persistent DebrisPile objects
            // that block LoS but are walkable. Shooting them later
            // chips them down into particle spray.
            self.spawn_wall_debris(tile, outcome.material);
        } else {
            // Quieter chip-away noise — shorter range, shorter TTL.
            self.emit_noise(at.0, at.1, 14.0, 0.8);
        }
    }

    /// Spawn a small cluster of DebrisPile objects around a newly-
    /// destroyed wall. Chunks prefer adjacent passable tiles (inside
    /// the room / on open floor). Broadcast via the existing
    /// tile-update-ish path would require a new proto message; for
    /// now objects sync only via WorldSync on late-join, which is
    /// fine since they're decorative / cover and don't carry state
    /// other peers need in real time.
    fn spawn_wall_debris(&mut self, tile: (i32, i32), material: Material) {
        let seed = self.next_seed(tile);
        // How many chunks fly: 1-3 based on seed bits.
        let chunk_count = 1 + ((seed >> 5) & 0x3) as i32;
        let offsets: &[(i32, i32)] = &[
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (2, 0),
            (-2, 0),
            (0, 2),
            (0, -2),
        ];
        let mut landed = 0;
        // Seeded shuffle via index wheel starting from a seeded offset.
        let start = (seed as usize) % offsets.len();
        for i in 0..offsets.len() {
            if landed >= chunk_count {
                break;
            }
            let (dx, dy) = offsets[(start + i) % offsets.len()];
            let tx = tile.0 + dx;
            let ty = tile.1 + dy;
            // Only drop on passable floor — skip walls, skip
            // existing objects.
            if !self.arena.is_passable(tx, ty) {
                continue;
            }
            if self.arena.object(tx, ty).is_some() {
                continue;
            }
            self.arena
                .set_object(tx, ty, Object::DebrisPile { material, remaining: 2 });
            landed += 1;
        }
    }

    /// Contagion: spread a burn status to the `n` nearest other
    /// enemies within `radius` of the origin. Quick + cheap — just
    /// scans the enemy list once.
    fn contagion_spread(&mut self, origin: (f32, f32), radius: f32, n: usize) {
        let r2 = radius * radius;
        let mut candidates: Vec<(usize, f32)> = self
            .enemies
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let dx = e.x - origin.0;
                let dy = e.y - origin.1;
                let d2 = dx * dx + dy * dy;
                if d2 < r2 { Some((i, d2)) } else { None }
            })
            .collect();
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        for (i, _) in candidates.into_iter().take(n) {
            if let Some(e) = self.enemies.get_mut(i) {
                e.apply_burn(8.0, 2.0);
            }
        }
    }

    /// GravityWell: yank the `n` nearest enemies `strength` tiles
    /// toward `origin`. No cross-target hitstop, just a brief pull.
    fn gravity_pull(
        &mut self,
        origin: (f32, f32),
        radius: f32,
        n: usize,
        strength: f32,
    ) {
        let r2 = radius * radius;
        let mut candidates: Vec<(usize, f32)> = self
            .enemies
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let dx = e.x - origin.0;
                let dy = e.y - origin.1;
                let d2 = dx * dx + dy * dy;
                if d2 < r2 { Some((i, d2)) } else { None }
            })
            .collect();
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        for (i, d2) in candidates.into_iter().take(n) {
            if d2 < 0.25 {
                continue; // already basically on top
            }
            if let Some(e) = self.enemies.get_mut(i) {
                let dx = origin.0 - e.x;
                let dy = origin.1 - e.y;
                let len = (dx * dx + dy * dy).sqrt().max(0.001);
                e.x += dx / len * strength;
                e.y += dy / len * strength;
            }
        }
    }

    /// Per-tick projectile-dodge scan. For every agile archetype,
    /// check the projectile list for anything on a near-collision
    /// course; if the projectile will pass within `dodge_radius` of
    /// this enemy inside the next `LOOKAHEAD_SECS`, sidestep
    /// perpendicular to the projectile's velocity (biased away from
    /// the projectile's line).
    ///
    /// Ignores `from_ai` — dodging applies to player bullets AND
    /// ally rockets alike. That naturally produces the "horde
    /// clears a path for the Rocketeer's shot" emergent behavior
    /// the design calls for; an agile Leaper watching a friendly
    /// rocket trace toward the player will step sideways rather
    /// than stand in the blast path.
    fn tick_enemy_dodges(&mut self, dt: f32) {
        /// How far ahead in time we extrapolate projectiles. At
        /// ~0.45s, a 140 world-px/s pulse round covers 63 tiles —
        /// enough runway that dodging enemies react visibly before
        /// the round arrives, not after it's already inside them.
        const LOOKAHEAD_SECS: f32 = 0.45;
        /// Lateral miss margin that still counts as "about to hit."
        /// Tuned to `hit_radius + this` so only genuine threats
        /// trigger dodges; bullets whistling past don't.
        const LATERAL_MARGIN: f32 = 1.4;
        /// Seconds of forward commitment once a dodge starts. Short
        /// enough to leave the firing line without decoupling from
        /// the target; long enough to be visually legible as "that
        /// Leaper just jinked."
        const DODGE_DURATION: f32 = 0.22;
        /// Cooldown after a dodge before the same enemy can dodge
        /// again. Without this, a Leaper under sustained fire would
        /// dodge every frame and never close.
        const DODGE_COOLDOWN: f32 = 0.9;

        // Decay in-flight dodge + cooldown timers before checking
        // for new threats — an enemy already committed to a dodge
        // doesn't re-trigger.
        for e in self.enemies.iter_mut() {
            if e.dodge_timer > 0.0 {
                e.dodge_timer = (e.dodge_timer - dt).max(0.0);
            }
            if e.dodge_cooldown > 0.0 {
                e.dodge_cooldown = (e.dodge_cooldown - dt).max(0.0);
            }
        }

        if self.projectiles.is_empty() {
            return;
        }

        // Snapshot projectile kinematics so the inner loop doesn't
        // borrow Game twice. Only projectiles with non-trivial
        // velocity and meaningful ttl contribute — expiring
        // projectiles aren't worth dodging.
        struct Threat {
            x: f32,
            y: f32,
            vx: f32,
            vy: f32,
            speed: f32,
        }
        let threats: Vec<Threat> = self
            .projectiles
            .iter()
            .filter_map(|p| {
                let v2 = p.vx * p.vx + p.vy * p.vy;
                if v2 < 1.0 || p.ttl <= 0.05 {
                    return None;
                }
                Some(Threat {
                    x: p.x,
                    y: p.y,
                    vx: p.vx,
                    vy: p.vy,
                    speed: v2.sqrt(),
                })
            })
            .collect();
        if threats.is_empty() {
            return;
        }

        for e in self.enemies.iter_mut() {
            if e.hp <= 0 || e.dodge_cooldown > 0.0 || e.dodge_timer > 0.0 {
                continue;
            }
            if !e.archetype.dodges_projectiles() {
                continue;
            }
            let er = e.hit_radius() + LATERAL_MARGIN;
            let ex = e.x;
            let ey = e.y;
            for t in &threats {
                // Displacement from projectile → enemy.
                let dx = ex - t.x;
                let dy = ey - t.y;
                // Unit velocity of the projectile.
                let inv_speed = 1.0 / t.speed;
                let ux = t.vx * inv_speed;
                let uy = t.vy * inv_speed;
                // Along-track + cross-track components in proj frame.
                let along = dx * ux + dy * uy;
                if along < 0.0 {
                    // Projectile already past the enemy.
                    continue;
                }
                let cross = dx * uy - dy * ux; // signed lateral offset
                if cross.abs() > er {
                    // Projectile will miss laterally.
                    continue;
                }
                // Time until the projectile's closest approach to the
                // enemy. Speed is non-zero per earlier check.
                let t_close = along / t.speed;
                if t_close > LOOKAHEAD_SECS {
                    continue;
                }
                // Commit a dodge perpendicular to projectile heading,
                // biased AWAY from the enemy's current cross offset.
                // If cross > 0 the enemy is on the +perpendicular
                // side of the line — push further +perpendicular.
                // Flip sign if cross < 0. `cross == 0` (dead-on hit)
                // picks +perpendicular deterministically.
                let sign = if cross >= 0.0 { 1.0 } else { -1.0 };
                e.dodge_vec = (uy * sign, -ux * sign);
                e.dodge_timer = DODGE_DURATION;
                e.dodge_cooldown = DODGE_COOLDOWN;
                break;
            }
        }
    }

    /// Chain splash: find `n` nearest other enemies within `radius` of
    /// `from_idx` and apply `damage` to each. Kills are pushed into
    /// `kills_out` for caller-side cleanup.
    /// Pairwise soft-push for overlapping enemies. Accumulates a
    /// displacement per enemy from every pair whose circles overlap,
    /// then applies it in a second pass so evaluation order doesn't
    /// bias who gets pushed. Each overlap contributes half its depth
    /// to each side, scaled by `PUSH_STIFFNESS * dt` — stiff enough
    /// to visibly spread a 20-swarmling cluster, soft enough that a
    /// cohesive group still chases a player as one mass.
    ///
    /// Stationary archetypes (turrets, sentinels with `base_speed == 0`)
    /// stay put and act as immovable collision boundaries — mobile
    /// enemies flow around them.
    fn resolve_enemy_separation(&mut self, dt: f32) {
        /// Overlap-depth × this × dt = world-units pushed per frame.
        /// Higher = snappier expansion; too high makes swarms jitter.
        const PUSH_STIFFNESS: f32 = 12.0;
        /// Extra "personal-space" radius added to each pair's
        /// min-distance threshold. Pushes apart even before true
        /// hit-box overlap — small value, big feel: the horde
        /// reads as a spreading wave with visible gaps between mobs
        /// instead of a dense monolithic pack. ~0.75 tiles is
        /// enough for zerglings (hit_radius 1.5) to feel "loose"
        /// without breaking the bunch-at-the-door moments.
        const SEPARATION_PADDING: f32 = 0.75;

        let n = self.enemies.len();
        if n < 2 {
            return;
        }
        // Flat vector of accumulated (dx, dy) displacements per enemy.
        // Using Vec + index loop dodges the borrow-checker dance that
        // split_at_mut would incur.
        let mut deltas: Vec<(f32, f32)> = vec![(0.0, 0.0); n];

        // Build the spatial grid fresh every call. Cell size is tied
        // to the largest `hit_radius` we expect (miniboss ≈ 6.5), so
        // any potential pair's overlap fits inside the 3×3 cell
        // neighborhood the grid queries. Rebuilding per-call avoids
        // the bookkeeping cost of incremental maintenance — the old
        // positions are stale anyway after the AI movement pass that
        // precedes separation.
        const CELL_SIZE: f32 = 14.0;
        let mut grid = crate::spatial::SpatialGrid::new(
            self.arena.width as f32,
            self.arena.height as f32,
            CELL_SIZE,
        );
        for (i, e) in self.enemies.iter().enumerate() {
            if e.hp > 0 {
                grid.insert(i as u32, e.x, e.y);
            }
        }

        // For each enemy, ask the grid for candidates in the 3×3
        // block around its cell. Only test each pair once (j > i).
        for i in 0..n {
            let (ax, ay, ar, a_alive) = {
                let a = &self.enemies[i];
                (a.x, a.y, a.hit_radius(), a.hp > 0)
            };
            if !a_alive {
                continue;
            }
            grid.for_each_near(ax, ay, |j_raw| {
                let j = j_raw as usize;
                // `j > i` keeps the unordered-pair contract — each
                // pair contributes once, not twice.
                if j <= i {
                    return;
                }
                let (bx, by, br, b_alive) = {
                    let b = &self.enemies[j];
                    (b.x, b.y, b.hit_radius(), b.hp > 0)
                };
                if !b_alive {
                    return;
                }
                let min_dist = ar + br + SEPARATION_PADDING;
                let dx = bx - ax;
                let dy = by - ay;
                let d2 = dx * dx + dy * dy;
                if d2 >= min_dist * min_dist {
                    return;
                }
                // Fully-stacked pair (d ≈ 0): emit a deterministic
                // nudge based on indices so the tie-break is stable
                // across frames, not a jittering coin-flip.
                let (nx, ny, overlap) = if d2 < 1e-4 {
                    let angle = ((i * 31 + j) as f32) * 0.61803;
                    (angle.cos(), angle.sin(), min_dist)
                } else {
                    let d = d2.sqrt();
                    (dx / d, dy / d, min_dist - d)
                };
                let push = overlap * 0.5 * PUSH_STIFFNESS * dt;
                deltas[i].0 -= nx * push;
                deltas[i].1 -= ny * push;
                deltas[j].0 += nx * push;
                deltas[j].1 += ny * push;
            });
        }
        for (i, (dx, dy)) in deltas.into_iter().enumerate() {
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            let e = &mut self.enemies[i];
            // Stationary archetypes (base_speed == 0) are anchored.
            // Their presence still shows up as repulsion on the other
            // side of the pair — turrets become mob-flow obstacles.
            if e.base_speed() <= 0.0 {
                continue;
            }
            let nx = e.x + dx;
            let ny = e.y + dy;
            // Respect walls — a push that would shove an enemy into
            // a structure silently fails rather than clipping them
            // through it. Keeps the swarm inside the arena geometry.
            if self.arena.is_passable(nx as i32, ny as i32) {
                e.x = nx;
                e.y = ny;
            }
        }
    }

    fn chain_from(
        &mut self,
        from_idx: usize,
        damage: i32,
        radius: f32,
        n: usize,
        kills_out: &mut Vec<(usize, u32)>,
    ) {
        let Some(origin) = self.enemies.get(from_idx).map(|e| (e.x, e.y)) else {
            return;
        };
        let r2 = radius * radius;
        // Find candidates (idx, distance²), skipping the originating enemy.
        let mut candidates: Vec<(usize, f32)> = self
            .enemies
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if i == from_idx {
                    return None;
                }
                let dx = e.x - origin.0;
                let dy = e.y - origin.1;
                let d2 = dx * dx + dy * dy;
                if d2 < r2 { Some((i, d2)) } else { None }
            })
            .collect();
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        candidates.truncate(n);

        for (idx, _) in candidates {
            let info = self.enemies.get_mut(idx).map(|e| {
                let died = e.apply_damage(damage);
                let color = if died { e.color() } else { e.gib_color() };
                let at = (e.x, e.y);
                let tile = (e.x as i32, e.y as i32);
                let arch = e.archetype;
                let is_miniboss = arch == Archetype::Miniboss;
                (died, color, at, tile, arch, is_miniboss)
            });
            if let Some((died, color, at, tile, arch, is_miniboss)) = info {
                let seed = self.next_seed(tile);
                if died {
                    self.emit_gore_burst(at, color, arch, seed);
                    if is_miniboss {
                        self.drop_miniboss_loot(arch, at.0, at.1);
                    }
                    kills_out.push((idx, 0));
                } else {
                    self.emit_blast(at, color, seed, 0);
                }
            }
        }
    }

    pub fn tick_client(&mut self, dt: f32) {
        if self.paused {
            return;
        }
        self.particles.retain_mut(|p| p.update(dt));
        self.hitscans.retain_mut(|h| {
            h.ttl -= dt;
            h.ttl > 0.0
        });
        self.tick_phantoms(dt);
        self.tick_toasts(dt);
        self.tick_kill_feed(dt);
    }

    /// Host-only: flip pause state. Clients follow via snapshot.
    pub fn toggle_pause(&mut self) {
        if self.is_authoritative {
            self.paused = !self.paused;
        }
    }

    fn next_seed(&mut self, tile: (i32, i32)) -> u64 {
        self.seed_counter = self.seed_counter.wrapping_add(0x9E3779B97F4A7C15);
        self.seed_counter ^ ((tile.0 as u64) << 32) ^ (tile.1 as u64)
    }

    fn emit_blast(&mut self, at: (f32, f32), color: Pixel, seed: u64, intensity: u8) {
        self.emit_blast_with_gore(at, color, seed, intensity, 0);
    }

    /// Spit a small cluster of blood particles above a wounded player.
    /// Size + count scales with `intensity` (1 = scratch, 3 = crit).
    /// Pure visual — does not paint ground substances. Called on a
    /// per-player cadence by the main tick while `bleed_ttl > 0`.
    /// Per-tick ambient particle emission from ground substances.
    /// Only iterates tiles within the camera viewport (+ small
    /// padding) — off-screen tiles wouldn't be rendered anyway and
    /// would waste the particle budget. Each tile rolls a Poisson-
    /// style `rate * dt` chance per substance behavior; particles
    /// inherit the substance's configured palette, drift, and TTL.
    ///
    /// Budgeted — skips emission entirely once the particle Vec is
    /// above 800 so combat gore always has room to spawn.
    fn tick_substance_ambient(&mut self, dt: f32) {
        const PARTICLE_BUDGET: usize = 800;
        if self.particles.len() >= PARTICLE_BUDGET {
            return;
        }
        // Viewport bounds in world tiles, +4 pad so edges stay alive
        // when the camera pans.
        let c = &self.camera;
        let half_w = (c.viewport_w as f32) * 0.5 / c.zoom;
        let half_h = (c.viewport_h as f32) * 0.5 / c.zoom;
        let minx = ((c.center.0 - half_w) as i32 - 4).max(0);
        let miny = ((c.center.1 - half_h) as i32 - 4).max(0);
        let maxx = ((c.center.0 + half_w) as i32 + 4)
            .min(self.arena.width as i32 - 1);
        let maxy = ((c.center.1 + half_h) as i32 + 4)
            .min(self.arena.height as i32 - 1);
        if minx >= maxx || miny >= maxy {
            return;
        }

        use rand::Rng;
        let reg = &self.content.substances;
        for y in miny..=maxy {
            for x in minx..=maxx {
                let g = self.arena.ground(x, y);
                let (id, _state) = match g {
                    crate::arena::Ground::Substance { id, state } => (id, state),
                    _ => continue,
                };
                let def = match reg.get(id) {
                    Some(d) => d,
                    None => continue,
                };
                let b = def.behavior;
                if b.emit_rate <= 0.0 || b.emit_palette_len == 0 {
                    continue;
                }
                // Poisson-ish: per-tile probability per tick.
                if self.kiosk_rng.r#gen::<f32>() > b.emit_rate * dt {
                    continue;
                }
                // Palette pick.
                let idx = (self.kiosk_rng.r#gen::<u8>() as usize)
                    % (b.emit_palette_len as usize);
                let color = b.emit_palette[idx];
                // Slight per-particle jitter so wisps don't all stack
                // on tile centers.
                let jx = (self.kiosk_rng.r#gen::<f32>() - 0.5) * 0.9;
                let jy = (self.kiosk_rng.r#gen::<f32>() - 0.5) * 0.9;
                let vx = (self.kiosk_rng.r#gen::<f32>() - 0.5) * 2.0 * b.emit_drift_x;
                let vy = b.emit_drift_y
                    + (self.kiosk_rng.r#gen::<f32>() - 0.5) * 2.0;
                let ttl = self
                    .kiosk_rng
                    .gen_range(b.emit_ttl_min..=b.emit_ttl_max.max(b.emit_ttl_min + 0.01));
                self.particles.push(crate::particle::Particle {
                    x: x as f32 + 0.5 + jx,
                    y: y as f32 + 0.5 + jy,
                    vx,
                    vy,
                    ttl,
                    ttl_max: ttl,
                    color,
                    size: b.emit_size.max(1),
                });
                // Early-out if a very dense emitter tile pushed us
                // to the cap this frame.
                if self.particles.len() >= PARTICLE_BUDGET {
                    return;
                }
            }
        }
    }

    /// Per-tick damage + sanity drain for entities standing on a
    /// substance. Plays at sim cadence so DOT accumulates over the
    /// same frames primitives+weapons tick. Applies to players AND
    /// enemies — an acid pool hurts anyone in it, which is exactly
    /// the friendly-fire-via-ground interaction the spec calls for.
    fn tick_substance_hazards(&mut self, dt: f32) {
        let reg = &self.content.substances;
        // Players
        for pi in 0..self.players.len() {
            let (px, py, alive) = {
                let p = &self.players[pi];
                (p.x, p.y, p.hp > 0)
            };
            if !alive {
                continue;
            }
            let g = self.arena.ground(px as i32, py as i32);
            if let crate::arena::Ground::Substance { id, .. } = g {
                let b = match reg.get(id) {
                    Some(d) => d.behavior,
                    None => continue,
                };
                if b.damage_per_sec > 0.0 {
                    // Quantize to integer damage; accumulate the
                    // fractional part so sub-1-dps substances still
                    // eventually tick.
                    let raw = (b.damage_per_sec * dt).ceil() as i32;
                    if raw > 0 {
                        self.players[pi].take_damage(raw);
                    }
                }
                if b.sanity_drain_per_sec > 0.0 {
                    self.players[pi].sanity =
                        (self.players[pi].sanity - b.sanity_drain_per_sec * dt).max(0.0);
                }
            }
        }
        // Enemies — living ones take substance damage too. Dead +
        // stationary (player turret = survivor team) skip; we don't
        // want turrets eaten by their own oil pool.
        let survivor_tag = crate::tag::Tag::new(crate::enemy::TEAM_SURVIVOR);
        for ei in 0..self.enemies.len() {
            let (ex, ey, alive, team) = {
                let e = &self.enemies[ei];
                (e.x, e.y, e.hp > 0, e.team)
            };
            if !alive || team == survivor_tag {
                continue;
            }
            let g = self.arena.ground(ex as i32, ey as i32);
            if let crate::arena::Ground::Substance { id, .. } = g {
                let b = match reg.get(id) {
                    Some(d) => d.behavior,
                    None => continue,
                };
                if b.damage_per_sec > 0.0 {
                    let raw = (b.damage_per_sec * dt).ceil() as i32;
                    if raw > 0 {
                        let _ = self.enemies[ei].apply_damage(raw);
                    }
                }
                // Fire stamps a burn status on top of raw damage so
                // the enemy keeps smoldering after walking out.
                if let Some((dps, ttl)) = b.applies_burn {
                    self.enemies[ei].apply_burn(dps, ttl);
                }
            }
        }
    }

    /// Fire propagation + decay. Once every 0.3s of sim time we
    /// iterate all fire tiles in the camera band, decay their state
    /// toward zero (auto-removing tiles that finish their ttl), and
    /// roll a chance to ignite adjacent flammable substances. The
    /// payoff: a Rocketeer lobbing a breach-ignite rocket into an
    /// oil pool turns a single tile into a spreading grease fire
    /// that hoses the whole room.
    fn tick_fire_propagation(&mut self, dt: f32) {
        // Accumulate dt into a private clock on `Game`. Process when
        // the clock crosses the period. 0.1s = 10 propagation
        // ticks/sec, fast enough that fire reads as actually
        // *spreading* visually instead of teleporting in chunks.
        const PROPAGATE_PERIOD: f32 = 0.1;
        self.fire_tick_accum += dt;
        if self.fire_tick_accum < PROPAGATE_PERIOD {
            return;
        }
        let tick_dt = self.fire_tick_accum;
        self.fire_tick_accum = 0.0;
        let fire_id = self.content.fire_id;
        let reg = &self.content.substances;
        // Widen the scan to the whole arena — fire shouldn't stop
        // propagating just because the camera moved. Budgeted: we
        // only process fire tiles, and their count is small in
        // practice (ttl caps their lifetime).
        let w = self.arena.width as i32;
        let h = self.arena.height as i32;
        // Collect (x, y, state) for every fire tile up front so we
        // can mutate arena without borrow conflicts in the loop.
        let mut fires: Vec<(i32, i32, u8)> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if let crate::arena::Ground::Substance { id, state } =
                    self.arena.ground(x, y)
                {
                    if id == fire_id {
                        fires.push((x, y, state));
                    }
                }
            }
        }
        if fires.is_empty() {
            return;
        }
        let fire_behavior = match reg.get(fire_id) {
            Some(d) => d.behavior,
            None => return,
        };
        let ttl_total = fire_behavior.ttl_secs.unwrap_or(3.5).max(0.5);
        // state 0..=255 tracks age; we step by (255/ttl) per tick.
        let decay = ((255.0 / ttl_total) * tick_dt).ceil() as u32;
        use rand::Rng;
        for (fx, fy, state) in fires {
            // Decay: bump state toward 255 (FreshToDried curve).
            let new_state = (state as u32 + decay).min(255) as u8;
            if new_state >= 250 {
                // Burn out. Reset tile to Floor (strips substance).
                self.arena.set_floor(fx, fy);
                continue;
            }
            self.arena.set_substance(fx, fy, fire_id, new_state);
            // Propagate to 4-adjacent flammable substances.
            const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            for (dx, dy) in DIRS {
                let nx = fx + dx;
                let ny = fy + dy;
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                if let crate::arena::Ground::Substance { id, .. } =
                    self.arena.ground(nx, ny)
                {
                    if id == fire_id {
                        continue;
                    }
                    let nb = match reg.get(id) {
                        Some(d) => d.behavior,
                        None => continue,
                    };
                    if !nb.flammable {
                        continue;
                    }
                    // 70% chance per propagation tick × 10 ticks/sec
                    // = the typical neighbor catches within ~150ms.
                    // A connected oil pool now visibly cascades
                    // outward like a fuse rather than ticking in
                    // step-frame chunks.
                    if self.kiosk_rng.r#gen::<f32>() < 0.70 {
                        self.arena.set_substance(nx, ny, fire_id, 0);
                    }
                }
            }
        }
    }

    fn emit_player_bleed(&mut self, x: f32, y: f32, intensity: u8, seed: u64) {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};
        // Fresh-blood red — slightly darker than enemy gib so player
        // wounds read as "you" in the middle of an enemy death field.
        let color = Pixel::rgb(220, 40, 40);
        let mut rng = SmallRng::seed_from_u64(seed.wrapping_mul(0xBEEF_CAFE));
        let count = match intensity {
            3 => 6,
            2 => 4,
            _ => 2,
        };
        for _ in 0..count {
            let angle: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
            let speed: f32 = rng.gen_range(10.0..28.0);
            let ttl: f32 = rng.gen_range(0.35..0.75);
            let size: u8 = if intensity >= 2 { 2 } else { 1 };
            self.particles.push(crate::particle::Particle {
                x,
                y: y - 0.4,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed - 6.0,
                ttl,
                ttl_max: ttl,
                color,
                size,
            });
        }
    }

    /// Detonate a rocket at `pos`. Applies an Explode reaction with
    /// radius/damage modulated by the primitive list: Breach widens
    /// the wall-damage ring, Ignite leaves a burn field, Chain arcs
    /// to nearby enemies. Emits the gore burst through the heavy
    /// profile so the blast reads as the "holy shit" moment rockets
    /// are supposed to be.
    fn detonate_rocket(
        &mut self,
        pos: (f32, f32),
        primitives: &[Primitive],
        seed: u64,
        from_ai: bool,
    ) {
        let mut radius = 5.0f32;
        let mut damage = 55i32;
        let has_breach = primitives.contains(&Primitive::Breach);
        let has_ignite = primitives.contains(&Primitive::Ignite);
        let has_chain = primitives.contains(&Primitive::Chain);

        if has_breach {
            radius *= 1.35;
        }

        // Main explosion ring — damages enemies + players in radius,
        // scorches the ground, punches corpses, emits a blast.
        self.apply_explode(pos.0, pos.1, radius, damage, seed);

        // Cinematic blood-shower on top of the explode's own blast —
        // rockets are loud.
        let color = if from_ai {
            Pixel::rgb(255, 140, 60)
        } else {
            Pixel::rgb(255, 200, 120)
        };
        self.emit_blast_with_gore(pos, color, seed.wrapping_mul(0xAB5C_EADE), 2, 1);
        // Rockets are the loudest thing short of a miniboss death —
        // wake the whole quarter.
        self.emit_noise(pos.0, pos.1, 55.0, 3.0);

        // Breach: expand wall damage in a ring.
        if has_breach {
            let tx = pos.0 as i32;
            let ty = pos.1 as i32;
            for dx in -2..=2_i32 {
                for dy in -2..=2_i32 {
                    if dx.abs() + dy.abs() > 3 {
                        continue;
                    }
                    let tpos = (tx + dx, ty + dy);
                    self.damage_wall_with_particles(
                        tpos,
                        (tpos.0 as f32 + 0.5, tpos.1 as f32 + 0.5),
                        &[],
                    );
                }
            }
        }

        // Ignite: leave a persistent burn field so survivors can't
        // just re-enter the crater.
        if has_ignite {
            self.apply_ignite(pos.0, pos.1, radius * 1.15, 9.0, 3.0);
            damage = damage.max(damage); // no-op; keeps damage in scope
        }

        // Chain: secondary shock arc to a handful of nearby enemies.
        if has_chain {
            self.apply_shock(pos.0, pos.1, radius * 1.8, damage / 2, 3);
        }
    }

    /// Cinematic enemy-kill burst. Looks up the archetype's gore
    /// profile, paints blood-pool trails at deterministically-sampled
    /// landing points (with wall splatter for chunks whose flight path
    /// intersects a wall), and emits the Blast with the gore tier so
    /// clients synthesize the matching particle cloud.
    fn emit_gore_burst(
        &mut self,
        at: (f32, f32),
        color: Pixel,
        archetype: Archetype,
        seed: u64,
    ) {
        let profile_name = gore_profile_for_with_content(archetype, &self.content);
        let profile = crate::gore::GoreProfile::named(profile_name);
        let tier = profile.tier.to_byte();
        self.paint_gore_landings(at, &profile, seed);
        self.emit_blast_with_gore(at, color, seed, 2, tier);
    }

    /// Walk the profile's pool-tile count, sample seeded landing
    /// offsets from (x, y), ray-cast each path. If the chunk's line
    /// hits a wall, splat on the floor tile just in front of it;
    /// otherwise paint at the landing. Host-only — clients rebuild
    /// the same pools via SubstancePaint broadcasts already queued
    /// by `paint_substance`.
    fn paint_gore_landings(
        &mut self,
        at: (f32, f32),
        profile: &crate::gore::GoreProfile,
        seed: u64,
    ) {
        let mut rng = SmallRng::seed_from_u64(seed.wrapping_mul(0xC2B2_AE35));
        let blood_id = self.content.blood_pool_id;
        // Epicenter gets a pool no matter what.
        self.paint_substance(at.0 as i32, at.1 as i32, blood_id, 0);
        for i in 0..profile.pool_tiles {
            let angle: f32 = rng.r#gen::<f32>() * std::f32::consts::TAU;
            // Landing distance: biased toward the middle of the spread
            // so pools cluster naturally. Extreme profiles reach further.
            let dist = rng.gen_range(0.6..1.0) * profile.spread;
            let tx = at.0 + angle.cos() * dist;
            let ty = at.1 + angle.sin() * dist;

            let (paint_x, paint_y) = if profile.wall_splatter {
                match self.arena.raycast_wall(at, (tx, ty)) {
                    Some(hit) => {
                        // Chunk hit the wall — splat on the floor cell
                        // just in front of the impact, following the
                        // raycast normal back into open space.
                        let splat_x =
                            hit.point.0 + hit.normal.0 as f32 * 0.5;
                        let splat_y =
                            hit.point.1 + hit.normal.1 as f32 * 0.5;
                        (splat_x, splat_y)
                    }
                    None => (tx, ty),
                }
            } else {
                (tx, ty)
            };

            // Alternating freshness so the pool reads with varied shade.
            let state = if i & 1 == 0 { 0 } else { 30 };
            self.paint_substance(paint_x as i32, paint_y as i32, blood_id, state);
        }
    }

    fn emit_blast_with_gore(
        &mut self,
        at: (f32, f32),
        color: Pixel,
        seed: u64,
        intensity: u8,
        gore_tier: u8,
    ) {
        let blast = DestructionBlast {
            x: at.0,
            y: at.1,
            color_rgb: [color.r, color.g, color.b],
            seed,
            intensity,
            gore_tier,
            dir_x: 0.0,
            dir_y: 0.0,
        };
        self.apply_blast(&blast);
        self.tick_blasts.push(blast);
    }

    pub fn apply_blast(&mut self, blast: &DestructionBlast) {
        let color = Pixel::rgb(blast.color_rgb[0], blast.color_rgb[1], blast.color_rgb[2]);
        let at = (blast.x, blast.y);

        // Gore-tier blasts run through the profile-aware spawner so a
        // cinematic enemy death produces 50+ chunks + 200+ splatter
        // with color variation, not the 10-particle chatter of a
        // background wall break.
        if blast.gore_tier > 0 {
            let profile = match crate::gore::GoreTier::from_byte(blast.gore_tier) {
                crate::gore::GoreTier::Heavy => crate::gore::GoreProfile::heavy(),
                crate::gore::GoreTier::Extreme => crate::gore::GoreProfile::extreme(),
                crate::gore::GoreTier::CorpseHit => crate::gore::GoreProfile::corpse_hit(),
                _ => crate::gore::GoreProfile::standard(),
            };
            // Direction bias: corpse-hit blasts pass a unit vector
            // so gore sprays forward along the bullet path instead
            // of exploding in a full sphere.
            let direction = if blast.dir_x == 0.0 && blast.dir_y == 0.0 {
                None
            } else {
                Some((blast.dir_x, blast.dir_y))
            };
            particle::spawn_gore_directional(
                &mut self.particles,
                at.0,
                at.1,
                color,
                blast.seed,
                &profile,
                direction,
            );
            return;
        }

        // Legacy path: wall breaks + small hits stay cheap.
        let mult = match blast.intensity {
            0 => 0.4,
            1 => 1.0,
            _ => 1.5,
        };
        let before = self.particles.len();
        particle::spawn_destruction(&mut self.particles, at.0, at.1, color, blast.seed);
        let spawned = self.particles.len() - before;
        let keep = ((spawned as f32) * mult).round() as usize;
        if keep < spawned {
            self.particles.truncate(before + keep.max(1));
        } else if mult > 1.0 {
            particle::spawn_destruction(
                &mut self.particles,
                at.0,
                at.1,
                color,
                blast.seed.wrapping_mul(1315423911),
            );
        }
    }

    fn emit_tile_update(&mut self, tile: (i32, i32)) {
        let t = self.arena.tile(tile.0, tile.1);
        let (kind, hp) = match t {
            Tile::Floor => (0u8, 0u8),
            Tile::Wall { hp, .. } => (1u8, hp),
            Tile::Rubble { .. } => (2u8, 0u8),
            Tile::Carcosa => (3u8, 0u8),
        };
        self.tick_tile_updates.push((tile.0, tile.1, kind, hp));
    }

    pub fn apply_tile_update(&mut self, x: i32, y: i32, kind: u8, hp: u8) {
        match kind {
            0 => self.arena.set_floor(x, y),
            1 => self.arena.set_wall(x, y, Material::Concrete, hp.max(1)),
            2 => self.arena.set_rubble(x, y, Material::Concrete),
            _ => self.arena.set_carcosa(x, y),
        }
    }

    /// Client-side handler for `ServerMsg::SubstancePaint`. Touches only
    /// the ground layer; structures + objects on top are preserved.
    pub fn apply_substance_paint(&mut self, x: i32, y: i32, substance_id: u16, state: u8) {
        self.arena.set_substance(
            x,
            y,
            crate::substance::SubstanceId(substance_id),
            state,
        );
    }

    /// Client-side handler for `ServerMsg::CorpseHit`. Looks up the
    /// corpse by id and applies the same seeded hole synthesis the host
    /// ran, yielding identical pixel punchouts. Spawns a matching gore
    /// blast locally.
    pub fn apply_corpse_hit(&mut self, id: u32, seed: u64, dir_x: f32, dir_y: f32) {
        let Some(i) = self.corpses.iter().position(|c| c.id == id) else {
            return;
        };
        let color = self.corpses[i].apply_hit(seed);
        let (x, y, destroyed) = {
            let c = &self.corpses[i];
            (c.x, c.y, c.is_destroyed())
        };
        // Directional corpse-hit spray matching the host side —
        // same seed + same direction vector → identical forward
        // cone of particles on both peers.
        let blast = DestructionBlast {
            x,
            y,
            color_rgb: [color.r, color.g, color.b],
            seed,
            intensity: 1,
            gore_tier: crate::gore::GoreTier::CorpseHit.to_byte(),
            dir_x,
            dir_y,
        };
        self.apply_blast(&blast);
        if destroyed {
            self.corpses.swap_remove(i);
        }
    }

    /// Merge an incoming snapshot's corpse list into local state,
    /// preserving hole state for corpses that already exist. New ids are
    /// pushed; ids missing from the snapshot are dropped. Called on
    /// clients whenever a `Snapshot` arrives.
    pub fn merge_corpse_snapshot(&mut self, snap: &[crate::net::proto::CorpseSnap]) {
        // Drop corpses no longer present in the snap.
        let keep: std::collections::HashSet<u32> = snap.iter().map(|s| s.id).collect();
        self.corpses.retain(|c| keep.contains(&c.id));
        // Upsert.
        for s in snap {
            let arch = Archetype::from_kind(s.kind);
            match self.corpses.iter_mut().find(|c| c.id == s.id) {
                Some(c) => {
                    c.x = s.x;
                    c.y = s.y;
                    c.hp = s.hp;
                }
                None => {
                    let mut c = Corpse::new(s.id, arch, s.x, s.y);
                    c.hp = s.hp;
                    self.corpses.push(c);
                }
            }
        }
    }

    fn spawn_corpse(&mut self, archetype: Archetype, x: f32, y: f32) {
        let id = self.next_corpse_id;
        self.next_corpse_id = self.next_corpse_id.saturating_add(1);
        self.corpses.push(Corpse::new(id, archetype, x, y));
    }

    /// Fire a named body reaction at `(x, y)`. Called on enemy death
    /// with the archetype's `body_on_death` name. Applies the reaction
    /// locally on the host and queues a reliable broadcast so clients
    /// replay with the same seed → identical damage + substance paint
    /// + visuals.
    pub fn fire_body_reaction(&mut self, name: &str, x: f32, y: f32, seed: u64) {
        // Resolve name → Reaction. Unknown names warn but don't crash
        // — a content-pack typo shouldn't kill the run.
        let reaction = match self.reactions.get(name).cloned() {
            Some(r) => r,
            None => {
                tracing::warn!(name, "body reaction not registered");
                return;
            }
        };
        self.apply_reaction(&reaction, x, y, seed);
        // We intentionally do NOT broadcast the reaction name — the
        // visual + stateful consequences (substance paints, blasts,
        // spawned enemies) all flow through their existing sync
        // channels (SubstancePaint, Blast, snapshot). Replaying the
        // reaction name on clients would dupe-apply.
        let _ = name;
    }

    /// Apply a reaction to the world at `(x, y)` using `seed` for any
    /// RNG-driven decisions. Called on both host and client — host
    /// originates, clients replay from the broadcast.
    pub fn apply_reaction(&mut self, reaction: &Reaction, x: f32, y: f32, seed: u64) {
        match reaction {
            Reaction::SpawnSubstance { id, radius, state } => {
                let sub_id = match self.content.substances.by_name(id) {
                    Some(s) => s,
                    None => {
                        tracing::warn!(substance = id, "unknown substance in reaction");
                        return;
                    }
                };
                self.paint_substance_radius(x, y, sub_id, *state, *radius, seed);
            }
            Reaction::Explode { radius, damage } => {
                self.apply_explode(x, y, *radius, *damage, seed);
            }
            Reaction::Ignite {
                radius,
                damage_per_sec,
                ttl,
            } => {
                self.apply_ignite(x, y, *radius, *damage_per_sec, *ttl);
            }
            Reaction::Shock {
                radius,
                damage,
                chain_count,
            } => {
                self.apply_shock(x, y, *radius, *damage, *chain_count);
            }
            Reaction::SpawnEnemy { archetype_id, count } => {
                self.apply_spawn_enemy(archetype_id, x, y, *count, seed);
            }
            Reaction::ConsumeTrigger => {
                // Resolution is caller-specific — no-op when dispatched
                // abstractly. Custom handlers use this semantically.
            }
            Reaction::Custom { handler } => {
                self.dispatch_custom_reaction(*handler, x, y, seed);
            }
        }
    }

    fn paint_substance_radius(
        &mut self,
        x: f32,
        y: f32,
        id: SubstanceId,
        state: u8,
        radius: f32,
        seed: u64,
    ) {
        let cx = x as i32;
        let cy = y as i32;
        let r = radius.ceil() as i32;
        let r2 = radius * radius;
        // Seeded RNG so host + client paint the same tiles despite
        // running independent sims.
        let mut rng = SmallRng::seed_from_u64(seed);
        for dy in -r..=r {
            for dx in -r..=r {
                let d2 = (dx * dx + dy * dy) as f32;
                if d2 > r2 {
                    continue;
                }
                // Center is guaranteed; outer ring paints probabilistically
                // so radius-2 pools read as organic splatter, not circles.
                let chance = (1.0 - d2.sqrt() / radius.max(0.001)).max(0.0);
                if dx != 0 || dy != 0 {
                    if rng.r#gen::<f32>() > chance {
                        continue;
                    }
                }
                self.paint_substance(cx + dx, cy + dy, id, state);
            }
        }
    }

    fn apply_explode(&mut self, x: f32, y: f32, radius: f32, damage: i32, seed: u64) {
        let r2 = radius * radius;
        // Enemies in radius eat damage.
        let mut kills: Vec<usize> = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let dx = e.x - x;
            let dy = e.y - y;
            if dx * dx + dy * dy <= r2 && e.apply_damage(damage) {
                kills.push(i);
            }
        }
        // Players in radius eat damage too — friendly-fire explosions.
        // Skip corpses-in-progress (hp ≤ 0) so dead players don't get
        // further pounded into negative HP.
        for p in &mut self.players {
            if p.hp <= 0 {
                continue;
            }
            let dx = p.x - x;
            let dy = p.y - y;
            if dx * dx + dy * dy <= r2 {
                p.take_damage(damage);
            }
        }
        // Corpses in radius punch holes + possibly collapse.
        let mut i = 0;
        while i < self.corpses.len() {
            let dx = self.corpses[i].x - x;
            let dy = self.corpses[i].y - y;
            if dx * dx + dy * dy <= r2 {
                let _ = self.corpses[i].apply_hit(seed.wrapping_add(i as u64));
                if self.corpses[i].is_destroyed() {
                    self.corpses.swap_remove(i);
                    continue;
                }
            }
            i += 1;
        }
        // Scorch the ground at the epicenter.
        let scorch = self.content.scorch_id;
        self.paint_substance_radius(x, y, scorch, 160, radius * 0.6, seed ^ 0xDEAD_F00D);
        // Visual blast.
        self.emit_blast((x, y), Pixel::rgb(255, 180, 60), seed, 2);
        // Cleanup enemy kills in reverse.
        kills.sort();
        for idx in kills.into_iter().rev() {
            if idx < self.enemies.len() {
                self.enemies.swap_remove(idx);
                self.kills += 1;
            }
        }
    }

    fn apply_ignite(&mut self, x: f32, y: f32, radius: f32, damage_per_sec: f32, ttl: f32) {
        let r2 = radius * radius;
        for e in &mut self.enemies {
            let dx = e.x - x;
            let dy = e.y - y;
            if dx * dx + dy * dy <= r2 {
                e.apply_burn(damage_per_sec, ttl);
            }
        }
    }

    fn apply_shock(&mut self, x: f32, y: f32, radius: f32, damage: i32, chain_count: u8) {
        let r2 = radius * radius;
        let mut hit = 0u8;
        let mut kills: Vec<usize> = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let dx = e.x - x;
            let dy = e.y - y;
            if dx * dx + dy * dy <= r2 && hit < chain_count {
                if e.apply_damage(damage) {
                    kills.push(i);
                }
                hit = hit.saturating_add(1);
            }
        }
        kills.sort();
        for idx in kills.into_iter().rev() {
            if idx < self.enemies.len() {
                self.enemies.swap_remove(idx);
                self.kills += 1;
            }
        }
    }

    fn apply_spawn_enemy(
        &mut self,
        archetype_id: &str,
        x: f32,
        y: f32,
        count: u32,
        _seed: u64,
    ) {
        let Some(arch) = crate::enemy::Archetype::from_name(archetype_id) else {
            tracing::warn!(archetype = archetype_id, "unknown archetype in reaction");
            return;
        };
        let stats = self.content.stats(arch);
        for _ in 0..count {
            self.enemies.push(Enemy::spawn(arch, stats, x, y));
        }
    }

    /// Rust-coded reaction escape hatch. Handler names are interned
    /// `Tag`s; custom behaviors slot in here.
    fn dispatch_custom_reaction(&mut self, handler: Tag, x: f32, y: f32, seed: u64) {
        match handler.as_str() {
            "spawn_swarmlings" => {
                // Three Swarmlings in a triangle around the corpse.
                let stats = self.content.stats(Archetype::Swarmling).clone();
                for i in 0..3 {
                    let angle = (i as f32) * std::f32::consts::TAU / 3.0
                        + (seed as f32 * 0.001).sin();
                    let ox = x + angle.cos() * 1.6;
                    let oy = y + angle.sin() * 1.6;
                    self.enemies.push(Enemy::spawn(
                        Archetype::Swarmling,
                        &stats,
                        ox,
                        oy,
                    ));
                }
            }
            "raise_eater" => {
                // Single Eater at the reaction site — the rare
                // necromantic-on-death raise.
                let stats = self.content.stats(Archetype::Eater).clone();
                self.enemies
                    .push(Enemy::spawn(Archetype::Eater, &stats, x, y));
            }
            "burst_floodlings" => {
                // 4 Floodlings burst out of the carrier sac.
                let stats = self.content.stats(Archetype::Floodling).clone();
                for i in 0..4 {
                    let angle = (i as f32) * std::f32::consts::TAU / 4.0
                        + (seed as f32 * 0.0001).sin();
                    let ox = x + angle.cos() * 1.5;
                    let oy = y + angle.sin() * 1.5;
                    self.enemies.push(Enemy::spawn(
                        Archetype::Floodling,
                        &stats,
                        ox,
                        oy,
                    ));
                }
            }
            other => {
                tracing::warn!(handler = other, "unknown custom reaction handler");
            }
        }
    }

    /// Client-side handler for a `BodyReaction` broadcast. Looks up
    /// the reaction by name and applies it with the same seed.
    pub fn apply_body_reaction(&mut self, name: &str, x: f32, y: f32, seed: u64) {
        if let Some(reaction) = self.reactions.get(name).cloned() {
            self.apply_reaction(&reaction, x, y, seed);
        }
    }

    /// Emit a noise event at `(x, y)` with the given hearing radius
    /// and TTL. Enemies within range will start investigating this
    /// point for a few seconds. Called on wall breaks, rocket blasts,
    /// Howler shrieks, enemy death screams, player gunfire.
    pub fn emit_noise(&mut self, x: f32, y: f32, radius: f32, ttl: f32) {
        self.noise_events.push(NoiseEvent { x, y, radius, ttl });
    }

    /// Host-side substance paint: apply locally AND queue a
    /// SubstancePaint broadcast for all clients.
    pub fn paint_substance(
        &mut self,
        x: i32,
        y: i32,
        id: crate::substance::SubstanceId,
        state: u8,
    ) {
        self.arena.set_substance(x, y, id, state);
        self.tick_ground_paints.push((x, y, id.0, state));
    }

    pub fn render(&mut self, fb: &mut Framebuffer) {
        self.perf.begin("render_total");
        let camera = self.camera;

        self.perf.begin("arena_render");
        self.arena.render(fb, &camera, &self.content.substances);
        self.perf.end("arena_render");

        self.perf.begin("corpse_render");
        for c in &self.corpses {
            c.render(fb, &camera, &self.content);
        }
        self.perf.end("corpse_render");

        self.perf.begin("particle_render");
        for part in &self.particles {
            part.render(fb, &camera);
        }
        self.perf.end("particle_render");

        for pk in &self.pickups {
            pk.render(fb, &camera);
        }
        for kiosk in &self.kiosks {
            kiosk.render(fb, &camera);
        }
        for h in &self.hitscans {
            render_tracer(fb, &camera, h);
        }
        for sign in &self.yellow_signs {
            sign.render(fb, &camera);
        }
        for phantom in &self.phantoms {
            phantom.render(fb, &camera, &self.content);
        }

        self.perf.begin("enemy_render");
        for e in &self.enemies {
            e.render(fb, &camera, &self.content);
        }
        self.perf.end("enemy_render");

        self.perf.begin("projectile_render");
        for p in &self.projectiles {
            p.render(fb, &camera);
        }
        self.perf.end("projectile_render");

        for pl in &self.players {
            let is_self = Some(pl.id) == self.local_id;
            let marked = Some(pl.id) == self.marked_player_id;
            pl.render(fb, &camera, is_self, marked);
        }
        self.render_crosshair(fb);

        if self.debug_spatial_grid {
            self.render_spatial_grid_overlay(fb, &camera);
        }

        self.perf.end("render_total");
        self.perf.end_frame();
        // Refresh the overlay cache if a report is due; no tracing
        // spam so the normal log stays readable.
        self.perf_overlay.pull_report(&mut self.perf);
    }

    pub fn local_player(&self) -> Option<&Player> {
        let id = self.local_id?;
        self.players.iter().find(|p| p.id == id)
    }

    pub fn local_runtime(&self) -> Option<&PlayerRuntime> {
        let id = self.local_id?;
        let i = self.players.iter().position(|p| p.id == id)?;
        self.runtimes.get(i)
    }

    pub fn aim_target_for_local(&self) -> Option<(f32, f32)> {
        let player = self.local_player()?;
        let target = self.mouse.aim_target(&self.camera);
        let dx = target.0 - player.x;
        let dy = target.1 - player.y;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-4 {
            return Some((1.0, 0.0));
        }
        let inv = len2.sqrt().recip();
        Some((dx * inv, dy * inv))
    }

    /// Recompute camera center based on the local player + whichever
    /// input device is currently driving aim. Mouse mode uses the
    /// cursor position for edge look-ahead; gamepad mode uses the
    /// remembered aim direction so the camera nudges where the player
    /// is looking, not where the (stale) cursor sits.
    /// Call once per render frame. No-op when there is no local player.
    pub fn update_camera_follow(&mut self) {
        let (px, py) = match self.local_player() {
            Some(p) => (p.x, p.y),
            None => return,
        };
        match self.input_mode {
            InputMode::Gamepad => {
                let aim = self.gamepad_aim.unwrap_or((0.0, 0.0));
                self.camera.follow_dir((px, py), aim);
            }
            InputMode::MouseKeyboard => {
                let mouse_screen = self.mouse.screen_pos();
                self.camera.follow((px, py), mouse_screen);
            }
        }
    }

    #[allow(dead_code)]
    fn _render_crosshair_below(&self, _fb: &mut Framebuffer) {}

    /// Draw the spatial-grid overlay. Rebuilds a grid from current
    /// enemy positions (cheap — same cell_size as the separation
    /// pass uses) and paints each populated cell's boundary into
    /// the framebuffer. Color intensity scales with population so
    /// clusters visually pop. Only runs while `debug_spatial_grid`
    /// is on — no cost otherwise.
    fn render_spatial_grid_overlay(
        &self,
        fb: &mut Framebuffer,
        camera: &crate::camera::Camera,
    ) {
        const CELL_SIZE: f32 = 14.0;
        let arena_w = self.arena.width as f32;
        let arena_h = self.arena.height as f32;
        let cols = (arena_w / CELL_SIZE).ceil() as i32;
        let rows = (arena_h / CELL_SIZE).ceil() as i32;
        if cols <= 0 || rows <= 0 {
            return;
        }
        // Bucket enemies by cell — O(N). Only live enemies count.
        let mut counts: Vec<u32> = vec![0; (cols * rows) as usize];
        for e in &self.enemies {
            if e.hp <= 0 {
                continue;
            }
            let cx = (e.x / CELL_SIZE) as i32;
            let cy = (e.y / CELL_SIZE) as i32;
            if cx < 0 || cy < 0 || cx >= cols || cy >= rows {
                continue;
            }
            counts[(cy * cols + cx) as usize] += 1;
        }

        let vw = camera.viewport_w;
        let vh = camera.viewport_h;
        for cy in 0..rows {
            for cx in 0..cols {
                let n = counts[(cy * cols + cx) as usize];
                if n == 0 {
                    continue;
                }
                // Cell corners in world space → screen space. Clip
                // cells entirely offscreen.
                let wx0 = cx as f32 * CELL_SIZE;
                let wy0 = cy as f32 * CELL_SIZE;
                let wx1 = ((cx + 1) as f32 * CELL_SIZE).min(arena_w);
                let wy1 = ((cy + 1) as f32 * CELL_SIZE).min(arena_h);
                let (sx0, sy0) = camera.world_to_screen((wx0, wy0));
                let (sx1, sy1) = camera.world_to_screen((wx1, wy1));
                let x0 = sx0.round() as i32;
                let y0 = sy0.round() as i32;
                let x1 = sx1.round() as i32;
                let y1 = sy1.round() as i32;
                if x1 < 0 || y1 < 0 {
                    continue;
                }
                if x0 >= vw as i32 || y0 >= vh as i32 {
                    continue;
                }
                // Color ramps green (quiet) → yellow → red (swarm).
                // The mapping is log-ish so a 100-enemy cell doesn't
                // saturate the scale when most cells hold 1–3.
                let intensity = ((n as f32).ln() / 6.0).min(1.0);
                let r = (60.0 + intensity * 195.0) as u8;
                let g = (255.0 - intensity * 200.0) as u8;
                let b = (80.0 - intensity * 60.0).max(20.0) as u8;
                let color = Pixel::rgb(r, g, b);
                // Draw horizontal + vertical edges only — filled
                // cells would obscure what's inside. Every 3rd pixel
                // so the grid reads as a hatch pattern rather than
                // a solid outline — keeps it legible over sprites.
                let cx0 = x0.max(0) as u16;
                let cy0 = y0.max(0) as u16;
                let cx1 = x1.min(vw as i32 - 1).max(0) as u16;
                let cy1 = y1.min(vh as i32 - 1).max(0) as u16;
                // Top edge.
                if y0 >= 0 && (y0 as u16) < vh {
                    let y = y0 as u16;
                    let mut x = cx0;
                    while x <= cx1 {
                        fb.set(x, y, color);
                        x = x.saturating_add(3);
                    }
                }
                // Left edge.
                if x0 >= 0 && (x0 as u16) < vw {
                    let x = x0 as u16;
                    let mut y = cy0;
                    while y <= cy1 {
                        fb.set(x, y, color);
                        y = y.saturating_add(3);
                    }
                }
            }
        }
    }

    fn render_crosshair(&self, fb: &mut Framebuffer) {
        let color = Pixel::rgb(255, 255, 255);
        let px = self.mouse.col.saturating_mul(2) + 1;
        let py = self.mouse.row.saturating_mul(3) + 1;
        if px < self.camera.viewport_w && py < self.camera.viewport_h {
            fb.set(px, py, color);
            if py > 0 {
                fb.set(px, py - 1, color);
            }
            if py + 1 < self.camera.viewport_h {
                fb.set(px, py + 1, color);
            }
            if px > 0 {
                fb.set(px - 1, py, color);
            }
            if px + 1 < self.camera.viewport_w {
                fb.set(px + 1, py, color);
            }
        }
    }
}

/// Archetype → gore profile name. Checks the content pack first (so
/// archetype TOML can set `gore_profile = "..."`), falls back to a
/// tier guess based on archetype size.
fn gore_profile_for_with_content(
    arch: Archetype,
    content: &ContentDb,
) -> &'static str {
    if let Some(name) = content
        .archetypes
        .get(&arch)
        .and_then(|s| s.gore_profile.as_deref())
    {
        match name {
            "heavy" => "heavy",
            "extreme" => "extreme",
            _ => "standard",
        }
    } else {
        gore_profile_for(arch)
    }
}

/// Fallback profile selection when the content pack didn't pin one.
/// Minibosses + eaters are cinematic; medium bodies are heavy; small
/// things stay at standard chatter.
fn gore_profile_for(arch: Archetype) -> &'static str {
    match arch {
        Archetype::Miniboss | Archetype::Eater => "extreme",
        Archetype::Pinkie | Archetype::Pmc | Archetype::Orb | Archetype::Charger => "heavy",
        _ => "standard",
    }
}

/// How many corpses an Eater consumes before its aggro flips from
/// corpse-seeking to player-charging. Keeps the boss predictable —
/// a player sees it eating and knows the timer is real.
pub const EATER_HUNGER_THRESHOLD: u8 = 5;
/// Consume radius: how close the Eater has to get to a corpse to
/// absorb it.
pub const EATER_CONSUME_RADIUS: f32 = 4.5;
/// HP + damage bonus per corpse consumed. Accumulates.
pub const EATER_HP_PER_CONSUME: i32 = 40;

/// Cover-biased intermediate target for Mover::Sneak archetypes.
/// Returns a point the enemy should walk toward this tick — either
/// the direct target (already near cover) or the midpoint between
/// the nearest wall and the target (hug the wall on approach).
/// Coarse but visible sidestepping behavior without a full A* pass.
fn sneak_cover_target(e: &Enemy, target: (f32, f32), arena: &Arena) -> (f32, f32) {
    let ex = e.x as i32;
    let ey = e.y as i32;
    // Already adjacent to cover? Advance directly.
    let near_cover = (-2..=2)
        .any(|dy| (-2..=2).any(|dx| arena.is_wall(ex + dx, ey + dy)));
    if near_cover {
        return target;
    }
    // Scan a 13×13 window for the nearest wall tile. Limited range
    // so a sneaker in open ground doesn't path to a wall across the
    // arena — if no cover is nearby, just advance normally.
    let mut best: Option<((f32, f32), i32)> = None;
    for dy in -6..=6 {
        for dx in -6..=6 {
            if arena.is_wall(ex + dx, ey + dy) {
                let d2 = dx * dx + dy * dy;
                if best.map_or(true, |(_, bd)| d2 < bd) {
                    best = Some(((ex as f32 + dx as f32 + 0.5, ey as f32 + dy as f32 + 0.5), d2));
                }
            }
        }
    }
    if let Some((wpos, _)) = best {
        // Midpoint between the cover tile and the target — reads as
        // "hug the wall en route."
        ((wpos.0 + target.0) * 0.5, (wpos.1 + target.1) * 0.5)
    } else {
        target
    }
}

/// Swept segment-vs-circle test. Returns the smallest t ∈ [0, 1] at
/// which the segment `from → to` first enters a circle of radius `r`
/// centered at `c`. Returns 0 if the segment *starts* inside the circle
/// so a projectile spawned inside an enemy still counts as a hit.
///
/// Shared by projectile↔enemy and projectile↔corpse collision so fast
/// movers (snipers, future lasers) can't tunnel through targets.
fn segment_circle_first_t(
    from: (f32, f32),
    to: (f32, f32),
    c: (f32, f32),
    r: f32,
) -> Option<f32> {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let fx = from.0 - c.0;
    let fy = from.1 - c.1;
    let a = dx * dx + dy * dy;
    if a < 1e-6 {
        // Degenerate (no movement) — fall back to a point-in-circle test.
        return if fx * fx + fy * fy < r * r {
            Some(0.0)
        } else {
            None
        };
    }
    let b = 2.0 * (fx * dx + fy * dy);
    let c_term = fx * fx + fy * fy - r * r;
    let disc = b * b - 4.0 * a * c_term;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    let t1 = (-b - sq) / (2.0 * a);
    let t2 = (-b + sq) / (2.0 * a);
    // Started inside: t1 < 0 ≤ t2 → immediate contact.
    if t1 < 0.0 && t2 >= 0.0 {
        return Some(0.0);
    }
    if (0.0..=1.0).contains(&t1) {
        return Some(t1);
    }
    None
}

fn lerp_pt(from: (f32, f32), to: (f32, f32), t: f32) -> (f32, f32) {
    (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
}

/// After a pierce, advance the projectile just past the struck circle
/// so next tick's swept test doesn't re-hit the same target.
fn nudge_past(p: &mut Projectile, hit: (f32, f32), r: f32) {
    let speed = (p.vx * p.vx + p.vy * p.vy).sqrt().max(0.1);
    p.x = hit.0 + p.vx / speed * (r + 0.5);
    p.y = hit.1 + p.vy / speed * (r + 0.5);
}

/// Brand-ID → signature halo color for kiosks and UI. Kept in sync with the
/// archetype palette so each brand reads distinctly on the arena floor.
fn brand_color(id: &str) -> Pixel {
    match id {
        // FPS lineage — warm reds / oranges.
        "doom_inferno" => Pixel::rgb(255, 100, 40),
        "quake_arena" => Pixel::rgb(200, 80, 60),
        "serious_sam" => Pixel::rgb(255, 180, 60),
        // Tactical / mil-sim — olives + industrial.
        "tarkov_scavs" => Pixel::rgb(150, 170, 100),
        "stalker_zone" => Pixel::rgb(180, 160, 90),
        // Roguelike chaos — violets + saturated pinks.
        "vampire_survivors" => Pixel::rgb(220, 140, 255),
        "risk_of_rain" => Pixel::rgb(255, 120, 180),
        // Horde co-op — infection greens / amber.
        "left_4_dead" => Pixel::rgb(120, 200, 80),
        "helldivers" => Pixel::rgb(255, 210, 70),
        "deep_rock" => Pixel::rgb(220, 150, 60),
        // Horror — desaturated, ochre, sickly.
        "resident_evil" => Pixel::rgb(160, 80, 60),
        "dead_space" => Pixel::rgb(180, 220, 180),
        // Aliens / parasites — biomass greens + violets.
        "zerg_tide" => Pixel::rgb(160, 80, 200),
        "halo_flood" => Pixel::rgb(140, 240, 150),
        // Industrial siege — steel + rust.
        "rimworld_mechs" => Pixel::rgb(180, 180, 200),
        "factorio_biters" => Pixel::rgb(120, 200, 120),
        // Soulsborne — pallid parchment.
        "soulsborne_invader" => Pixel::rgb(220, 200, 160),
        _ => Pixel::rgb(255, 255, 255),
    }
}

/// Draw a tracer line between two world-space points, camera-transformed
/// and fading with ttl. Used for enemy hitscan visuals.
fn render_tracer(fb: &mut Framebuffer, camera: &Camera, trace: &HitscanTrace) {
    let intensity = (trace.ttl / 0.14).clamp(0.0, 1.0);
    let base = Pixel::rgb(255, 120, 40);
    let faded = Pixel::rgb(
        (base.r as f32 * intensity) as u8,
        (base.g as f32 * intensity) as u8,
        (base.b as f32 * intensity) as u8,
    );
    let (fx, fy) = camera.world_to_screen(trace.from);
    let (tx, ty) = camera.world_to_screen(trace.to);
    let dx = tx - fx;
    let dy = ty - fy;
    let steps = (dx.abs().max(dy.abs()).ceil() as i32).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let px = (fx + dx * t).round() as i32;
        let py = (fy + dy * t).round() as i32;
        if px >= 0 && py >= 0 {
            fb.set(px as u16, py as u16, faded);
        }
    }
}
