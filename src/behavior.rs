//! Behavior framework — composable targeting / movement / action /
//! reaction primitives. One vocabulary of small typed pieces, stitched
//! together per-archetype and per-brand.
//!
//! B1 lands the full v1 palette as callable types + a TOML authoring
//! surface. The sim does NOT yet consult most of these primitives at
//! runtime; that wiring lands incrementally through B2 (Flood retrofit)
//! and B3 (all existing brands). This milestone's job is to make the
//! vocabulary exist and be parseable from brand TOML.
//!
//! Runtime shape, per `Behavior.md`:
//! - exactly one [`Targeter`] (who to act on)
//! - exactly one [`Mover`] (how to close / keep distance)
//! - 0..N [`Action`]s (priority-ordered; first whose trigger matches
//!   and cooldown is ready fires this tick — dispatch lives in the sim,
//!   not here)
//! - 0..N [`Reaction`]s (event-driven; unification with `body_effect.rs`
//!   is B5)
//! - a [`TagSet`] of interaction-matrix keys
//!
//! Archetypes resolve their default [`BehaviorSet`] through the
//! [`BehaviorRegistry`]. `with_builtins()` populates it with the pre-B0
//! match-arm defaults; brand TOML declared under `[behavior]` on an
//! archetype stat block merges over that default at content-load time.
//!
//! Unknown string IDs (e.g. `targeter = "typo"`) do NOT fail the
//! load — they log a warn and resolve to [`Targeter::Inert`] /
//! [`Mover::Inert`] / [`Action::Inert`] so one typo in a brand can't
//! take the whole archetype offline.
//!
//! The `DirectorOverride` field on every `Enemy` sits above the
//! behavior layer: when a Director (dead player) issues a move/attack
//! command, the enemy reads the override before falling back to its
//! intrinsic `Targeter`. Empty today — scaffolding for the RTS-style
//! control path.

use std::collections::HashMap;

use serde::Deserialize;

use crate::enemy::Archetype;
use crate::tag::{Tag, TagSet};

// ============================================================================
// Targeter
// ============================================================================

/// Who the enemy wants to act on this tick. Resolves to a world
/// position (or entity id) at dispatch time; the `Mover` + `Action`s
/// take it from there.
#[derive(Clone, Debug, PartialEq)]
pub enum Targeter {
    /// Closest living player. Hastur-mark priority is enforced by
    /// the game-layer `marked_player_id` override on top of this.
    NearestPlayer,
    /// Closest living player, but promote a marked survivor if any
    /// are within range. Captures the "mark demands attention" shape
    /// explicitly instead of relying on game-layer fix-up.
    MarkedOrNearestPlayer,
    /// Closest living entity whose team ∈ `self.hostiles`. The
    /// building block for faction war (Flood vs horde, parasite vs
    /// everything alive).
    NearestHostile,
    /// Like `NearestHostile` but excludes any entity already carrying
    /// the `infected` tag. Prevents a headcrab swarm from infinitely
    /// re-infecting the same host.
    NearestAttachable,
    /// Corpse-seeker that flips to players once `hunger_threshold`
    /// corpses have been eaten. Used by the Eater.
    NearestCorpse { hunger_threshold: u8 },
    /// Invert the targeting vector — run *away* from the nearest
    /// hostile. For panicked infected hosts and civilian-flavor AI.
    FleeHostile,
    /// Stationary defender — picks no target, lets Actions fire at
    /// whatever wanders into range. Turrets + Howlers + emplacements.
    Stationary,
    /// Fallback for unknown IDs / parse failures. Picks no target;
    /// the sim should treat as "idle."
    Inert,
}

// ============================================================================
// Mover
// ============================================================================

/// How the enemy moves toward (or away from) its target.
#[derive(Clone, Debug, PartialEq)]
pub enum Mover {
    /// Straight-line chase respecting wall collision. What every
    /// existing enemy does today.
    Direct,
    /// A* pathfinding around walls. Recomputed every `recompute_ms`
    /// or when a wall ahead changes. Used by Breachers so they can
    /// actually route to a player instead of getting stuck.
    AStar { recompute_ms: u32 },
    /// Prefer tiles with cover on the target-facing side — stays out
    /// of direct LoS when possible while still advancing. Used by
    /// Rocketeers and future STALKER-style ambushers.
    Sneak {
        /// Multiplier on speed when in cover (0..2 typical).
        cover_speed_bias: f32,
    },
    /// Charge mover — winds up at distance, then sprints. Existing
    /// Charger behavior expressed explicitly so content can swap it.
    #[allow(dead_code)]
    Charge,
    /// Ignore wall collision entirely. Flying / phasing archetypes.
    #[allow(dead_code)]
    Hover,
    /// Windup → ballistic arc → lands. Pairs with `LeapAttach` for
    /// the headcrab pattern. `range` caps the leap distance; `windup`
    /// is the pre-leap telegraph duration.
    Leap { windup: f32, range: f32 },
    /// Disappears, re-emerges near target after a cooldown. Rat-swarm
    /// flavor. Sim impl deferred; here as a callable type.
    Burrow { cooldown: f32 },
    /// Fallback for unknown IDs — no movement this tick.
    Inert,
}

// ============================================================================
// Action — attack / interact primitives
// ============================================================================

/// One discrete thing an enemy can do to the world this tick.
/// Supersedes the old `Attacker` enum — its variants are folded in here
/// alongside the B1 interaction primitives (Convert, Raise, Consume,
/// LeapAttach).
///
/// Dispatch order lives in the sim: the first Action whose trigger
/// matches + cooldown is ready fires. Trigger semantics are currently
/// implicit (melee/hitscan/rocket/breach: "target in range"; others:
/// see per-variant docs); an explicit `trigger` field per-Action is a
/// planned B2 refinement.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Touch damage at contact reach.
    Melee,
    /// Hitscan shot with a windup tell.
    Hitscan { range: f32, tell: f32 },
    /// Fires a Rocket entity that flies through the world and
    /// explodes on impact.
    Rocket {
        range: f32,
        rocket_speed: f32,
        cooldown: f32,
        tell: f32,
    },
    /// Smashes through walls + hits players on contact. Path-blocked
    /// targets get the wall attacked instead of a stall.
    Breach {
        wall_damage: u8,
        swing_cooldown: f32,
        tell: f32,
    },
    /// Contact-attach action. Fires an `on_attach_success` event on
    /// the struck target; typically paired with a Convert Action that
    /// triggers on that event. Headcrab + facehugger shape.
    LeapAttach {
        #[serde(default = "d_leap_range")]
        range: f32,
        #[serde(default = "d_leap_windup")]
        windup: f32,
        #[serde(default = "d_leap_cooldown")]
        cooldown: f32,
    },
    /// Transform a living target. See `Behavior.md` §"The Convert
    /// primitive" and §"Phased conversion" for the full spec.
    Convert {
        /// Archetype id of the terminal form (string — resolves to
        /// [`Archetype`] via [`Archetype::from_name`] at spawn time).
        into: String,
        #[serde(default)]
        mode: ConvertMode,
        /// Swap-mode only: remove the host entity at terminal fire.
        /// Ignored in Stack mode (host slot is reused).
        #[serde(default)]
        kill_host: bool,
        /// Explicit team for the terminal form. `None` + `inherit_team
        /// = false` → the `into` archetype's own default team.
        #[serde(default)]
        into_team: Option<String>,
        /// When `into_team` is None, take the infector's team instead
        /// of the `into` archetype's default.
        #[serde(default)]
        inherit_team: bool,
        /// Depth cap on cascading conversions. @tunable — default 3
        /// per Behavior.md; revisit after playtest.
        #[serde(default = "d_chain_cap")]
        chain_cap: u8,
        #[serde(default)]
        trigger: ConvertTrigger,
        /// Phases of staged conversion. Empty = instant conversion
        /// (matches existing Flood tag-swap behavior).
        #[serde(default)]
        phases: Vec<Phase>,
        /// What happens if the host dies mid-infection. Default is
        /// `Cancel` — kill the host, no terminal. Brands flip to
        /// `TriggerTerminal` for canon-faithful inevitability.
        #[serde(default)]
        on_host_death: OnHostDeath,
    },
    /// Reanimate a corpse entity. Distinct from Convert (which targets
    /// living hostiles). Necromancer / Flood-reanimation flavor.
    Raise {
        into: String,
        range: f32,
        #[serde(default)]
        windup: f32,
        #[serde(default)]
        cooldown: f32,
        #[serde(default)]
        into_team: Option<String>,
        /// Remove the corpse on raise. `false` leaves a re-raisable
        /// body — cheese-vulnerable, but some flavors want it.
        #[serde(default)]
        consume_corpse: bool,
    },
    /// Eat something at point-blank for HP + optional growth. Rats
    /// eating corpses, Eater-style growth-on-feed.
    Consume {
        /// What to eat: "corpse", "substance", "enemy". Defaults to
        /// just "corpse" so the common case is terse.
        #[serde(default = "d_consume_targets")]
        targets: Vec<String>,
        range: f32,
        hp_gain: f32,
        /// If set, eating `growth_threshold` hp worth of food triggers
        /// a self-Convert into this archetype.
        #[serde(default)]
        growth_into: Option<String>,
        #[serde(default)]
        growth_threshold: Option<u32>,
    },
    /// Emit N units of an archetype on a cadence. Nest / spawner
    /// flavor; no host required.
    Spawn {
        into: String,
        count: u32,
        cooldown: f32,
    },
    /// Damage-area-on-death or damage-area-on-proximity.
    SelfDestruct {
        #[serde(default)]
        hp_threshold: Option<i32>,
        #[serde(default)]
        proximity: Option<f32>,
        damage: i32,
        radius: f32,
    },
    /// Catch-all for unknown action kinds in TOML. The deserializer
    /// falls back here when a `kind = "..."` tag matches no variant.
    /// Sim treats as no-op.
    #[serde(other)]
    Inert,
}

// ============================================================================
// Phase + OnHostDeath + Convert enums
// ============================================================================

/// One stage of a staged conversion. See `Behavior.md` §"Phased
/// conversion" for the full design rationale and the phase-flavor
/// table.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Phase {
    /// Log/debug label. Also produces the `infected_<name>` tag when
    /// the phase is active (B2 wiring).
    pub name: String,
    /// Seconds the phase runs before advancing. 0 is reserved for
    /// condition-gated phases (deferred past B1).
    pub duration: f32,
    /// Tags to add to the host while this phase is active.
    pub tags_add: Vec<String>,
    /// Temporary host-BehaviorSet swap by registry ID.
    pub behavior_override: Option<String>,
    /// Sprite overlay id (e.g., "headcrab_clinging").
    pub sprite_overlay: Option<String>,
    /// Hue shift on the host — palette key string resolved at render
    /// time.
    pub palette_tint: Option<String>,
    /// Ambient VFX emitters on the host (smoke_wisp, acid_drip, etc.).
    /// The VfxId registry grows as brands need effects.
    pub ambient_vfx: Vec<String>,
    /// Speed multiplier on the host.
    pub speed_mult: f32,
    /// Per-second HP drain; negative regens.
    pub hp_drain_per_sec: f32,
    /// Melee contact during this phase re-fires the parent Convert on
    /// the touched unit. Respects `chain_cap`.
    pub contagious: bool,
}

impl Default for Phase {
    fn default() -> Self {
        Self {
            name: String::new(),
            duration: 0.0,
            tags_add: Vec::new(),
            behavior_override: None,
            sprite_overlay: None,
            palette_tint: None,
            ambient_vfx: Vec::new(),
            speed_mult: 1.0,
            hp_drain_per_sec: 0.0,
            contagious: false,
        }
    }
}

/// What happens when the host entity dies mid-conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnHostDeath {
    /// Infection is discarded. Engine default: gives the player a real
    /// lever ("kill them before they turn") rather than making
    /// conversion inevitable.
    #[default]
    Cancel,
    /// Terminal fires immediately at the corpse. Canon-faithful for
    /// headcrab / Flood / classic zombie lore — brands opt in.
    TriggerTerminal,
    /// Skip to the final phase, continue ticking. "Dying accelerates
    /// the burst" flavor.
    AdvancePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvertMode {
    /// Kill the host, spawn `into` at its tile with fresh HP.
    /// Headcrab → zombie semantics. Clean; loses HP continuity.
    #[default]
    Swap,
    /// Keep the host slot, overlay identity (new behavior, new tags,
    /// stat multiplier, sprite swap). Flood's pre-B2 semantics.
    Stack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvertTrigger {
    /// Fires after a successful LeapAttach.
    #[default]
    OnAttachSuccess,
    /// Fires when the infector lands a hit on the host.
    OnHit,
    /// Fires when the infector kills the host.
    OnKill,
    /// Fires the tick the Action is reached (no trigger condition).
    Immediate,
}

// ============================================================================
// Reaction (stubby in B1; body_effect unification is B5)
// ============================================================================

/// Event-driven behavior. v1 stores a string effect id that resolves
/// through `body_effect::ReactionRegistry` at dispatch time. The sim
/// doesn't consume `Reaction`s stored on BehaviorSets yet — they're
/// parsed + stored so brand TOML can author reactions in the same
/// block as actions, and B5 unifies with body_effect.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Reaction {
    #[serde(rename = "kind")]
    pub trigger: ReactionTrigger,
    /// String handler id — resolves through `body_effect::
    /// ReactionRegistry` today (same handlers used by the legacy
    /// `body_on_death` field). In B5 this field also accepts inline
    /// `Action`s.
    pub effect: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReactionTrigger {
    OnDeath,
    OnHit,
    OnAttachSuccess,
    OnHostileNear,
    OnKill,
}

// ============================================================================
// InfectionState — runtime state for a host carrying a staged Convert
// ============================================================================

/// Live infection riding a host enemy. Installed by the generic
/// Convert dispatch in `game.rs` when an infector's trigger condition
/// fires; ticked each sim step via [`InfectionState::tick`]; fires the
/// terminal (Swap spawns the `into_archetype` at the host's tile,
/// Stack overlays identity) when all phases expire.
///
/// Stored as `Option<Box<InfectionState>>` on `Enemy` so uninfected
/// hosts only pay a pointer worth of cache footprint — phases carry a
/// `Vec`, and cloning one per uninfected enemy would sum to real
/// memory on a 500-enemy tick.
#[derive(Clone, Debug)]
pub struct InfectionState {
    /// Archetype to spawn at terminal. Resolved from the parent
    /// Convert action's string `into` to an enum at install time so
    /// the phase tick doesn't re-parse every frame.
    pub into_archetype: Archetype,
    pub mode: ConvertMode,
    /// Swap-only: kill the host at terminal. Ignored in Stack mode
    /// where the host slot is reused.
    pub kill_host: bool,
    /// Explicit post-terminal team override. `None` defers to the
    /// `into` archetype's Enemy::spawn default.
    pub into_team: Option<Tag>,
    pub on_host_death: OnHostDeath,
    pub phases: Vec<Phase>,
    /// Current phase index into `phases`. When it equals or exceeds
    /// `phases.len()`, the terminal is ready to fire.
    pub phase_idx: usize,
    /// Seconds remaining in the current phase.
    pub phase_timer: f32,
}

impl InfectionState {
    /// Advance the phase timer by `dt`. Returns what the caller
    /// should do next this tick.
    pub fn tick(&mut self, dt: f32) -> InfectionTick {
        // Empty phases — terminal fires the same tick the infection
        // is installed. Matches B2 Floodling (phases: []) semantics
        // when the generic path ever gets extended to cover it.
        if self.phase_idx >= self.phases.len() {
            return InfectionTick::TerminalFire;
        }
        self.phase_timer -= dt;
        if self.phase_timer > 0.0 {
            return InfectionTick::Continue;
        }
        self.phase_idx += 1;
        if self.phase_idx >= self.phases.len() {
            return InfectionTick::TerminalFire;
        }
        self.phase_timer = self.phases[self.phase_idx].duration;
        InfectionTick::AdvancedPhase
    }

    /// The currently-active [`Phase`], if any.
    pub fn current_phase(&self) -> Option<&Phase> {
        self.phases.get(self.phase_idx)
    }
}

/// Result of advancing an infection by one sim tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfectionTick {
    /// Still inside the current phase; keep ticking.
    Continue,
    /// Phase boundary crossed; next phase is now active.
    AdvancedPhase,
    /// All phases done — dispatch should fire the terminal.
    TerminalFire,
}

// ============================================================================
// BehaviorSet — the composed shape
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
pub struct BehaviorSet {
    pub targeter: Targeter,
    pub mover: Mover,
    /// Priority-ordered action list. First matching + ready entry
    /// fires per tick. Empty = no offensive dispatch.
    pub actions: Vec<Action>,
    /// Event reactions. See [`Reaction`] — not yet consumed by the
    /// sim in B1; bridged to body_effect in B5.
    pub reactions: Vec<Reaction>,
    /// Interaction-matrix tags (e.g. "biological", "armored",
    /// "heavy"). Keyed the same way as gore / substance tags.
    pub tags: TagSet,
}

impl BehaviorSet {
    /// The historical baseline: nearest-player + direct chase + melee
    /// + no tags. Still the default fallback when a registry lookup
    /// misses, and the underlying shape of most ground-pounder
    /// archetypes.
    pub fn direct_melee() -> Self {
        Self {
            targeter: Targeter::NearestPlayer,
            mover: Mover::Direct,
            actions: vec![Action::Melee],
            reactions: Vec::new(),
            tags: TagSet::default(),
        }
    }
}

// ============================================================================
// BehaviorToml — the serde layer
// ============================================================================

/// Per-archetype `[behavior]` block in TOML. Every field is optional;
/// `apply_toml` merges non-empty values over the registry's default
/// for the archetype so a brand can swap just the mover without
/// losing the archetype's actions/tags.
///
/// ```toml
/// [behavior]
/// targeter = "nearest_attachable"
/// mover    = "leap"
///
/// [[behavior.actions]]
/// kind = "leap_attach"
/// range = 3.5
///
/// [[behavior.actions]]
/// kind = "convert"
/// into = "zombie"
/// mode = "swap"
/// kill_host = true
///
/// [[behavior.actions.phases]]
/// name = "mounted"
/// duration = 2.5
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BehaviorToml {
    /// String id for the Targeter — `"nearest_player"`, `"flee_hostile"`, etc.
    pub targeter: Option<String>,
    /// String id for the Mover — `"direct"`, `"leap"`, `"astar"`, etc.
    /// Param-bearing movers use default params; richer forms are a B2 refinement.
    pub mover: Option<String>,
    pub actions: Vec<Action>,
    pub reactions: Vec<Reaction>,
    pub tags: Vec<String>,
}

impl BehaviorToml {
    /// Resolve a bare targeter id string to the enum variant. Unknown
    /// strings log a warn and return [`Targeter::Inert`] — sim stays
    /// alive, one enemy idles.
    fn resolve_targeter(id: &str) -> Targeter {
        match id {
            "nearest_player" => Targeter::NearestPlayer,
            "marked_or_nearest_player" => Targeter::MarkedOrNearestPlayer,
            "nearest_hostile" => Targeter::NearestHostile,
            "nearest_attachable" => Targeter::NearestAttachable,
            "nearest_corpse" => Targeter::NearestCorpse { hunger_threshold: 5 },
            "flee_hostile" => Targeter::FleeHostile,
            "stationary" => Targeter::Stationary,
            "inert" => Targeter::Inert,
            other => {
                tracing::warn!(
                    targeter = other,
                    "unknown targeter id in behavior TOML — falling back to Inert"
                );
                Targeter::Inert
            }
        }
    }

    /// Resolve a bare mover id string to the enum variant. Param-
    /// bearing movers use their defaults here; explicit params on a
    /// mover (e.g. a long-range Leap) will want a richer TOML form in B2.
    fn resolve_mover(id: &str) -> Mover {
        match id {
            "direct" => Mover::Direct,
            "astar" => Mover::AStar { recompute_ms: 650 },
            "sneak" => Mover::Sneak { cover_speed_bias: 1.35 },
            "charge" => Mover::Charge,
            "hover" => Mover::Hover,
            "leap" => Mover::Leap { windup: 0.35, range: 22.0 },
            "burrow" => Mover::Burrow { cooldown: 2.0 },
            "inert" => Mover::Inert,
            other => {
                tracing::warn!(
                    mover = other,
                    "unknown mover id in behavior TOML — falling back to Inert"
                );
                Mover::Inert
            }
        }
    }

    /// Count how many actions landed on the `Inert` sink (unknown
    /// `kind`). Reported as a warn count per archetype so brand
    /// authors see typos without rebuilding.
    fn inert_action_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| matches!(a, Action::Inert))
            .count()
    }
}

// ============================================================================
// BehaviorRegistry
// ============================================================================

/// Name → `BehaviorSet` dispatch table. The name is the snake_case
/// archetype ID (matches `Archetype::to_name`). `with_builtins`
/// populates defaults; `apply_toml` merges brand overrides on top.
#[derive(Default)]
pub struct BehaviorRegistry {
    entries: HashMap<String, BehaviorSet>,
}

impl BehaviorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, id: impl Into<String>, set: BehaviorSet) {
        self.entries.insert(id.into(), set);
    }

    pub fn get(&self, id: &str) -> Option<&BehaviorSet> {
        self.entries.get(id)
    }

    /// Resolve an archetype to its current BehaviorSet. Clones out of
    /// the registry. Falls back to `direct_melee` + `warn!` on miss
    /// so a brand typo can't take the sim down.
    pub fn for_archetype(&self, archetype: Archetype) -> BehaviorSet {
        let name = archetype.to_name();
        if let Some(set) = self.entries.get(name) {
            return set.clone();
        }
        tracing::warn!(
            archetype = name,
            "behavior registry miss — falling back to direct_melee"
        );
        BehaviorSet::direct_melee()
    }

    /// Merge a TOML `[behavior]` block over the archetype's registered
    /// default. Non-empty TOML fields override; empty fields inherit.
    /// Called once per archetype at content-load time from `Game::new`.
    ///
    /// If the archetype has no registered default (e.g. a content pack
    /// adds a brand-new archetype id), the merge starts from
    /// `direct_melee` and logs a warn — the sim stays alive with a
    /// sensible fallback while the author fills in the gap.
    pub fn apply_toml(&mut self, archetype_name: &str, toml: &BehaviorToml) {
        let mut base = match self.entries.get(archetype_name) {
            Some(existing) => existing.clone(),
            None => {
                tracing::warn!(
                    archetype = archetype_name,
                    "apply_toml on unregistered archetype — starting from direct_melee"
                );
                BehaviorSet::direct_melee()
            }
        };
        if let Some(t) = toml.targeter.as_ref() {
            base.targeter = BehaviorToml::resolve_targeter(t);
        }
        if let Some(m) = toml.mover.as_ref() {
            base.mover = BehaviorToml::resolve_mover(m);
        }
        if !toml.actions.is_empty() {
            base.actions = toml.actions.clone();
            let inert = toml.inert_action_count();
            if inert > 0 {
                tracing::warn!(
                    archetype = archetype_name,
                    inert_actions = inert,
                    "behavior TOML contained unknown action kinds — resolved to Inert"
                );
            }
        }
        if !toml.reactions.is_empty() {
            base.reactions = toml.reactions.clone();
        }
        if !toml.tags.is_empty() {
            base.tags = TagSet::from_string_slice(&toml.tags);
        }
        self.entries.insert(archetype_name.to_string(), base);
    }

    /// Install every built-in archetype's BehaviorSet. Mirrors the
    /// pre-B0 hardcoded match in `behaviors_for(arch)` exactly; the
    /// sim is byte-identical to before the registry existed.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();

        // Baseline direct-melee workhorses.
        r.register("rusher", BehaviorSet::direct_melee());
        r.register("pinkie", BehaviorSet::direct_melee());
        r.register("pmc", BehaviorSet::direct_melee());
        r.register("swarmling", BehaviorSet::direct_melee());
        r.register("miniboss", BehaviorSet::direct_melee());

        // Tagged direct-melee variants.
        //
        // B3 metadata pass: where the archetype has an intent beyond
        // plain direct-melee (Charger's windup, Leaper's leap, Orb's
        // death-explode, Splitter's burst), declare it explicitly here
        // so the registry is self-describing. Sim consumption is
        // deferred — hardcoded dispatch in `game.rs` still drives most
        // of these — but the registry is now the spec-of-record for
        // "what does this archetype actually do?".
        r.register(
            "charger",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                // Charge supersedes the old Direct + "charge" tag
                // pairing. Same mechanic; clearer declaration. The sim
                // still reads Archetype::Charger directly for the
                // sprint windup at game.rs:~2062.
                mover: Mover::Charge,
                actions: vec![Action::Melee],
                // Declared-but-not-consumed: the windup telegraph that
                // plays before the sprint commits. B5 wires reactions
                // into the sim; until then this surfaces the intent.
                reactions: vec![Reaction {
                    trigger: ReactionTrigger::OnHostileNear,
                    effect: "charge_telegraph".to_string(),
                }],
                tags: TagSet::default(),
            },
        );
        r.register(
            "orb",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Melee],
                // Orb body_on_death = "explode_big" in archetype TOML.
                // Declaring the reaction here makes the archetype's
                // "floating bomb" identity legible at the registry
                // level. Body-effect dispatch still reads the TOML
                // string; B5 collapses.
                reactions: vec![Reaction {
                    trigger: ReactionTrigger::OnDeath,
                    effect: "explode_big".to_string(),
                }],
                tags: TagSet::from_strs(&["floating"]),
            },
        );
        r.register(
            "leaper",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                // LeapAttach is the priority action; Melee is the
                // fallback when already in contact. Params roughly
                // match the hardcoded leap ranges at game.rs leap_*.
                actions: vec![
                    Action::LeapAttach {
                        range: 4.0,
                        windup: 0.45,
                        cooldown: 1.6,
                    },
                    Action::Melee,
                ],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["biological", "agile", "leaper"]),
            },
        );
        r.register("phaser", tagged_melee(&["arcane", "agile", "phaser"]));
        r.register(
            "splitter",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Melee],
                // The whole point of the archetype: burst into
                // Swarmlings on death. Spec-example in Behavior.md —
                // "Splitter becomes legible as a reaction-driven
                // spawner." Today the behavior fires via the
                // archetype TOML body_on_death field; declaring it
                // here means a brand can override behavior without
                // editing both surfaces after B5 lands.
                reactions: vec![Reaction {
                    trigger: ReactionTrigger::OnDeath,
                    effect: "spawn_swarmlings".to_string(),
                }],
                tags: TagSet::from_strs(&["biological", "fragmenting"]),
            },
        );
        r.register("zergling", tagged_melee(&["biological", "swarm", "agile"]));
        // Floodling — carries the B2 retrofit's canonical Convert action.
        // Dispatch at `game.rs::tick_flood_conversions` reads this action
        // to drive the in-place Stack-mode swap of any living non-Flood
        // host within touch range. The 30/70 Flood/FloodCarrier split is
        // preserved as a dispatch-layer quirk — B3 cleanup target.
        r.register(
            "floodling",
            BehaviorSet {
                targeter: Targeter::NearestHostile,
                mover: Mover::Direct,
                actions: vec![
                    Action::Melee,
                    Action::Convert {
                        into: "flood_carrier".to_string(),
                        mode: ConvertMode::Stack,
                        kill_host: false,
                        into_team: Some("flood".to_string()),
                        inherit_team: false,
                        chain_cap: 0,
                        trigger: ConvertTrigger::OnHit,
                        phases: Vec::new(),
                        on_host_death: OnHostDeath::TriggerTerminal,
                    },
                ],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["parasite", "infectious"]),
            },
        );
        r.register("flood", tagged_melee(&["flood", "infected", "feral"]));
        r.register(
            "flood_carrier",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Melee],
                // Completes the Flood story: Floodling converts a host
                // into a FloodCarrier (via the B2 Convert action), the
                // Carrier's eventual death bursts 4-5 fresh Floodlings
                // per archetype TOML body_on_death = "burst_floodlings".
                reactions: vec![Reaction {
                    trigger: ReactionTrigger::OnDeath,
                    effect: "burst_floodlings".to_string(),
                }],
                tags: TagSet::from_strs(&["flood", "carrier", "volatile"]),
            },
        );

        // Hitscan shooters.
        r.register(
            "revenant",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Hitscan { range: 40.0, tell: 0.45 }],
                reactions: Vec::new(),
                tags: TagSet::default(),
            },
        );
        r.register(
            "marksman",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Hitscan { range: 40.0, tell: 0.45 }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["tactical"]),
            },
        );
        r.register(
            "killa",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Hitscan { range: 60.0, tell: 0.18 }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["biological", "armored", "heavy", "boss"]),
            },
        );

        // Corpse-eater — spec example for the Consume primitive.
        // Priority ordering: Consume when a corpse is in range; fall
        // through to Melee once the Targeter's `hunger_threshold` of
        // corpses has been eaten (aggro flip → players). The hunger-
        // flip itself still lives in `game.rs` — it depends on the
        // `consumed: u8` counter on Enemy, which isn't yet part of
        // the Consume primitive's spec. B4+ may promote the aggro-
        // flip to a declarative "condition-gated phase" on Consume.
        r.register(
            "eater",
            BehaviorSet {
                targeter: Targeter::NearestCorpse { hunger_threshold: 5 },
                mover: Mover::Direct,
                actions: vec![
                    Action::Consume {
                        targets: vec!["corpse".to_string()],
                        range: 2.5,
                        hp_gain: 0.0,
                        growth_into: None,
                        growth_threshold: None,
                    },
                    Action::Melee,
                ],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["abomination", "hungry"]),
            },
        );

        // Wall-smashers.
        r.register(
            "breacher",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::AStar { recompute_ms: 650 },
                actions: vec![Action::Breach {
                    wall_damage: 3,
                    swing_cooldown: 1.4,
                    tell: 0.55,
                }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["biological", "heavy", "breacher"]),
            },
        );
        r.register(
            "sapper",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Breach {
                    wall_damage: 8,
                    swing_cooldown: 0.3,
                    tell: 0.3,
                }],
                // Sapper runs at a wall, smashes, dies mid-smash; the
                // boom fires via archetype TOML body_on_death =
                // "explode_big". Declaring the reaction here makes
                // the "walking bomb" identity explicit.
                reactions: vec![Reaction {
                    trigger: ReactionTrigger::OnDeath,
                    effect: "explode_big".to_string(),
                }],
                tags: TagSet::from_strs(&["biological", "volatile", "breacher"]),
            },
        );
        r.register(
            "juggernaut",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Breach {
                    wall_damage: 4,
                    swing_cooldown: 1.1,
                    tell: 0.4,
                }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["mechanical", "heavy", "armored", "breacher"]),
            },
        );

        // Sneaking rocketeer.
        r.register(
            "rocketeer",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Sneak { cover_speed_bias: 1.35 },
                actions: vec![Action::Rocket {
                    range: 60.0,
                    rocket_speed: 55.0,
                    cooldown: 3.5,
                    tell: 0.7,
                }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["tactical", "rocketeer"]),
            },
        );

        // Stationary emplacements.
        r.register(
            "howler",
            BehaviorSet {
                targeter: Targeter::Stationary,
                mover: Mover::Direct,
                actions: vec![Action::Melee],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["cultist", "support", "noisy"]),
            },
        );
        r.register(
            "sentinel",
            BehaviorSet {
                targeter: Targeter::Stationary,
                mover: Mover::Direct,
                actions: vec![Action::Hitscan { range: 48.0, tell: 0.55 }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["mechanical", "turret"]),
            },
        );
        r.register(
            "player_turret",
            BehaviorSet {
                targeter: Targeter::Stationary,
                mover: Mover::Direct,
                actions: vec![Action::Hitscan { range: 28.0, tell: 0.3 }],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["mechanical", "ally", "turret"]),
            },
        );

        // -- B4 first-content: Half-Life Headcrab + Zombie -------------
        //
        // Headcrab is the spec-aligned Behavior.md §4.1 archetype: a
        // leap-attach parasite that Swap-Converts a host into a Zombie
        // after a two-phase gestation. The BehaviorSet declares the
        // full intent; sim dispatch for LeapAttach / OnAttachSuccess /
        // phased Convert is deferred past B4 — this milestone's scope
        // is authoring, not integration. Today the only Convert
        // dispatch path in `game.rs` is the Floodling pass, which
        // gates on Archetype::Floodling; Headcrabs spawned via the
        // half_life brand sit declaratively until the generic dispatch
        // lands.
        r.register(
            "headcrab",
            BehaviorSet {
                targeter: Targeter::NearestAttachable,
                mover: Mover::Direct,
                actions: vec![
                    Action::LeapAttach {
                        range: 3.5,
                        windup: 0.35,
                        cooldown: 1.2,
                    },
                    Action::Convert {
                        into: "zombie".to_string(),
                        mode: ConvertMode::Swap,
                        kill_host: true,
                        into_team: Some("horde".to_string()),
                        inherit_team: false,
                        chain_cap: 1,
                        trigger: ConvertTrigger::OnAttachSuccess,
                        phases: vec![
                            Phase {
                                name: "mounted".to_string(),
                                duration: 2.5,
                                tags_add: vec!["infected".to_string()],
                                behavior_override: Some("panicked_host".to_string()),
                                sprite_overlay: Some("headcrab_clinging".to_string()),
                                palette_tint: None,
                                ambient_vfx: Vec::new(),
                                speed_mult: 1.0,
                                hp_drain_per_sec: 0.0,
                                contagious: false,
                            },
                            Phase {
                                name: "deteriorating".to_string(),
                                duration: 3.5,
                                tags_add: vec![
                                    "infected".to_string(),
                                    "symptomatic".to_string(),
                                ],
                                behavior_override: Some("stumbling_host".to_string()),
                                sprite_overlay: Some(
                                    "headcrab_deteriorating".to_string(),
                                ),
                                palette_tint: None,
                                ambient_vfx: vec!["blood_mist".to_string()],
                                speed_mult: 0.4,
                                hp_drain_per_sec: 3.0,
                                contagious: false,
                            },
                        ],
                        on_host_death: OnHostDeath::TriggerTerminal,
                    },
                    // Melee fallback when the host is already in contact
                    // range but LeapAttach is on cooldown.
                    Action::Melee,
                ],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["parasite", "infectious", "biological"]),
            },
        );
        // Zombie — the converted marine. Shambling direct-melee with
        // the "infected" tag so other parasite-faction behaviors can
        // key off it. No further Convert chain: chain_cap = 1 on the
        // Headcrab's Convert means a Zombie can't become something
        // else via re-infection (prevents runaway "everything becomes
        // one thing" cascades). Tags deliberately include "flesh" so
        // existing gore interactions trigger as expected.
        r.register(
            "zombie",
            BehaviorSet {
                targeter: Targeter::NearestPlayer,
                mover: Mover::Direct,
                actions: vec![Action::Melee],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&[
                    "biological",
                    "flesh",
                    "infected",
                    "shambling",
                ]),
            },
        );

        // -- B4 first-content: Starving Rats -------------------------------
        //
        // Behavior.md §4.4 — Rat eats corpses to grow into DireRat.
        // Consume action with growth_into + growth_threshold is the
        // spec-example for that primitive. NearestCorpse targeter
        // prioritizes corpses; when none remain, the sim's generic
        // target-fallback aggros players (same pattern as Eater). The
        // Consume action's growth → self-Convert path isn't wired in
        // the sim yet (deferred past B4, same as Headcrab's Convert);
        // the registry carries the full spec for when dispatch lands.
        r.register(
            "rat",
            BehaviorSet {
                targeter: Targeter::NearestCorpse { hunger_threshold: 255 },
                mover: Mover::Direct,
                actions: vec![
                    Action::Consume {
                        targets: vec!["corpse".to_string(), "enemy".to_string()],
                        range: 1.2,
                        hp_gain: 6.0,
                        growth_into: Some("dire_rat".to_string()),
                        // 30 HP of eaten fodder → swap to DireRat.
                        // Matches Behavior.md §4.4 exactly.
                        growth_threshold: Some(30),
                    },
                    Action::Melee,
                ],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["biological", "swarm", "vermin", "starving"]),
            },
        );
        // DireRat — grown form. Bigger, slower, no Consume (already
        // fed). Pure melee threat; kept on the swarm team so its
        // hostility model matches Rats. Not a miniboss-tier threat by
        // itself — the danger is a ROOM full of them.
        r.register(
            "dire_rat",
            BehaviorSet {
                targeter: Targeter::NearestHostile,
                mover: Mover::Direct,
                actions: vec![Action::Melee],
                reactions: Vec::new(),
                tags: TagSet::from_strs(&["biological", "swarm", "vermin", "heavy"]),
            },
        );

        r
    }
}

/// Builder shorthand: NearestPlayer + Direct + Melee + the given tags.
fn tagged_melee(tags: &[&str]) -> BehaviorSet {
    BehaviorSet {
        targeter: Targeter::NearestPlayer,
        mover: Mover::Direct,
        actions: vec![Action::Melee],
        reactions: Vec::new(),
        tags: TagSet::from_strs(tags),
    }
}

// ============================================================================
// Director overrides (unchanged from B0)
// ============================================================================

/// Verbs a Director / scripted override can issue to a single enemy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandVerb {
    MoveTo,
    AttackAt,
    AttackEntity,
    Guard,
}

#[derive(Clone, Copy, Debug)]
pub struct DirectorOverride {
    pub verb: CommandVerb,
    pub point: (f32, f32),
    pub entity: Option<u32>,
    pub ttl: f32,
}

#[allow(dead_code)]
pub fn has_tag(set: &BehaviorSet, tag: Tag) -> bool {
    set.tags.has(tag)
}

// ============================================================================
// Serde defaults
// ============================================================================

fn d_chain_cap() -> u8 {
    3
}
fn d_leap_range() -> f32 {
    3.5
}
fn d_leap_windup() -> f32 {
    0.35
}
fn d_leap_cooldown() -> f32 {
    1.2
}
fn d_consume_targets() -> Vec<String> {
    vec!["corpse".to_string()]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ARCHETYPES: &[Archetype] = &[
        Archetype::Rusher,
        Archetype::Pinkie,
        Archetype::Charger,
        Archetype::Revenant,
        Archetype::Marksman,
        Archetype::Pmc,
        Archetype::Swarmling,
        Archetype::Orb,
        Archetype::Miniboss,
        Archetype::Eater,
        Archetype::Breacher,
        Archetype::Rocketeer,
        Archetype::Leaper,
        Archetype::Sapper,
        Archetype::Juggernaut,
        Archetype::Howler,
        Archetype::Sentinel,
        Archetype::Splitter,
        Archetype::Killa,
        Archetype::Zergling,
        Archetype::Floodling,
        Archetype::Flood,
        Archetype::FloodCarrier,
        Archetype::PlayerTurret,
        Archetype::Phaser,
        Archetype::Headcrab,
        Archetype::Zombie,
        Archetype::Rat,
        Archetype::DireRat,
    ];

    #[test]
    fn with_builtins_covers_every_archetype() {
        let reg = BehaviorRegistry::with_builtins();
        for arch in ALL_ARCHETYPES {
            let name = arch.to_name();
            assert!(
                reg.get(name).is_some(),
                "archetype {name:?} has no entry in BehaviorRegistry::with_builtins"
            );
        }
    }

    #[test]
    fn for_archetype_matches_registry_get() {
        let reg = BehaviorRegistry::with_builtins();
        for arch in ALL_ARCHETYPES {
            let via_helper = reg.for_archetype(*arch);
            let via_name = reg.get(arch.to_name()).cloned().unwrap();
            assert_eq!(via_helper, via_name);
        }
    }

    #[test]
    fn missing_archetype_falls_back_to_direct_melee() {
        let empty = BehaviorRegistry::new();
        let fb = empty.for_archetype(Archetype::Rusher);
        assert_eq!(fb, BehaviorSet::direct_melee());
    }

    /// Spot-check that archetypes with signature combos survive the
    /// B0→B1 refactor — the shape changed from `attacker: Attacker`
    /// to `actions: Vec<Action>`, so this is the regression armor.
    #[test]
    fn signature_archetypes_retain_their_combo() {
        let reg = BehaviorRegistry::with_builtins();

        let breacher = reg.get("breacher").unwrap();
        assert!(matches!(breacher.mover, Mover::AStar { .. }));
        assert!(matches!(breacher.actions[0], Action::Breach { .. }));

        let rocketeer = reg.get("rocketeer").unwrap();
        assert!(matches!(rocketeer.mover, Mover::Sneak { .. }));
        assert!(matches!(rocketeer.actions[0], Action::Rocket { .. }));

        let eater = reg.get("eater").unwrap();
        assert!(matches!(eater.targeter, Targeter::NearestCorpse { .. }));

        let howler = reg.get("howler").unwrap();
        assert_eq!(howler.targeter, Targeter::Stationary);

        let sentinel = reg.get("sentinel").unwrap();
        assert_eq!(sentinel.targeter, Targeter::Stationary);
        assert!(matches!(sentinel.actions[0], Action::Hitscan { .. }));
    }

    // -- B1 additions ---------------------------------------------------

    #[test]
    fn toml_empty_block_inherits_everything() {
        let mut reg = BehaviorRegistry::with_builtins();
        let before = reg.get("breacher").cloned().unwrap();
        let empty: BehaviorToml = toml::from_str("").unwrap();
        reg.apply_toml("breacher", &empty);
        assert_eq!(reg.get("breacher").cloned().unwrap(), before);
    }

    #[test]
    fn toml_partial_override_merges() {
        let mut reg = BehaviorRegistry::with_builtins();
        let before = reg.get("pinkie").cloned().unwrap();
        // Flip just the mover; actions/tags/targeter should survive.
        let src = r#"mover = "leap""#;
        let t: BehaviorToml = toml::from_str(src).unwrap();
        reg.apply_toml("pinkie", &t);
        let after = reg.get("pinkie").cloned().unwrap();
        assert_eq!(after.targeter, before.targeter);
        assert!(matches!(after.mover, Mover::Leap { .. }));
        assert_eq!(after.actions, before.actions);
    }

    #[test]
    fn toml_unknown_targeter_falls_back_to_inert() {
        let mut reg = BehaviorRegistry::with_builtins();
        let src = r#"targeter = "nonexistent_targeter""#;
        let t: BehaviorToml = toml::from_str(src).unwrap();
        reg.apply_toml("rusher", &t);
        assert_eq!(reg.get("rusher").unwrap().targeter, Targeter::Inert);
    }

    #[test]
    fn toml_unknown_action_kind_becomes_inert() {
        let src = r#"
            [[actions]]
            kind = "not_a_real_action"
            foo = 42
        "#;
        let t: BehaviorToml = toml::from_str(src).unwrap();
        assert_eq!(t.actions.len(), 1);
        assert_eq!(t.actions[0], Action::Inert);
    }

    #[test]
    fn toml_parses_full_headcrab_convert() {
        // Full example from Behavior.md §4.1 — exercises Convert with
        // phases, OnHostDeath override, two phases with overlay + vfx.
        let src = r#"
            targeter = "nearest_attachable"
            mover    = "leap"

            [[actions]]
            kind = "leap_attach"
            range = 3.5
            windup = 0.35
            cooldown = 1.2

            [[actions]]
            kind = "convert"
            trigger = "on_attach_success"
            into = "zombie"
            mode = "swap"
            kill_host = true
            into_team = "horde"
            chain_cap = 1
            on_host_death = "trigger_terminal"

            [[actions.phases]]
            name = "mounted"
            duration = 2.5
            tags_add = ["infected"]
            behavior_override = "panicked_host"
            sprite_overlay = "headcrab_clinging"

            [[actions.phases]]
            name = "deteriorating"
            duration = 3.5
            tags_add = ["infected", "symptomatic"]
            behavior_override = "stumbling_host"
            sprite_overlay = "headcrab_deteriorating"
            ambient_vfx = ["blood_mist"]
            speed_mult = 0.4
            hp_drain_per_sec = 3.0
        "#;
        let t: BehaviorToml = toml::from_str(src).expect("headcrab TOML must parse");
        assert_eq!(t.actions.len(), 2);
        assert!(matches!(t.actions[0], Action::LeapAttach { .. }));
        let Action::Convert {
            ref into,
            mode,
            kill_host,
            ref into_team,
            chain_cap,
            on_host_death,
            ref phases,
            ..
        } = t.actions[1]
        else {
            panic!("second action must be Convert");
        };
        assert_eq!(into, "zombie");
        assert_eq!(mode, ConvertMode::Swap);
        assert!(kill_host);
        assert_eq!(into_team.as_deref(), Some("horde"));
        assert_eq!(chain_cap, 1);
        assert_eq!(on_host_death, OnHostDeath::TriggerTerminal);
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0].name, "mounted");
        assert_eq!(phases[0].duration, 2.5);
        assert_eq!(phases[1].ambient_vfx, vec!["blood_mist".to_string()]);
        assert!((phases[1].speed_mult - 0.4).abs() < 1e-6);
    }

    #[test]
    fn toml_parses_raise_and_consume() {
        let src = r#"
            [[actions]]
            kind = "raise"
            into = "risen_skeleton"
            range = 6.0
            windup = 0.8
            cooldown = 4.0
            into_team = "undead"
            consume_corpse = true

            [[actions]]
            kind = "consume"
            targets = ["corpse", "enemy"]
            range = 1.2
            hp_gain = 6.0
            growth_into = "dire_rat"
            growth_threshold = 30
        "#;
        let t: BehaviorToml = toml::from_str(src).unwrap();
        assert!(matches!(t.actions[0], Action::Raise { .. }));
        assert!(matches!(t.actions[1], Action::Consume { .. }));
    }

    // -- B4: first-content Headcrab + Zombie ----------------------------

    /// Headcrab resolves via both sides of the archetype name mapping.
    /// If this breaks, every from_name/to_name match site needs an
    /// update.
    #[test]
    fn headcrab_and_zombie_archetype_roundtrip() {
        assert_eq!(Archetype::from_name("headcrab"), Some(Archetype::Headcrab));
        assert_eq!(Archetype::from_name("zombie"), Some(Archetype::Zombie));
        assert_eq!(Archetype::Headcrab.to_name(), "headcrab");
        assert_eq!(Archetype::Zombie.to_name(), "zombie");
        // Kind encoding survives round-trip — net snapshot protocol
        // relies on this.
        assert_eq!(
            Archetype::from_kind(Archetype::Headcrab.to_kind()),
            Archetype::Headcrab
        );
        assert_eq!(
            Archetype::from_kind(Archetype::Zombie.to_kind()),
            Archetype::Zombie
        );
    }

    /// Headcrab's registry default carries the spec-aligned composition:
    /// LeapAttach priority-action → Convert with Swap mode, 2 phases,
    /// on_host_death = TriggerTerminal. This is the B4 authoring-stress
    /// test — proves the full v1 palette round-trips through Rust
    /// construction.
    #[test]
    fn headcrab_default_is_spec_aligned() {
        let reg = BehaviorRegistry::with_builtins();
        let hc = reg.get("headcrab").unwrap();
        assert_eq!(hc.targeter, Targeter::NearestAttachable);
        assert!(matches!(hc.actions.first(), Some(Action::LeapAttach { .. })));
        let convert = hc
            .actions
            .iter()
            .find_map(|a| match a {
                Action::Convert {
                    into,
                    mode,
                    kill_host,
                    into_team,
                    chain_cap,
                    trigger,
                    phases,
                    on_host_death,
                    ..
                } => Some((
                    into.clone(),
                    *mode,
                    *kill_host,
                    into_team.clone(),
                    *chain_cap,
                    *trigger,
                    phases.clone(),
                    *on_host_death,
                )),
                _ => None,
            })
            .expect("Headcrab must carry a Convert action");
        let (into, mode, kill_host, into_team, chain_cap, trigger, phases, on_host_death) =
            convert;
        assert_eq!(into, "zombie");
        assert_eq!(mode, ConvertMode::Swap);
        assert!(kill_host);
        assert_eq!(into_team.as_deref(), Some("horde"));
        assert_eq!(chain_cap, 1);
        assert_eq!(trigger, ConvertTrigger::OnAttachSuccess);
        assert_eq!(on_host_death, OnHostDeath::TriggerTerminal);
        assert_eq!(phases.len(), 2, "Headcrab has mounted + deteriorating");
        assert_eq!(phases[0].name, "mounted");
        assert!((phases[0].duration - 2.5).abs() < 1e-6);
        assert_eq!(phases[1].name, "deteriorating");
        assert!((phases[1].duration - 3.5).abs() < 1e-6);
        assert!((phases[1].speed_mult - 0.4).abs() < 1e-6);
    }

    /// Zombie's registry default is a simple direct-melee — but it
    /// must carry the "infected" tag so Headcrab's NearestAttachable
    /// targeter excludes it (prevents a Headcrab from trying to
    /// re-infect an already-converted marine).
    #[test]
    fn zombie_default_carries_infected_tag() {
        let reg = BehaviorRegistry::with_builtins();
        let z = reg.get("zombie").unwrap();
        assert_eq!(z.targeter, Targeter::NearestPlayer);
        assert_eq!(z.actions, vec![Action::Melee]);
        assert!(z.tags.has(crate::tag::Tag::new("infected")));
    }

    /// Rat + DireRat name/kind round-trip like every other archetype.
    #[test]
    fn rat_archetype_roundtrip() {
        assert_eq!(Archetype::from_name("rat"), Some(Archetype::Rat));
        assert_eq!(Archetype::from_name("dire_rat"), Some(Archetype::DireRat));
        assert_eq!(Archetype::Rat.to_name(), "rat");
        assert_eq!(Archetype::DireRat.to_name(), "dire_rat");
        assert_eq!(Archetype::from_kind(Archetype::Rat.to_kind()), Archetype::Rat);
        assert_eq!(
            Archetype::from_kind(Archetype::DireRat.to_kind()),
            Archetype::DireRat
        );
    }

    /// Rat's Consume action carries the spec-aligned growth metadata —
    /// Behavior.md §4.4: `growth_into = "dire_rat"`, threshold = 30 HP
    /// of consumed fodder. If this regresses, the Consume primitive
    /// stops driving the growth mechanic when sim dispatch lands.
    #[test]
    fn rat_consume_has_spec_growth() {
        let reg = BehaviorRegistry::with_builtins();
        let rat = reg.get("rat").unwrap();
        let consume = rat
            .actions
            .iter()
            .find_map(|a| match a {
                Action::Consume {
                    targets,
                    range,
                    hp_gain,
                    growth_into,
                    growth_threshold,
                } => Some((
                    targets.clone(),
                    *range,
                    *hp_gain,
                    growth_into.clone(),
                    *growth_threshold,
                )),
                _ => None,
            })
            .expect("Rat must declare a Consume action");
        let (targets, range, hp_gain, growth_into, growth_threshold) = consume;
        assert!(targets.iter().any(|t| t == "corpse"));
        assert!(targets.iter().any(|t| t == "enemy"));
        assert!((range - 1.2).abs() < 1e-6);
        assert!((hp_gain - 6.0).abs() < 1e-6);
        assert_eq!(growth_into.as_deref(), Some("dire_rat"));
        assert_eq!(growth_threshold, Some(30));
    }

    /// DireRat is the growth form. Must NOT declare Consume itself
    /// (would cause infinite growth loops) and must carry the "swarm"
    /// tag so it inherits the brand's hostile-to-all identity.
    #[test]
    fn dire_rat_default_is_no_growth_melee() {
        let reg = BehaviorRegistry::with_builtins();
        let dr = reg.get("dire_rat").unwrap();
        assert_eq!(dr.actions, vec![Action::Melee]);
        assert!(dr.tags.has(crate::tag::Tag::new("swarm")));
        // Targeter should be NearestHostile so it engages cross-team
        // (Behavior.md §4.4 "eats everything" flavor).
        assert_eq!(dr.targeter, Targeter::NearestHostile);
        assert!(
            !dr.actions.iter().any(|a| matches!(a, Action::Consume { .. })),
            "DireRat should NOT declare Consume — prevents infinite growth loops"
        );
    }

    // -- B4.5: InfectionState tick behavior -----------------------------

    fn make_phase(name: &str, duration: f32) -> Phase {
        Phase { name: name.to_string(), duration, ..Phase::default() }
    }

    fn infection_with_phases(phases: Vec<Phase>) -> InfectionState {
        InfectionState {
            into_archetype: Archetype::Zombie,
            mode: ConvertMode::Swap,
            kill_host: true,
            into_team: None,
            on_host_death: OnHostDeath::TriggerTerminal,
            phase_timer: phases.first().map(|p| p.duration).unwrap_or(0.0),
            phases,
            phase_idx: 0,
        }
    }

    /// Empty phases → terminal fires the same tick an infection is
    /// installed. This is the Floodling-style instant-convert path
    /// (if/when Floodling migrates to the generic dispatch); and the
    /// default for any brand that authors a Convert without phases.
    #[test]
    fn infection_with_no_phases_fires_terminal_immediately() {
        let mut infection = infection_with_phases(Vec::new());
        assert_eq!(infection.tick(0.033), InfectionTick::TerminalFire);
    }

    /// Single-phase infection: ticks through the duration, then fires
    /// terminal on the tick the timer crosses zero.
    #[test]
    fn single_phase_advances_to_terminal() {
        let mut infection = infection_with_phases(vec![make_phase("rising", 0.1)]);
        // Two 0.05s ticks of phase 0.
        assert_eq!(infection.tick(0.05), InfectionTick::Continue);
        assert_eq!(infection.tick(0.05), InfectionTick::TerminalFire);
    }

    /// Two-phase infection (headcrab-style: mounted → deteriorating →
    /// terminal). Each boundary crossing yields AdvancedPhase; only
    /// the final boundary yields TerminalFire. Current phase reports
    /// the right index throughout.
    #[test]
    fn two_phase_headcrab_advances_through_phases() {
        let mut infection = infection_with_phases(vec![
            make_phase("mounted", 2.5),
            make_phase("deteriorating", 3.5),
        ]);
        assert_eq!(infection.current_phase().unwrap().name, "mounted");

        // Three ticks of ~1.0s each — 1.0, 2.0 inside mounted; tick
        // at 3.0s crosses into deteriorating.
        assert_eq!(infection.tick(1.0), InfectionTick::Continue);
        assert_eq!(infection.tick(1.0), InfectionTick::Continue);
        assert_eq!(infection.tick(1.0), InfectionTick::AdvancedPhase);
        assert_eq!(infection.current_phase().unwrap().name, "deteriorating");

        // Four more ticks of 1.0s — first three fit inside 3.5s
        // deteriorating, fourth crosses past it to terminal.
        assert_eq!(infection.tick(1.0), InfectionTick::Continue);
        assert_eq!(infection.tick(1.0), InfectionTick::Continue);
        assert_eq!(infection.tick(1.0), InfectionTick::Continue);
        assert_eq!(infection.tick(1.0), InfectionTick::TerminalFire);
    }

    /// After TerminalFire has been returned once, further ticks keep
    /// returning TerminalFire — keeps the dispatch idempotent if a
    /// terminal queues in one frame but doesn't fire until the next.
    #[test]
    fn terminal_fire_is_sticky() {
        let mut infection = infection_with_phases(Vec::new());
        assert_eq!(infection.tick(0.033), InfectionTick::TerminalFire);
        assert_eq!(infection.tick(0.033), InfectionTick::TerminalFire);
    }

    // -- B3: on-brand metadata enrichment -------------------------------

    /// Charger is the canonical Mover::Charge user. Prior to B3 it
    /// used Direct + a "charge" tag; this test is the tripwire that
    /// catches anyone reverting the declaration to the tag form.
    #[test]
    fn charger_uses_mover_charge() {
        let reg = BehaviorRegistry::with_builtins();
        let charger = reg.get("charger").unwrap();
        assert_eq!(charger.mover, Mover::Charge);
        // Telegraph reaction is declarative-only in B3; sim still
        // drives the sprint windup via hardcoded logic keyed on
        // Archetype::Charger. We assert presence so the intent is
        // discoverable through the registry.
        assert!(
            charger
                .reactions
                .iter()
                .any(|r| r.trigger == ReactionTrigger::OnHostileNear
                    && r.effect == "charge_telegraph"),
            "Charger should declare the hostile-near telegraph reaction"
        );
    }

    /// Leaper's priority action is LeapAttach; Melee is the fallback
    /// when the host is already in contact range.
    #[test]
    fn leaper_declares_leap_attach_before_melee() {
        let reg = BehaviorRegistry::with_builtins();
        let leaper = reg.get("leaper").unwrap();
        assert!(
            matches!(leaper.actions.first(), Some(Action::LeapAttach { .. })),
            "Leaper's first action should be LeapAttach"
        );
        assert!(
            matches!(leaper.actions.get(1), Some(Action::Melee)),
            "Leaper's second action should be Melee"
        );
    }

    /// Eater declares Consume targeting corpses as its priority action.
    /// The hunger-threshold aggro flip is handled by sim state today;
    /// this test only guards the declarative intent.
    #[test]
    fn eater_declares_consume_on_corpses() {
        let reg = BehaviorRegistry::with_builtins();
        let eater = reg.get("eater").unwrap();
        assert!(matches!(eater.targeter, Targeter::NearestCorpse { .. }));
        let consume = eater.actions.iter().find_map(|a| match a {
            Action::Consume { targets, range, .. } => Some((targets, *range)),
            _ => None,
        });
        let (targets, range) = consume.expect("Eater must declare a Consume action");
        assert!(targets.iter().any(|t| t == "corpse"));
        assert!(range > 0.0);
    }

    /// Spec-example: "Splitter becomes legible as a reaction-driven
    /// spawner" — the intent should be visible at the registry level,
    /// not only in the archetype TOML `body_on_death` field.
    #[test]
    fn splitter_declares_on_death_spawn() {
        let reg = BehaviorRegistry::with_builtins();
        let splitter = reg.get("splitter").unwrap();
        assert!(
            splitter
                .reactions
                .iter()
                .any(|r| r.trigger == ReactionTrigger::OnDeath
                    && r.effect == "spawn_swarmlings")
        );
    }

    /// The Flood story's other half: Floodling converts a host into a
    /// FloodCarrier (B2), FloodCarrier's death bursts fresh Floodlings.
    #[test]
    fn flood_carrier_declares_burst_on_death() {
        let reg = BehaviorRegistry::with_builtins();
        let fc = reg.get("flood_carrier").unwrap();
        assert!(
            fc.reactions
                .iter()
                .any(|r| r.trigger == ReactionTrigger::OnDeath
                    && r.effect == "burst_floodlings")
        );
    }

    /// Orb is a floating bomb; explode-on-death is load-bearing intent.
    #[test]
    fn orb_declares_explode_on_death() {
        let reg = BehaviorRegistry::with_builtins();
        let orb = reg.get("orb").unwrap();
        assert!(
            orb.reactions
                .iter()
                .any(|r| r.trigger == ReactionTrigger::OnDeath
                    && r.effect == "explode_big")
        );
    }

    /// Sapper is the walking-bomb variant — breach action + explode
    /// on death. Both must show up in the declaration.
    #[test]
    fn sapper_declares_breach_plus_explode() {
        let reg = BehaviorRegistry::with_builtins();
        let sapper = reg.get("sapper").unwrap();
        assert!(matches!(sapper.actions.first(), Some(Action::Breach { .. })));
        assert!(
            sapper
                .reactions
                .iter()
                .any(|r| r.trigger == ReactionTrigger::OnDeath
                    && r.effect == "explode_big")
        );
    }

    // -- B2: Flood retrofit guards --------------------------------------

    /// Floodling's default BehaviorSet must carry a Convert action.
    /// The game-layer Flood dispatch gates on this — if the registry
    /// loses it, Floodlings silently stop converting victims, which
    /// would be a much worse regression than a bench-time panic.
    #[test]
    fn floodling_default_has_convert_action() {
        let reg = BehaviorRegistry::with_builtins();
        let set = reg.get("floodling").unwrap();
        let convert = set
            .actions
            .iter()
            .find_map(|a| match a {
                Action::Convert {
                    into,
                    mode,
                    into_team,
                    chain_cap,
                    on_host_death,
                    phases,
                    ..
                } => Some((into, *mode, into_team, *chain_cap, *on_host_death, phases.len())),
                _ => None,
            })
            .expect("Floodling must declare a Convert action after B2");
        let (into, mode, into_team, chain_cap, on_host_death, phase_count) = convert;
        assert_eq!(into, "flood_carrier");
        assert_eq!(mode, ConvertMode::Stack);
        assert_eq!(into_team.as_deref(), Some("flood"));
        assert_eq!(chain_cap, 0);
        assert_eq!(on_host_death, OnHostDeath::TriggerTerminal);
        assert_eq!(phase_count, 0, "B2 flood is instant-on-hit; phases arrive with B4");
    }

    /// A brand override that strips Floodling's Convert action must
    /// turn the dispatch into a no-op — the registry is now the
    /// single source of truth for "does this archetype infect?".
    /// This covers the inverse of `floodling_default_has_convert_action`.
    #[test]
    fn floodling_convert_can_be_overridden_away() {
        let mut reg = BehaviorRegistry::with_builtins();
        // TOML that replaces the action list with just Melee — no Convert.
        let src = r#"
            [[actions]]
            kind = "melee"
        "#;
        let t: BehaviorToml = toml::from_str(src).unwrap();
        reg.apply_toml("floodling", &t);
        let set = reg.get("floodling").unwrap();
        let has_convert = set
            .actions
            .iter()
            .any(|a| matches!(a, Action::Convert { .. }));
        assert!(
            !has_convert,
            "brand override must be able to remove Floodling's Convert"
        );
    }

    #[test]
    fn convert_defaults_match_spec() {
        // Spec: OnHostDeath defaults to Cancel, chain_cap defaults
        // to 3, phases defaults to empty, mode defaults to Swap.
        let src = r#"
            [[actions]]
            kind = "convert"
            into = "zombie"
        "#;
        let t: BehaviorToml = toml::from_str(src).unwrap();
        let Action::Convert { mode, chain_cap, on_host_death, ref phases, .. } = t.actions[0]
        else {
            panic!("expected Convert");
        };
        assert_eq!(mode, ConvertMode::Swap);
        assert_eq!(chain_cap, 3);
        assert_eq!(on_host_death, OnHostDeath::Cancel);
        assert!(phases.is_empty());
    }
}
