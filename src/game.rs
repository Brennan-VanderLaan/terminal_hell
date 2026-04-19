use crate::arena::{Arena, Material, Tile};
use crate::camera::Camera;
use crate::carcosa::{Phantom, YellowSign};
use crate::content::ContentDb;
use crate::enemy::{Archetype, Enemy};
use crate::fb::{Framebuffer, Pixel};
use crate::mouse::Mouse;
use crate::particle::{self, Particle};
use crate::pickup::Pickup;
use crate::player::Player;
use crate::primitive::{Primitive, Rarity};
use crate::projectile::{Projectile, ProjectileOutcome};
use crate::vote::VoteKiosk;
use crate::waves::{WaveDirector, WaveEvent};
use crate::weapon::Weapon;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

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
}

/// Flat HUD-facing view of a player's weapon inventory.
pub struct LocalLoadoutView {
    pub active_slot: u8,
    pub slots: [Option<(Rarity, Vec<Primitive>)>; 2],
}

/// Transient tracer for enemy hitscan shots. Lives ~0.12s as visual feedback.
#[derive(Clone, Debug)]
pub struct HitscanTrace {
    pub from: (f32, f32),
    pub to: (f32, f32),
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
    /// True when this process owns the authoritative sim (solo or host).
    /// Clients of a remote server run with this false; the menu uses it
    /// to hide host-only controls (like the pause toggle) from clients.
    pub is_authoritative: bool,
    /// When true, authoritative ticks + client-side particle ticks are
    /// skipped. Synced via snapshot so clients freeze in step with the
    /// host.
    pub paused: bool,
    pub local_id: Option<u32>,
    seed_counter: u64,
    next_player_id: u32,
    next_pickup_id: u32,
    next_kiosk_id: u32,
    loot_rng: SmallRng,
    kiosk_rng: SmallRng,
    pub tick_tile_updates: Vec<(i32, i32, u8, u8)>,
    pub tick_blasts: Vec<DestructionBlast>,
    pub hitscans: Vec<HitscanTrace>,
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
            is_authoritative: true,
            paused: false,
            local_id: None,
            seed_counter: 0x9E3779B97F4A7C15,
            next_player_id: 1,
            next_pickup_id: 1,
            next_kiosk_id: 1,
            loot_rng: SmallRng::seed_from_u64(0xBAD_F00D),
            kiosk_rng: SmallRng::seed_from_u64(0xCAFE_BABE),
            tick_tile_updates: Vec::new(),
            tick_blasts: Vec::new(),
            hitscans: Vec::new(),
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
                            rt.weapons[0].as_ref().map(|w| (w.rarity, w.slots.clone())),
                            rt.weapons[1].as_ref().map(|w| (w.rarity, w.slots.clone())),
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
                snap.slot0.as_ref().map(|l| (l.rarity, l.primitives.clone())),
                snap.slot1.as_ref().map(|l| (l.rarity, l.primitives.clone())),
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
        let weapon = pickup.into_weapon();
        self.runtimes[pi].install_active(weapon);
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
    pub fn try_cycle_weapon(&mut self, player_id: u32) {
        if let Some(i) = self.player_index(player_id) {
            self.runtimes[i].cycle();
        }
    }

    fn drop_miniboss_loot(&mut self, x: f32, y: f32) {
        let rarity = Rarity::Rare;
        let slot_count = rarity.slot_count();
        let mut slots = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            slots.push(self.roll_primitive());
        }
        let id = self.next_pickup_id;
        self.next_pickup_id += 1;
        self.pickups.push(Pickup::new(id, x, y, rarity, slots));
    }

    fn apply_wave_event(&mut self, event: WaveEvent) {
        match event {
            WaveEvent::EnterVote => self.spawn_vote_kiosks(),
            WaveEvent::ExitVote => self.resolve_vote(),
            WaveEvent::WaveStart(_) => {
                self.add_corruption(2.0);
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
                player.sanity -= 18.0 * dt;
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
        let winner = self
            .kiosks
            .iter()
            .max_by_key(|k| k.votes)
            .map(|k| (k.brand_id.clone(), k.votes));
        if let Some((brand_id, votes)) = winner {
            // Only add the brand if someone actually voted — otherwise the
            // stack stays as-is (the team abstained).
            if votes > 0 && !self.active_brands.contains(&brand_id) {
                self.active_brands.push(brand_id);
            }
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
        if !self.alive || self.paused {
            // Still clear per-tick outgoing event buffers even while paused
            // so a paused frame doesn't re-broadcast stale blasts.
            self.tick_tile_updates.clear();
            self.tick_blasts.clear();
            return;
        }
        self.tick_tile_updates.clear();
        self.tick_blasts.clear();
        self.elapsed_secs += dt;
        // Passive corruption tick; drivers below add more per event.
        self.add_corruption(dt * 0.15);
        self.tick_carcosa(dt);
        self.tick_sanity(dt);
        self.tick_marks(dt);
        self.tick_daemon(dt);
        self.tick_signs(dt);

        for (player, runtime) in self.players.iter_mut().zip(self.runtimes.iter_mut()) {
            player.apply_remote_input(
                runtime.input.move_x,
                runtime.input.move_y,
                runtime.input.aim_x,
                runtime.input.aim_y,
                runtime.input.firing,
                &self.arena,
                dt,
            );
            for w in runtime.weapons.iter_mut().flatten() {
                w.tick(dt);
            }
        }

        let muzzle_offset = 7.5;
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        for (player, runtime) in self.players.iter().zip(self.runtimes.iter_mut()) {
            if !runtime.input.firing {
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
            if let Some(p) = weapon.try_fire(origin, target, player.id) {
                new_projectiles.push(p);
            }
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

        // Step enemies + touch damage + ranged fire + status ticks.
        // Marked players (Hastur gaze) override target selection so AI
        // flocks toward them; all enemies prioritize the marked survivor if
        // they have one.
        let marked_pos = self.marked_player_id.and_then(|id| {
            self.players.iter().find(|p| p.id == id).map(|p| (p.id, p.x, p.y))
        });
        let positions: Vec<(u32, f32, f32)> =
            self.players.iter().map(|p| (p.id, p.x, p.y)).collect();
        let mut enemy_status_kills: Vec<usize> = Vec::new();
        let mut ranged_shots: Vec<(u32, crate::enemy::RangedShot)> = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            // Target: marked player if it exists, else nearest living player.
            let target = marked_pos.or_else(|| {
                positions
                    .iter()
                    .min_by(|a, b| {
                        let da = (a.1 - e.x).powi(2) + (a.2 - e.y).powi(2);
                        let db = (b.1 - e.x).powi(2) + (b.2 - e.y).powi(2);
                        da.partial_cmp(&db).unwrap()
                    })
                    .copied()
            });
            if let Some((pid, tx, ty)) = target {
                e.update((tx, ty), &self.arena, dt);
                // Apply touch damage to the nearest player (not the marked
                // one specifically — attacks trigger on contact).
                for player in &mut self.players {
                    let base = e.touch_player(player.x, player.y);
                    if base > 0 {
                        let scale = if Some(player.id) == marked_pos.map(|(id, _, _)| id) {
                            1.25
                        } else {
                            1.0
                        };
                        let dmg = ((base as f32) * scale).round() as i32;
                        player.hp -= dmg;
                        if player.hp < 0 {
                            player.hp = 0;
                        }
                    }
                }
                if e.is_ranged() {
                    if let Some(shot) = e.try_ranged_attack((tx, ty), &self.arena) {
                        ranged_shots.push((pid, shot));
                    }
                }
            }
            let burn_damage = e.tick_statuses(dt);
            if burn_damage > 0 && e.apply_damage(burn_damage) {
                enemy_status_kills.push(i);
            }
        }

        // Resolve ranged shots: apply damage to the intended player if they
        // haven't stepped out of the line, and record a visible tracer.
        for (pid, shot) in ranged_shots {
            if let Some(idx) = self.player_index(pid) {
                let scale = self.damage_scalar_for(pid);
                let dmg = ((shot.damage as f32) * scale).round() as i32;
                let p = &mut self.players[idx];
                p.hp -= dmg;
                if p.hp < 0 {
                    p.hp = 0;
                }
            }
            self.hitscans.push(HitscanTrace {
                from: shot.from,
                to: shot.to,
                ttl: 0.14,
            });
        }
        // Status kills don't get removed here; they feed into the unified
        // end-of-tick kill cleanup so we only swap_remove once per enemy.
        // Emit kill blasts + miniboss loot drops for status kills now, while
        // positions are still valid.
        for &idx in &enemy_status_kills {
            let info = self
                .enemies
                .get(idx)
                .map(|e| ((e.x as i32, e.y as i32), e.color(), (e.x, e.y),
                         e.archetype == Archetype::Miniboss));
            if let Some((tile, color, at, is_miniboss)) = info {
                let seed = self.next_seed(tile);
                self.emit_blast(at, color, seed, 2);
                if is_miniboss {
                    self.drop_miniboss_loot(at.0, at.1);
                }
            }
        }

        // Step projectiles + resolve impacts. `retain_mut` keeps piercing /
        // ricocheting projectiles alive; despawns others.
        let mut wall_hits: Vec<((f32, f32), (i32, i32), Vec<Primitive>)> = Vec::new();
        let mut enemy_hits: Vec<(usize, (f32, f32), i32, u32, Vec<Primitive>)> = Vec::new();

        self.projectiles.retain_mut(|p| {
            match p.update(&self.arena, dt) {
                ProjectileOutcome::Expired => return false,
                ProjectileOutcome::HitWall { at, tile } => {
                    wall_hits.push((at, tile, p.primitives.clone()));
                    if p.bounces_left > 0 {
                        p.reflect_off(tile);
                        p.bounces_left -= 1;
                        return true;
                    }
                    return false;
                }
                ProjectileOutcome::Alive => {}
            }
            for (i, e) in self.enemies.iter().enumerate() {
                let dx = p.x - e.x;
                let dy = p.y - e.y;
                let r = e.hit_radius();
                if dx * dx + dy * dy < r * r {
                    enemy_hits.push((i, (p.x, p.y), p.damage, p.owner_id, p.primitives.clone()));
                    if p.pierces_left > 0 {
                        p.pierces_left -= 1;
                        // Nudge forward so we don't re-collide next tick.
                        let len = (p.vx * p.vx + p.vy * p.vy).sqrt().max(0.1);
                        p.x += p.vx / len * (r + 0.5);
                        p.y += p.vy / len * (r + 0.5);
                        return true;
                    }
                    return false;
                }
            }
            true
        });

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
            let (died, color, tile, is_miniboss, pos) = match self.enemies.get_mut(*idx) {
                Some(e) => {
                    if primitives.contains(&Primitive::Ignite) {
                        e.apply_burn(12.0, 2.5);
                    }
                    let died = e.apply_damage(*dmg);
                    let color = if died { e.color() } else { e.gib_color() };
                    (
                        died,
                        color,
                        (e.x as i32, e.y as i32),
                        e.archetype == Archetype::Miniboss,
                        (e.x, e.y),
                    )
                }
                None => continue,
            };
            let seed = self.next_seed(tile);
            let intensity = if died { 2 } else { 0 };
            self.emit_blast(*at, color, seed, intensity);

            if primitives.contains(&Primitive::Chain) {
                self.chain_from(*idx, (*dmg as f32 * 0.5) as i32, 12.0, 2, &mut kills_this_tick);
            }

            if died {
                kills_this_tick.push((*idx, *owner_id));
                if is_miniboss {
                    self.drop_miniboss_loot(pos.0, pos.1);
                    self.add_corruption(5.0);
                }
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
            // Credit Overdrive to the killer's active weapon, if any.
            if killer_id != 0 {
                if let Some(i) = self.player_index(killer_id) {
                    if let Some(w) = self.runtimes[i].active_weapon_mut() {
                        w.note_kill();
                    }
                }
            }
            // Small Corruption bump per kill; miniboss handled below via
            // the earlier is_miniboss path.
            self.add_corruption(0.2);
            self.enemies.swap_remove(idx);
            self.kills += 1;
        }

        self.particles.retain_mut(|p| p.update(dt));
        self.hitscans.retain_mut(|h| {
            h.ttl -= dt;
            h.ttl > 0.0
        });

        if !self.players.is_empty() && self.players.iter().all(|p| p.hp <= 0) {
            self.alive = false;
            // Death corruption spike; each downed player contributes.
            let down_count = self.players.iter().filter(|p| p.hp <= 0).count() as f32;
            self.add_corruption(7.0 * down_count);
        }
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
    }

    /// Chain splash: find `n` nearest other enemies within `radius` of
    /// `from_idx` and apply `damage` to each. Kills are pushed into
    /// `kills_out` for caller-side cleanup.
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
                let is_miniboss = e.archetype == Archetype::Miniboss;
                (died, color, at, tile, is_miniboss)
            });
            if let Some((died, color, at, tile, is_miniboss)) = info {
                let seed = self.next_seed(tile);
                self.emit_blast(at, color, seed, if died { 2 } else { 0 });
                if died {
                    if is_miniboss {
                        self.drop_miniboss_loot(at.0, at.1);
                    }
                    kills_out.push((idx, 0));
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
        let blast = DestructionBlast {
            x: at.0,
            y: at.1,
            color_rgb: [color.r, color.g, color.b],
            seed,
            intensity,
        };
        self.apply_blast(&blast);
        self.tick_blasts.push(blast);
    }

    pub fn apply_blast(&mut self, blast: &DestructionBlast) {
        let color = Pixel::rgb(blast.color_rgb[0], blast.color_rgb[1], blast.color_rgb[2]);
        let at = (blast.x, blast.y);
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

    pub fn render(&self, fb: &mut Framebuffer) {
        let camera = &self.camera;
        self.arena.render(fb, camera);
        for part in &self.particles {
            part.render(fb, camera);
        }
        for pk in &self.pickups {
            pk.render(fb, camera);
        }
        for kiosk in &self.kiosks {
            kiosk.render(fb, camera);
        }
        for h in &self.hitscans {
            render_tracer(fb, camera, h);
        }
        for sign in &self.yellow_signs {
            sign.render(fb, camera);
        }
        for phantom in &self.phantoms {
            phantom.render(fb, camera);
        }
        for e in &self.enemies {
            e.render(fb, camera);
        }
        for p in &self.projectiles {
            p.render(fb, camera);
        }
        for pl in &self.players {
            let is_self = Some(pl.id) == self.local_id;
            let marked = Some(pl.id) == self.marked_player_id;
            pl.render(fb, camera, is_self, marked);
        }
        self.render_crosshair(fb);
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

    /// Recompute camera center based on the local player + mouse look-ahead.
    /// Call once per render frame. No-op when there is no local player.
    pub fn update_camera_follow(&mut self) {
        let (px, py) = match self.local_player() {
            Some(p) => (p.x, p.y),
            None => return,
        };
        let mouse_screen = self.mouse.screen_pos();
        self.camera.follow((px, py), mouse_screen);
    }

    #[allow(dead_code)]
    fn _render_crosshair_below(&self, _fb: &mut Framebuffer) {}

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

/// Brand-ID → signature halo color for kiosks and UI. Kept in sync with the
/// archetype palette so each brand reads distinctly on the arena floor.
fn brand_color(id: &str) -> Pixel {
    match id {
        "fps_arena" => Pixel::rgb(255, 130, 60),
        "tactical" => Pixel::rgb(150, 170, 100),
        "chaos_roguelike" => Pixel::rgb(220, 140, 255),
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
