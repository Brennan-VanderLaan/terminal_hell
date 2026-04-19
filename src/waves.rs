use crate::arena::Arena;
use crate::enemy::{Archetype, Enemy};
use rand::Rng;
use rand::rngs::SmallRng;
use rand::SeedableRng;

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
    pub spawn_timer: f32,
    pub intermission_timer: f32,
    /// Banner display timer — non-zero while the "WAVE N" banner is up.
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
            spawn_timer: 0.0,
            intermission_timer: 2.5,
            banner_ttl: 0.0,
            rng: SmallRng::seed_from_u64(seed),
        }
    }

    pub fn total_budget(&self) -> u32 {
        self.rusher_budget + self.pinkie_budget
    }

    pub fn tick(&mut self, arena: &Arena, enemies: &mut Vec<Enemy>, dt: f32) {
        self.banner_ttl = (self.banner_ttl - dt).max(0.0);
        match self.state {
            WaveState::Spawning => {
                self.spawn_timer -= dt;
                while self.total_budget() > 0 && self.spawn_timer <= 0.0 {
                    let archetype = self.next_archetype();
                    if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena) {
                        enemies.push(Enemy::spawn(archetype, sx, sy));
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
                    self.plan_wave(arena, enemies);
                    self.spawn_timer = 0.0;
                    self.banner_ttl = 2.5;
                    self.state = WaveState::Spawning;
                }
            }
        }
    }

    fn plan_wave(&mut self, arena: &Arena, enemies: &mut Vec<Enemy>) {
        let w = self.wave;
        // Every 5 waves: a miniboss spawns immediately, on top of the normal
        // wave budget. The miniboss doesn't count toward `total_budget` — the
        // director waits for the horde to clear, which includes the miniboss.
        if w % 5 == 0 && w > 0 {
            if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena) {
                enemies.push(Enemy::spawn(Archetype::Miniboss, sx, sy));
            }
        }

        // Base rushers scale linearly; pinkies start at wave 3 and scale
        // gentler.
        self.rusher_budget = 4 + w * 2;
        self.pinkie_budget = if w >= 3 { (w - 2) / 2 + 1 } else { 0 };
    }

    fn next_archetype(&mut self) -> Archetype {
        if self.pinkie_budget > 0 && self.rng.gen_range(0..4) == 0 {
            self.pinkie_budget -= 1;
            Archetype::Pinkie
        } else if self.rusher_budget > 0 {
            self.rusher_budget -= 1;
            Archetype::Rusher
        } else if self.pinkie_budget > 0 {
            self.pinkie_budget -= 1;
            Archetype::Pinkie
        } else {
            self.rusher_budget = self.rusher_budget.saturating_sub(1);
            Archetype::Rusher
        }
    }
}

fn pick_spawn(rng: &mut SmallRng, arena: &Arena) -> Option<(f32, f32)> {
    let w = arena.width as i32;
    let h = arena.height as i32;
    for _ in 0..64 {
        let edge = rng.gen_range(0..4u8);
        let (tx, ty) = match edge {
            0 => (rng.gen_range(2..w - 2), 2),
            1 => (rng.gen_range(2..w - 2), h - 3),
            2 => (2, rng.gen_range(2..h - 2)),
            _ => (w - 3, rng.gen_range(2..h - 2)),
        };
        if arena.is_passable(tx, ty) && arena.is_passable(tx, ty + 1) {
            return Some((tx as f32 + 0.5, ty as f32 + 0.5));
        }
    }
    None
}
