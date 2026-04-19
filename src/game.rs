use crate::arena::{Arena, Material, Tile};
use crate::enemy::Enemy;
use crate::fb::{Framebuffer, Pixel};
use crate::mouse::Mouse;
use crate::particle::{self, Particle};
use crate::player::Player;
use crate::projectile::{Projectile, ProjectileOutcome};
use crate::waves::WaveDirector;
use crate::weapon::Weapon;

/// Per-player state the host tracks to drive sim from incoming inputs.
pub struct PlayerInput {
    pub move_x: f32,
    pub move_y: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub firing: bool,
}

pub struct PlayerRuntime {
    pub weapon: Weapon,
    pub input: PlayerInput,
}

pub struct DestructionBlast {
    pub x: f32,
    pub y: f32,
    pub color_rgb: [u8; 3],
    pub seed: u64,
    pub intensity: u8, // 0=small hit, 1=wall destroyed, 2=enemy killed
}

pub struct Game {
    pub arena: Arena,
    pub players: Vec<Player>,
    pub runtimes: Vec<PlayerRuntime>,
    pub projectiles: Vec<Projectile>,
    pub particles: Vec<Particle>,
    pub enemies: Vec<Enemy>,
    pub director: WaveDirector,
    pub mouse: Mouse,
    pub origin: (i32, i32),
    pub kills: u32,
    pub alive: bool,
    pub elapsed_secs: f32,
    /// Local player ID (host's own slot, or the client's assigned slot).
    /// Used for aim centering and render self-identification.
    pub local_id: Option<u32>,
    seed_counter: u64,
    next_player_id: u32,
    /// Events generated during the current tick; the server drains these to
    /// build outgoing packets. Clients use `apply_destruction_event` to
    /// materialise local particles from server-sent events instead.
    pub tick_tile_updates: Vec<(i32, i32, u8, u8)>, // (x, y, kind, hp)
    pub tick_blasts: Vec<DestructionBlast>,
}

impl Game {
    pub fn new(arena: Arena, origin: (i32, i32)) -> Self {
        Self {
            arena,
            players: Vec::new(),
            runtimes: Vec::new(),
            projectiles: Vec::with_capacity(128),
            particles: Vec::with_capacity(1024),
            enemies: Vec::with_capacity(64),
            director: WaveDirector::new(0xC0FFEE),
            mouse: Mouse::default(),
            origin,
            kills: 0,
            alive: true,
            elapsed_secs: 0.0,
            local_id: None,
            seed_counter: 0x9E3779B97F4A7C15,
            next_player_id: 1,
            tick_tile_updates: Vec::new(),
            tick_blasts: Vec::new(),
        }
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
            weapon: Weapon::pulse(),
            input: PlayerInput { move_x: 0.0, move_y: 0.0, aim_x: 1.0, aim_y: 0.0, firing: false },
        });
        id
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

    /// Authoritative tick: applies all player inputs, updates enemies,
    /// resolves combat. Use this on the host.
    pub fn tick_authoritative(&mut self, dt: f32) {
        if !self.alive {
            return;
        }
        self.tick_tile_updates.clear();
        self.tick_blasts.clear();
        self.elapsed_secs += dt;

        // Step players from their buffered inputs.
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
            runtime.weapon.tick(dt);
        }

        // Fire weapons.
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        for (player, runtime) in self.players.iter().zip(self.runtimes.iter_mut()) {
            if runtime.input.firing {
                let origin = (player.x, player.y);
                let target = (
                    player.x + runtime.input.aim_x * 100.0,
                    player.y + runtime.input.aim_y * 100.0,
                );
                if let Some(p) = runtime.weapon.try_fire(origin, target) {
                    new_projectiles.push(p);
                }
            }
        }
        self.projectiles.append(&mut new_projectiles);

        self.director.tick(&self.arena, &mut self.enemies, dt);

        // Step enemies toward the nearest player; apply touch damage.
        let positions: Vec<(u32, f32, f32)> =
            self.players.iter().map(|p| (p.id, p.x, p.y)).collect();
        for e in &mut self.enemies {
            if let Some(&(_id, tx, ty)) = positions
                .iter()
                .min_by(|a, b| {
                    let da = (a.1 - e.x).powi(2) + (a.2 - e.y).powi(2);
                    let db = (b.1 - e.x).powi(2) + (b.2 - e.y).powi(2);
                    da.partial_cmp(&db).unwrap()
                })
            {
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
            }
        }

        // Step projectiles; collect impacts.
        let mut wall_hits: Vec<((f32, f32), (i32, i32))> = Vec::new();
        let mut enemy_hits: Vec<(usize, (f32, f32), i32)> = Vec::new();

        self.projectiles.retain_mut(|p| {
            match p.update(&self.arena, dt) {
                ProjectileOutcome::Expired => return false,
                ProjectileOutcome::HitWall { at, tile } => {
                    wall_hits.push((at, tile));
                    return false;
                }
                ProjectileOutcome::Alive => {}
            }
            for (i, e) in self.enemies.iter().enumerate() {
                let dx = p.x - e.x;
                let dy_top = p.y - e.y;
                let dy_bot = p.y - (e.y + 1.0);
                let dy = if dy_top.abs() < dy_bot.abs() { dy_top } else { dy_bot };
                if dx.abs() < 0.9 && dy.abs() < 0.9 {
                    enemy_hits.push((i, (p.x, p.y), p.damage));
                    return false;
                }
            }
            true
        });

        for (at, tile) in wall_hits {
            if let Some(outcome) = self.arena.damage_wall(tile.0, tile.1) {
                let seed = self.next_seed(tile);
                let color = if outcome.destroyed {
                    outcome.material.intact_color()
                } else {
                    outcome.material.debris_color()
                };
                let intensity = if outcome.destroyed { 1 } else { 0 };
                self.emit_blast(at, color, seed, intensity);
                self.emit_tile_update(tile);
            }
        }

        for (idx, at, dmg) in enemy_hits.iter().rev() {
            let (died, color, tile) = match self.enemies.get_mut(*idx) {
                Some(e) => {
                    let died = e.apply_damage(*dmg);
                    let color = if died { e.color() } else { e.gib_color() };
                    (died, color, (e.x as i32, e.y as i32))
                }
                None => continue,
            };
            let seed = self.next_seed(tile);
            let intensity = if died { 2 } else { 0 };
            self.emit_blast(*at, color, seed, intensity);
            if died {
                self.enemies.swap_remove(*idx);
                self.kills += 1;
            }
        }

        self.particles.retain_mut(|p| p.update(dt));

        if !self.players.is_empty() && self.players.iter().all(|p| p.hp <= 0) {
            self.alive = false;
        }
    }

    /// Non-authoritative tick: advances client-side particles only. Positions,
    /// enemies, projectiles, and arena come from the server snapshot.
    pub fn tick_client(&mut self, dt: f32) {
        self.particles.retain_mut(|p| p.update(dt));
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

    /// Materialise a destruction blast as local particles. Called on the host
    /// as it emits, and on clients as they receive server events.
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

    /// Apply a tile update from a server snapshot. No-op for tiles outside
    /// the arena (shouldn't happen, but guards against corrupted packets).
    pub fn apply_tile_update(&mut self, x: i32, y: i32, kind: u8, hp: u8) {
        match kind {
            0 => self.arena.set_wall(x, y, Material::Concrete, 0), /* fallthrough won't happen */
            1 => self.arena.set_wall(x, y, Material::Concrete, hp.max(1)),
            _ => self.arena.set_rubble(x, y, Material::Concrete),
        }
        // Kind == 0 (Floor) comes through when a wall was annihilated below
        // threshold in future primitives — not emitted today, but handle it:
        if kind == 0 {
            self.arena.set_floor(x, y);
        }
    }

    pub fn render(&self, fb: &mut Framebuffer) {
        let (ox, oy) = self.origin;
        self.arena.render(fb, ox, oy);
        for part in &self.particles {
            part.render(fb, ox, oy);
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

    /// Terminal-cell mouse position → aim target in arena pixel coords.
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

    fn render_crosshair(&self, fb: &mut Framebuffer) {
        let color = Pixel::rgb(255, 255, 255);
        let cx = self.mouse.col;
        let py = self.mouse.row.saturating_mul(2);
        fb.set(cx, py, color);
        fb.set(cx, py + 1, color);
        if cx > 0 {
            fb.set(cx - 1, py, color);
        }
        fb.set(cx + 1, py, color);
    }
}
