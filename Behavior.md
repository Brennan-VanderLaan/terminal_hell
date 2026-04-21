# Behavior.md — Data-Driven Enemy AI Composition

**Status:** v0.1 draft · 2026-04-21
**Companion to:** `Spec.md` (v2). Refines §4.5 (primitives-on-enemies) and §6.1.2 (interaction matrix), both deferred there.

---

## Why this exists

Enemy AI today is a hardcoded match on `Archetype` in `src/behavior.rs` — `behaviors_for(arch)` returns one `BehaviorSet { Targeter, Mover, Attacker, TagSet }` per archetype, and a new behavior flavor requires Rust edits in N places. That's fine for the six archetypes currently shipping, and catastrophic at the 100+ brand-authored variants we're targeting (Half-Life parasites, Flood, zombies, rat swarms, Zerg mutations, Blight, nanites, ...).

This doc defines the composition layer that closes that gap: a **Rust-registered palette of behavior primitives, composed per-enemy in TOML**. No DSL. No runtime scripting. Fancy behaviors stay Rust-authored; brands compose them.

**Non-goals (v1):**
- Runtime scripting (Lua, Rhai, a custom DSL). Revisit only if authoring pressure demonstrates the palette is insufficient.
- Player conversion (headcrab-onto-player). That's a Director-mode-shaped problem and wants its own pass.
- Full replacement of `body_effect.rs`. It already does half of what we need; we'll unify in a later milestone, not a rewrite.

---

## Principles

1. **Rust owns the palette; TOML picks and configures.** New behavior logic = `fn register(...)` in a primitive module. Brands reference by string ID.
2. **Compose, don't inherit.** No archetype "extends" another. An enemy = stats + team + hostiles + a composition of primitives.
3. **Reuse existing anchors.** The `team: Tag` + `hostiles: TagSet` model already works (Flood is the proof). `body_effect.rs::Reaction::Custom` already maps TOML strings to registered handlers — same pattern.
4. **Fail loud in dev, graceful in release.** Unknown primitive IDs in TOML log an error and fall back to `Inert` so one typo can't take down a spawn pool.
5. **No hidden per-frame cost.** Primitives resolve to enum variants at load time. The hot loop stays a match over typed enums, not a dynamic dispatch.

---

## Data model

### Team + hostility (unchanged)

Already sufficient for "faction terror" (question 3). Flood is hostile to `["survivor", "horde"]` and that's what makes it threatening to everything alive. We extend the vocabulary with new team tags (`parasite`, `crazy`, `undead`, `swarm`) but no new machinery.

Special team `"crazy"` convention: `hostiles = ["*"]` (hostile to every team including own-team). Used for converted units whose new identity is unhinged — see Convert primitive below.

### BehaviorDef (new, TOML)

```toml
# content/<brand>/enemies/<archetype>.toml
[behavior]
targeter = "nearest_attachable"
mover    = "leap"

[[actions]]
kind = "leap_attach"
range = 3.5
windup = 0.35
cooldown = 1.2

[[actions]]
kind = "convert"
into = "zombie"
mode = "swap"          # swap | stack
kill_host = true
into_team = "horde"    # optional; default = self.team
inherit_team = false
trigger = "on_attach_success"

[[reactions]]
kind = "on_death"
effect = "spawn_swarmlings"   # resolves via body_effect registry
```

### Composition axes

| Axis | Cardinality | Notes |
|---|---|---|
| **Targeter** | exactly 1 | Picks the subject of this tick's intent. |
| **Mover** | exactly 1 | How the enemy closes/keeps distance on the current target. |
| **Actions** | 0..N | Priority-ordered. First action whose `trigger` matches and whose `cooldown` is ready fires this tick. |
| **Reactions** | 0..N | Event-driven (`on_death`, `on_hit`, `on_attach_success`, `on_hostile_near`). Unifies with `body_effect.rs` in B3. |

The existing `BehaviorSet { targeter, mover, attacker, tags }` becomes an internal view over these axes — `attacker` is just "the first Action primitive in the list, if any".

---

## v1 Primitive palette

This is the floor, not the ceiling. Each line is one `register(...)` call in the corresponding primitive module.

### Targeters (`src/behavior/target.rs`)

- `nearest_player` — current default for hordes
- `marked_or_nearest_player` — respects Hastur's mark; falls back to nearest
- `nearest_hostile` — nearest entity whose team ∈ self.hostiles (enables faction war without assuming "player")
- `nearest_attachable` — like `nearest_hostile` but also requires the target to not already carry the `infected` tag — prevents re-infecting own conversions
- `nearest_corpse` — for consumers/raisers
- `flee_hostile` — inverts — useful for converted-away civilians or Carcosa sanity-drained prey
- `stationary` — turrets, spawners
- (existing hardcoded targeting code at `game.rs:1905–1924` moves behind this registry during B0.)

### Movers (`src/behavior/mover.rs`)

- `direct` — greedy line
- `astar` — existing pathfinding
- `sneak` — existing rocketeer LoS-avoidance
- `charge` — existing pinkie telegraph + commit
- `hover` — keeps a preferred range band
- `leap` — windup → ballistic arc → lands; pairs with `leap_attach` action
- `burrow` — disappears, re-emerges near target (for rat-swarm flavor)

### Actions (`src/behavior/action.rs`)

- `melee`, `hitscan`, `rocket`, `breach`, `beam` — existing attackers, lifted into the registry
- **`leap_attach`** — contact action; fires `on_attach_success` event on the host
- **`convert`** — living-target only; see next section
- **`consume`** — eat a corpse/substance at point-blank. Gains HP, optionally increments a growth counter that swaps the eater to a larger archetype at threshold (rat → dire_rat). "Use the body" as fuel.
- **`raise`** — corpse-target only. Spawn a new unit from a corpse. Necromancy, Flood reanimation, parasite-from-body. "Use the body" as recruit.
- **`spawn`** — emit N units of a given archetype at a periodic cadence (nest/spawner flavor). No host required.
- **`self_destruct`** — damage-area-on-death, triggered at low HP or proximity

Split of concerns for body-acting primitives: **Convert** acts on living hostiles, **Consume** and **Raise** act on corpses. Keeps each primitive's target-kind check simple and lets brands pick the flavor of "using a body" independently.

### Reactions (`src/behavior/reaction.rs`)

- `on_death` — unifies with existing `body_effect.rs` handlers (no migration required in B1; bridge in B3)
- `on_hit` — taking damage triggers something (rage, teleport, call ally)
- `on_attach_success` — leap-attach landed — fires Convert
- `on_hostile_near` — proximity trigger; enables "the room knows you're here"
- `on_kill` — the enemy killed something (growth, buff, rally)

---

## The Convert primitive (spec detail)

This is the whole point of the conversation. Full parameter surface:

Convert targets living hostiles only. Corpse handling lives in `raise` / `consume`.

```rust
pub struct Convert {
    pub into: ArchetypeId,          // archetype to become / spawn at terminal
    pub mode: ConvertMode,          // Swap | Stack
    pub kill_host: bool,            // Swap-only; ignored in Stack
    pub into_team: Option<Tag>,     // None = inherit self.team; Some("crazy") = the twist
    pub inherit_team: bool,         // alt when into_team is None: take infector's team
    pub chain_cap: u8,              // @tunable — default 3; revisit after playtesting
    pub trigger: ConvertTrigger,    // on_attach_success | on_hit | on_kill | immediate
    pub phases: Vec<Phase>,         // empty = instant; see "Phased conversion" below
    pub on_host_death: OnHostDeath, // what happens if host dies mid-infection
}

pub enum ConvertMode {
    /// Host entity is destroyed; a new entity of `into` spawns at the host's tile
    /// with fresh HP and clean state. Headcrab → zombie semantics.
    Swap,
    /// Host entity keeps its slot. Its BehaviorDef is replaced with `into`'s,
    /// stats multiply by the archetype's relative baseline, sprite/palette
    /// swap, and `team` switches per `into_team` / `inherit_team`. This is
    /// what Flood already does at `enemy.rs:428–441`; we formalize it here.
    Stack,
}
```

### Team routing (question 4 answer)

Precedence when resolving the post-conversion team:

1. `into_team` set explicitly → use that. This is how a brand spawns a "crazy" unit: `into_team = "crazy"` with the archetype's own `hostiles = ["*"]`.
2. `inherit_team = true` → convert-target takes the **infector's** team. Makes the parasite literally a spreading faction.
3. Default → the `into` archetype's own `team` in its own TOML. Most conservative.

Per-unit overrides live in each archetype's TOML — same story, just with a `team` field in `[stats]`.

### Chain cap

`chain_cap` is the enemy-side rate limiter against "everything becomes one thing". Each convert increments a hidden `convert_depth` on the result; once at cap, further conversions targeting that entity become no-ops. Default 3, marked `@tunable` — we'll re-land on a real number after the Flood retrofit and first playtest, not during spec-writing.

---

## Phased conversion (infection, gestation, latency)

Conversion is almost never instant. Headcrabs mount and writhe for a few seconds before the victim stands up as a zombie. Chestbursters gestate (hours in lore, compressed to ~1 wave here) before bursting from the host. Flood takes a beat to rise. A zombie bite has a fever phase before rise. The `convert` primitive expresses all of these via an optional `phases` list — when empty, conversion is instant; when populated, the host enters an `InfectionState` that ticks per-phase before the terminal swap/stack fires.

### Phase data model

```rust
pub struct Phase {
    pub name: &'static str,         // log/debug label; also the `infected_<name>` tag
    pub duration: f32,              // seconds; 0 is reserved for condition-gated phases (deferred)
    pub tags_add: TagSet,           // e.g. ["infected", "symptomatic"]
    pub behavior_override: Option<BehaviorId>,  // temporary host-behavior swap
    pub sprite_overlay: Option<SpriteId>,       // e.g. "headcrab_clinging"
    pub palette_tint: Option<ColorShift>,       // subtle hue shift on the host
    pub ambient_vfx: Vec<VfxId>,    // registered ambient effects: "smoke_wisp",
                                    // "acid_drip", "ember_halo", "blood_mist",
                                    // "flood_ichor_spray". Composable — a phase
                                    // can stack several.
    pub speed_mult: f32,            // default 1.0
    pub hp_drain_per_sec: f32,      // negative = regen; default 0
    pub contagious: bool,           // melee contact re-fires the parent Convert on the touched unit
}

pub enum OnHostDeath {
    /// Host killed mid-infection → infection is discarded. Default — gives
    /// the player a real lever ("kill them before they turn") and turns
    /// conversion into a hazard to respond to rather than an inevitability.
    /// Brands override per-unit for flavor-faithful exceptions.
    Cancel,
    /// Host killed mid-infection → terminal fires immediately at the corpse.
    /// Opt-in for headcrab-mounted marine, Flood thrashing host, classic
    /// raise-from-any-dead zombie lore. Authors declare this when the
    /// brand's fiction says "you can't stop it by killing the host."
    TriggerTerminal,
    /// Host killed mid-infection → skip to final phase with its full duration,
    /// continue ticking. Lets authors express "dying accelerates the burst."
    AdvancePhase,
}
```

### Phase flavors and how they map to brand intent

Each phase swaps the host's behavior wholesale via `behavior_override` — a registered BehaviorId. Common patterns the palette should ship:

| Flavor | `behavior_override` | Typical use |
|---|---|---|
| Latent | `None` | Chestburster early gestation — host acts identical to uninfected. The payoff of letting a marine walk around for 30 seconds not knowing. |
| Panicked | `flee_hostile` | Mounted-headcrab victim running from friends. |
| Convulsing | `stationary` | Late gestation; the host stops fighting and seizes. |
| Rabid | `nearest_player` + `charge` | Zombie-fever phase; the dying host becomes hyperaggressive. |
| Thrashing | `nearest_hostile` + `direct` | Flood rising phase; the host is already switching teams. |

A phase with no `behavior_override`, no `sprite_overlay`, no `palette_tint`, and empty `ambient_vfx` is a **true latent phase** — the host looks and acts identical to an uninfected unit. This is the intended shape for chestburster-style gestations (canon-faithful: nothing visible).

But most infections want *some* tell, and `palette_tint` + `ambient_vfx` exist to let the brand author it without a full sprite swap. Examples of what a latent-ish phase can carry:

- A **Blight** plague: `palette_tint = "sickly_green"` + `ambient_vfx = ["pustule_burst"]` — the unit looks visibly diseased before it turns.
- A **Pyro** infection: `ambient_vfx = ["ember_halo", "smoke_wisp"]` — smoke wisps off the host, foreshadowing.
- An **Acid** gestation: `ambient_vfx = ["acid_drip"]` — the host leaves a damaging substance trail while infected, punishing players who don't prioritize.
- A **Psionic** takeover: `palette_tint = "yellow_shift"` alone — Carcosa-adjacent. Pairs with the existing corruption palette system.

The general shape: **`palette_tint` + `ambient_vfx` cover "something is visibly wrong with this unit" without committing to a new sprite.** Brands pick the tell that matches their horror vocabulary (rot, fire, acid, taint, sigil), or opt out for pure-latent horror.

### Contagion

Setting `contagious: true` on a phase makes a melee-contact event from the host re-fire the parent `convert` primitive on the touched unit. This is how "zombie bites spread" — the infected-and-rabid phase of the human's infection passes the same conversion onto the next human. Opt-in, default off. Contagion respects `chain_cap` so epidemics can't run away.

**Generation propagation (v1):** propagated infections carry the **same phases and durations** as the parent — a 6-second headcrab conversion stays 6 seconds whether you're patient zero or the twentieth victim. Simpler to reason about; rate-limit via `chain_cap`. Tunable decay (e.g. 0.7× duration per hop, or phase-count decrement per generation) is explicitly deferred until a playtest shows it's needed.

### Stacking rules

- A host can only carry **one** active infection at a time. A second `convert` landing on an already-infected host is a no-op (doesn't reset the timer, doesn't swap the infection). Prevents two headcrabs racing to convert the same marine.
- Priority/override (stronger infection displaces weaker) is explicitly deferred — flag as future work in whichever brand first needs it.

### Defaults

- `phases: []` — instant conversion. Matches today's Flood tag-swap behavior exactly.
- `on_host_death: Cancel` — host death during infection cancels the infection. Conversion is a hazard to respond to, not a fait accompli. Per-unit override via the `on_host_death` field on each archetype's Convert primitive.
- `contagious: false` — melee contact does not spread infection unless the brand opts in.
- `palette_tint: None`, `ambient_vfx: []` — no visible tell unless the brand declares one. True-latent is authored by silence.

---

## Worked examples

### 1. Headcrab → zombie (Half-Life brand)

```toml
# content/half_life/enemies/headcrab.toml
[stats]
archetype = "headcrab"
hp = 18
speed = 14.0
team = "parasite"
hostiles = ["survivor", "horde"]    # leaps onto anything alive

[behavior]
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
on_host_death = "trigger_terminal"   # shooting the mounted marine still raises a zombie

[[actions.phases]]
name = "mounted"
duration = 2.5                       # headcrab clamped on, host thrashes
tags_add = ["infected"]
behavior_override = "panicked_host"  # marine runs from friends
sprite_overlay = "headcrab_clinging"

[[actions.phases]]
name = "deteriorating"
duration = 3.5                       # flesh breaks down, host stumbles
tags_add = ["infected", "symptomatic"]
behavior_override = "stumbling_host"
sprite_overlay = "headcrab_deteriorating"
ambient_vfx = ["blood_mist"]         # visible tell: flesh is giving up
speed_mult = 0.4
hp_drain_per_sec = 3.0
```

Total conversion time ~6s — tunable per headcrab variant. Fast headcrab gets phases halved, poison headcrab gets a third phase (`convulsing`, +4s) and the terminal `zombie_poison`.

### 2. Chestburster (Aliens brand — true latent gestation)

```toml
# content/aliens/enemies/facehugger.toml
[stats]
archetype = "facehugger"
team = "parasite"
hostiles = ["survivor", "horde"]

[behavior]
targeter = "nearest_attachable"
mover    = "leap"

[[actions]]
kind = "leap_attach"
range = 3.0
windup = 0.25
cooldown = 1.5

[[actions]]
kind = "convert"
trigger = "on_attach_success"
into = "chestburster"
mode = "swap"
kill_host = true
into_team = "crazy"                  # newborn xeno is indiscriminately hostile
# on_host_death defaults to "cancel" — kill the host early, no burster.
# To flip to canon-Alien ("the burst happens even from a dead host"),
# set `on_host_death = "trigger_terminal"` explicitly.
chain_cap = 1

# Facehugger detaches + dies after implantation — the infection is already
# riding its timer on the host. The facehugger primitive dies in its own
# on_attach_success reaction (see brand TOML, elided here).

[[actions.phases]]
name = "implanted"
duration = 25.0                      # canon-faithful true-latent: no tells at all
tags_add = ["infected"]
# behavior_override = None (host acts normal)
# no sprite_overlay, no palette_tint, no ambient_vfx — fully invisible
# The marine looks identical to an uninfected one. Horror by omission.

[[actions.phases]]
name = "convulsing"
duration = 2.0
tags_add = ["infected", "symptomatic"]
behavior_override = "convulsing"     # host doubles over, stationary
sprite_overlay = "chestburst_telegraph"
ambient_vfx = ["blood_spatter"]      # now it's finally visible — telegraph for the player
speed_mult = 0.0
```

The 25-second fully-invisible latent phase is load-bearing — it's the whole horror of the pattern. A **Blight** variant of the same primitive would shorten to `duration = 10.0` and add `palette_tint = "sickly_green"` + `ambient_vfx = ["pustule_burst"]` — same action, same mode, different tells, different fiction.

### 3. Floodling → flood_carrier (formalizes existing Flood behavior)

```toml
[behavior]
targeter = "nearest_hostile"
mover    = "direct"

[[actions]]
kind = "melee"
range = 1.8

[[actions]]
kind = "convert"
trigger = "on_hit"
into = "flood_carrier"
mode = "stack"                  # keep host slot, overlay identity
into_team = "flood"
chain_cap = 0
on_host_death = "trigger_terminal"

[[actions.phases]]
name = "rising"
duration = 1.2                   # quick but not instant
tags_add = ["infected", "flood_nascent"]
behavior_override = "thrashing"  # host seizes toward nearest ally
sprite_overlay = "flood_rising"
ambient_vfx = ["flood_ichor_spray"]  # visceral, reuses existing flood_ichor substance
```

The single short phase is what makes Flood feel visceral rather than magical — the host visibly thrashes for a beat before re-emerging as Flood. Pre-retrofit Flood today is effectively a 0-duration Stack convert; the retrofit adds the beat explicitly.

### 4. Starving rat swarm (no conversion — Consume + growth)

```toml
[stats]
team = "swarm"
hostiles = ["survivor", "horde", "parasite"]   # eats everything edible

[behavior]
targeter = "nearest_corpse"     # falls through to nearest_hostile when none
mover    = "direct"

[[actions]]
kind = "consume"
targets = ["corpse", "enemy"]
range = 1.2
hp_gain = 6
growth_into = "dire_rat"
growth_threshold = 30           # total hp eaten before swap
```

`consume` at the threshold triggers an internal `convert { mode = swap, into = "dire_rat" }`. Same primitive, same code path.

### 5. Crazed (convert produces hostile-to-all unit)

```toml
[[actions]]
kind = "convert"
trigger = "on_attach_success"
into = "crazed_husk"
mode = "swap"
into_team = "crazy"              # crazed_husk.toml declares hostiles = ["*"]
```

The `"crazy"` team is just a tag with universal hostility — it doesn't need Rust changes; it's the targeter's `*` glob that wildcards hostility resolution.

### 6. Necromancer (raise corpses)

```toml
[behavior]
targeter = "nearest_corpse"
mover    = "hover"

[[actions]]
kind = "raise"
into = "risen_skeleton"
range = 6.0
windup = 0.8
cooldown = 4.0
into_team = "undead"
consume_corpse = true            # remove the body; false would leave it re-raisable
```

`raise` is distinct from `convert`: it acts on corpse entities, not living hostiles. Same split applies to Flood's corpse-reanimation behavior (rise-as-infected) and the parasite pack's "burst-from-body" spawner flavor.

---

## Migration plan

Retrofit-first: prove the system on existing behavior before adding new content. If Stack mode can't express Flood, or Swap mode can't express the body-effect spawners we already ship, we want to find out during a retrofit — not after authoring five new packs on top.

### B0 — Extract (no behavior change)
Move the hardcoded `behaviors_for(Archetype)` match in `behavior.rs` into a string-ID registry populated at startup. Every shipping archetype resolves to its existing `BehaviorSet` via ID. Sim output byte-identical — this is the safety step. Unblocks everything downstream.

### B1 — Author surface + core primitives
Archetype TOML grows a `[behavior]` block (optional — default is the registry lookup by archetype ID). Unknown IDs warn + fall back to Inert. Ships the full v1 palette: Convert (Swap + Stack), Raise, Consume, leap_attach, seek/flee targeters, leap/burrow movers. Nothing uses these yet in content; this milestone just makes them callable.

### B2 — Flood retrofit (dogfood Stack mode)
Reimplement the existing Flood system in the new vocabulary. Floodling's tag-swap-on-hit becomes `convert { mode = "stack", into = "flood_carrier", into_team = "flood" }`. FloodCarrier's `burst_floodlings` body effect stays routed through `body_effect.rs` for now (B5 unifies). First sub-step: author a dedicated Flood-vs-horde bench scenario in `src/bench/scenarios.rs` (we only have incidental Flood spawns today in the death-paint test at scenarios.rs:724–769). Capture a baseline telemetry profile before the retrofit; success criterion is that the retrofitted Flood produces statistically-equivalent telemetry on the same seed. If it diverges, the spec is wrong and we fix the spec, not the test.

### B3 — Remaining existing brands
Rewrite every shipping archetype's behavior in the new system, taking the opportunity to tighten on-brand decision-making rather than mechanically mirroring the current match block. Eater eats corpses (Consume + growth). Splitter becomes legible as a reaction-driven spawner. Pinkie's charge windup becomes an `on_hostile_near` telegraph. Brand-by-brand; land one, playtest, next. Expected outcome: every existing enemy feels more specifically itself than it does today.

### B4 — First new content
With the retrofits proving the vocabulary, land the headcrab/zombie and starving-rat brands. These are green-field authoring tests — if authoring them needs a new primitive, that primitive joins the palette.

### B5 — Reactions unify with body_effect
Migrate `body_effect.rs` handlers behind `Reaction::on_death`. Existing named handlers (`spawn_swarmlings`, `burst_floodlings`, `raise_eater`, ...) keep working; they're just accessed through the behavior registry now. Brand TOML authors triggers + actions in one place. Deferred until B2–B4 because the retrofit should happen against the stable body_effect surface, not a moving one.

### B6 — Primitives-on-enemies (closes Spec §4.5)
Enemies grow weapon-style primitive slots (Ignite, Contagion, Acid, Cryo, Chain, ...). A `contagion`-slotted zombie hits propagate `infected` tags on living hosts, gating which Convert primitives fire. A `cryo` headcrab's leap-attach freezes the host on contact. This is the long-promised M3 moment.

**Out of scope here:** anything beyond B6 (full M3 interaction matrix in TOML, Director-side possession of behavior primitives, Carcosa/sanity-tinted perception). Each gets its own doc when they come due.

---

## Open questions (for later, not blocking B0–B2)

- **Perception radius**: currently targeters scan all enemies; should primitives declare a LoS/earshot range for "the room knows you're here" flavor?
- **Behavior authoring UX**: should the recorder / dev tools show a live BehaviorDef dump for the enemy under the cursor? (Cheap F-key overlay; defer to B2+.)
- **Interaction with Hastur's marks**: does a marked survivor become a higher-priority convert target, or does the mark take precedence? (Probably: Convert respects mark — infection is its own form of attention.)
- **Save/replay determinism**: seed the RNG used by `nearest_attachable` tie-breaking from the same tick-seed the destruction system uses.
