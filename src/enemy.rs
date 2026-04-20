use crate::arena::Arena;
use crate::camera::{Camera, MipLevel};
use crate::content::ArchetypeStats;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::BurnStatus;
use crate::sprite::{self, SpriteOverlay};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Archetype {
    // FPS arena brand
    Rusher,
    Pinkie,
    Charger,
    Revenant,
    // Tactical mil-sim brand
    Marksman,
    Pmc,
    // Chaos roguelikes brand
    Swarmling,
    Orb,
    // Shared miniboss
    Miniboss,
    /// Emergent boss — spawns from rare necromantic-on-death interaction.
    /// Walks corpse-to-corpse consuming them, grows thicker + angrier,
    /// eventually rampages toward the nearest player.
    Eater,
    /// Wall-smasher: A*-paths toward the nearest player and attacks
    /// any wall that blocks its path. Creates new entrances into
    /// bunkered-up positions.
    Breacher,
    /// Rocket launcher: stays at standoff range, fires explosive
    /// projectiles that fly through the world and detonate on impact.
    Rocketeer,
    /// Pouncer: prowls slowly, telegraphs a wind-up, then dashes in
    /// a straight line at 4× speed. Exposed cooldown after landing.
    Leaper,
    /// Walking bomb: runs at the nearest wall or player, lights a
    /// fuse on proximity, detonates in a breach-style explosion.
    /// New wall-breaker variant — louder + wider than the Breacher's
    /// targeted smash.
    Sapper,
    /// Slow tank that smashes any wall it bumps. Slower + hardier
    /// than the Breacher; no A* — it just plows forward and punches
    /// whatever's in the way.
    Juggernaut,
    /// Support archetype: never attacks, never approaches. Emits
    /// noise events on a cadence to call the rest of the horde
    /// toward wherever it's shrieking from. Weak. Prioritize killing.
    Howler,
    /// Stationary turret: spawns in place, fires a long-range
    /// hitscan shot on a cooldown. Doesn't move. Tank-y until dead.
    Sentinel,
    /// Fragile body — on death, spawns several Swarmlings where it
    /// fell. Pressure-multiplier enemy.
    Splitter,
    /// Tarkov-homage heavy. Armored LMG operator with a 60-round mag,
    /// forced reloads, periodic grenade lobs, aggressive closure.
    /// Boss-tier spawn that pushes survivors out of bunkered cover.
    Killa,
    /// StarCraft-homage swarm unit. Paper-thin HP, sprint-class speed,
    /// spawns in bursts so players feel the wave. Easy kill 1v1, a
    /// problem at dozens.
    Zergling,
    /// Halo-homage parasite. Fast, low HP — on contact with a living
    /// enemy, converts that enemy to Flood faction. On contact with
    /// a player, deals damage and dies.
    Floodling,
    /// Converted host. Faction::Flood — attacks every non-Flood
    /// entity (players AND regular enemies). Spawned by Floodling
    /// conversion; doesn't appear in normal spawn pools.
    Flood,
    /// Halo Flood-lore carrier form: slow bloated walking sac. On
    /// death explodes and vents 4-5 Floodlings. Faction::Flood.
    FloodCarrier,
    /// Player-deployed turret. Team = "survivor", hostiles =
    /// ["horde", "flood"]. Stationary hitscan — shoots enemies,
    /// enemies shoot back, dies like any other entity.
    PlayerTurret,
}

/// Generic team + hostility system. Instead of a hardcoded Faction
/// enum, every enemy carries:
///   * `team` — a Tag naming which camp it belongs to (e.g. "horde",
///     "flood", "marines", "civvies", future content).
///   * `hostiles` — the Tags it considers targets. Survivor-hunters
///     set this to `["survivor"]`; Flood-style factions add `["horde",
///     "survivor"]`; future feuding factions slot their own in.
///
/// Target selection picks the nearest living entity whose team is
/// in this enemy's hostile set, *or* the nearest living survivor if
/// "survivor" is in the hostile set. No Faction enum — content packs
/// can author new camps without touching Rust.
pub const TEAM_HORDE: &str = "horde";
pub const TEAM_FLOOD: &str = "flood";
pub const TEAM_SURVIVOR: &str = "survivor";

impl Archetype {
    pub fn from_kind(kind: u8) -> Self {
        match kind {
            1 => Archetype::Pinkie,
            2 => Archetype::Miniboss,
            3 => Archetype::Charger,
            4 => Archetype::Revenant,
            5 => Archetype::Marksman,
            6 => Archetype::Pmc,
            7 => Archetype::Swarmling,
            8 => Archetype::Orb,
            9 => Archetype::Eater,
            10 => Archetype::Breacher,
            11 => Archetype::Rocketeer,
            12 => Archetype::Leaper,
            13 => Archetype::Sapper,
            14 => Archetype::Juggernaut,
            15 => Archetype::Howler,
            16 => Archetype::Sentinel,
            17 => Archetype::Splitter,
            18 => Archetype::Killa,
            19 => Archetype::Zergling,
            20 => Archetype::Floodling,
            21 => Archetype::Flood,
            22 => Archetype::FloodCarrier,
            23 => Archetype::PlayerTurret,
            _ => Archetype::Rusher,
        }
    }
    pub fn to_kind(self) -> u8 {
        match self {
            Archetype::Rusher => 0,
            Archetype::Pinkie => 1,
            Archetype::Miniboss => 2,
            Archetype::Charger => 3,
            Archetype::Revenant => 4,
            Archetype::Marksman => 5,
            Archetype::Pmc => 6,
            Archetype::Swarmling => 7,
            Archetype::Orb => 8,
            Archetype::Eater => 9,
            Archetype::Breacher => 10,
            Archetype::Rocketeer => 11,
            Archetype::Leaper => 12,
            Archetype::Sapper => 13,
            Archetype::Juggernaut => 14,
            Archetype::Howler => 15,
            Archetype::Sentinel => 16,
            Archetype::Splitter => 17,
            Archetype::Killa => 18,
            Archetype::Zergling => 19,
            Archetype::Floodling => 20,
            Archetype::Flood => 21,
            Archetype::FloodCarrier => 22,
            Archetype::PlayerTurret => 23,
        }
    }

    /// Snake-case content id → Archetype enum. Inverse of the lookup
    /// used by `ContentDb::load_core`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "rusher" => Archetype::Rusher,
            "pinkie" => Archetype::Pinkie,
            "charger" => Archetype::Charger,
            "revenant" => Archetype::Revenant,
            "marksman" => Archetype::Marksman,
            "pmc" => Archetype::Pmc,
            "swarmling" => Archetype::Swarmling,
            "orb" => Archetype::Orb,
            "miniboss" => Archetype::Miniboss,
            "eater" => Archetype::Eater,
            "breacher" => Archetype::Breacher,
            "rocketeer" => Archetype::Rocketeer,
            "leaper" => Archetype::Leaper,
            "sapper" => Archetype::Sapper,
            "juggernaut" => Archetype::Juggernaut,
            "howler" => Archetype::Howler,
            "sentinel" => Archetype::Sentinel,
            "splitter" => Archetype::Splitter,
            "killa" => Archetype::Killa,
            "zergling" => Archetype::Zergling,
            "floodling" => Archetype::Floodling,
            "flood" => Archetype::Flood,
            "flood_carrier" => Archetype::FloodCarrier,
            "player_turret" => Archetype::PlayerTurret,
            _ => return None,
        })
    }
}

pub struct Enemy {
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub archetype: Archetype,
    // Stats pulled from the content pack at spawn. Kept on the struct so
    // runtime code (movement, collision, damage) stays one field-access away
    // and doesn't need a content-db lookup per tick.
    speed: f32,
    touch_damage: i32,
    hit_radius: f32,
    reach: f32,
    preferred_distance: f32,
    ranged_damage: i32,
    touch_cooldown: f32,
    pub burn: Option<BurnStatus>,
    /// Cryo slow stacks. Each stack = -15% move speed; 3 stacks
    /// means "frozen, shatter-ready." Decays to zero over `ttl`.
    pub frost: Option<crate::primitive::FrostStatus>,
    pub hit_flash: f32,
    pub attack_cooldown: f32,
    pub tell_timer: f32,
    pub tell_target: (f32, f32),
    /// Eater-specific: number of corpses consumed. Drives sprite scale
    /// (visually thiccer per consumed) + aggro flip (→ players after N).
    /// Zero on every other archetype.
    pub consumed: u8,
    /// Current A* path in tile coordinates. Populated for Mover::AStar
    /// archetypes (Breacher); empty vec means "no path or direct
    /// chase." Cleared + recomputed on `path_timer` tick.
    pub path: Vec<(i32, i32)>,
    pub path_timer: f32,
    /// Sprint timer for Mover::Charge (Charger). When > 0, the enemy
    /// moves at a speed multiplier toward `tell_target` regardless
    /// of normal targeting. Set by the charge trigger; ticks down
    /// each frame. Used only by archetypes with the Charge mover.
    pub sprint_timer: f32,
    pub sprint_cooldown: f32,
    /// Leaper state: 0 prowl, 1 winding up, 2 leaping, 3 recovering.
    /// Drives the dash-pounce AI in Game's enemy loop.
    pub leap_state: u8,
    pub leap_timer: f32,
    pub leap_vec: (f32, f32),
    /// Client-pushed noise target — set by Game when this enemy
    /// noticed a nearby loud event. Enemies without LoS to a player
    /// path toward this point until it expires.
    pub noise_target: Option<(f32, f32)>,
    pub noise_ttl: f32,
    /// Anti-stuck tracking. Counts ticks where the enemy had a
    /// movement intent but didn't actually move (blocked in a corner,
    /// tangled with another enemy's body, etc). When the counter
    /// passes a threshold, `unstick_timer` activates a short
    /// perpendicular wiggle to slide around the obstacle.
    pub stuck_ticks: u16,
    pub unstick_timer: f32,
    pub unstick_dir: (f32, f32),
    /// Camp this enemy belongs to. Defaults to `"horde"`; Flood-style
    /// archetypes switch to `"flood"`. Generic — content can add new
    /// teams (`"syndicate"`, `"marines"`, etc) without touching Rust.
    pub team: crate::tag::Tag,
    /// Tags this enemy considers targets. `"survivor"` means players;
    /// `"horde"` means the default enemy camp, etc. Emergent faction
    /// war is just "my hostiles include their team."
    pub hostiles: crate::tag::TagSet,
    /// Killa-specific: rounds left in the current LMG mag. When zero,
    /// a forced reload locks firing for `reload_timer`.
    pub mag: u16,
    pub reload_timer: f32,
    /// Killa-specific: cooldown before the next grenade lob. Not used
    /// by other archetypes.
    pub grenade_cooldown: f32,
    /// Scripted / Director command overriding this enemy's intrinsic
    /// targeting. `None` today; scaffolding for the RTS-style control
    /// path (see src/behavior.rs::DirectorOverride).
    pub director_override: Option<crate::behavior::DirectorOverride>,
}

impl Enemy {
    pub fn spawn(archetype: Archetype, stats: &ArchetypeStats, x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            hp: stats.hp,
            archetype,
            speed: stats.speed,
            touch_damage: stats.touch_damage,
            hit_radius: stats.hit_radius,
            reach: stats.reach.unwrap_or(stats.hit_radius),
            preferred_distance: stats.preferred_distance.unwrap_or(0.0),
            ranged_damage: stats.ranged_damage.unwrap_or(0),
            touch_cooldown: 0.0,
            burn: None,
            frost: None,
            hit_flash: 0.0,
            attack_cooldown: initial_attack_cooldown(archetype),
            tell_timer: 0.0,
            tell_target: (0.0, 0.0),
            consumed: 0,
            path: Vec::new(),
            path_timer: 0.0,
            sprint_timer: 0.0,
            sprint_cooldown: 0.0,
            leap_state: 0,
            leap_timer: 0.0,
            leap_vec: (0.0, 0.0),
            noise_target: None,
            noise_ttl: 0.0,
            stuck_ticks: 0,
            unstick_timer: 0.0,
            unstick_dir: (0.0, 0.0),
            // Flood-lineage archetypes carry the "flood" team and
            // hostile to both "horde" and "survivor" — that's the
            // faction-war behavior. Everyone else is "horde" and
            // targets "survivor" only. Content packs can author new
            // teams by seeding different defaults via TOML (future).
            team: if matches!(archetype, Archetype::Flood | Archetype::FloodCarrier) {
                crate::tag::Tag::new(TEAM_FLOOD)
            } else if matches!(archetype, Archetype::PlayerTurret) {
                crate::tag::Tag::new(TEAM_SURVIVOR)
            } else {
                crate::tag::Tag::new(TEAM_HORDE)
            },
            hostiles: if matches!(archetype, Archetype::Flood | Archetype::FloodCarrier) {
                crate::tag::TagSet::from_strs(&[TEAM_SURVIVOR, TEAM_HORDE])
            } else if matches!(archetype, Archetype::PlayerTurret) {
                crate::tag::TagSet::from_strs(&[TEAM_HORDE, TEAM_FLOOD])
            } else {
                crate::tag::TagSet::from_strs(&[TEAM_SURVIVOR])
            },
            mag: if matches!(archetype, Archetype::Killa) { 60 } else { 0 },
            reload_timer: 0.0,
            grenade_cooldown: if matches!(archetype, Archetype::Killa) { 4.0 } else { 0.0 },
            director_override: None,
        }
    }

    pub fn speed(&self) -> f32 {
        self.current_speed()
    }

    /// Effective move speed this tick, including sprint / leap
    /// modifiers. Chargers get 2.5× while their sprint_timer is
    /// positive; Leapers get 4× while leaping. Cryo stacks slow
    /// linearly down to 0 speed at 3 stacks (fully frozen).
    pub fn current_speed(&self) -> f32 {
        let mut s = self.speed;
        if self.sprint_timer > 0.0 {
            s *= 2.5;
        }
        if self.leap_state == 2 {
            s *= 4.0;
        }
        if let Some(f) = &self.frost {
            // 1 stack → 85%, 2 → 70%, 3+ → 30% (functionally frozen).
            let mul = match f.stacks {
                0 => 1.0,
                1 => 0.85,
                2 => 0.70,
                _ => 0.30,
            };
            s *= mul;
        }
        s
    }

    /// Apply a Cryo stack. 3+ stacks = frozen/shatter-ready.
    pub fn apply_frost(&mut self, ttl: f32) {
        let f = self.frost.get_or_insert_with(|| {
            crate::primitive::FrostStatus { stacks: 0, ttl: 0.0 }
        });
        f.stacks = f.stacks.saturating_add(1).min(3);
        f.ttl = f.ttl.max(ttl);
    }

    pub fn base_speed(&self) -> f32 {
        self.speed
    }

    pub fn touch_damage(&self) -> i32 {
        self.touch_damage
    }

    pub fn hit_radius(&self) -> f32 {
        self.hit_radius
    }

    pub fn color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(255, 60, 60),
            Archetype::Pinkie => Pixel::rgb(255, 130, 170),
            Archetype::Charger => Pixel::rgb(255, 100, 80),
            Archetype::Revenant => Pixel::rgb(200, 230, 255),
            Archetype::Marksman => Pixel::rgb(150, 170, 100),
            Archetype::Pmc => Pixel::rgb(120, 140, 170),
            Archetype::Swarmling => Pixel::rgb(255, 80, 40),
            Archetype::Orb => Pixel::rgb(200, 120, 255),
            Archetype::Miniboss => Pixel::rgb(255, 190, 60),
            Archetype::Eater => Pixel::rgb(140, 30, 180),
            Archetype::Breacher => Pixel::rgb(160, 80, 40),
            Archetype::Rocketeer => Pixel::rgb(110, 160, 200),
            Archetype::Leaper => Pixel::rgb(180, 60, 100),
            Archetype::Sapper => Pixel::rgb(255, 120, 30),
            Archetype::Juggernaut => Pixel::rgb(120, 120, 140),
            Archetype::Howler => Pixel::rgb(240, 180, 220),
            Archetype::Sentinel => Pixel::rgb(80, 200, 180),
            Archetype::Splitter => Pixel::rgb(170, 120, 60),
            Archetype::Killa => Pixel::rgb(90, 130, 100),
            Archetype::Zergling => Pixel::rgb(160, 80, 200),
            Archetype::Floodling => Pixel::rgb(180, 220, 120),
            Archetype::Flood => Pixel::rgb(120, 255, 140),
            Archetype::FloodCarrier => Pixel::rgb(200, 240, 150),
            Archetype::PlayerTurret => Pixel::rgb(80, 220, 255),
        }
    }

    pub fn gib_color(&self) -> Pixel {
        match self.archetype {
            Archetype::Rusher => Pixel::rgb(180, 30, 40),
            Archetype::Pinkie => Pixel::rgb(190, 70, 120),
            Archetype::Charger => Pixel::rgb(170, 60, 40),
            Archetype::Revenant => Pixel::rgb(130, 160, 200),
            Archetype::Marksman => Pixel::rgb(90, 110, 60),
            Archetype::Pmc => Pixel::rgb(70, 90, 120),
            Archetype::Swarmling => Pixel::rgb(160, 40, 20),
            Archetype::Orb => Pixel::rgb(140, 70, 200),
            Archetype::Miniboss => Pixel::rgb(200, 120, 40),
            Archetype::Eater => Pixel::rgb(100, 20, 140),
            Archetype::Breacher => Pixel::rgb(110, 50, 24),
            Archetype::Rocketeer => Pixel::rgb(70, 100, 140),
            Archetype::Leaper => Pixel::rgb(120, 40, 70),
            Archetype::Sapper => Pixel::rgb(180, 80, 20),
            Archetype::Juggernaut => Pixel::rgb(80, 80, 95),
            Archetype::Howler => Pixel::rgb(170, 120, 150),
            Archetype::Sentinel => Pixel::rgb(40, 120, 110),
            Archetype::Splitter => Pixel::rgb(120, 80, 40),
            Archetype::Killa => Pixel::rgb(60, 90, 70),
            Archetype::Zergling => Pixel::rgb(100, 50, 130),
            Archetype::Floodling => Pixel::rgb(120, 160, 80),
            Archetype::Flood => Pixel::rgb(80, 180, 100),
            Archetype::FloodCarrier => Pixel::rgb(140, 200, 110),
            Archetype::PlayerTurret => Pixel::rgb(30, 120, 160),
        }
    }

    pub fn preferred_distance(&self) -> f32 {
        self.preferred_distance
    }

    pub fn update(&mut self, target: (f32, f32), arena: &Arena, dt: f32) {
        self.touch_cooldown = (self.touch_cooldown - dt).max(0.0);
        self.attack_cooldown = (self.attack_cooldown - dt).max(0.0);
        self.hit_flash = (self.hit_flash - dt * 6.0).max(0.0);
        if self.tell_timer > 0.0 {
            self.tell_timer = (self.tell_timer - dt).max(0.0);
        }
        if self.sprint_timer > 0.0 {
            self.sprint_timer = (self.sprint_timer - dt).max(0.0);
        }
        if self.sprint_cooldown > 0.0 {
            self.sprint_cooldown = (self.sprint_cooldown - dt).max(0.0);
        }
        if self.leap_timer > 0.0 {
            self.leap_timer = (self.leap_timer - dt).max(0.0);
        }
        if self.noise_ttl > 0.0 {
            self.noise_ttl = (self.noise_ttl - dt).max(0.0);
            if self.noise_ttl <= 0.0 {
                self.noise_target = None;
            }
        }

        if self.unstick_timer > 0.0 {
            self.unstick_timer = (self.unstick_timer - dt).max(0.0);
        }

        let dx = target.0 - self.x;
        let dy = target.1 - self.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 < 1e-4 {
            self.stuck_ticks = 0;
            return;
        }
        let dist = dist2.sqrt();
        let inv = 1.0 / dist;

        // Ranged units try to hold preferred distance; they back off when
        // closer than preferred * 0.7 and advance when further than
        // preferred * 1.2. Melee units always close.
        let pref = self.preferred_distance();
        let (mut dir_x, mut dir_y) = if pref > 0.0 && dist < pref * 0.7 {
            (-dx * inv, -dy * inv)
        } else if pref > 0.0 && dist <= pref * 1.2 {
            (0.0, 0.0)
        } else {
            (dx * inv, dy * inv)
        };

        // Unstick override: if we've been stuck, use a perpendicular
        // escape direction for the wiggle duration. Keeps enemies
        // moving without teleporting — they slide sideways out of
        // corners while the timer runs, then resume chasing.
        if self.unstick_timer > 0.0 {
            dir_x = self.unstick_dir.0;
            dir_y = self.unstick_dir.1;
        }

        let want_motion = dir_x.abs() > 0.001 || dir_y.abs() > 0.001;
        let step_x = dir_x * self.speed() * dt;
        let step_y = dir_y * self.speed() * dt;
        let pre_x = self.x;
        let pre_y = self.y;

        let nx = self.x + step_x;
        if !collides(arena, nx, self.y) {
            self.x = nx;
        }
        let ny = self.y + step_y;
        if !collides(arena, self.x, ny) {
            self.y = ny;
        }

        // Stuck detection + anti-stuck wiggle. If the enemy wanted to
        // move but barely budged (pure wall collision on both axes),
        // count it. After ~20 ticks (~0.67s at 30Hz) pick a random
        // perpendicular slide direction and commit for 0.7s. No
        // teleports — just a brief sidestep to escape the corner.
        let moved = (self.x - pre_x).abs() + (self.y - pre_y).abs();
        if want_motion && moved < 0.02 && self.unstick_timer <= 0.0 {
            self.stuck_ticks = self.stuck_ticks.saturating_add(1);
            if self.stuck_ticks >= 20 {
                // Perpendicular to the desired direction — flip 90°
                // left or right based on a cheap pseudo-random bit
                // from the enemy's own position.
                let bit = ((self.x.to_bits() ^ self.y.to_bits()) & 1) != 0;
                let (perp_x, perp_y) = if bit {
                    (-dir_y, dir_x)
                } else {
                    (dir_y, -dir_x)
                };
                self.unstick_dir = (perp_x, perp_y);
                self.unstick_timer = 0.7;
                self.stuck_ticks = 0;
            }
        } else if moved > 0.05 {
            // Actually moving again — reset.
            self.stuck_ticks = 0;
        }
    }

    /// If this enemy is ready to fire AND has line of sight to the given
    /// player position, start (or continue) its ranged attack. Returns
    /// `Some(target)` when the shot actually fires this tick.
    pub fn try_ranged_attack(&mut self, player: (f32, f32), arena: &Arena) -> Option<RangedShot> {
        if !self.is_ranged() {
            return None;
        }
        let dx = player.0 - self.x;
        let dy = player.1 - self.y;
        let dist2 = dx * dx + dy * dy;
        // Max engagement range scales with the archetype's
        // preferred_distance — Marksman/Sentinel/Killa shoot far;
        // other ranged units stay mid-range.
        let pref = self.preferred_distance.max(30.0);
        let max_range = pref * 1.9;
        if dist2 > max_range * max_range || dist2 < 4.0 * 4.0 {
            return None;
        }
        if !arena.has_line_of_sight((self.x, self.y), player) {
            return None;
        }

        // Mid-tell: keep winding up, no fire yet.
        if self.tell_timer > 0.0 {
            return None;
        }

        // Tell just finished (timer hit 0 this tick with a target set) → fire.
        if self.tell_target != (0.0, 0.0) {
            let target = self.tell_target;
            self.tell_target = (0.0, 0.0);
            self.attack_cooldown = 2.2;
            if arena.has_line_of_sight((self.x, self.y), target) {
                return Some(RangedShot {
                    from: (self.x, self.y),
                    to: target,
                    damage: self.ranged_damage(),
                });
            }
            return None;
        }

        // No active tell and cooldown ready → start one.
        if self.attack_cooldown <= 0.0 {
            self.tell_timer = 0.45;
            self.tell_target = player;
        }
        None
    }

    pub fn is_ranged(&self) -> bool {
        matches!(
            self.archetype,
            Archetype::Revenant
                | Archetype::Marksman
                | Archetype::Sentinel
                | Archetype::Killa
                | Archetype::PlayerTurret
        )
    }

    pub fn is_telegraphing(&self) -> bool {
        self.tell_timer > 0.0
    }

    fn ranged_damage(&self) -> i32 {
        self.ranged_damage
    }

    pub fn apply_damage(&mut self, dmg: i32) -> bool {
        self.hp -= dmg;
        self.hit_flash = 1.0;
        self.hp <= 0
    }

    pub fn apply_burn(&mut self, damage_per_sec: f32, ttl: f32) {
        match &mut self.burn {
            Some(b) if b.damage_per_sec >= damage_per_sec => {
                b.ttl = b.ttl.max(ttl);
            }
            _ => {
                self.burn = Some(BurnStatus { damage_per_sec, ttl, accum: 0.0 });
            }
        }
    }

    pub fn tick_statuses(&mut self, dt: f32) -> i32 {
        let mut damage = 0;
        if let Some(burn) = &mut self.burn {
            burn.accum += burn.damage_per_sec * dt;
            burn.ttl -= dt;
            while burn.accum >= 1.0 {
                damage += 1;
                burn.accum -= 1.0;
            }
            if burn.ttl <= 0.0 {
                self.burn = None;
            }
        }
        if let Some(frost) = &mut self.frost {
            frost.ttl -= dt;
            if frost.ttl <= 0.0 {
                self.frost = None;
            }
        }
        damage
    }

    /// True if this enemy carries an "armored" tag in its content
    /// definition. Used by the ShieldBreak primitive for bonus
    /// damage vs plated targets.
    pub fn is_armored(&self, content: &crate::content::ContentDb) -> bool {
        content
            .archetypes
            .get(&self.archetype)
            .and_then(|s| s.tags.as_ref())
            .map(|tags| tags.iter().any(|t| t == "armored" || t == "heavy"))
            .unwrap_or(false)
    }

    pub fn touch_player(&mut self, px: f32, py: f32) -> i32 {
        if self.touch_cooldown > 0.0 || self.touch_damage == 0 {
            return 0;
        }
        let dx = self.x - px;
        let dy = (self.y - py).abs().min((self.y + 1.0 - py).abs());
        if dx.abs() < self.reach && dy < self.reach {
            self.touch_cooldown = 0.5;
            return self.touch_damage;
        }
        0
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera, content: &crate::content::ContentDb) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let center = (sx.round() as i32, sy.round() as i32);

        let mip = camera.mip_level();
        if mip.shows_sprite() {
            let mut s = sprite::enemy_sprite_from_content(self.archetype, content);
            if self.hit_flash > 0.0 {
                s.tint_toward(Pixel::rgb(255, 255, 255), self.hit_flash.min(0.75));
            } else if self.tell_timer > 0.0 {
                s.tint_toward(Pixel::rgb(255, 245, 230), 0.55);
            } else if self.burn.is_some() {
                s.tint_toward(Pixel::rgb(255, 140, 40), 0.35);
            }
            // Build a sprite-local overlay at Close/Hero tiers so the
            // higher-detail zoom levels show extra state beats: burn soot
            // scatter, tell-halo sparkle, hit splatter. No overlay at the
            // Normal tier — the base tint still communicates state.
            let overlay = if mip.shows_overlay() {
                Some(build_enemy_overlay(self, mip))
            } else {
                None
            };
            // Eater visibly bloats as it consumes — each corpse bumps
            // the render scale so by the time it aggros, players can
            // see the problem lumbering toward them.
            let render_scale = if self.archetype == Archetype::Eater {
                camera.zoom * (1.0 + self.consumed as f32 * 0.15)
            } else {
                camera.zoom
            };
            s.blit_scaled_with_overlay(fb, center, render_scale, overlay.as_ref());
        } else if matches!(mip, MipLevel::Blob) {
            // Blob keeps the signature color and picks up burn/tell
            // feedback via a flat fill color swap.
            let color = if self.tell_timer > 0.0 {
                Pixel::rgb(255, 245, 230)
            } else if self.burn.is_some() {
                Pixel::rgb(255, 140, 40)
            } else {
                self.color()
            };
            sprite::render_blob(fb, center, color);
        } else {
            sprite::render_dot(fb, center, self.color());
        }
    }
}

/// Derive a sprite-local overlay from an enemy's current state. Called at
/// Close/Hero mip tiers only — at Normal the base tint does the work.
/// Positions are hash-derived from (archetype, x, y) so the decorations
/// stick to the enemy and don't jitter between frames.
fn build_enemy_overlay(e: &Enemy, mip: MipLevel) -> SpriteOverlay {
    let mut overlay = SpriteOverlay::new();
    let tmpl = sprite::enemy_sprite(e.archetype);
    let hi = matches!(mip, MipLevel::Hero);

    // Burn soot: a handful of dark pixels scattered over the sprite.
    // Dark-red at the Close tier, full soot black at Hero.
    if let Some(b) = &e.burn {
        let soot = if hi {
            Pixel::rgb(20, 10, 0)
        } else {
            Pixel::rgb(60, 20, 10)
        };
        let count = if hi { 5 } else { 3 };
        let seed = deterministic_seed(e.archetype.to_kind() as u64, e.x, e.y)
            ^ (b.ttl.to_bits() as u64);
        for i in 0..count {
            let (sx, sy) = sample_sprite_coord(seed.wrapping_mul(0x9E37_79B9).wrapping_add(i), tmpl.w, tmpl.h);
            overlay.stain(sx, sy, soot);
        }
    }

    // Hit flash sparkle — an extra white highlight pixel at Hero tier.
    if hi && e.hit_flash > 0.35 {
        let seed = deterministic_seed(0xFACE, e.x, e.y);
        let (sx, sy) = sample_sprite_coord(seed, tmpl.w, tmpl.h);
        overlay.stain(sx, sy, Pixel::rgb(255, 255, 255));
    }

    // Ranged enemies winding up a shot get a bright fore-eye at Hero.
    if hi && e.tell_timer > 0.0 {
        if tmpl.w >= 4 && tmpl.h >= 3 {
            overlay.stain(tmpl.w / 2, 1, Pixel::rgb(255, 230, 120));
        }
    }

    overlay
}

fn deterministic_seed(base: u64, x: f32, y: f32) -> u64 {
    let xb = x.to_bits() as u64;
    let yb = y.to_bits() as u64;
    base
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(xb.rotate_left(13))
        .wrapping_add(yb.rotate_left(29))
}

fn sample_sprite_coord(seed: u64, w: u16, h: u16) -> (u16, u16) {
    let w = w.max(1) as u64;
    let h = h.max(1) as u64;
    let sx = (seed % w) as u16;
    let sy = ((seed / w) % h) as u16;
    (sx, sy)
}

pub struct RangedShot {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub damage: i32,
}

fn initial_attack_cooldown(archetype: Archetype) -> f32 {
    match archetype {
        Archetype::Revenant | Archetype::Marksman => 1.2,
        _ => 0.0,
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}

