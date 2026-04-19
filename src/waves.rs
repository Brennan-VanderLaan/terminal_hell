use crate::arena::Arena;
use crate::content::ContentDb;
use crate::enemy::{Archetype, Enemy};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

pub enum WaveState {
    /// Still spawning enemies into the arena.
    Spawning,
    /// All enemies for the wave are spawned; waiting for the team to clean up.
    Clearing,
    /// Brief pause between waves.
    Intermission,
}

pub struct WaveDirector {
    pub wave: u32,
    pub state: WaveState,
    pub rusher_budget: u32,
    pub pinkie_budget: u32,
    pub charger_budget: u32,
    pub revenant_budget: u32,
    pub spawn_timer: f32,
    pub intermission_timer: f32,
    pub banner_ttl: f32,
    rng: SmallRng,
}

impl WaveDirector {
    pub fn new(seed: u64) -> Self {
        Self {
            wave: 0,
            state: WaveState::Intermission,
            rusher_budget: 0,
            pinkie_budget: 0,
            charger_budget: 0,
            revenant_budget: 0,
            spawn_timer: 0.0,
            intermission_timer: 2.5,
            banner_ttl: 0.0,
            rng: SmallRng::seed_from_u64(seed),
        }
    }

    pub fn total_budget(&self) -> u32 {
        self.rusher_budget + self.pinkie_budget + self.charger_budget + self.revenant_budget
    }

    pub fn tick(
        &mut self,
        arena: &Arena,
        enemies: &mut Vec<Enemy>,
        content: &ContentDb,
        dt: f32,
    ) {
        self.banner_ttl = (self.banner_ttl - dt).max(0.0);
        match self.state {
            WaveState::Spawning => {
                self.spawn_timer -= dt;
                while self.total_budget() > 0 && self.spawn_timer <= 0.0 {
                    let archetype = self.next_archetype();
                    if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena) {
                        let stats = content.stats(archetype);
                        enemies.push(Enemy::spawn(archetype, stats, sx, sy));
                    }
                    self.spawn_timer += 0.55;
                }
                if self.total_budget() == 0 {
                    self.state = WaveState::Clearing;
                }
            }
            WaveState::Clearing => {
                if enemies.is_empty() {
                    self.intermission_timer = 3.0;
                    self.state = WaveState::Intermission;
                }
            }
            WaveState::Intermission => {
                self.intermission_timer -= dt;
                if self.intermission_timer <= 0.0 {
                    self.wave += 1;
                    self.plan_wave(arena, enemies, content);
                    self.spawn_timer = 0.0;
                    self.banner_ttl = 2.5;
                    self.state = WaveState::Spawning;
                }
            }
        }
    }

    fn plan_wave(&mut self, arena: &Arena, enemies: &mut Vec<Enemy>, content: &ContentDb) {
        let w = self.wave;
        let brand = content.active_brand();
        let s = &brand.scaling;

        if w % 5 == 0 && w > 0 {
            if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena) {
                let stats = content.stats(Archetype::Miniboss);
                enemies.push(Enemy::spawn(Archetype::Miniboss, stats, sx, sy));
            }
        }

        self.rusher_budget = s.base_rushers + w * s.rushers_per_wave;
        self.pinkie_budget = if w >= s.pinkie_start_wave {
            s.pinkie_base + (w - s.pinkie_start_wave) / s.pinkie_scale_denom.max(1)
        } else {
            0
        };
        self.charger_budget = if w >= s.charger_start_wave {
            s.charger_base + (w - s.charger_start_wave) / s.charger_scale_denom.max(1)
        } else {
            0
        };
        self.revenant_budget = if w >= s.revenant_start_wave {
            s.revenant_base + (w - s.revenant_start_wave) / s.revenant_scale_denom.max(1)
        } else {
            0
        };
    }

    fn next_archetype(&mut self) -> Archetype {
        // Build a weighted pool of currently-available archetypes so
        // rushers don't always spawn first and ranged units don't cluster.
        let mut pool: Vec<Archetype> = Vec::new();
        if self.revenant_budget > 0 {
            pool.push(Archetype::Revenant);
        }
        if self.charger_budget > 0 {
            pool.extend(std::iter::repeat(Archetype::Charger).take(2));
        }
        if self.pinkie_budget > 0 {
            pool.extend(std::iter::repeat(Archetype::Pinkie).take(2));
        }
        if self.rusher_budget > 0 {
            pool.extend(std::iter::repeat(Archetype::Rusher).take(4));
        }
        let pick = pool.get(self.rng.gen_range(0..pool.len().max(1))).copied()
            .unwrap_or(Archetype::Rusher);
        match pick {
            Archetype::Rusher => self.rusher_budget = self.rusher_budget.saturating_sub(1),
            Archetype::Pinkie => self.pinkie_budget = self.pinkie_budget.saturating_sub(1),
            Archetype::Charger => self.charger_budget = self.charger_budget.saturating_sub(1),
            Archetype::Revenant => self.revenant_budget = self.revenant_budget.saturating_sub(1),
            Archetype::Miniboss => {}
        }
        pick
    }
}

fn pick_spawn(rng: &mut SmallRng, arena: &Arena) -> Option<(f32, f32)> {
    let w = arena.width as i32;
    let h = arena.height as i32;
    // Past the 3-pixel perimeter plus a couple pixels of breathing room so
    // enemies don't spawn with their sprites clipping through the wall.
    let margin = 6;
    if w < margin * 3 || h < margin * 3 {
        return None;
    }
    for _ in 0..128 {
        let edge = rng.gen_range(0..4u8);
        let (tx, ty) = match edge {
            0 => (rng.gen_range(margin..w - margin), margin),
            1 => (rng.gen_range(margin..w - margin), h - margin - 1),
            2 => (margin, rng.gen_range(margin..h - margin)),
            _ => (w - margin - 1, rng.gen_range(margin..h - margin)),
        };
        if arena.is_passable(tx, ty) && arena.is_passable(tx, ty + 1) {
            return Some((tx as f32 + 0.5, ty as f32 + 0.5));
        }
    }
    None
}
