//! Wave scheduler + intermission state machine.
//!
//! The director drives four states per wave:
//! - `Spawning`: budget-driven spawns; archetype picks come from the
//!   currently-active brand stack weighted pool.
//! - `Clearing`: wait for the team to finish the wave.
//! - `Intermission(phase)`: 4-phase window (Breathe / Vote / Stock /
//!   Warning). During Vote, the game layer spawns brand-bleed kiosks;
//!   during Stock, the team reads their HUD and repositions.
//!
//! Brand bleed-through is additive: voting a new brand in appends it to
//! the active stack. Each stacked brand contributes its spawn weights to
//! the pool the director samples from.

use crate::arena::Arena;
use crate::content::{BrandDef, ContentDb};
use crate::enemy::{Archetype, Enemy};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

const WAVE_BUDGET_BASE: u32 = 6;
const WAVE_BUDGET_RAMP: u32 = 2;
const PER_EXTRA_BRAND_BONUS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntermissionPhase {
    Breathe,
    Vote,
    Stock,
    Warning,
}

impl IntermissionPhase {
    pub fn duration(self) -> f32 {
        match self {
            Self::Breathe => 5.0,
            Self::Vote => 12.0,
            Self::Stock => 8.0,
            Self::Warning => 5.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Breathe => "BREATHE",
            Self::Vote => "VOTE",
            Self::Stock => "STOCK",
            Self::Warning => "WARNING",
        }
    }
    pub fn next(self) -> Option<IntermissionPhase> {
        match self {
            Self::Breathe => Some(Self::Vote),
            Self::Vote => Some(Self::Stock),
            Self::Stock => Some(Self::Warning),
            Self::Warning => None,
        }
    }
}

pub enum WaveState {
    Spawning,
    Clearing,
    Intermission(IntermissionPhase),
}

#[derive(Clone, Copy, Debug)]
pub enum WaveEvent {
    EnterVote,
    ExitVote,
    WaveStart(u32),
}

pub struct WaveDirector {
    pub wave: u32,
    pub state: WaveState,
    pub wave_budget: u32,
    pub spawn_timer: f32,
    pub phase_timer: f32,
    pub banner_ttl: f32,
    weighted_pool: Vec<(Archetype, u32)>,
    rng: SmallRng,
}

impl WaveDirector {
    pub fn new(seed: u64) -> Self {
        Self {
            wave: 0,
            // Start in Breathe (skip straight into a vote on the very first
            // intermission so brand choice happens up-front too).
            state: WaveState::Intermission(IntermissionPhase::Breathe),
            wave_budget: 0,
            spawn_timer: 0.0,
            phase_timer: IntermissionPhase::Breathe.duration(),
            banner_ttl: 0.0,
            weighted_pool: Vec::new(),
            rng: SmallRng::seed_from_u64(seed),
        }
    }

    pub fn current_phase(&self) -> Option<IntermissionPhase> {
        match self.state {
            WaveState::Intermission(p) => Some(p),
            _ => None,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, WaveState::Spawning | WaveState::Clearing)
    }

    pub fn tick(
        &mut self,
        arena: &Arena,
        enemies: &mut Vec<Enemy>,
        content: &ContentDb,
        active_brands: &[String],
        events: &mut Vec<WaveEvent>,
        dt: f32,
    ) {
        self.banner_ttl = (self.banner_ttl - dt).max(0.0);

        match self.state {
            WaveState::Spawning => {
                self.spawn_timer -= dt;
                while self.wave_budget > 0 && self.spawn_timer <= 0.0 {
                    let archetype = self.next_archetype();
                    if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena) {
                        let stats = content.stats(archetype);
                        enemies.push(Enemy::spawn(archetype, stats, sx, sy));
                    }
                    self.wave_budget -= 1;
                    self.spawn_timer += 0.55;
                }
                if self.wave_budget == 0 {
                    self.state = WaveState::Clearing;
                }
            }
            WaveState::Clearing => {
                if enemies.is_empty() {
                    self.state = WaveState::Intermission(IntermissionPhase::Breathe);
                    self.phase_timer = IntermissionPhase::Breathe.duration();
                }
            }
            WaveState::Intermission(phase) => {
                self.phase_timer -= dt;
                if self.phase_timer <= 0.0 {
                    // Emit transition event BEFORE the phase actually swaps.
                    if phase == IntermissionPhase::Vote {
                        events.push(WaveEvent::ExitVote);
                    }
                    match phase.next() {
                        Some(next_phase) => {
                            self.state = WaveState::Intermission(next_phase);
                            self.phase_timer = next_phase.duration();
                            if next_phase == IntermissionPhase::Vote {
                                events.push(WaveEvent::EnterVote);
                            }
                        }
                        None => {
                            // End of intermission → start the next wave.
                            self.wave += 1;
                            self.banner_ttl = 2.5;
                            self.plan_wave(arena, enemies, content, active_brands);
                            events.push(WaveEvent::WaveStart(self.wave));
                            self.state = WaveState::Spawning;
                        }
                    }
                }
            }
        }
    }

    fn plan_wave(
        &mut self,
        arena: &Arena,
        enemies: &mut Vec<Enemy>,
        content: &ContentDb,
        active_brands: &[String],
    ) {
        // Miniboss every 5 waves — pick from the FIRST active brand's
        // miniboss entry so the brand that opened the run sets the boss
        // flavor at each checkpoint.
        if self.wave % 5 == 0 && self.wave > 0 {
            if let Some(boss) = first_miniboss(content, active_brands) {
                if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena) {
                    let stats = content.stats(boss);
                    enemies.push(Enemy::spawn(boss, stats, sx, sy));
                }
            }
        }

        // Combined weighted pool from every active brand.
        self.weighted_pool = merge_weights(content, active_brands);

        let brand_bonus =
            (active_brands.len().saturating_sub(1) as u32) * PER_EXTRA_BRAND_BONUS;
        self.wave_budget = WAVE_BUDGET_BASE + self.wave * WAVE_BUDGET_RAMP + brand_bonus;
        self.spawn_timer = 0.0;
    }

    fn next_archetype(&mut self) -> Archetype {
        if self.weighted_pool.is_empty() {
            return Archetype::Rusher;
        }
        let total: u32 = self.weighted_pool.iter().map(|(_, w)| *w).sum();
        if total == 0 {
            return self.weighted_pool[0].0;
        }
        let pick = self.rng.gen_range(0..total);
        let mut acc = 0u32;
        for (arch, w) in &self.weighted_pool {
            acc += *w;
            if pick < acc {
                return *arch;
            }
        }
        self.weighted_pool[0].0
    }
}

fn merge_weights(content: &ContentDb, active_brands: &[String]) -> Vec<(Archetype, u32)> {
    use std::collections::HashMap;
    let mut combined: HashMap<Archetype, u32> = HashMap::new();
    for id in active_brands {
        let Some(brand) = content.brand(id) else {
            continue;
        };
        for arch_name in &brand.spawn_pool {
            let Ok(arch) = archetype_from_name(arch_name) else {
                continue;
            };
            let weight = brand.spawn_weights.get(arch_name).copied().unwrap_or(1);
            *combined.entry(arch).or_insert(0) += weight;
        }
    }
    combined.into_iter().collect()
}

fn first_miniboss(content: &ContentDb, active_brands: &[String]) -> Option<Archetype> {
    let brand_id = active_brands.first()?;
    let brand: &BrandDef = content.brand(brand_id)?;
    archetype_from_name(&brand.miniboss).ok()
}

fn archetype_from_name(name: &str) -> Result<Archetype, ()> {
    match name {
        "rusher" => Ok(Archetype::Rusher),
        "pinkie" => Ok(Archetype::Pinkie),
        "charger" => Ok(Archetype::Charger),
        "revenant" => Ok(Archetype::Revenant),
        "marksman" => Ok(Archetype::Marksman),
        "pmc" => Ok(Archetype::Pmc),
        "swarmling" => Ok(Archetype::Swarmling),
        "orb" => Ok(Archetype::Orb),
        "miniboss" => Ok(Archetype::Miniboss),
        _ => Err(()),
    }
}

fn pick_spawn(rng: &mut SmallRng, arena: &Arena) -> Option<(f32, f32)> {
    let w = arena.width as i32;
    let h = arena.height as i32;
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
