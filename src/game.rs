use crate::arena::{Arena, Material, Tile};
use crate::content::ContentDb;
use crate::enemy::{Archetype, Enemy};
use crate::fb::{Framebuffer, Pixel};
use crate::mouse::Mouse;
use crate::particle::{self, Particle};
use crate::pickup::Pickup;
use crate::player::Player;
use crate::primitive::{Primitive, Rarity};
use crate::projectile::{Projectile, ProjectileOutcome};
use crate::waves::WaveDirector;
use crate::weapon::Weapon;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

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
    pub director: WaveDirector,
    pub mouse: Mouse,
    pub origin: (i32, i32),
    pub kills: u32,
    pub alive: bool,
    pub elapsed_secs: f32,
    pub local_id: Option<u32>,
    seed_counter: u64,
    next_player_id: u32,
    next_pickup_id: u32,
    loot_rng: SmallRng,
    pub tick_tile_updates: Vec<(i32, i32, u8, u8)>,
    pub tick_blasts: Vec<DestructionBlast>,
    pub hitscans: Vec<HitscanTrace>,
    /// Set by the client from incoming snapshots so the HUD can render the
    /// local player's loadout even though the client doesn't run the
    /// authoritative weapon sim.
    pub remote_weapons: Vec<crate::net::proto::WeaponSnap>,
}

impl Game {
    pub fn new(arena: Arena, content: ContentDb, origin: (i32, i32)) -> Self {
        Self {
            arena,
            content,
            players: Vec::new(),
            runtimes: Vec::new(),
            projectiles: Vec::with_capacity(128),
            particles: Vec::with_capacity(1024),
            enemies: Vec::with_capacity(64),
            pickups: Vec::with_capacity(16),
            director: WaveDirector::new(0xC0FFEE),
            mouse: Mouse::default(),
            origin,
            kills: 0,
            alive: true,
            elapsed_secs: 0.0,
            local_id: None,
            seed_counter: 0x9E3779B97F4A7C15,
            next_player_id: 1,
            next_pickup_id: 1,
            loot_rng: SmallRng::seed_from_u64(0xBAD_F00D),
            tick_tile_updates: Vec::new(),
            tick_blasts: Vec::new(),
            hitscans: Vec::new(),
            remote_weapons: Vec::new(),
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

    pub fn set_origin(&mut self, origin: (i32, i32)) {
        self.origin = origin;
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

    /// Attempt to grab the nearest pickup. Replaces the player's active
    /// weapon. Called on interact keypress (E) events.
    pub fn try_interact(&mut self, player_id: u32) {
        let Some(pi) = self.player_index(player_id) else {
            return;
        };
        let (px, py) = (self.players[pi].x, self.players[pi].y);
        let r2 = Pickup::pickup_radius() * Pickup::pickup_radius();
        let mut best: Option<(usize, f32)> = None;
        for (i, pk) in self.pickups.iter().enumerate() {
            let dx = pk.x - px;
            let dy = pk.y - py;
            let d2 = dx * dx + dy * dy;
            if d2 <= r2 && best.map_or(true, |(_, b)| d2 < b) {
                best = Some((i, d2));
            }
        }
        if let Some((idx, _)) = best {
            let pickup = self.pickups.swap_remove(idx);
            let weapon = pickup.into_weapon();
            self.runtimes[pi].install_active(weapon);
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
        if !self.alive {
            return;
        }
        self.tick_tile_updates.clear();
        self.tick_blasts.clear();
        self.elapsed_secs += dt;

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

        self.director.tick(&self.arena, &mut self.enemies, &self.content, dt);

        // Step enemies + touch damage + ranged fire + status ticks.
        let positions: Vec<(u32, f32, f32)> =
            self.players.iter().map(|p| (p.id, p.x, p.y)).collect();
        let mut enemy_status_kills: Vec<usize> = Vec::new();
        let mut ranged_shots: Vec<(u32, crate::enemy::RangedShot)> = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let nearest = positions.iter().min_by(|a, b| {
                let da = (a.1 - e.x).powi(2) + (a.2 - e.y).powi(2);
                let db = (b.1 - e.x).powi(2) + (b.2 - e.y).powi(2);
                da.partial_cmp(&db).unwrap()
            });
            if let Some(&(pid, tx, ty)) = nearest {
                e.update((tx, ty), &self.arena, dt);
                for player in &mut self.players {
                    let dmg = e.touch_player(player.x, player.y);
                    if dmg > 0 {
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
                let p = &mut self.players[idx];
                p.hp -= shot.damage;
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
        self.particles.retain_mut(|p| p.update(dt));
        self.hitscans.retain_mut(|h| {
            h.ttl -= dt;
            h.ttl > 0.0
        });
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
        };
        self.tick_tile_updates.push((tile.0, tile.1, kind, hp));
    }

    pub fn apply_tile_update(&mut self, x: i32, y: i32, kind: u8, hp: u8) {
        match kind {
            0 => self.arena.set_floor(x, y),
            1 => self.arena.set_wall(x, y, Material::Concrete, hp.max(1)),
            _ => self.arena.set_rubble(x, y, Material::Concrete),
        }
    }

    pub fn render(&self, fb: &mut Framebuffer) {
        let (ox, oy) = self.origin;
        self.arena.render(fb, ox, oy);
        for part in &self.particles {
            part.render(fb, ox, oy);
        }
        for pk in &self.pickups {
            pk.render(fb, ox, oy);
        }
        for h in &self.hitscans {
            render_tracer(fb, ox, oy, h);
        }
        for e in &self.enemies {
            e.render(fb, ox, oy);
        }
        for p in &self.projectiles {
            p.render(fb, ox, oy);
        }
        for pl in &self.players {
            let is_self = Some(pl.id) == self.local_id;
            pl.render(fb, ox, oy, is_self);
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
        let target = self.mouse.aim_target(self.origin);
        let dx = target.0 - player.x;
        let dy = target.1 - player.y;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-4 {
            return Some((1.0, 0.0));
        }
        let inv = len2.sqrt().recip();
        Some((dx * inv, dy * inv))
    }

    #[allow(dead_code)]
    fn _render_crosshair_below(&self, _fb: &mut Framebuffer) {}

    fn render_crosshair(&self, fb: &mut Framebuffer) {
        let color = Pixel::rgb(255, 255, 255);
        let px = self.mouse.col.saturating_mul(2) + 1;
        let py = self.mouse.row.saturating_mul(3) + 1;
        fb.set(px, py, color);
        if py > 0 {
            fb.set(px, py - 1, color);
        }
        fb.set(px, py + 1, color);
        if px > 0 {
            fb.set(px - 1, py, color);
        }
        fb.set(px + 1, py, color);
    }
}

/// Draw a tracer line between two world-space points, fading toward the end.
fn render_tracer(fb: &mut Framebuffer, ox: i32, oy: i32, trace: &HitscanTrace) {
    let intensity = (trace.ttl / 0.14).clamp(0.0, 1.0);
    let base = Pixel::rgb(255, 120, 40);
    let faded = Pixel::rgb(
        (base.r as f32 * intensity) as u8,
        (base.g as f32 * intensity) as u8,
        (base.b as f32 * intensity) as u8,
    );
    let from_x = ox as f32 + trace.from.0;
    let from_y = oy as f32 + trace.from.1;
    let to_x = ox as f32 + trace.to.0;
    let to_y = oy as f32 + trace.to.1;
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let steps = (dx.abs().max(dy.abs()).ceil() as i32).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let px = (from_x + dx * t).round() as i32;
        let py = (from_y + dy * t).round() as i32;
        if px >= 0 && py >= 0 {
            fb.set(px as u16, py as u16, faded);
        }
    }
}
