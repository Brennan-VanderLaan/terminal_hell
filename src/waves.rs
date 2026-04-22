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

const WAVE_BUDGET_BASE: u32 = 12;
const WAVE_BUDGET_RAMP: u32 = 4;
const PER_EXTRA_BRAND_BONUS: u32 = 4;

/// Soft clearing deadline. If the team hasn't killed every hostile
/// within this many seconds of the last spawn, the director advances
/// anyway so a single straggler can't stall the run.
const CLEAR_TIMEOUT_BASE: f32 = 50.0;
/// Per-wave shrinkage so late-run stragglers don't get to coast for
/// almost a minute apiece.
const CLEAR_TIMEOUT_PER_WAVE: f32 = 0.5;
/// Floor for the auto-advance deadline so late waves still give
/// survivors *some* time to mop up the last shape on screen.
const CLEAR_TIMEOUT_MIN: f32 = 25.0;

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
            // Breathe kept tight (3s). Vote gets the extra time so
            // the team has room to read kiosks + coordinate choice
            // of brand-bleed. Stock + Warning stay short.
            Self::Breathe => 3.0,
            Self::Vote => 13.0,
            Self::Stock => 4.0,
            Self::Warning => 3.0,
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
    /// Countdown during the `Clearing` state. Reaches zero → the
    /// director advances to the intermission even if hostiles remain.
    /// Left at zero while not in `Clearing`.
    pub clear_timer: f32,
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
            clear_timer: 0.0,
            weighted_pool: Vec::new(),
            rng: SmallRng::seed_from_u64(seed),
        }
    }

    /// Per-wave clearing deadline: how long the team gets to kill
    /// the last stragglers before the director force-advances.
    fn clear_timeout_for(&self) -> f32 {
        let shrink = (self.wave as f32) * CLEAR_TIMEOUT_PER_WAVE;
        (CLEAR_TIMEOUT_BASE - shrink).max(CLEAR_TIMEOUT_MIN)
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
        player_anchors: &[(f32, f32)],
        dt: f32,
    ) {
        self.banner_ttl = (self.banner_ttl - dt).max(0.0);

        match self.state {
            WaveState::Spawning => {
                self.spawn_timer -= dt;
                while self.wave_budget > 0 && self.spawn_timer <= 0.0 {
                    let archetype = self.next_archetype();
                    // Swarm archetypes fire multiple clusters at once
                    // across the perimeter instead of a single spawn.
                    // Zerg rush: dense melee biomass.
                    // Flood tide: parasites that convert anything they
                    // touch — terrifying in volume because each one
                    // can turn the horde against itself.
                    if let Some(params) = swarm_params(archetype) {
                        let cluster_count =
                            self.rng.gen_range(params.clusters.0..=params.clusters.1);
                        for _ in 0..cluster_count {
                            let Some((cx, cy)) =
                                pick_spawn(&mut self.rng, arena, player_anchors)
                            else {
                                continue;
                            };
                            let cluster_size = self
                                .rng
                                .gen_range(params.cluster_size.0..=params.cluster_size.1);
                            for _ in 0..cluster_size {
                                let ox: f32 =
                                    self.rng.gen_range(-params.spread..params.spread);
                                let oy: f32 =
                                    self.rng.gen_range(-params.spread..params.spread);
                                let stats = content.stats(archetype);
                                let sx = cx + ox;
                                let sy = cy + oy;
                                let brand_id = self
                                    .pick_contributing_brand(
                                        archetype, content, active_brands,
                                    )
                                    .map(|s| crate::tag::Tag::new(&s));
                                let mut e = Enemy::spawn(archetype, stats, sx, sy);
                                e.brand_id = brand_id;
                                enemies.push(e);
                                crate::audio::emit(
                                    archetype.audio_id(),
                                    "spawn",
                                    Some((sx, sy)),
                                );
                            }
                        }
                        self.wave_budget =
                            self.wave_budget.saturating_sub(params.budget_cost);
                        self.spawn_timer += params.cadence_pause;
                        continue;
                    }

                    if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena, player_anchors) {
                        let stats = content.stats(archetype);
                        let brand_id = self
                            .pick_contributing_brand(archetype, content, active_brands)
                            .map(|s| crate::tag::Tag::new(&s));
                        let mut e = Enemy::spawn(archetype, stats, sx, sy);
                        e.brand_id = brand_id;
                        enemies.push(e);
                        crate::audio::emit(
                            archetype.audio_id(),
                            "spawn",
                            Some((sx, sy)),
                        );
                    }
                    self.wave_budget -= 1;
                    // Faster spawn cadence so bigger budgets actually
                    // ramp the threat, not drip across 30 seconds.
                    // Late-wave bursts arrive in ~0.25s chunks.
                    self.spawn_timer += 0.28;
                }
                if self.wave_budget == 0 {
                    self.state = WaveState::Clearing;
                    // Start the auto-advance countdown — once spawns
                    // are done, survivors have this long to finish
                    // the last stragglers before the director forces
                    // the next intermission.
                    self.clear_timer = self.clear_timeout_for();
                }
            }
            WaveState::Clearing => {
                // Count only HOSTILE enemies toward wave clear —
                // player turrets and any other survivor-team
                // entities don't block wave progression. Same for
                // future friendly spawns.
                let survivor_tag = crate::tag::Tag::new(crate::enemy::TEAM_SURVIVOR);
                let hostile_alive = enemies
                    .iter()
                    .any(|e| e.hp > 0 && e.team != survivor_tag);
                self.clear_timer = (self.clear_timer - dt).max(0.0);
                // Either condition advances: every hostile is dead,
                // OR the deadline ran out so one camping straggler
                // can't stall the whole run.
                if !hostile_alive || self.clear_timer <= 0.0 {
                    self.state = WaveState::Intermission(IntermissionPhase::Breathe);
                    self.phase_timer = IntermissionPhase::Breathe.duration();
                    self.clear_timer = 0.0;
                }
            }
            WaveState::Intermission(phase) => {
                self.phase_timer -= dt;
                if self.phase_timer <= 0.0 {
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
                            self.wave += 1;
                            self.banner_ttl = 2.5;
                            self.plan_wave(arena, enemies, content, active_brands, player_anchors);
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
        player_anchors: &[(f32, f32)],
    ) {
        if self.wave % 5 == 0 && self.wave > 0 {
            if let Some(boss) = first_miniboss(content, active_brands) {
                if let Some((sx, sy)) = pick_spawn(&mut self.rng, arena, player_anchors) {
                    let stats = content.stats(boss);
                    enemies.push(Enemy::spawn(boss, stats, sx, sy));
                    crate::audio::emit(boss.audio_id(), "spawn", Some((sx, sy)));
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

    /// Pick which brand is attributed to a rolled spawn. When
    /// multiple brands contribute the same archetype (e.g. both
    /// Doom and Tarkov declaring a `rusher`), we pick one weighted
    /// by that brand's declared weight for the archetype. This makes
    /// sprite flavor follow the pool contribution — a brand that
    /// weights rusher=8 gets 80% of the visual identity of rushers
    /// spawned during a mixed wave.
    pub fn pick_contributing_brand(
        &mut self,
        archetype: Archetype,
        content: &ContentDb,
        active_brands: &[String],
    ) -> Option<String> {
        // Build (brand_id, weight) list of brands that actually pool
        // this archetype. Brands that don't pool it contribute 0.
        let arch_name = archetype.snake_name();
        let mut candidates: Vec<(String, u32)> = Vec::new();
        let mut total: u32 = 0;
        for id in active_brands {
            let Some(brand) = content.brand(id) else {
                continue;
            };
            if !brand.spawn_pool.iter().any(|a| a == arch_name) {
                continue;
            }
            let weight = brand.spawn_weights.get(arch_name).copied().unwrap_or(1);
            total += weight;
            candidates.push((id.clone(), weight));
        }
        if candidates.is_empty() || total == 0 {
            return active_brands.first().cloned();
        }
        let pick = self.rng.gen_range(0..total);
        let mut acc = 0u32;
        for (id, w) in &candidates {
            acc += *w;
            if pick < acc {
                return Some(id.clone());
            }
        }
        Some(candidates[0].0.clone())
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
    // Delegate to the central lookup so every new archetype
    // (Breacher, Rocketeer, Leaper, Sapper, Juggernaut, Howler,
    // Sentinel, Splitter, and future adds) flows through here
    // automatically. This function used to carry its own hardcoded
    // list which silently dropped all the post-v0.6 archetypes from
    // the spawn pool — huge content bug.
    Archetype::from_name(name).ok_or(())
}

/// Swarm parameters for archetypes that spawn in mass clusters rather
/// than as single units. Returning Some here triggers the multi-
/// cluster path in the director; None falls through to the normal
/// one-per-pick spawn.
struct SwarmParams {
    /// (min, max) number of separate clusters laid down per trigger.
    clusters: (i32, i32),
    /// (min, max) size of each cluster.
    cluster_size: (i32, i32),
    /// Half-width of the per-enemy position scatter around the
    /// cluster center.
    spread: f32,
    /// Budget slots consumed by one swarm trigger.
    budget_cost: u32,
    /// Extra spawn-cadence pause after the burst.
    cadence_pause: f32,
}

fn swarm_params(arch: Archetype) -> Option<SwarmParams> {
    match arch {
        // Zerg rush — melee biomass, 4-7 clusters of 8-12.
        Archetype::Zergling => Some(SwarmParams {
            clusters: (4, 7),
            cluster_size: (8, 12),
            spread: 2.5,
            budget_cost: 2,
            cadence_pause: 0.9,
        }),
        // Flood tide — parasites in dense groups. Slightly more
        // clusters than zerglings because each one threatens to
        // convert the horde it's next to.
        Archetype::Floodling => Some(SwarmParams {
            clusters: (4, 6),
            cluster_size: (10, 15),
            spread: 2.8,
            budget_cost: 2,
            cadence_pause: 1.0,
        }),
        // Starving rats — dense vermin pours. Small clusters but many
        // of them; cheap budget cost so the swarm overwhelms rather
        // than attritions. Matches the Behavior.md §4.4 "eats
        // everything" flavor — density is the threat.
        Archetype::Rat => Some(SwarmParams {
            clusters: (5, 9),
            cluster_size: (6, 10),
            spread: 1.8,
            budget_cost: 1,
            cadence_pause: 0.7,
        }),
        _ => None,
    }
}

/// Pick a spawn tile along the arena perimeter, far from every player,
/// in free (passable) space. "Outside-in" vibe — enemies should feel
/// like they're coming from *over there*, not materializing next to
/// the survivors.
fn pick_spawn(
    rng: &mut SmallRng,
    arena: &Arena,
    player_anchors: &[(f32, f32)],
) -> Option<(f32, f32)> {
    let w = arena.width as i32;
    let h = arena.height as i32;
    let margin = 8;
    if w < margin * 3 || h < margin * 3 {
        return None;
    }

    // Band width: spawns land in the outer `band` tiles of the arena.
    let band = 24i32;
    // Minimum Euclidean distance to any player. If no players exist,
    // accept any edge tile.
    let min_dist: f32 = 50.0;
    let min_dist2 = min_dist * min_dist;

    for _ in 0..256 {
        // Pick one of the 4 edge bands, then a tile inside that band.
        let edge = rng.gen_range(0..4u8);
        let (tx, ty) = match edge {
            // Top band.
            0 => (
                rng.gen_range(margin..w - margin),
                rng.gen_range(margin..margin + band).min(h - margin - 1),
            ),
            // Bottom band.
            1 => (
                rng.gen_range(margin..w - margin),
                rng.gen_range((h - margin - band).max(margin)..h - margin),
            ),
            // Left band.
            2 => (
                rng.gen_range(margin..margin + band).min(w - margin - 1),
                rng.gen_range(margin..h - margin),
            ),
            // Right band.
            _ => (
                rng.gen_range((w - margin - band).max(margin)..w - margin),
                rng.gen_range(margin..h - margin),
            ),
        };

        // Must be free floor — not inside a wall, not inside an object.
        if !arena.is_passable(tx, ty) || !arena.is_passable(tx, ty + 1) {
            continue;
        }

        // Must be far from every player so enemies don't pop at a
        // survivor's elbow when someone's holding the perimeter.
        let too_close = player_anchors.iter().any(|&(px, py)| {
            let dx = tx as f32 + 0.5 - px;
            let dy = ty as f32 + 0.5 - py;
            dx * dx + dy * dy < min_dist2
        });
        if too_close {
            continue;
        }

        return Some((tx as f32 + 0.5, ty as f32 + 0.5));
    }

    // Last-ditch fallback for small arenas / degenerate layouts —
    // ignore the player-distance constraint. Still edge-biased.
    for _ in 0..64 {
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
