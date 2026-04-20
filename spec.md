# Terminal Hell — Design & Technical Specification (v2, reflects v0.7 shipped state)

> *"You opened a terminal. You shouldn't have read that log."*

A terminal-native, top-down multiplayer roguelike shooter where **every shooter
you have ever loved is leaking through the tear**, and the King in Yellow is
watching. You and 1–7 friends hold a procedural arena against escalating waves
drawn from every corner of gaming history — Doom imps, Covenant grunts, Tarkov
Scavs, Zerg swarms, Vampire Survivors flocks, Helldivers bugs, STALKER
mutants — while Carcosa bleeds into your shell one wave at a time. Loot is your
class. Primitives compose. Walls explode into glyph confetti. Dead players
ascend into **chaos**: any of them can possess any entity, spawn anything,
breach any wall, for one shared purpose — end the living so the next run can
begin. There are no revives. There is no vendor. There is no hand-holding.
Hastur is already closer than you think.

---

## 1. Elevator pitch

You and friends are queued for a game that won't load for another 25 minutes.
Someone runs `terminal_hell serve`. Everyone else runs `terminal_hell connect
<code>`. You drop into a shell session that shouldn't have loaded this log,
and the Yellow Sign is already onscreen. You hold a position. Pick up what
drops. Get weirder with each wave. Vote on which reality bleeds in next.
Watch the arena come apart in half-block gibs as a Sapper breach squad
vaporizes the wall you were hiding behind. When you die — and you will die —
you don't spectate. You *ascend*. You join the horde as a small god, and
your only job is to make sure the rest of your friends join you.

Run ends when everyone is dead. There is no win state, only how deep you
descended before Hastur got bored.

---

## 2. Design pillars

1. **Plays in the gaps.** Runnable in the time between queue pop and match
   start. Single binary, room code, nickname, go. No launcher, no updates,
   no menu deeper than two screens.
2. **Glyph-physics is the feel.** BC2 Levolution and Red Faction GeoMod,
   rendered in half-block ASCII. Walls fragment. Corpses gib. Shockwaves
   push entities and debris. Every kill generates emergent geometry.
3. **Everything composes.** A single universal effect bus — one vocabulary
   of *primitives* — powers weapons, movement verbs, enemy modifiers,
   hazards, and terrain effects. Breach + Ignite = fireball that chews
   walls. Contagion + Ricochet + Acid Blood = that story your friends will
   retell for a year.
4. **Loot is identity.** No classes. No loadout select. You spawn with a
   baseline kit and you *become* whatever you hoard, loot off corpses, and
   keep alive. The game you're playing at wave 20 is not the game you
   started.
5. **Emergence > authored drama.** The systems are load-bearing; the content
   is sauce. Authors add primitives and enemies; the system invents the
   stories.
6. **Skill-based horror, not RNG punishment.** Carcosa corruption is
   *Amnesia lineage* — deterministic effects that a skilled player can read
   and counter. Sanity changes visuals and enemy behavior in predictable
   ways. No RNG tax on "you just had a bad frame."
7. **Death is a promotion to chaos.** The moment your HP hits zero, you are
   a small god. No revive window. No downed state. Pick what you want to
   possess; pick how you want the living to die.
8. **Terminal-first, not terminal-tolerated.** Mouse aim, truecolor
   half-block rendering, spatial audio, 60fps. Feels *good* in a modern
   terminal. Degrades gracefully; refuses to run badly.
9. **Carcosa is the voice.** Every UI string, every ambient line, every
   menu copy sits under the Yellow Sign. Synthwave neon over cosmic horror
   — the King in Yellow on a CRT.
10. **Friends-first infra.** One player hosts, others connect. No accounts,
    no matchmaker, no servers to pay for. Local leaderboards, local
    unlocks.

### Non-goals (for v1)

- Public matchmaking, lobbies, or any central account service.
- Mobile, web, or non-terminal clients.
- Anti-cheat beyond "host is authoritative." This is a friends game.
- PvP competitive modes. The Director fantasy *is* the PvP, disguised.
- Shipping trademarked brand content in the core repository. (See §25.)
- Second chances. Phoenix items, self-rez, downed state — not in v1.
- A vendor, a shop, currency, or any purchaseable upgrades. If you want it,
  you loot it.

---

## 3. The central fiction

You are **inside a shell**. The machine is yours. The filesystem is your
reality. You opened a log you shouldn't have. Embedded in that log was **the
Yellow Sign** — the glyph-sigil of Hastur, the King in Yellow. You read it.
You cannot unread it.

Since then, reality has been *leaking*. The machine's process table is
crowded with executables that don't exist. Memory pages are being corrupted
with sprites from games long uninstalled. The `/var/log` directory is
screaming. A Covenant grunt skittered across your terminal three nights ago
and shot at the cursor.

Now you and your friends have barricaded into a segment of the filesystem
and you are holding it against everything Carcosa can push through the tear.
The longer you hold, the more the tear widens. The longer you hold, the
louder the King gets. The longer you hold, the more *you* start showing up
in Carcosa's memory, too.

Every shooter you have ever loved is trying to kill you. They are not the
enemies, exactly. They are being *puppeted*. And when you die, you will be
puppeted too.

Chambers' Yellow Mythos is public domain. The King is ours to use. The
shooters are homage; the horror is original.

### 3.1 Tone

- **Voice:** Synthwave + cosmic horror. Neon over rot. UI copy is deadpan
  and fatalistic with occasional liturgical flashes. Death screens do not
  apologize.
- **Palette shift:** Runs start in cool synthwave magenta/cyan. Amber
  bleeds in with corruption. Late-game is all yellow and ochre and the
  black of unlit terminal cells.
- **Violence:** Over-the-top gore delivered in glyph particles. Limbs are
  punctuation. Entrails are brackets. Blood is `~` trails that fade to
  `.`.
- **Humor:** Exists. Is dry. Is always in service of the dread. No fourth
  wall breaks. No jokes *at* the setting.
- **Music:** Synthwave bed (player-produced via eurorack + FL Studio)
  layered with Carcosa overlay — discordant strings, bent brass, a lurking
  tone that *is* the King's presence.

---

## 4. User stories

### 4.1 Host (Hana)
- As a host, I run `terminal_hell serve` and get a 6-character room code I
  can paste into Discord. Friends join in under 30 seconds.
- As a host, I can configure enabled content packs before starting. I am
  never forced into a setup screen.
- As a host, I want the run to continue if someone drops, and for them to
  rejoin seamlessly.
- As a host, I want unlocks persisted locally per nickname so my regular
  group's unlocks carry over between nights.

### 4.2 Survivor (Sam)
- As a survivor, I aim with the mouse and move with WASD; it feels like a
  real top-down shooter, not nethack.
- As a survivor, I can see my teammates' health, sanity, and current
  inventory at a glance.
- As a survivor, picking up a new weapon or primitive should feel like a
  *decision*, not a vacuum. What I'm carrying now matters; what I swap for
  matters.
- As a survivor, I can loot corpses — mine and the horde's. The Covenant
  Zealot drops a plasma blade; I can have it.
- As a survivor, I never get a "downed" screen. If I die, I'm a Director on
  the next frame.

### 4.3 Director (Dee)
- As a dead player, I immediately do something. No spectator box. Possess,
  spawn, breach, hazard — my choice, now.
- As a Director, the only hard rule is Influence budget. Otherwise I can
  *be* anything on the field and *do* anything Carcosa allows.
- As a Director, I'm not rewarded for efficiency; I'm rewarded for
  cruelty, creativity, and spectacle.

### 4.4 Late joiner (Lin)
- As a late joiner, I enter a 10-second spectator orientation.
- As a late joiner, I spawn with the baseline kit. No catch-up loot. I
  scavenge my way up.
- As a late joiner, my unlocks still apply to future picks.

### 4.5 Content author (Cam)
- As an author, I add a new themed wave by writing TOML: enemy archetypes,
  audio refs, modifier pool. No Rust required for basic content.
- As an author, I can declare new primitives in TOML too — parameterized
  from a small base set of effect-verbs provided by the engine.
- As an author, my pack's enemies auto-slot into the primitive system;
  everything already composes.

### 4.6 Audio contributor (the user)
- As audio author, I drop OGG/WAV samples into a pack folder and reference
  them from TOML by symbolic name. Hot-reload works in dev mode.
- As audio author, I can author Carcosa-overlay stems and enemy vox
  bundles independently; the engine mixes them.

---

## 5. Goals & success criteria

### 5.1 Experience goals
- **Feel:** Input-to-render latency under 16ms local, under 80ms LAN, under
  150ms internet. Hit-reg is snappy; tracers are readable.
- **Legibility:** A new player identifies friends, enemies, projectiles,
  hazards, pickups, Carcosa terrain, and the Yellow Sign within 5 seconds
  of first wave.
- **Session shape:** No cap. Most runs end in 15–40 minutes because most
  runs die. Legendary runs can push 2+ hours. All runs end on death; none
  on a clock.
- **Replay:** After 10 runs, a player should have seen ~50% of the
  possible wave compositions, because brand-bleed ordering is player-voted
  and primitive drops are cascading. The same seed should play differently
  twice.
- **Carcosa:** The first time a player sees the Yellow Sign unannounced
  onscreen, they should recoil. Every time after, they should respect it.

### 5.2 Technical goals
- **Frame budget:** 60fps render, 30Hz simulation, 20Hz network.
- **Player count:** 2–8; comfort target 4–6.
- **Footprint:** Single binary under 25MB. Content packs are optional
  extras.
- **Platforms:** Linux, macOS, Windows (Windows Terminal, kitty, WezTerm,
  iTerm2, Alacritty). Keyboard-aim fallback for dumb terminals.
- **Build:** `cargo run` starts solo practice in under 5 seconds.

---

## 6. The core verbs

This section is the heart of the game. All mechanics flow through it.

### 6.1 The universal effect bus (primitives)

A **primitive** is a named effect-component in a global vocabulary. Any
actor in the game (player weapon, enemy weapon, enemy body, projectile,
hazard, terrain tile, movement verb, corpse) can have a slotted list of
primitives. Primitives compose by well-defined interaction rules.

The vocabulary is authored (not procedurally generated) but the
*combinations* are emergent. Content packs can declare new primitives; the
engine exposes a small set of base effect-verbs (apply-damage, spawn-entity,
modify-stat, trigger-on-event, move-entity, etc.) that primitives are built
from in TOML.

#### 6.1.1 Starter primitive pool (v1 design list — ~24; 12 shipped in v0.7)

**Combat primitives:**
- ✅ `ignite` — burns target; ticks damage; ignites adjacent flammables.
- ✅ `breach` — kinetic force; damages/destroys terrain tiles; knockback.
- ✅ `ricochet` — projectile bounces off terrain; preserves damage for N
  bounces.
- ✅ `chain` — on-hit, arcs to nearest N unlinked targets.
- ✅ `contagion` — on-hit, status effects (burn, slow) propagate to
  adjacent enemies. Turns a single Ignite proc into a cascading fire.
- ✅ `acid` — enemy hit leaves a damaging green pool on the ground
  (shipped as a bloom-capable substance `acid_pool`).
- ✅ `pierce` — projectile passes through target; preserves damage.
- ✅ `cryo` — slows target by 15% per stack; 3 stacks = effectively frozen
  + shatter crit on next hit.
- ✅ `gravity-well` — hit creates brief attractor; pulls 3 nearest
  enemies ~1.5 tiles inward. Soft CC for flanker control.
- ✅ `overdrive` — next shot after kill is empowered (2× damage); consumed
  on use.

**Utility / defensive:**
- ✅ `shield-break` — +50% damage vs archetypes with `armored`/`heavy`
  tags. Anti-armor primitive.
- ✅ `siphon` — on kill, restores +8 sanity to the owning player. Rewards
  clean kills during Carcosa spirals.
- `deployable` — primitive applied to an item makes it placeable as a
  stationary trap/turret version of itself. *(planned)*
- `sustained` — extends duration of any duration-bearing effect. *(planned)*
- `marked` — on hit, target is highlighted and takes bonus damage from
  others. *(planned; marks currently applied by Hastur's gaze, not by
  player weapon hits)*
- `reflect` — brief window where damage is returned. *(planned)*
- `phoenix-seed` *(removed for v1 per design)* — (placeholder: was
  considered, cut to preserve "no second chances")
- `feedback` — taking damage charges next outgoing shot. *(planned)*

**Movement primitives (traversal slot):** shipped as a separate system —
see §6.3 traversal verbs. The primitive pool and the traversal slot are
adjacent systems, not unified in v0.7.

**Carcosa primitives (rare, granted by Hastur events only; none shipped):**
- `yellow-glyph` — projectile or hit leaves a Yellow Sign tile for 5s that
  drains sanity from enemies. *(planned)*
- `sign-bound` — while active, Carcosa terrain does not drain YOUR sanity. *(planned)*

#### 6.1.2 Interaction rules

Every primitive declares its interactions with others via a sparse
interaction matrix in TOML. Most combinations default to "both apply
independently"; specified interactions override. Examples:

- `ignite + breach` → on-hit creates a fire-shockwave that breaches N
  adjacent wall tiles and ignites them.
- `chain + contagion` → chain arcs from the original target to *allied*
  teammates' targets as well (controlled friendly spread).
- `blink + breach` → blink through walls, breaching each tile passed
  through.
- `ricochet + pierce` → projectile both passes through *and* bounces
  afterward. Exponentially stacks on re-hit.
- `grapple + chain` → grapple hook connects through targets; chain-pulls
  them all inward.
- `gravity-well + ignite` → creates a burning vortex. Anyone within
  radius gets yanked and torched.

The interaction matrix is itself data. Authors ship new primitives with
their own interaction declarations against existing primitives. Symmetric
by default; asymmetric interactions are explicit.

**v0.6 status:** the interaction logic is currently **hardcoded in
`src/game.rs`** rather than data-driven. Breach damages the 3×3 adjacent
tile ring on wall hit; Chain arcs to 2 nearest enemies within 12 tiles;
Pierce nudges projectiles past the enemy's hit radius; Ricochet reflects
off walls until `bounces_left` is exhausted. The TOML-declared interaction
matrix is deferred pending more primitives in the pool.

### 6.2 Weapons

Weapons are **base fire-mode + primitive slots**.

#### 6.2.1 Base fire-modes (v1 design list — 8; 5 shipped in v0.7)

| Fire mode      | Status | Feel                                                    |
|----------------|--------|---------------------------------------------------------|
| `pulse`        | ✅     | Single projectile per click; baseline feel. Glyph `·`   |
| `auto`         | ✅     | 16 Hz projectile stream, ~18 damage; medium cadence. Glyph `»` |
| `burst`        | ✅     | 3-round burst per click, 0.07s spacing; high click damage. Glyph `⋯` |
| `pump`         | ✅     | 5-pellet cone (~25° spread); slow cooldown; close-range. Glyph `◊` |
| `rail`         | ✅     | Single high-damage pierce (+4 pierces stacked on top of any Pierce primitives); 260 world-px/s. Glyph `═` |
| `lob`          | planned | Grenade launcher; arcs, bounces                         |
| `cone`         | planned | Continuous cone stream (flamer/plasma); no reload, heats|
| `beam`         | planned | Continuous line; locks on held target; ramps damage     |

Every projectile `pulse` fires carries the weapon's primitive list; each
fire-mode chooses how many projectiles spawn per click, at what spacing,
and with what base damage. The mode is stored on the `Weapon` runtime
struct and travels through the `WeaponLoadout` snapshot so clients render
the same mode icon in the HUD loadout strip + the Tab inventory panel.

**Signature fire-modes per archetype:** every archetype in
`content/core/archetypes.toml` declares a `signature_fire_mode` — weapons
that drop from that enemy's corpse default to that mode. Marksmen drop
rails, Rocketeers drop pulse (the explosive kind), Pinkies drop pumps,
Killa drops auto. See §9.2.

#### 6.2.2 Slots & rarity

Weapons drop with **1–4 primitive slots** based on rarity:

| Rarity     | Slots | Drop rate (base)                  |
|------------|-------|-----------------------------------|
| Common     | 1     | 55%                               |
| Uncommon   | 2     | 30%                               |
| Rare       | 3     | 12%                               |
| Elite      | 4     | 2.5%                              |
| Carcosa    | 3–4 + 1 Carcosa-only primitive | 0.5% |

Picking up a weapon = swap in one of your 2 weapon slots. Inventory
decisions happen every pickup.

#### 6.2.3 Authored signature passes

**v0.7 shipped: per-archetype signature-drop system** — not the "named
exotic weapon" layer, but the lighter archetype-biasing layer that makes
kills feel thematic. Every archetype TOML row declares:

- `signature_primitives: Vec<String>` — the primitive pool rolled into
  slots when *this* enemy drops a weapon. Sampled with replacement so a
  Rare 3-slot roll can stack the same primitive.
- `signature_fire_mode: Option<String>` — fire mode for the dropped
  weapon. Optional; absent = random.

Examples: Marksman → `[pierce, shield_break]` + `rail`; Pinkie →
`[breach, shield_break]` + `pump`; Rocketeer → `[breach, ignite]` +
`pulse`; Killa → `[overdrive, shield_break, breach]` + `auto`.

Drops flow through this pool in two places:
1. **Miniboss kills** — always drop a Rare weapon with the dying
   archetype's signature.
2. **Regular kills** — 1.5% chance (3% with Butcher perk) to drop a
   weapon using the killed archetype's signature. Rarity scales with
   wave number so late runs see more Uncommon/Rare.

**Brand signatures** extend the system to the perk + traversal layer:
each brand TOML declares `signature_traversal` + `signature_perks`. Perk
drops bias toward the brand's picks; traversal drops default to the
brand's verb. Tarkov → slide + survivor/last-stand/quick-hands; Doom →
dash + rampage/adrenaline/quick-hands; Halo Flood → blink + gloom-shroud/
last-stand/corpse-pulse; etc.

**Still planned (deferred to M12+):** named-exotic weapons — the `Nail
Gun`, `Bent Fork`, `Harpoon`, `Blood Rattle` lineage. Carry unique
base-fire-mode variants, unlock via achievement, appear as hand-placed
Carcosa rewards. Not in v0.7.

### 6.3 Movement

**v0.7 shipped:** the traversal slot is wired. 5 verbs available; each
player has exactly one equipped. Pickup replaces the equipped verb.
Activated with **F** (kept separate from LShift — LShift is reserved for
future sprint). Each verb has its own cooldown.

| Verb      | Cooldown | Effect                                                            |
|-----------|----------|-------------------------------------------------------------------|
| `dash`    | 1.4s     | Short directional burst in movement direction + 0.25s i-frames    |
| `blink`   | 2.5s     | Short teleport in aim direction; passes through walls; small sanity cost |
| `grapple` | 3.5s     | Hook pulls player rapidly toward first wall/enemy/corpse in aim line |
| `slide`   | 2.0s     | Locked-motion forward slide 0.6s; 30% damage resist through slide |
| `phase`   | 5.0s     | Next wall tile walked into is ignored for collision (2s window); sanity cost |

The "verbs-as-primitives" unification (§6.1 movement list) and the
composition matrix (e.g. `dash + ignite` = burn trail) are still planned
for a later milestone. Today the traversal verb is a self-contained
state machine on the `Player` struct; it doesn't compose with weapon
primitives yet.

Movement itself is WASD-at-fixed-speed with axis-separated wall
collision. Mouse is always aim. Traversal verb fires on F press-edge.

### 6.4 Inventory

**v0.7 shipped:**
- 2 weapon slots, `Q` to cycle, `E` to pick up / swap active ✅
- 1 traversal slot (replaced on pickup, auto-grab on collision) ✅
- 1 perk stack: up to all 10 perks — picked perks are permanent for the
  run, auto-grab on collision ✅
- Consumable inventory: turret kits (deployable via T), medkits, sanity
  doses, armor plates ✅
- **Tab inventory panel** — full overlay showing HP/armor/sanity,
  consumable counts, perk list with flavor text, loadout per slot with
  rarity + mode + primitives ✅
- **Pickup toasts** — "+ medkit +35" HUD stack on the right edge; newest
  at top, fades over 3.5s, capped at 5 entries ✅
- **Ground labels** — floating text above each pickup showing its
  contents ("Rare · pump [BR]", "perk · Rampage"); brightens when the
  player is near ✅
- **Auto-grab rule** — consumables (medkit/sanity/armor/perk/traversal)
  pick up on collision. Equipment (weapons, turret kits) still require
  E, so a rare rolled loadout isn't accidentally swapped by jogging past
  a Common weapon ✅

**Full design (planned):**

- 1 armor slot (replaced on pickup) *(planned — armor currently an
  unslotted damage buffer)*
- 3 utility slots (grenades, deployables, consumables; cycled with mouse
  wheel or 3/4/5) *(planned)*
- 1 "sidearm" slot — a fixed cheap pistol that cannot be lost, the only
  guaranteed weapon you keep. *(planned)*

No crafting. No combining. No drop-and-pick-up-again micro. Picking up is
swapping (for weapons) or auto-consuming (for everything else).

### 6.4a Perks

Run-persistent passive modifiers picked up from miniboss drops + wave-
milestone rewards (every 5 waves). 10 shipped; each perk is a single
enum variant, modifier hooks fire from the relevant sim path.

| Perk         | Glyph | Effect                                              |
|--------------|-------|-----------------------------------------------------|
| Corpse Pulse | `+`   | +4 HP on every enemy kill                            |
| Iron Will    | `I`   | +1.5 sanity regen / s                                |
| Quick Hands  | `Q`   | +25% fire rate on every weapon                       |
| Survivor     | `S`   | +50 max armor; each new wave starts with +10 armor   |
| Rampage      | `R`   | 3 kills within 4s → +30% outgoing damage for 8s      |
| Last Stand   | `L`   | HP < 25% → 40% incoming damage resistance            |
| Bloodhound   | `B`   | Standing on a blood pool regens +2 HP/s              |
| Butcher      | `U`   | 2× chance on consumable drops + 2× signature-weapon  |
| Gloom Shroud | `G`   | Carcosa terrain drains sanity 60% slower             |
| Adrenaline   | `A`   | Taking damage → 0.6s of +60% movement speed          |

Brand-signature perk pools bias which perks drop (§6.2.3). Duplicate
perks don't stack — rolls skip any perk already owned, falling back to
the full unowned set if the brand's signature pool is exhausted.

### 6.5 Controls (default, rebindable)

| Action              | Input                                  | Shipped |
|---------------------|----------------------------------------|---------|
| Move                | WASD                                   | ✅      |
| Aim                 | Mouse                                  | ✅      |
| Fire                | LMB or Space                           | ✅      |
| Zoom camera         | Scrollwheel (0.2×–3.0×)                | ✅      |
| Pickup (equipment)  | E                                      | ✅      |
| Pickup (consumables)| Run into them                          | ✅      |
| Vote at kiosk       | E                                      | ✅      |
| Cycle weapon        | Q                                      | ✅      |
| Deploy turret kit   | T                                      | ✅      |
| Traversal verb      | F                                      | ✅      |
| Inventory overlay   | Tab                                    | ✅      |
| Perf overlay        | F3                                     | ✅      |
| Spatial-grid debug  | F4                                     | ✅      |
| ESC menu            | Esc                                    | ✅      |
| Log console         | Backtick `` ` ``                       | ✅      |
| Alt-fire            | RMB or Shift+LMB                       | planned |
| Reload              | R                                      | planned |
| Swap / Cycle utility| 1–2 / 3–5                              | planned |
| Ping                | Middle-click or G                      | planned |
| Team chat           | Enter (line-buffered)                  | planned |

**Director controls:** A separate binding set for cursor-driven spawn /
possess / hazard / breach commands. See §10.3.

---

## 7. Run structure

### 7.1 Core loop

```
┌─ Connect ─────────┐
│                    │
▼                    │
Drop in ─► Hold position ─► Wave builds ─► Intermission (vote, reposition) ─┐
                                                                            │
                                    ┌───── Survive ─────────────────────────┘
                                    │
                                    ▼
                              Die → Director mode (instant)
                                    │
                                    ├──── 1+ survivor left → keep playing
                                    │
                                    └──── All survivors dead → Run ends
                                                                │
                                                                ▼
                                            Leaderboard + achievement tally
                                                                │
                                                                ▼
                                                          Lobby / requeue
```

### 7.2 Wave structure

- **Wave length:** 60–180s. Variable by team performance and Corruption
  level. Faster kill cadence shortens the wave; stalling lengthens it.
- **Intermission:** 25–35s (see §7.3).
- **Miniboss cadence:** Every 5 waves, a *named* unit from the currently
  active brand spawns. Minibosses drop guaranteed Rare+ loot and push
  Corruption by a flat amount on death.
- **Ambient pressure:** During intermissions, 10–20% of baseline spawns
  continue. Never total peace.
- **No cap:** Waves do not end the run. Only death does.
- ✅ **Clear-phase auto-advance timer (v0.7):** once spawns are done the
  wave enters a `Clearing` state with a countdown — 50s at wave 1,
  shrinking 0.5s per wave to a floor of 25s. Killing every hostile
  advances immediately (the original "clear the wave" path); the
  countdown is a fallback that force-advances so one straggler camping
  in a corner can't stall the whole run. Surfaced in the HUD as a
  "next wave in X.Xs" strip, flashing red under 10s. Synced to clients
  via the Snapshot.

### 7.3 Intermission loop

Each intermission is a **25–35s phased window**:

```
[ 5s BREATHE ] [ 12s VOTE ] [ 8s STOCK ] [ 5s WARNING ]
   ambient       walk up to    reposition,   audio ramps,
   spawns        vote terminal  examine loot, next wave
   still active  to bleed in    inventory     announces
                  next brand   management
```

- **Breathe:** Ambient spawns continue. No respite. The team that camped
  the bunker entrance is still camping it.
- **Vote:** 2–3 **brand-bleed terminals** appear as interactable glyph
  kiosks on fixed spots in the arena. Each is tagged with the brand it
  represents and a one-line flavor ("`/dev/doom` — the imps scream," or
  "`/proc/arma` — patient men with rifles"). Players walk onto a kiosk
  and press E. Majority wins; ties resolve by the **Hastur Daemon** (see
  §8.6) — it picks the one that maximizes Corruption gain.
- **Stock:** Vote is locked; no vendor opens (per design); players
  reposition and examine their inventories. The next wave's preview is
  shown on the HUD.
- **Warning:** Audio bed rolls up. Wave theme fades in. Spawn telegraph
  glyphs (a subtle noise pattern that hints at spawn points) begin.

**No revives during intermission.** Dead teammates are Directors and are
actively hostile. Corpses remain and are lootable by the living.

### 7.4 Death & the end of a run

- HP hits 0 → immediate transition to Director mode (§10).
- No downed state. No revive window. No final-stand heroics.
- Dead players keep their unlocks credit for the session; they also earn
  Director achievement progress during the rest of the run.
- Run ends when the last survivor's HP hits 0. All Directors see the
  run-end screen together. Leaderboard, achievements, Corruption peak,
  wave reached, seed displayed.

---

## 8. Carcosa — the King in Yellow system

This is the game's soul. All sections above interact with it.

### 8.1 Corruption %

The arena has a global `Corruption` value, 0–100+.

**Drivers (gains):**
- Enemy kills: small per-kill contribution (scales by brand).
- Time passing: linear baseline.
- Miniboss kill: flat +5%.
- Wave completion: flat +2%.
- Hastur notice event resolved (failed to break LoS): +3%.
- Player death: +7%.

**Drivers (losses):**
- Sanity kills — killing while at full sanity: tiny per-kill reduction.
  Rewards skill; corruption can slightly *decay* if team is flawless.
- Destroying a Yellow Sign anchor (Carcosa terrain hotspot): -10%. Rare,
  costly to find; requires high-AoE response.

Corruption never decreases past the highest-reached threshold — once
crossed, thresholds stay crossed. Carcosa doesn't un-bleed.

### 8.2 Threshold beats (skill-based, Amnesia lineage)

All effects at thresholds are **deterministic and readable**. No random
gear corruption. No random possession chaos. A player who understands
the system can pre-empt every effect.

*v0.6 snapshot: the palette tint, Carcosa terrain, marks, phantoms, and
Yellow Sign visitations are in-game. Corrupted enemy variants, false-
friendlies / snap-out-of-it, regenerating walls, Daemon possession, and
the brand-discipline-break are still ahead. Each beat below is annotated
with `✅` for shipped, `planned` for not.*

**25% — First bleed.**
- ✅ Palette begins shifting: cool magenta/cyan picks up amber (global
  framebuffer tint that ramps with Corruption).
- planned — Yellow Sign glyphs briefly flash in empty arena tiles (today,
  Signs are triggered only by the Daemon, not as ambient 2s pulses).
- ✅ Sanity drain begins (see §8.3) at a low rate; Carcosa tiles start
  spawning.
- ✅ Hastur Daemon moves from passive to active (see §8.6).

**50% — The King stirs.**
- planned — Music bed fractures (no audio shipped yet; see §13).
- ✅ **Hastur begins cycling marks.** One survivor at a time is *marked*.
  Marked players take +25% incoming damage and are prioritized by all
  AI. Mark rotates every 45s to a new survivor.
- planned — **Corrupted enemy variants** (25% spawn chance) — not shipped;
  the Corruption-specific primitive sub-pool doesn't exist yet.
- planned — Audio hallucinations (no audio shipped yet).

**75% — Reality thins.**
- partial — **Phantom enemies** are shipped as client-side illusions when
  the local player's sanity drops below 50, regardless of Corruption
  level. **False-friendlies** (rendering teammates as enemies) are
  *deferred*. Phantoms have a 1-frame white flash as their spawn tell.
- planned — **Snap-out-of-it** (hurting a false-friendly triggers a bell
  + grace window) — depends on false-friendlies, also deferred.
- planned — HUD corruption (ammo counters lagging, health in Carcosa
  ochre) — not shipped.
- planned — **Destroyed walls regenerating as Carcosa terrain** — not
  shipped; destroyed walls stay as rubble/floor.
- planned — **Brand discipline breaks** — not explicitly wired; brand
  mixing happens whenever multiple brands are active regardless of
  Corruption threshold.
- planned — Intermission compression — not shipped; intermission is a
  fixed ~30s regardless of Corruption.

**100% — Carcosa proper.**
- partial — Carcosa tiles keep spawning at higher density as Corruption
  rises; "safe space is gone" is more gradient than binary in v0.6.
- planned — Pallid Mask / Tatters of the King / sign-bearer archetypes
  — no Carcosa-original enemies shipped.
- planned — Hastur Daemon direct possession of enemies — not shipped.
- planned — Double-mark cadence — single mark only today.
- planned — Corruption-gains-from-kills dropping to zero at 100% — not
  wired; kills keep contributing at any Corruption.

### 8.3 Sanity (per player)

Each survivor has a **Sanity** value, 0–100, starting at 100.

**Drains:**
- Time on Carcosa terrain (active drain).
- Being marked by Hastur (active drain).
- Looking directly at the Yellow Sign (passive drain; but rarely — the
  sign is usually in peripheral).
- Hearing unresolved Carcosa audio stingers.
- Adjacency to another player with <25 sanity.

**Regens:**
- Passive +1.5/s when clean (not on Carcosa, not marked). ✅ shipped.
- planned — Skill-based kills: headshots (critical hits), kills without
  taking damage during that engagement, pistol-only kills — not
  distinguished in v0.6.
- planned — Destroying Yellow Sign anchor tiles — anchor destruction not
  shipped.
- planned — Intermission breathe phase (small regen) — not wired.
- planned — The `siphon` primitive (on-kill regen) — primitive not
  shipped.

**Mechanical effects by sanity bracket:**

| Sanity | Effect |
|--------|--------|
| 100–76 | Normal. Palette reads as intended. |
| 75–51  | Audio overlay intrudes. Sanity-drain effects in §8.2 begin applying more aggressively *to you*. |
| 50–26  | HUD minor corruption. Visual sign-glyph intrusions. Enemies prioritize you slightly more. Movement-verb cooldowns rise 15%. |
| 25–1   | Phantom / false-friendly hallucinations specifically targeted at you (even below 75% Corruption). Enemies *actively path toward you* preferentially (Amnesia-style aggressive targeting). Sanity trickle continues. |
| 0      | **Compromised.** You are rendered to teammates with a Yellow Sign halo. Enemies mob you. You can still fight. You cannot regen sanity except through skill-kills. Getting back above 0 is possible but demands carry from teammates. |

Sanity is never a death sentence by itself. It *is* a force multiplier
against you. Skill and team play pull you back.

### 8.4 Hastur's gaze — the marks system

Once Corruption 50%+ (or any time at 100%+ where doubles apply):

- A **mark** is placed over one survivor's glyph. Entire team can see
  whose turn it is.
- Marked survivor takes +25% damage, is prioritized by all AI, drains
  sanity from teammates within 3 tiles.
- Marks last ~45s, then rotate to a random non-marked survivor.
- A marked player who survives their mark uninjured grants the entire
  team a small sanity regen pulse.
- Skilled expression: the team screens the marked player, trades
  engagements to protect them, uses the mark offensively by drawing
  enemies into prepared positions.

### 8.5 Carcosa terrain

At 25%+ Corruption, specific arena tiles begin converting to **Carcosa
terrain**:

- Visually: yellow-ochre tiles with subtle Yellow Sign inlays.
- Mechanical: standing on Carcosa drains sanity (0.5/s baseline, scales
  up by Corruption %). Enemies crossing Carcosa gain a temporary
  `marked` primitive (they broadcast their position to teammates briefly
  — tactical tradeoff).
- ✅ Carcosa terrain is **not destructible by normal means**. The
  `yellow-glyph` primitive (suppress for 8s) is *planned* and depends on
  Carcosa-primitive shipping.
- planned — Anchor tile clusters + anchor destruction for -10%
  Corruption — not shipped.
- planned — At 75%, destroyed walls regenerate as Carcosa terrain —
  not shipped (destroyed walls stay as rubble/floor today).

### 8.6 The Hastur Daemon

An AI system that **always exists**. Before any player has died, it is
the sole pressure-source beyond scripted wave spawns. It is the
always-watching presence that makes the fiction feel real.

**Responsibilities:**
- Idle: observes. Does nothing but log pressure telemetry. ✅
- Active (from Corruption 25%): triggers periodic *notice events*.
  *v0.6 reality: a single notice-event variant is shipped — a Yellow
  Sign manifestation that drains sanity from all living players while
  visible, placed near a random player. The three spec'd variants
  below are planned follow-ups.*
  - *"The King notices."* — Yellow Sign glyph lingers onscreen for 4s.
    During those 4s, one random enemy in the arena gains a temporary
    primitive. Survivors can see the target lighting up. *(planned)*
  - *"The King exhales."* — All ambient spawns in the next 8s are
    teleport-spawned in the peripheral vision of survivors. *(planned)*
  - *"The King reaches."* — A random destroyed-wall tile converts to
    Carcosa terrain. *(planned)*
- On the first player death: the Daemon **hands off** to human Directors.
  It remains in a reduced role, applying ambient pressure (small
  per-wave spawns) and handling ties in brand votes.

At 100% Corruption, the Daemon is also what possesses enemies with Yellow
halos (see §8.2).

---

## 9. The horde — brands, enemies, waves

### 9.1 Brand categories

**Tier-1 (v1 ship):**

- **FPS arena lineage.** Doom / Quake / Unreal Tournament / Soldat / Duke
  Nukem / Serious Sam. Fast enemies, aggressive approach, pickup-weapon
  drops are biased higher in this brand. Homage archetypes: imps (rush +
  projectile), pinkies (armored rush), revenants (homing rocket),
  cacodemons (flying suicide), cyber-flyers, chaingun-hulk minibosses.
- **Tactical mil-sim.** Arma / Tarkov / Gray Zone Warfare / STALKER /
  Metal Gear / Rainbow Six. Slow enemies with teeth. Ranged dominance.
  Supressors. Armor. Homage archetypes: patient riflemen, scav snipers,
  zone mutants, armored PMC blocks, the Bent-Back Man (STALKER
  bloodsucker homage) as a miniboss.
- **Chaos roguelikes.** Vampire Survivors / Risk of Rain / Nuclear Throne
  / Noita / Binding of Isaac. Swarms, projectile hells, cascading
  modifiers. Homage archetypes: tiny-fast-many (reaper swarm), imp-type
  wisp swarms, flying wizards that leave bullet flowers, crawlers that
  birth smaller crawlers, self-detonating orbs, a giant rotating eye
  miniboss.

Each Tier-1 brand ships with:
- 5–7 enemy archetypes (component-composed) *(design target; v0.6
  counts: FPS arena 4, tactical 3, chaos roguelike 2 — all below
  target)*
- 1–2 minibosses *(v0.6: one shared `Miniboss` archetype across all
  brands; per-brand miniboss identities still to author)*
- A music stem + ambient audio bed *(planned; audio pillar not yet
  shipped, see §13)*
- A vox bundle (2–3s stingers) *(planned)*
- Recognizable-but-non-trademarked palette ✅

**Tier-2 (post-v1 pack plan):**
- Horde / co-op lineage (L4D / Vermintide / DRG / Helldivers / Killing
  Floor)
- Alien / sci-fi (StarCraft / Halo / Alien franchise / Metroid /
  Mass Effect)
- Horror (RE4 / Silent Hill / Dead Space / FEAR / Amnesia — paying
  lineage respect to our own sanity system)
- Campy action (Postal / Saints Row / Blood Dragon / Duke)
- Soulsborne (invasion-style enemies: the Invader archetype)
- Rimworld / Factorio (biter/raid waves)

### 9.2 Enemy archetype composition

*v0.6 reality: archetype *stats* are data-driven via
`content/core/archetypes.toml` (hp / speed / touch_damage / hit_radius /
reach / preferred_distance / ranged_damage). Archetype *identity* still
lives in the Rust `enum Archetype` — adding a new archetype currently
requires both a TOML stats row AND a code change to register the enum
variant + sprite builder. Full component-composed archetypes (AI, senses,
modifiers, loot tables) are planned.*

**Planned full schema:**

```toml
[archetypes.scav_rifleman]
base       = "patient_ranged"
vitals     = { hp = 80, armor = "kinetic_light" }
weapon     = "mosin_burst"         # base fire-mode + primitives
ai         = "tactical_sniper"     # behavior tree ID
senses     = { sight = 14, hearing = 10, requires_los = true }
modifiers  = ["sustained_aim"]
aesthetic  = { glyph = "R", palette = "tarkov_tan", vox_bundle = "scav_ru" }
primitives = []                    # empty by default; Corruption adds
corpse_loot = { chance = 0.35, table = "tactical_drops_uncommon" }
```

**Actual v0.6 schema:**

```toml
[marksman]
hp = 45
speed = 10.0
touch_damage = 6
hit_radius = 2.4
reach = 2.0
preferred_distance = 32.0
ranged_damage = 22
```

**Corpse looting** is planned (not shipped in v0.6). Today, miniboss
kills drop a Rare weapon pickup at their position, and non-miniboss
kills don't drop loot. Extending to per-archetype `corpse_loot` tables
is a follow-up.

### 9.3 Wave composition

Waves are **theme-driven but vote-driven**:

- ✅ Wave 1 is always the *opening* brand (default: tactical).
- ✅ Waves 2+ are composed from currently-bled-in brands. v0.6 uses a
  combined weighted pool from all active brands (weights declared in
  each brand's `[spawn_weights]` table); there's no order-bias yet —
  newly-voted brands contribute equally to the pool as the opener.
- planned — Corruption-driven brand discipline relaxation (§8.2 at 75%).

Miniboss waves (every 5) currently pull from the *first* active brand's
miniboss entry (which in v0.6 always resolves to the shared `Miniboss`
archetype). Per-brand miniboss identities are a content follow-up.

### 9.4 Authoring a new wave

One TOML file. Declares enemy weights, ambient audio ref, miniboss
candidate, environmental modifiers (optional: low-light, weather, etc.),
and brand-identity palette.

No Rust. That is a design contract.

*v0.6 reality: authoring a new BRAND is a pure TOML task — drop
`content/core/brands/<id>.toml` with `spawn_pool`, `miniboss`,
`spawn_weights` and it's discovered at startup. Authoring a new
ARCHETYPE still requires a Rust change (enum + sprite). The contract
holds for brand-level content; archetype-level is partial.*

---

## 10. Director mode — pure chaos

*v0.6 status: **not shipped.** No death-to-Director transition, no
Influence economy, no possession / spawn / hazard / command APIs. This
section remains the design target for M8 and is unchanged from v2 of
the spec.*


### 10.1 Entry

- HP hits 0 → zero-frame transition to Director.
- No spectator screen. No "go again" button. You are the horde now.
- Dead players share a Director pool (hive-mind). No roles. Any Director
  can do anything any other Director can do, subject to Influence.

### 10.2 Influence budget

**The only hard rule.**

- Each Director accrues Influence over time: +1/s baseline.
- Bonus Influence on events: +5 per survivor hit, +15 per downed-to-kill
  action, +30 when a survivor dies.
- Per-Director cap: 100. Spending empties the bar. No hoarding; go use it.
- Influence pool is per-Director; Directors cannot share or transfer.
  (Encourages parallel play.)

**Why no other guardrails?** Because the game is hard enough otherwise.
The Corruption curve, the no-revive rule, the shrinking arena, the marks
— all of that means survivors are already under pressure. The Directors'
job is to add flavor, not break the game. If they meta-abuse, the
Influence cap naturally throttles.

### 10.3 Capabilities

Every capability costs Influence.

| Action                 | Cost   | Notes                                        |
|------------------------|--------|----------------------------------------------|
| Spawn basic enemy      | 5–15   | Cost scales with archetype power + wave     |
| Spawn miniboss         | 60     | Limited to 1 per wave                        |
| Place hazard (small)   | 10     | Gas, caltrops, sign-terrain patch            |
| Place hazard (large)   | 30     | Collapsing-ceiling, sustained toxic field    |
| Possess unit           | 15 + drain | 0.5/s drain; uncap; you die when the unit dies |
| Possess with buff      | 30     | Possessed unit gains +20% vitals + your signature primitive |
| AI override (focus/rush)| 5 / 3s | Target area; nearby AI shifts priorities    |
| Breach (wall tile)     | 8      | Any wall tile; glyph-particle spectacle      |
| Direct-place enemy     | +5 tax | Spawn into survivor LoS instead of peripheral |

Directors can spawn anywhere. Spawning in LoS costs extra. Possession is
any unit on the field including Carcosa originals at 100%. Possessed
units gain the Director's signature primitive (derived from what that
Director's character had equipped at death — a Director who died holding
`ignite` applies `ignite` to their possession).

### 10.4 Director UI

- **Commander view:** Top-down, full-arena, fog-of-war-free.
- **Command palette:** Chord-driven like nethack extended commands
  (`:spawn imp`, `:hazard gas`, `:possess <cursor>`). Mouse for targeting.
- **Quick bar:** Last-used commands pinned; one-keystroke re-use.
- **Possession view:** Swaps to the possessed unit's local camera + HUD
  + primitive. Survivors' perspective; feels like playing *as* the
  possessed thing.

### 10.5 Co-op dynamics

- **Ping-and-commit.** Directors don't vote on plays. Anyone can act.
  Encourages emergent teamwork ("I'll breach the south wall, you possess
  the revenant coming through it") without formal coordination cost.
- **Friendly fire ON among Directors.** You can cross-wire each other's
  possessions if you want. Call it; own it. It's funny.
- **Shared score.** All Directors share run-end kill credit against
  survivors. No intra-Director competition; the "team" is the horde.

### 10.6 Joining as Director mid-run

A late joiner who arrives after all survivors are dead immediately enters
Director mode with a reduced Influence cap (50) for 1 wave, then joins
the normal cap.

---

## 11. Destruction system — the pillar

This section is a pillar. Everything interacts with it.

### 11.1 Tile-chunked physics

*v0.6 reality: tiles have `hp` + `material` + a `Carcosa` variant.
Multiple materials (wood/bone/flesh) and the richer per-tile metadata
below are design targets.*

- Every **arena tile** has *(shipped)*:
  - `material` (currently just `Concrete` and `Carcosa` — metal / wood /
    bone / flesh are planned)
  - `hp` (shipped)
  - planned — `armor_type`
  - planned — `chunk_count` (debris particle count is a global constant
    today, not per-tile)
  - planned — `collapses_adjacent` (no destruction propagation today)
  - planned — `regenerates_as_carcosa` (no regen today)

- Tiles are destroyed by accumulated damage (and by `breach` primitive
  which hits the 3×3 adjacent ring). On destruction:
  1. ✅ Spawns glyph-particles radially with drag + TTL decay + color
     fade.
  2. ✅ Leaves a rubble tile behind (passable, rendered darker).
  3. ✅ Projectiles pass through rubble (v0.7). Arena exposes
     `blocks_projectile` as a separate predicate from `blocks_los` —
     `DebrisPile` (loose rubble) blocks sight but not bullets, so
     walking-through and shooting-through are symmetric. Enemy AI
     still uses `blocks_los` for target sighting so rubble remains
     soft cover vs ranged scans.
  4. planned — Dust-cloud effect.
  5. planned — Explosive knockback to adjacent entities.
  6. planned — Destruction propagation through `collapses_adjacent`
     tiles.

### 11.2 Glyph particles & gore

- Enemy deaths spawn gibs. Gibs are glyph-particles with longer decay
  (2–4s) and friction. They accumulate. Late-game arena floors are
  *busy*.
- Blood trails (`~` and `.`) fade over 1.5s unless on Carcosa terrain
  (where they persist as Yellow-Sign inlays).
- Gore is readable. Doesn't obscure hitboxes. It's set-dressing, not
  obstruction.
- Art direction: overly, theatrically violent — glyph-comic-book. Cosmic
  horror framing contextualizes it without removing the fun.

### 11.3 Persistent arena decay

- Destruction in early waves persists. Your bunker is rubble by wave 20.
- This is the emergent-story axis. The arena *tells* the run's story.
- At 75% Corruption, destroyed walls start returning as Carcosa terrain
  (§8.5) — a fresh horror layer on top of your accumulated wreckage.

### 11.4 Primitive interactions with terrain

Documented per primitive; examples:
- ✅ `breach`: on wall hit, damages the 3×3 ring of adjacent tiles too.
  (The "×3 structural multiplier" framing is conceptually what happens
  in v0.6 — Breach's adjacent-tile damage is effectively the multiplier.)
- ✅ `ricochet`: bounces off walls; `bounces_left` consumed per hit.
- planned — `ignite`: flammable tiles ignite and tick.
- planned — `cryo`: freezes liquid gibs into floor obstacles.
- planned — `gravity-well`: pulls loose debris into a pile.
- planned — `acid`: slowly dissolves non-metal terrain.

### 11.5 Networking destruction

- ✅ Terrain state is authoritative on host.
- ✅ Tile state deltas broadcast on the reliable channel (intact /
  damaged / destroyed / rubble / carcosa). Full initial tile sync was
  removed in v0.5 after the 10× world jump made it too large for UDP —
  the Welcome message now ships the seed and clients regenerate the
  arena from it.
- ✅ Particle effects are *client-side visual only* — the client spawns
  deterministic particle runs from a seeded RNG (seed = monotonic
  counter XOR'd with tile coords on the host). Late-joiner tile-delta
  replay is planned.

---

## 12. Arena

### 12.1 Procedural generation

- ✅ Single procedural arena per run. Seeded from a shared RNG; seed
  displayed at run start. Leaderboard / seed-of-the-day sharing planned.
- ✅ Generator outputs: open central disc (spawn + vote kiosk placement),
  scattered rectangular cover blobs, and long sightline-breaker walls.
  Cover blob + breaker counts scale with arena area (~1 blob per 4K
  world pixels, ~1 breaker per 20K).
- ✅ **Size: 1600×800 world pixels (fixed, independent of terminal
  size).** This is 10× the v0.4 terminal-fit arena; the camera system
  (§17.5a) windows into it.
- planned — Explicit named chokepoint placement; today chokepoints
  emerge naturally from cover blobs.

### 12.2 Arena contract

Every arena must support:
- Clear line-of-sight gradients (some parts are open for ranged play;
  some are close-quarters).
- At least one "hold-out" position with 3+ cover sides.
- Multiple viable approach paths for attackers.
- At least 6 destructible structural walls (for Levolution-style
  spectacle).
- Brand-bleed vote kiosks distributed (not co-located — the team has to
  split or consensus-move).

### 12.3 Day/night & environmental

*(planned; no day/night variants or environmental modifiers shipped in
v0.6.)* Some arenas will be night-variants (low-light; vision-range
reduction). Environmental modifiers can be layered per brand: fog
(tactical), toxic rain (chaos roguelike), zero-G pockets (Carcosa
late-game), etc.

### 12.4 Arena themes

*(planned; v0.6 has one generic procgen — concrete walls on a black
floor, scaling to the fixed 1600×800 world. Multi-theme authoring
is deferred.)*

- `server_room` — metal + concrete; high cover density; short sight-lines
  in racks.
- `ossuary` — bone material; low cover; long sight-lines.
- `neon_slaughterhouse` — metal + flesh; synthwave palette; lots of
  blood.
- `zone_outpost` — mixed materials; mil-sim grit; a visible anomaly
  generator in the center.
- `carcosa_tower` — starts as a fortress; at 50% Corruption the
  architecture *shifts* — stairs invert, walls invert, and the arena
  becomes something you recognize less well.

---

## 13. Audio — pillar, not polish

*v0.6 status: **not shipped.** No audio engine wired; `rodio`/`cpal` are
not in the crate stack. This section remains the design target for M9.*

The audio system is load-bearing gameplay, not flavor.

### 13.1 Architecture

- Engine: `rodio` over `cpal`. Independent mixers for music / SFX / vox /
  ambient / Carcosa-overlay.
- 2D spatial: stereo pan + distance attenuation from the player camera
  listener.
- Format: OGG Vorbis (preferred); WAV for short SFX.

### 13.2 Music — dual-stem dynamic mixing

- **Synthwave base stem** (player-produced via eurorack + FL Studio).
  Plays always. Loops over 2–4 minute arcs.
- **Carcosa overlay stem** — discordant strings / bent brass / sub-drone.
  Volume mixed against `Corruption %`. At 0% overlay is inaudible; at
  100% the overlay dominates.
- **Wave-theme stems** — per-brand layer. During a wave themed by a
  brand, that brand's stem rides on top of the base.
- **Miniboss stinger** — short chromatic sting on miniboss spawn.

### 13.3 Audio-as-gameplay

- **Enemies audible before visible.** Every spawning enemy has a 0.3s
  spawn sound with spatial position. Competent players orient via audio.
- **Sound-based stealth.** Certain enemy archetypes (STALKER bloodsucker
  homage) are *silent in direct engagement but audible in peripheral*
  — you can hear them moving if you don't look.
- **Threat telegraphing.** Every non-trivial enemy ability has a unique
  pre-fire audio tell. Railguns charge with a whine. Revenant rockets
  lock with a beep. Cacodemon bile inflates with a wet burble.
- **Sanity-adaptive audio.** At low sanity, audio begins to *lie*: false
  footsteps, phantom vox lines, music-mix instability. But (skill-based)
  each audio hallucination has a tell — false footsteps lack a subtle
  surface-material variance; real ones have it.
- **The King's voice.** At each Corruption threshold, a non-looping
  spoken stinger plays: "It notices." "It stirs." "It thins." "It
  arrives." Recorded by the audio author with heavy processing.

### 13.4 Audio production pipeline

- `content/*/audio/` directories hold OGGs referenced by symbolic name.
- Dev mode: hot-reload on file change (behind a feature flag).
- Authors can ship a full `audio_bundle.toml` that declares the
  relationship between events and sound refs, so swapping in new samples
  is a one-TOML edit.
- Placeholder CC0 samples ship with core pack for M1 feel; real samples
  produced externally and committed against content packs.

### 13.5 Carcosa audio

- Ambient bed always has a faint sub-drone at low Corruption. At 25%
  the drone becomes audible. At 50% it resolves into pitch.
- Yellow Sign occurrences are accompanied by a *signature stinger* — a
  short, resonant brass-with-bending-pitch stab. Recognizable. Player
  conditioning target.
- At 100% Carcosa, the synthwave base stem is processed in real time
  (ring mod + pitch-shift down + bit-reduction) during possession events.

---

## 14. Meta progression — unlocks only

*v0.6 status: **not shipped.** No persistence, no unlock tracking, no
achievements, no leaderboards, no daily-seed. Each run is fully
disposable today. This section remains the design target for M10.*

No stat inflation. No currency. No stash. Only **new primitive types**
enter your pool over time.

### 14.1 What unlocks

- **Primitives** — new effect-components that can drop in future runs.
- **Base fire-mode variants** — new weapon archetypes (rarer).
- **Movement verbs** — technically a primitive class but flagged
  separately for selection clarity.
- **Enemy archetypes (Director spawn menu)** — new units Directors can
  spawn as.
- **Cosmetic glyph/palette sets** — your `@` can be green.
- **Achievement badges** — bragging rights only.

### 14.2 Unlock triggers

**Achievement triggers only.** Examples:

- *"Kill 50 enemies with `breach`"* → unlocks `shatter` primitive.
- *"Reach Corruption 75% without dropping below 50 sanity"* → unlocks
  `sign-bound` primitive.
- *"Survive being marked by Hastur for the full 45s without leaving
  cover"* → unlocks `reflect` primitive.
- *"As Director, cause 3 survivor deaths from a single possession"* →
  unlocks Director spawn: `Pallid Mask`.
- *"Solo kill a miniboss"* → unlocks `overdrive` primitive.
- *"Destroy 100 walls in a single run"* → unlocks destructible theme
  arena variant.

Achievements are authored, categorized, and tracked per-nickname.

### 14.3 Director-side unlocks

Director play counts. Director achievements ("cause a team-wipe via a
single breach chain," "possess 5 different archetype types in one run")
unlock Director-side primitives and spawn archetypes. Playing Director
well is a first-class unlock path.

### 14.4 Persistence

- Per-nickname unlock file:
  `~/.terminal_hell/unlocks/<nickname>@<host_pubkey_hash>.ron`.
- Keyed by nickname + host-identity so "Sam" on Hana's host and "Sam" on
  Greg's host are separate unlock contexts.
- Human-readable RON; users can back up, share with consent.
- Leaderboard: `~/.terminal_hell/leaderboard.ron`, top 100 runs per host:
  wave reached, duration, kills, team, peak Corruption, seed.

### 14.5 Seed of the day

Optional opt-in: a daily seed is generated from a date-derived hash.
Daily leaderboard per-host for that seed. Friend groups chase the daily
as a shared watercooler.

---

## 15. Join in progress

*v0.6 reality: basic JIP works — a new client can `connect` mid-run
and is added as a survivor at arena center with default HP. None of
the polish below is shipped.*

- planned — 10s spectator orientation overlay.
- planned — Baseline kit assignment at JIP (clients currently spawn
  with whatever Game::add_player gives them, same as host start).
- planned — Wave-N enemy scaling already applies (authoritatively
  shared), so joiners naturally face current-wave pressure.
- planned — Unlock-scoped drop pool at JIP (blocked on §14 persistence).
- planned — "All survivors dead → joiner becomes Director" (blocked on
  §10 Director).
- planned — AI-controlled placeholder for dropped connections; no
  rejoin window today.

---

## 16. Solo mode

`terminal_hell solo` starts offline practice:

- ✅ Host and client in same process. No networking stack started.
- ✅ Hastur Daemon is the only pressure (no human Directors possible
  because Director mode doesn't exist yet).
- ✅ At first death, the run ends immediately. No solo-specific
  balancing yet — solo uses the same spawn budget + Corruption curve
  as co-op host.
- planned — Solo runs counting toward achievement progress (blocked on
  §14 persistence).
- planned — Seed-of-the-day in solo.

---

## 17. Technical architecture

### 17.1 Overview

```
                                ┌─ stun.l.google.com (public IP probe)
                                │
┌──────────────────────────┐    │       ┌──────────────────────────┐
│  terminal_hell (client)  │ ◄──┴─UDP──►│  terminal_hell (host)    │
│                          │    renet   │                          │
│  crossterm input         │            │  sim @30Hz authoritative │
│  custom framebuffer:     │            │  netcode @20Hz snapshot  │
│    sextants + braille    │            │  content-pack loader     │
│  camera + mip-mapped     │            │  corruption + Daemon AI  │
│    sprite blit           │            │  UPnP auto-forward       │
│  ring-buffer log console │            │                          │
│  share-code auth         │◄───HTTP ──►│  install HTTP server     │
│  ESC menu + pause mirror │    TCP     │  (binary + HMAC + scripts)│
└──────────────────────────┘            └──────────────────────────┘
```

Host runs both client and server in the same process. Server is truth;
clients snap to server state each snapshot (interpolation + prediction
not yet shipped — see §17.3). Session authentication uses a 128-bit
token carried in the share code (§17.9).

### 17.2 Crate stack

**Currently in `Cargo.toml` (v0.6):**

| Concern                | Crate                              |
|------------------------|------------------------------------|
| TUI input / ANSI       | `crossterm`                        |
| ratatui (dep only, not actively used) | `ratatui`           |
| Networking             | `renet` 2.x + `renet_netcode` 2.x  |
| Serialization (wire)   | `bincode` 2 (serde feature)        |
| Serialization (derive) | `serde`                            |
| Content config         | `toml`, `include_dir`              |
| RNG (small, seeded)    | `rand` (SmallRng) + `getrandom`    |
| HTTP install server    | `tiny_http`                        |
| UPnP port forward      | `igd`                              |
| Binary signing         | `hmac` + `sha2`                    |
| Share code encoding    | `base32`                           |
| Clipboard              | `arboard`                          |
| Logging                | `tracing` + `tracing-subscriber`   |
| Error contexts         | `anyhow`                           |
| CLI                    | `clap`                             |

**Planned (design target, not in Cargo.toml):**

| Concern      | Crate                  |
|--------------|------------------------|
| Audio        | `rodio` + `cpal` (M9)  |
| Persistence  | `ron` (M10)            |
| ECS          | `hecs` — deferred; a hand-rolled `Game` struct has held up fine so far |

**Deliberately absent:** `tokio` (renet 2.x is sync; no async runtime
needed). `rand_pcg` (we use `SmallRng` from `rand`). No Bevy — headless
Bevy ECS is overkill and pulls deps.

### 17.3 Simulation

- ✅ 30Hz fixed-step tick. Authoritative host. Clients send input
  commands; server simulates; server broadcasts snapshots.
- ✅ Host-only actions (shooting, wall damage, pickup consumption,
  vote registration) are server-arbitrated.
- ✅ Pause is host-controlled (§17.13). Snapshot carries `paused`;
  clients freeze their particle / phantom ticks when the host pauses.
- ✅ **Enemy soft separation (v0.7):** pairwise push-apart at the end
  of the enemy tick — each overlapping pair (`hit_radius_a + hit_radius_b
  + 0.75 personal-space padding`) contributes half the overlap depth
  to each enemy's displacement, scaled by `PUSH_STIFFNESS × dt`.
  Mobs visibly spread out into a spiral/ring when they cluster on a
  player rather than stacking on one tile — "wall of teeth" feel.
  Stationary archetypes (turrets, sentinels) are anchored but still
  radiate repulsion so mobile enemies flow around them. Wall-clamped
  via `arena.is_passable` so pushes never clip into structures.
- ✅ **Uniform spatial grid (v0.7):** `src/spatial.rs`. Rebuilt per
  tick; `cell_size` = 14 world-units (≈ 2× miniboss hit_radius, so
  any overlap fits inside the 3×3 cell neighborhood). O(1) neighbor
  lookup via `for_each_near(x, y, fn)`. Cuts separation work from
  O(N²) to O(N × 9) at dense populations. F4 overlays the grid in
  watch modes — green-to-red cell fill scales with log(population)
  so hot spots pop visually.
- ✅ **Team-bucketed cross-faction scan (v0.7):** enemies are bucketed
  by `team` tag once per tick; each enemy only scans buckets matching
  its own `hostiles` set. Zerg-on-zerg is free because the horde tag
  isn't in any horde enemy's hostile set. This replaced an earlier
  flat-Vec scan that was O(N²) per tick (100M team-checks at 10k
  enemies). See §17.14 for the benchmark data that caught it.
- planned — Client-side input prediction for movement. Today clients
  snap to server state each snapshot @ 20Hz, which is fine on LAN.
- planned — 100ms interpolation buffer. Today: hard snap.
- planned — Lag compensation (hitscan rewind up to 200ms by RTT).
- planned — Deterministic replay from input stream. Deterministic-given-
  seed holds for spawn RNG + particle seeding, but f32 sim math may
  diverge across platforms for true replay parity.

### 17.4 Netcode

- ✅ Transport: UDP via `renet` 2.x + `renet_netcode`.
- ✅ Reliability channels:
  - `ReliableOrdered` — Welcome, TileUpdate, PlayerJoined/Left, RunEnded,
    client-side one-shot events (Interact / CycleWeapon).
  - `Unreliable` — input (client→server), snapshots (server→client),
    Blast events (visual-only).
- ⚠️ Snapshots: **full state** @ 20Hz (not yet delta-compressed). See
  `build_snapshot` in `src/net/server.rs`.
- ✅ Input packets: every render frame (~60Hz) with the latest state;
  last-N-frame redundancy not shipped (reliable-ordered action
  messages cover the one-shot cases).
- ⚠️ Bandwidth: untested at scale. The 10× world + full snapshots means
  bandwidth scales with entity count rather than deltas.
- ✅ Join handshake: Welcome carries `{your_id, arena_w, arena_h,
  arena_seed}` — clients regenerate the arena from the seed rather
  than receiving the full tile blob.
- ✅ Session authentication: the share code's 128-bit token is placed
  in `ClientAuthentication::Unsecure { user_data: … }`. Server
  validates on `ClientConnected`, disconnects on mismatch. Strangers
  who guess the IP+port but lack the token cannot start a handshake.
- ✅ NAT traversal strategy: STUN probe via `stun.l.google.com:19302`
  + UPnP auto-port-open via `igd` + CG-NAT detection via RFC 6598
  prefix (`100.64.0.0/10`). When none of those succeed, the host is
  pointed at Tailscale as the manual fallback. Full rendezvous-server
  + TURN relay are **deliberately not shipped** (design call in §17.11).
- See §17.9 for the share-code format, §17.11 for NAT details.

### 17.5 Rendering

Hand-rolled framebuffer that bypasses ratatui's widget layer for the arena.
ratatui remains available for menus and overlays. Two logical layers per
frame compose into one emitted character per terminal cell:

- **Sextant layer** (primary). Solid-fill pixels at **2×3 per terminal
  cell** using the Unicode "Symbols for Legacy Computing" block
  (U+1FB00..U+1FB3B, plus the reused `' '` / `▌` / `▐` / `█` cells).
  6× the pixel density of the old half-block approach.
- **Braille layer** (overlay). Dot pixels at **2×4 per terminal cell**
  using U+2800 + 8-bit pattern. Only renders in cells where the sextant
  pattern is empty, so it acts as fine detail behind solid sprites —
  perfect for bullet trails, sparks, and eventually Yellow Sign dust.

Per-cell resolve: count each of the 6 sextant pixels by color; pick the
most common non-black color as FG, the next-most-common as BG, and emit
the sextant glyph whose bit pattern marks the FG-colored positions. Cells
with no sextant pixels fall through to the braille layer.

**Sprite system**: each entity renders via a procedurally generated sprite
— a small `Vec<Option<Pixel>>` grid stamped into the sextant buffer at
the entity's position. Transparent cells preserve whatever the layer
underneath drew. Archetypes define sprite builders from stats (size,
palette, silhouette): the player is a cyan 8×11 humanoid with aim-tracking
barrel; rushers, pinkies, and the miniboss each get hand-shaped
silhouettes that read from across the arena.

**Draw pipeline:**
1. Clear both layers.
2. Render terrain (walls + rubble; floor intentionally left black so the
   braille layer shows through).
3. Render entities via their sprite builders.
4. Render projectile cores (2×2 sextant cluster) + sextant smoke trail.
5. Layer braille dust trail behind projectiles.
6. Render HUD overlay via direct ANSI (post-blit text, survives the
   diff because its cells don't change between frames).
7. Resolve sextant layer per cell; fall through to braille for empty
   cells; diff against previous frame; emit only changed cells.

- Truecolor assumed; 256-color fallback via `TERM`/`COLORTERM` probing
  (not yet implemented in v0.6).
- Keyboard-aim fallback if no mouse support detected (deferred).
- Minimum terminal size: 80×30. Below that: refuse with resize prompt.
- Required font glyph coverage: U+1FB00..U+1FB3B (sextants) and
  U+2800..U+28FF (braille). Cascadia Code, JetBrains Mono Nerd, Fira
  Code Nerd, Iosevka all work. Detection is deferred — users see tofu
  if they pick a bad font; the README documents known-good options.

### 17.5a Camera + mip-mapped rendering

The sim always works in **world coordinates** (fixed 1600×800 pixel
arena, independent of terminal size). The camera is a pure render
concept that transforms world positions onto the sextant framebuffer.

**`Camera`** (`src/camera.rs`):
- `center: (f32, f32)` — world position at the middle of the viewport.
- `zoom: f32` — screen pixels per world pixel. Clamped to
  `[ZOOM_MIN=0.2, ZOOM_MAX=3.0]`. Scrollwheel steps by 1.25× per notch.
- `viewport_w/h: u16` — screen pixels (2 × terminal cols, 3 × terminal
  rows). Recomputed on terminal resize.
- `world_to_screen(world)` / `screen_to_world(screen)` — the transform
  pair. Mouse aim inverse-transforms through the camera so aim is
  always correct regardless of zoom / pan.

**Follow behaviour** — camera recentered each frame on the local
player, with optional **edge-nudge** look-ahead: when the cursor is in
the outer 30% of the viewport, the camera leans in that direction up
to ~40 world pixels (divided by zoom so the screen-space look-ahead
stays consistent).

**Mip levels** — derived from zoom:

| Mip | Zoom range   | Entity rendering           |
|-----|--------------|----------------------------|
| L0  | `zoom ≥ 1.0` | Full procedural sprite, scaled 1:1 or up |
| L1  | `0.35 – 1.0` | 3×3 colored blob at entity center |
| L2  | `< 0.35`     | Single pixel at entity center |

Walls render tile-by-tile with nearest-neighbor scaling at all zoom
levels; at L2 the whole 1600×800 arena compresses onto the viewport
and entities become motes on a map overview.

**Corruption palette tint** is applied at blit time by the framebuffer
itself via `set_tint(color, amount)` — every lit pixel lerps toward
amber with the tint amount scaling with Corruption %. Preserves sprite
shape while letting the whole arena drift toward Carcosa over the
course of a run.

### 17.6 Content packs

**v0.6 actual layout** (embedded via `include_dir!`):

```
content/
  core/
    archetypes.toml                # stats per archetype
    brands/
      fps_arena.toml
      tactical.toml
      chaos_roguelike.toml
```

**Planned extended layout:**

```
content/
  core/
    primitives.toml                # planned — today hardcoded in Rust
    fire_modes.toml                # planned
    archetypes.toml                # shipped
    brands/                        # shipped
    audio/ music/ sfx/ vox/        # planned — M9
    arenas/                        # planned — multi-theme
  carcosa_core/                    # planned — M7 polish
    daemon.toml / sign.toml / carcosa_primitives.toml / audio/
  <user_pack>/                     # planned — external packs
```

- ✅ Core pack embedded in the binary via `include_dir!` at build time.
- planned — `--pack NAME` CLI arg + external pack loading from a disk
  `content/` directory.
- planned — Pack metadata manifests (dependencies, brands, primitives).
- planned — Client pack-metadata sync at connect.

### 17.7 Persistence

*v0.6 status: **not shipped.** No files written anywhere by the game
process. Each run is fully disposable. The paths below remain the
design target.*

- Unlocks: `~/.terminal_hell/unlocks/<nick>@<host_pubkey_hash>.ron`
- Leaderboard: `~/.terminal_hell/leaderboard.ron`
- Replay (stretch): input-stream recording; deterministic sim makes it
  feasible for v1.1.

### 17.8 Build & distribution

- ✅ `cargo build --release` → single binary (`terminal_hell[.exe]`).
- ✅ Core content embedded via `include_dir!`.
- **CLI (v0.6 actual):**
  - `terminal_hell solo`
  - `terminal_hell serve [--port P]` (default port 4646, HTTP on port+1)
  - `terminal_hell connect <addr>` — accepts either `ip:port` OR a
    `TH01…` share code (§17.9).
- **Planned CLI:**
  - `--pack NAME` (multi-pack load)
  - `--list-packs`
  - `--seed-of-the-day`
- **Session handoff:** no 6-char relay-backed room codes; instead,
  full share codes carry IP + game port + HTTP port + session token in
  a single pasteable string (§17.9).

### 17.9 Share codes & session auth

A **share code** is a `TH01` prefix + 40 base32 chars encoding a
25-byte payload: `[schema:1] [ip:4] [game_port:2] [http_port:2]
[token:16]`. Base32 (RFC 4648, no padding) means no char in the code
is a shell metacharacter — codes are safe as raw args in bash / zsh /
PowerShell.

The **128-bit session token** is generated fresh per `serve` from the
OS CSPRNG (`getrandom`). It acts as:

- **Auth secret**: placed into `ClientAuthentication::Unsecure {
  user_data: … }`. On `ClientConnected`, the host reads the first 16
  bytes of the user_data and compares against the session token —
  mismatched or missing tokens trigger an immediate disconnect.
- **HMAC key** for binary signing (§17.10).

Token expires with the host process — a fresh `serve` rolls a new one.
Leaked codes become useless as soon as the host restarts.

**Trust model.** The token protects the network handshake; it does NOT
certify that the binary the friend receives matches "canonical
terminal_hell." It certifies "the binary the friend receives came
from the same host whose share code they pasted." The implicit trust
boundary is "you trust whoever sent you this code," same as any
friend-hosted game. Ed25519 release-signing with a published public
key is a planned follow-up when the project goes public.

### 17.10 HTTP install server + auto-install

A `tiny_http` listener runs on `http_port` (default 4647) alongside the
game server. Endpoints:

- `GET /install.sh` — bash installer, **generated per request** with
  the request's `Host` header as `$httpBase` and the embedded share
  code's IP rewritten to match. This means `iwr
  http://127.0.0.1:4647/install.sh` returns a script that connects to
  `127.0.0.1`, not to the STUN-discovered public IP — fixes NAT
  hairpinning on self-test.
- `GET /install.ps1` — PowerShell equivalent.
- `GET /binary[?platform=…]` — the host's own executable bytes, with
  `X-Binary-HMAC: <hex>` response header = `HMAC-SHA256(session_token,
  binary_bytes)`. Install scripts verify before executing.

**Install script flow** (both bash and PowerShell):

1. Baked constants: `CODE`, `VERSION`, `HTTP_BASE`, `TOKEN_HEX`.
2. Probe: if local `terminal_hell --version` matches the session
   version → skip download, `exec terminal_hell connect $CODE`.
3. Download `/binary` + read `X-Binary-HMAC`.
4. Verify HMAC locally (bash via `openssl dgst -mac HMAC -macopt
   "hexkey:$TOKEN_HEX"`; PowerShell via
   `System.Security.Cryptography.HMACSHA256`).
5. Install to `~/.local/bin/terminal_hell` (Unix) or
   `%USERPROFILE%\bin\terminal_hell.exe` (Windows).
6. `exec terminal_hell connect $CODE`.

**One-liners** the host pastes into Discord:

```
curl -sSfL http://<host>:4647/install.sh | sh
iwr http://<host>:4647/install.ps1 -UseBasicParsing | iex
```

**Limitations** (v0.6):
- Host serves its own-platform binary only. A Linux host + Windows
  friend → friend's script errors with "unsupported platform, install
  manually."
- HTTP only (no TLS). HMAC defeats passive MITM; active attackers on
  the network path who already hold the token are not blocked.
- No multi-architecture bundle or Ed25519-signed release channel.

### 17.11 NAT traversal

Scope of v0.6: enough traversal for "convince friends to try it" while
staying infra-free.

- **STUN self-probe** (`src/stun.rs`, hand-rolled ~130 LOC). On
  `serve`, one binding request to `stun.l.google.com:19302`; parses
  XOR-MAPPED-ADDRESS out of the response. 3s timeout. The public IP
  goes into the share code. Fallback: LAN IP (detected by opening a
  connected UDP socket aimed at `8.8.8.8:80` and reading the local
  side).
- **CG-NAT detection** via the RFC 6598 prefix `100.64.0.0/10`. If the
  STUN-reported IP is in that range, the host is warned and told to
  use Tailscale — direct connections won't work.
- **UPnP auto-port-open** (`src/upnp.rs`, using `igd`). Attempts to
  open UDP `4646` (game) + TCP `4647` (HTTP install) on the LAN
  gateway with a 2h lease. 3s discovery timeout. Partial success
  (one of two) is reported distinctly from total failure.
- **Not shipped:** rendezvous signaling server, TURN relay,
  full ICE/WebRTC. These were evaluated (design discussion earlier);
  cost and operational commitment didn't justify the ~10% of hosts
  they'd rescue. Tailscale is the documented fallback for symmetric
  NAT / CG-NAT / hole-punch-refused networks.

### 17.12 UI overlays: menu + log console

Two independent overlays, both drawn post-framebuffer-blit as direct
ANSI writes:

- **ESC menu** (`src/menu.rs`): centered panel. Items (dynamic by
  context):
  - `Resume` — closes the menu.
  - `Pause run` / `Unpause run` — host-only; synced to clients
    (§17.13).
  - `Copy connect string` — clipboard via `arboard`. Stderr-dump
    fallback if clipboard unavailable.
  - `Copy install one-liner` — multi-line bash + PowerShell block.
  - `Quit`.
  ESC is modal — it traps keys (Up/Down/Enter/Esc) while open. When
  the menu is closed, normal game input resumes.
- **Log console** (`src/console.rs`): top-55% pull-down overlay
  showing the tail of a 1024-line ring buffer fed by tracing. Color-
  coded by level (red ERROR / amber WARN / cyan INFO / gray DEBUG).
  Backtick (`` ` ``) toggles. Unlike the menu, the console is
  **passive** — it doesn't trap input. Players can keep moving /
  shooting / voting while watching logs scroll.

Rendering order: arena → HUD → console → menu. Menu sits on top of
the console when both are open.

### 17.13 Pause semantics

`Game.paused` is a host-authoritative flag synced in every snapshot.

- Only `Game.is_authoritative` peers (solo + host) can flip pause via
  the menu toggle. Clients trying to toggle get a "only the host can
  pause" feedback message; the toggle item itself is hidden from the
  client menu anyway.
- When `paused`, `tick_authoritative` short-circuits (no sim advance,
  no event emission — per-tick event buffers still clear). Clients
  see `Snapshot.paused = true` and short-circuit `tick_client`, so
  particles and phantoms freeze in place.
- A centered `⏸ PAUSED` banner renders HUD-level, visible to all
  peers. Host sees "press ESC → Unpause"; clients see "HOST PAUSED"
  for clarity on who holds the toggle.

### 17.14 Benchmark mode

Shipped in v0.7. Peer to `solo` / `serve` / `connect`: a new CLI
subcommand `terminal_hell bench` that reuses the main `Game` engine
verbatim, swapping keyboard/network input for scripted inputs and the
wave director for declarative timed spawn batches. Purpose: quantify
where frames go, catch regressions on CI, and isolate rendering cost
from sim cost.

```
terminal_hell bench [--scenario NAME] [--headless] [--debug-grid] [--output FILE]
```

#### 17.14.1 CLI flags

- `--scenario NAME` — run one scenario from the catalogue. Omit to run
  the full batch.
- `--headless` — skip rendering entirely. No `TerminalGuard`, no
  framebuffer compose, no stdout writes. Sim + particle tick still run
  at wall-clock 30Hz so telemetry matches live gameplay. CI-friendly;
  runs without a TTY.
- `--watch` (default) — full render pipeline at 60 fps so you can
  literally watch 10k zerglings stampede a turret ring. Delta between
  watch + headless reports attributes cost to rendering.
- `--debug-grid` — start the spatial-grid overlay on. F4 also toggles
  it mid-scenario.
- `--output results.json` — writes a structured JSON batch report.
  Pretty-printed text still goes to stderr.

#### 17.14.2 Interrupt handling

Both paths respect Ctrl+C. Watch mode polls crossterm events each frame
(raw-mode swallows the native signal, so Esc + Ctrl+C are read from the
event stream). Headless installs a `ctrlc` OS-level handler. Both flip
a shared `AtomicBool`; the runner checks it every frame, breaks
cleanly, emits a **partial report** for the scenario in flight, and
skips the remainder of the batch.

#### 17.14.3 Scenario authoring

Rust-native for v0.7 — scenarios live in `src/bench/scenarios.rs` as
plain structs, one function per scenario. TOML authoring is planned
once the schema has settled.

```rust
pub struct Scenario {
    pub name: &'static str,
    pub summary: &'static str,
    pub arena: (u16, u16),           // can differ per scenario
    pub arena_seed: u64,             // fixed for reproducibility
    pub players: Vec<ScriptedPlayer>,
    pub spawns: Vec<ScenarioSpawn>,  // time-gated dispatch
    pub duration: Duration,
    pub stop_when_clear: bool,       // early-out on full clear
}
```

Each `ScriptedPlayer` carries a `pos`, optional `turret_kits`, and a
`PlayerScript` enum driving per-tick input:

- `Stationary` — no input. Canonical idle baseline.
- `ShootNearest` — no movement; aim at nearest enemy, hold fire.
- `CircleStrafe { center, radius, rate }` — circle a point, aim at
  nearest enemy, hold fire. Closest to "real play."
- `HoldAndDeploy { deploy_secs }` — stand and shoot; consume turret
  kits on a cadence. Used by the `turret_wall_vs_zerg` scenario so
  the player builds a ring of turrets at realistic deploy pace.

Each `ScenarioSpawn` is a timed batch — at `at_secs`, place `count`
instances of `archetype` using a `SpawnLayout`:

- `Point(x, y)` — all at one spot with a small deterministic jitter
  (golden-ratio stir so the tie-break is stable).
- `Ring { center, radius }` — evenly distributed on a circle.
- `Grid { center, spacing }` — square grid pattern.
- `Edges` — biased along the outer rim, mirroring the wave director's
  "from every direction" flavor. This is what most scenarios use.

#### 17.14.4 Starter catalogue

Shipped in `src/bench/scenarios.rs`. Short durations (5–20s) so the
full batch runs inside a CI minute; longer stress tests opt in.

| Scenario                 | Purpose                                      |
|--------------------------|----------------------------------------------|
| `baseline_empty`         | Solo player, no enemies — idle sim cost       |
| `rusher_50`              | Combat baseline                              |
| `rusher_500`             | Mid-swarm stress                             |
| `rusher_2000`            | Big-swarm stress                             |
| `zerg_tide_10k`          | Ten thousand zerglings — limit case          |
| `turret_wall_vs_zerg`    | Player + 12 turret ring vs 10k zerglings     |
| `map_scale_small/medium/large` | Same 500-enemy swarm across 3 arena sizes |
| `breacher_wall_stress`   | 30 breachers A*-smashing walls               |
| `sentinel_gauntlet`      | 20 sentinels around a strafing player        |

#### 17.14.5 Report format

Each `ScenarioReport` carries: name + render mode, arena dims, frame
count, tick latency percentiles (p50/p95/p99/max), frame latency
percentiles, effective FPS, peak entity counts (enemies / particles /
projectiles / corpses), and a sorted subsystem breakdown sourced from
`FrameProfile::take_totals` (a whole-scenario aggregate alongside the
existing 1s rolling window).

Pretty-printed example:

```
─── turret_wall_vs_zerg [headless] arena 1200×800 · 20.0s / 592 frames · 29.6 fps ───
  tick   p50   3545µs  p95   4485µs  p99   4979µs  max   8686µs
  frame  p50   3562µs  p95   4487µs  p99   4981µs  max   8688µs
  peak  enemies 10012  particles 13  projectiles 21  corpses 0
  subsystems (avg µs/frame):
    tick_total               2191µs  total calls    599
    enemy_separation         1427µs  total calls    349
    enemy_loop                667µs  total calls    349
    projectile_step            69µs  total calls    349
```

JSON output for diffing across commits is hand-rolled (flat schema,
no `serde_json` dep).

#### 17.14.6 CI hook

`tests/bench_smoke.rs` runs 3 short headless scenarios and asserts
tick-time thresholds. Regression catches live in PR CI:

- `bench_smoke_baseline` — idle sim; asserts tick p95 < 2ms.
- `bench_smoke_rusher_200` — 200-enemy swarm; asserts tick p95 < 5ms.
- `bench_smoke_sentinel_ring` — 8 sentinels + circle-strafe player;
  asserts peak enemies + frame count sanity.

Thresholds are intentionally loose — catch catastrophic regressions,
not chase false failures. Tune downward as the sim stabilizes.

#### 17.14.7 Measured wins from v0.7

The bench's first useful finding was a hidden O(N²) in the enemy
loop's cross-faction target scan — a flat-Vec scan over every living
enemy's team tag, per enemy. The team-bucket fix (§17.3) dropped
`enemy_loop` at 10k enemies from **2175ms/frame → 0.994ms** (~2000×).
At the same time, the spatial grid cut `enemy_separation` at 10k from
**410ms → 30ms** (~13×). `turret_wall_vs_zerg` went from 0.2 fps to
~30 fps sustained.

Both fixes were invisible to normal play (sub-1k enemies never
triggered the quadratic cost enough to register) and would have
surfaced as "game feels fine until late game, then jank" without a
dedicated stress-bench.

---

## 18. Data model (sketch)

### 18.1 Entity

```
Entity {
  Id
  Position { x: f32, y: f32 }
  Velocity { vx: f32, vy: f32 }
  Collider { shape, radius }
  Vitals { hp, hp_max, armor, armor_type }
  Sanity? { value, max }               // players only
  Faction { player | ai | director_spawned | carcosa | neutral }
  Renderable { glyph, palette, layer }
  Weapon? { archetype_id, ammo, mag, cooldown, slots: [PrimitiveId; 0..4] }
  TraversalVerb? { primitive_id, cooldown }
  AI? { behavior_id, target, state, aggression_bias }
  Audio { footstep_bundle, vox_bundle }
  PrimitiveStack { Vec<PrimitiveInstance> }
  Corruption? { marked_by_hastur: bool, on_carcosa_tile: bool }
  Networked { entity_id, owner }
  Possession? { possessed_by: DirectorId, signature_primitive }
  CorpseLoot? { table_id, looted: bool }
}
```

### 18.2 Network messages

**v0.6 actual (see `src/net/proto.rs`):**

```
ClientMsg:
  Input(ClientInput { move_x, move_y, aim_x, aim_y, firing })
  Interact       // one-shot; ReliableOrdered channel
  CycleWeapon    // one-shot; ReliableOrdered channel

ServerMsg:
  Welcome { your_id, arena_w, arena_h, arena_seed }
  Snapshot(Snapshot)            // full state; see below
  TileUpdate { x, y, kind, hp } // ReliableOrdered
  Blast { x, y, color, seed, intensity }  // Unreliable
  PlayerJoined { id }
  PlayerLeft { id }
  RunEnded { wave, kills, elapsed_secs }

Snapshot:
  wave, kills, alive, elapsed_secs, corruption, marked_player_id, paused
  players: [PlayerSnap { id, x, y, hp, aim_x, aim_y, sanity }]
  enemies: [EnemySnap { x, y, hp, kind }]
  projectiles: [ProjSnap { x, y }]
  pickups: [PickupSnap { id, x, y, rarity, primitives }]
  kiosks: [KioskSnap { id, x, y, brand_id, brand_name, color, votes }]
  hitscans: [HitscanSnap { from_x, from_y, to_x, to_y, ttl }]
  yellow_signs: [SignSnap { id, x, y, ttl, ttl_max }]
  weapons: [WeaponSnap { player_id, active_slot, slot0?, slot1? }]
  active_brands: Vec<String>
  intermission_phase: u8, phase_timer: f32
```

**Planned** (unchanged from v2 design): `Handshake` w/ unlock hash,
`DirectorCommand`, `LootCorpse` (when looting lands), `Chat`, `Ping`/
`Pong`, `UnlockGranted`, delta-compressed `Snapshot`, tick/seq-numbered
inputs for replay / rollback.

---

## 19. Milestones

Each milestone ends in something runnable. Destruction is a pillar, so it
comes earlier than v1 had it.

**M0 — Hello, terminal (1 week). ✅ v0.2.** Single binary, custom
framebuffer renders a moving player on a static map. WASD moves it.
Panic-safe terminal restore.

**M1 — Local shooter with destruction (3 weeks). ✅ v0.2.** Mouse aim,
projectile bullets with trails, one fire-mode, one enemy archetype with
chase AI, wall collision. Tile-chunked destruction online from day one —
walls break, glyph particles fly, debris persists. **Rendering upgraded
from half-blocks to the sextant+braille hybrid** described in §17.5, with
procedural multi-pixel sprites per archetype. Proof of combat feel and
destruction pillar together.

**M2 — Two-player LAN (2–3 weeks). ✅ v0.2.** renet authoritative host,
20Hz snapshots on an unreliable channel, tile-update + destruction events
on a reliable channel, client-side particle synthesis from server-sent
seeds. Host + one+ clients shoot the same enemies and break the same
walls. Interpolation and lag compensation deferred — LAN feels fine at
20Hz snap-to-server.

**M2.5 — Content scaffolding (current shipped). ✅ v0.2.** Three enemy
archetypes (Rusher, Pinkie, Miniboss), wave scheduler with banner
announcements, miniboss every 5 waves, run-end summary, HP/wave/kills
HUD. Damage + death + immediate run-end (no Director mode yet).

**M3 — Primitive bus scaffold. ✅ v0.5.** 6 of ~8 target primitives
shipped (Ignite, Breach, Ricochet, Chain, Pierce, Overdrive). Weapons
are base fire-mode + slots (rarity-scaled 1–3 slots; v0.6 ships only
the `pulse` fire-mode). Primitive interactions hardcoded in
`src/game.rs` — the data-driven interaction matrix from §6.1.2 is
deferred pending a larger primitive pool.

**M4 — Content-pack plumbing + first brand. ✅ v0.5.** TOML content
loader via `include_dir!`. Archetype stats data-driven via
`content/core/archetypes.toml`. FPS arena brand in
`brands/fps_arena.toml` with 4 archetypes (Rusher, Pinkie, Charger,
Revenant). Audio stem / vox deferred to M9.

**M5 — Wave structure & intermission. ✅ v0.5.** 4-phase intermission
(Breathe 5s / Vote 12s / Stock 8s / Warning 5s). Brand-bleed vote
kiosks with player-vote accumulation. 3 brands authored (FPS arena,
Tactical, Chaos Roguelike). Brand stack with weighted sampling from
active brands. Miniboss every 5 waves.

**M6 — Procedural arena. ✅ v0.5.** Seed-driven generator. Central disc
for spawn + kiosks, rectangular cover blobs + long sightline-breakers
scaled to arena area. **World size jumped to 1600×800 fixed pixels
(10× the pre-camera arena);** the camera system (§17.5a) windows into
it. Welcome message ships the seed only — clients regenerate locally.
Day/night variants deferred.

**M7 — Carcosa. ◐ v0.6 (partial).** Corruption %, per-player sanity,
Hastur marks, Carcosa terrain spawning, Hastur Daemon with Yellow
Sign visitations, client-side phantom enemies at low sanity,
corruption-driven palette tint. **Deferred from the full threshold
spec:** corrupted-variant enemy spawns at 50%, false-friendly rendering
+ snap-out-of-it at 75%, destroyed-walls-regenerating-as-Carcosa,
anchor-tile destruction (-10%), Daemon direct possession at 100%.

**M7.5 — Session handoff + camera + menu (new milestone, ✅ v0.6).**
Not in the original spec — landed because it was the missing piece
between "technically plays" and "you can actually get a friend in."
Covers:
- Camera system with scrollwheel zoom (0.2×–3.0×) and mip levels.
- Fixed 1600×800 world independent of terminal size.
- Share codes (`TH01…`), session tokens, secure-ish netcode auth.
- STUN self-probe + CG-NAT detection + UPnP auto-port-open.
- HTTP install server + bash/PowerShell auto-install + HMAC signing.
- ESC menu (Resume / Pause / Copy / Quit) with host-synced pause.
- Backtick log console pulling from an in-memory tracing ring buffer.

**M7.6 — Progression + quality-of-life + perf scaffolding (new
milestone, ✅ v0.7).** Another unplanned chunk — shipped because the
game reached "technically complete loop" and immediately needed a
progression layer + data-driven perf work to feel like a real game
instead of a tech demo. Covers:
- **Primitives pool expanded to 12:** added `acid`, `cryo`, `contagion`,
  `gravity-well`, `siphon`, `shield-break`.
- **Fire modes: 5 shipped** (pulse/auto/burst/pump/rail) — every weapon
  drop carries a mode, rendered in the HUD loadout strip + Tab panel.
- **10 perks** with run-persistent passive effects; picked from
  miniboss drops + every-5-waves milestone drops.
- **5 traversal verbs** (dash/blink/grapple/slide/phase) with per-verb
  cooldowns + i-frames + sanity costs, bound to F.
- **Tab inventory overlay**, pickup labels, pickup toasts
  (per-player multiplayer-aware), auto-grab for consumables.
- **Wave clearing timer** — force-advance after a timeout so one
  straggler can't stall the run.
- **Rubble shoot-through** — projectiles pass broken walls the same
  way the player does.
- **Player bleed FX** — damage taken triggers a seconds-long blood
  emission scaled by hit severity.
- **Enemy soft separation** — pairwise push-apart with personal-space
  padding so the horde reads as a wave, not a stack.
- **Signature drop passes** — per-archetype signature primitives +
  fire mode; per-brand signature traversal + signature perks.
- **Self-healing install scripts** — HMAC mismatch re-fetches the
  installer from the host, matching the current session's token.
- **Benchmark mode** — full `bench` subcommand with scripted scenarios,
  telemetry capture, headless + watch modes, JSON reports, CI
  integration tests. See §17.14.
- **Uniform spatial grid** + **team-bucketed AI scans**. The bench
  caught a hidden O(N²) in cross-faction targeting (2175ms/frame at
  10k zerglings); team-bucket fix brought it to 0.994ms. Separation
  grid trimmed another 410ms → 30ms. Game runs 10k zerglings at
  ~30fps sustained now.

**M8 — Director mode (3 weeks).** *Not started.* Death-to-Director
transition, Influence economy, possession / spawn / hazard / breach /
command, Director UI, co-op Director dynamics.

**M9 — Audio pillar (2 weeks).** *Not started.* Full rodio integration,
spatial audio, dual-stem dynamic mix, hot-reload dev mode, Carcosa
audio layer, audio-tells across all enemy abilities.

**M10 — Unlocks & persistence (1–2 weeks).** *Not started.* Per-
nickname unlock files, local leaderboard, achievement triggers
(survivor + Director).

**M11 — Join-in-progress + polish (2 weeks).** *Not started — basic
JIP works, the polish pass (orientation overlay, baseline kit,
connection-drop recovery with AI placeholder, settings UI, sanity
audio/visual polish) is ahead.*

**M12 — Content push (3 weeks).** *Not started.* Hit the 5–7-archetype
target per brand, bring primitive pool to ~24, per-brand minibosses,
author 5 arena theme variants.

**M13 — Closed playtest, balance, ship.**

Rough total: **6–8 months of solo nights-and-weekends**, nearly double
the ambition of v1 because destruction + Carcosa are big. Milestones
M1–M5 are each shippable-feeling standalone for a solo-practice mode
before the spec fully lands.

---

## 20. Open design questions

- **Corpse-looting UX vs. performance.** Spec §9.2 promises 0.7s loot
  channels with per-archetype tables. Needs an inventory UX pass
  before shipping, plus a re-examination of the pickup system now that
  weapons work differently than the spec assumed.
- **Client-side prediction for movement.** Today clients snap to server
  state. Adding prediction means handling misprediction reconciliation
  — worth it for WAN latency, overkill for LAN. Defer until we have
  real internet-play telemetry.
- **How much of the Carcosa threshold depth actually matters.** Shipped
  minimum (palette / marks / terrain / Daemon / phantoms) already
  generates the "King is closing in" feeling. The full depth (corrupted
  variants, false-friendlies, snap-out-of-it) may be diminishing
  returns — worth playtesting before committing to them all.
- **Corruption-kill loss tuning.** How *much* can skill-kills decrease
  Corruption? Should it be genuine slowdown or just vibes? Playtest.
- **Director Influence precise economy.** Baselines here are guesses;
  hard numbers are spreadsheet-then-playtest.
- **Mark cadence at 100%.** 2 simultaneous marks + doubled rotation
  might be too punishing. Curve to taste.
- **Hallucination tells UX.** The tells must be *learnable within 2
  runs*. If first-time players can't ever disambiguate, the system
  feels unfair. Needs UX iteration.
- **Snap-out-of-it cost.** Is hurting a hallucinated friendly a hit
  against friendly HP, a no-op, or partial-damage? (Design default:
  partial — enough to hurt, not enough to kill, forces a real "pull
  yourself together" moment.)
- **Brand-vote kiosk placement tension.** Should kiosks be intentionally
  *spread* so voting requires team movement (good), or clustered
  (accessible)? Likely spread, but tune in playtesting.
- **Director griefing via self-sabotage.** Directors can friendly-fire
  each other. Fine by design; watch for degenerate cases.
- **Pack asset sync.** v1 requires identical packs on all clients.
  v1.1: partial auto-sync for audio assets from host.
- **Seed-of-the-day coordination.** A shared daily hash requires agreement
  on a date source. Simple approach: UTC date. Works.
- **Minimum terminal size.** 100×30 is the design target. Enforce or
  gracefully reflow for 80×24? Leaning enforce.

---

## 21. Risks

| Risk                                                | Likelihood | Mitigation                                                 |
|-----------------------------------------------------|------------|------------------------------------------------------------|
| Destruction pillar eats netcode budget              | High       | Destruction events are reliable-unordered; particles are client-side visual only with deterministic seeds. Early stress test at M2. |
| Universal effect bus becomes balance nightmare      | High       | Interaction matrix is *authored*, not procedural. Start with conservative matrix; expand as tested. |
| Carcosa feels unfair despite "skill-based" intent   | Medium     | Tells must be learnable in 2 runs. UX pass during M7.      |
| Director mode is unbalanceable                      | Medium     | Influence cap + long playtest. Hastur Daemon exists as design fallback if human Directors prove un-tunable. |
| Sanity friendly-fire causes friend-group resentment | Medium     | Snap-out-of-it bell + visual tell + partial damage; tune downward if playtest shows it's toxic. |
| Mouse input varies across terminals                 | Medium     | Detect + keyboard-aim fallback.                            |
| Content-pack schema too rigid / too loose           | Medium     | Author one brand end-to-end before finalizing schema (M4). |
| Audio production is a rabbit hole                   | Medium     | CC0 placeholders ship. Real samples are a trailing track.  |
| Scope kills the project                             | High       | M1–M5 are each standalone-playable. Half-spec ships as a solo roguelike worth playing. |
| Terminal rendering fragmentation                    | Low        | Min-size gate; generous reflow; probe at connect.          |
| Yellow Sign fatigue                                 | Low        | Sign cadence is telegraphed and rare early; ramps late.    |
| Legal: brand homage too close to brand fidelity     | Medium     | Core repo ships non-branded names + generic palettes. Third-party "brand-fidelity" packs live external to the repo per §25. |
| Active MITM on install HTTP compromises clients     | Medium     | HTTP is plaintext today; HMAC defeats passive MITM only. Mitigation target: Ed25519 release-signed binaries with pinned public key in the install script. Tracked for post-v1. |
| Host serving wrong-platform binary to a friend      | Low        | Script errors with "unsupported platform, install manually" rather than executing a broken binary. Multi-platform release bundle planned. |
| UPnP refused + symmetric NAT + no Tailscale         | Medium     | Clear error telling the host to install Tailscale or forward manually. ~10% of home networks hit this; not solved by any infra we'd ship today. |

---

## 22. Glossary

- **Arena** — the procedural 1600×800-pixel map for a single run.
- **Braille layer** — the 2×4-dot overlay rendered beneath the sextant
  layer; shows through where sextants are empty. For fine detail: bullet
  dust, sparks.
- **Brand** — a category of shooter homage (FPS arena, tactical, chaos
  roguelike, etc.).
- **Brand bleed** — the vote-based introduction of a new brand into the
  current wave's enemy pool.
- **Camera** — renderer-owned `(center, zoom, viewport)` that transforms
  world coordinates to screen pixels. Sim never reads from it.
- **Carcosa** — the fictional city from the Yellow Mythos; metaphysically
  bleeding into the shell. Also the visual state of the arena at high
  Corruption.
- **Carcosa terrain** — Yellow-Sign-inlaid tiles that drain sanity and
  empower enemies crossing them.
- **Console (log)** — backtick-toggled pull-down overlay showing the
  tail of an in-memory tracing ring buffer. Passive; doesn't trap input.
- **Corruption %** — global arena metric (0–100+) tracking the advance of
  Carcosa into the run.
- **Director** — a dead player; plays the antagonist/chaos side. *(M8,
  not yet shipped.)*
- **Gibs** — glyph-particle remains of destroyed entities.
- **Hastur** — the King in Yellow. The game's antagonist presence.
- **Hastur Daemon** — the AI system running Hastur's passive/active
  pressure before (and alongside) human Directors.
- **HMAC** — MAC signing of the served binary with the session token as
  key. Install scripts verify before executing.
- **Influence** — Director currency for spawning, possessing, hazards.
- **Install one-liner** — the `curl | sh` / `iwr | iex` command a host
  pastes to a friend. Fetches session-specific installer from the host's
  HTTP server, verifies binary HMAC, installs, runs `connect`.
- **Mark** — Hastur's gaze on a specific survivor; bonus-damage and
  AI-priority target.
- **Menu** — ESC-toggled modal overlay with Resume / Pause / Copy / Quit.
- **Mip level** — L0/L1/L2 entity-rendering tier derived from zoom. L0
  full sprite, L1 3×3 blob, L2 1-pixel dot.
- **Miniboss** — named themed elite every 5 waves.
- **Pack** — content bundle (brands + archetypes + primitives + audio +
  arenas).
- **Pause** — host-only toggle that freezes the authoritative sim and
  all clients' particle / phantom ticks. Synced via snapshot.
- **Phantom** — client-side hallucination enemy at low sanity. Pure
  visual; not in the authoritative world.
- **Primitive** — a named effect-component in the universal composition
  bus.
- **Run** — single session from drop-in to last-survivor-down.
- **Sanity** — per-player 0–100 metric; low sanity causes deterministic
  visual/audio/AI effects.
- **Session token** — 128-bit random per-`serve` secret. Carried in the
  share code; authenticates client→host handshakes; keys the binary HMAC.
- **Sextant layer** — the 2×3 solid-fill pixel grid that is the primary
  rendering surface. Each terminal cell holds 6 sub-pixels resolved into
  one Unicode sextant glyph.
- **Share code** — `TH01…` 44-char string encoding host IP + game port +
  HTTP port + session token. Shell-safe. Pasted into `terminal_hell
  connect TH01…`.
- **Sprite** — a procedurally-generated 2D `Option<Pixel>` grid stamped
  into the sextant layer to render an entity.
- **STUN** — public-IP discovery via `stun.l.google.com:19302`. One-shot
  probe on `serve`; no rendezvous / TURN infrastructure.
- **Traversal verb** — a movement primitive (dash, blink, grapple, etc.).
  *(planned; not yet shipped.)*
- **UPnP** — IGD-based auto-port-forwarding on the LAN gateway via `igd`.
  Best-effort; logs + continues on failure.
- **Wave** — 60–180s pressure period with a brand theme.
- **Yellow Sign** — Hastur's sigil; appears onscreen during Daemon
  notice events. Drains sanity while visible.

---

## 23. What this document is not

- Not a balance spreadsheet. Numbers are placeholders until playtested.
- Not an API contract. Data model is directional.
- Not a backlog. Milestones are targets.
- Not a promise. Scope will change after M1 when we learn the feel.

---

## 24. What changed from v1 (for reviewers of the prior spec)

- **Classes: cut entirely.** Replaced with loot-driven identity. Tarkov
  shape.
- **Universal effect bus** added as a core abstraction. Movement verbs
  and weapon mods are the same vocabulary.
- **Destruction elevated to a pillar.** Tile-chunked physics from M1.
- **Carcosa / Yellow Sign** is the central fiction and the antagonistic
  system — not just tone.
- **Sanity + Corruption** replace generic "difficulty scaling" as the
  antagonist curve. Skill-based, Amnesia-lineage.
- **No revives, no vendor, no second chances.** Design simplification +
  tone hardening.
- **Director is pure chaos**, shared hive-mind, only the Influence
  budget throttles. No role separation.
- **Brand bleed is player-voted** at intermissions. Replay variance
  cranked; narrative arc preserved.
- **Hastur Daemon** replaces "ambient pressure" as a designed system
  with telegraphed notice events.
- **Audio is a gameplay pillar** with authored tells, dual-stem
  dynamic mixing, Carcosa overlay.
- **Meta-progression stays unlocks-only** — but unlocks = new primitives
  = literal new combinatorial content for the effect bus. Meta is
  content, not stats.
- **Rendering: sextants (2×3) + braille (2×4), not half-blocks.**
  Upgraded during early implementation after the initial half-block build
  looked like data-viz. Procedurally-generated multi-pixel sprites per
  archetype. See §17.5.
- **Camera system with mip-mapped sprites** — added in v0.6 when the
  world scaled up 10×. Scrollwheel zoom 0.2×–3.0× with three mip tiers
  (full sprite / 3×3 blob / 1-pixel dot). Mouse aim inverse-transforms
  through the camera. See §17.5a.
- **World size is now fixed at 1600×800 pixels** independent of terminal
  size. The camera is the viewport; terminal size just changes how much
  of the world you can see at once.
- **Session handoff via share codes.** `TH01…` strings encode host IP +
  game port + HTTP port + 128-bit session token. Replace manual IP
  entry. See §17.9.
- **Secure-ish netcode auth.** Session token carried in netcode
  user_data; host rejects handshakes lacking the token. Strangers who
  discover the IP but don't have the code can't start a handshake.
- **HTTP install server + HMAC-signed binary + auto-install scripts.**
  Host serves `install.sh` / `install.ps1` / `binary` endpoints. Scripts
  are generated per request from the `Host` header (fixes NAT hairpin
  on self-test). HMAC-SHA256 signing with the session token defeats
  passive MITM binary substitution. See §17.10.
- **NAT traversal that actually works for friends.** STUN probe via
  Google → public-IP in share code. UPnP auto-port-open. CG-NAT
  detection → Tailscale fallback. No rendezvous server, no TURN relay
  — decision matrix in §17.11.
- **ESC menu + backtick log console** (§17.12). Menu has host-only
  pause toggle, copy-connect, copy-install, quit. Log console is a
  pull-down terminal showing live tracing output.
- **Host-controlled pause synced to all peers** (§17.13). Snapshot
  carries `paused`; all client-side motion freezes when the host
  toggles. Banner tells clients who holds the toggle.

### 24.1 Shipped in v0.6

**Playable today:**

- Solo, `serve`, and `connect` modes over UDP with session-token
  authentication. Host-authoritative sim at 30Hz + 20Hz full
  snapshots.
- Share codes (`TH01…` base32, shell-safe) replace manual IP entry.
  `connect` accepts either a code or raw `ip:port`.
- HTTP install server on the host auto-serves session-specific
  `install.sh` / `install.ps1` + HMAC-signed binary. Version
  shortcircuit skips download when the friend's local binary matches.
  Per-request script regeneration with Host-header-based rewriting
  makes self-test work.
- STUN public-IP probe via Google. UPnP auto-port-open via `igd`.
  CG-NAT detection with Tailscale fallback messaging.
- Sextant + braille framebuffer. Camera with scrollwheel zoom
  (0.2×–3.0×), mip-based entity rendering (L0 sprite / L1 blob /
  L2 dot), edge-nudge follow, fixed 1600×800 world.
- 6 primitives (`ignite` / `breach` / `ricochet` / `chain` / `pierce`
  / `overdrive`) composing via rarity-scaled weapon slots. Weapon
  pickups drop from miniboss kills.
- 8 enemy archetypes across 3 brands (FPS arena: Rusher, Pinkie,
  Charger, Revenant; Tactical: Marksman, PMC; Chaos Roguelike:
  Swarmling, Orb; plus the shared Miniboss). Stats data-driven via
  `content/core/archetypes.toml`. Ranged enemies use hitscan tracers
  with 0.45s tell.
- Wave scheduler with 4-phase intermission (Breathe / Vote / Stock /
  Warning). Brand-bleed vote kiosks with active-brand-stack weighted
  sampling. Three brands authored.
- Carcosa: Corruption curve drives a global palette tint. Per-player
  sanity drains on Carcosa terrain + under marks + under Sign gaze,
  regens passively. Hastur marks rotate at 50%+. Carcosa terrain
  spawns at 25%+. Hastur Daemon emits Yellow Sign visitations on a
  corruption-scaled cadence. Client-side phantom enemies spawn at
  low sanity.
- Tile-chunked destruction with per-material palette, persistent
  rubble, client-side deterministic particle spawning from host-
  emitted blast events.
- Procgen arena generator with seed sync; clients regenerate the arena
  from the seed rather than receiving the tile blob.
- ESC menu with host-only Pause (synced), Copy connect, Copy install,
  Quit. Backtick log console showing live tracing output.
- HUD: wave / HP / kills / corruption / sanity / marked indicator,
  kiosk brand labels, zoom indicator, wave banner, game-over screen.

**Shipped but below spec'd depth:**

- Primitives: 6 of ~24 planned. No movement primitives, no Carcosa
  primitives, no data-driven interaction matrix.
- Weapon fire-modes: 1 (`pulse`) of 8 planned.
- Brand archetype counts below the "5-7 per brand" target.
- Carcosa thresholds: palette / marks / terrain / Daemon / phantoms
  shipped; corrupted-variant enemies, false-friendlies, snap-out-of-
  it, anchor-tile destruction, regenerating Carcosa walls, Daemon
  direct possession all deferred.
- Inventory: 2 weapon slots only. Traversal / armor / utility /
  sidearm slots planned.
- Sanity regen: passive only (no skill-kill / headshot differentiation).
- Netcode: full snapshots, no delta compression. No client prediction
  or lag compensation.
- Install server: host-platform binary only (cross-platform release
  bundle planned).

**Not yet shipped at all:**

- Director mode (§10).
- Audio pillar (§13).
- Meta-progression / unlocks / leaderboard / seed-of-the-day (§14, §17.7).
- Movement-as-primitive traversal slot (§6.3).
- Corpse looting (§9.2).
- Day/night variants + multi-theme arenas (§12.3–12.4).
- Signed-release channel / multi-platform binary bundles (Ed25519
  public-key verification on top of HMAC).
- Persistent late-joiner tile-delta replay.

### 24.2 Shipped in v0.7 (as of this writing)

Delta on top of v0.6 — the "progression layer + QoL + perf scaffolding"
milestone (§M7.6):

**Progression:**
- Primitive pool went 6 → **12**. Added `acid`, `cryo`, `contagion`,
  `gravity-well`, `siphon`, `shield-break`. See §6.1.1.
- Fire modes went 1 → **5**: `pulse`, `auto`, `burst`, `pump`, `rail`.
  Every weapon carries a mode, rendered in the loadout strip + Tab
  panel. See §6.2.1.
- **10 perks** shipped — run-persistent passive modifiers granted from
  miniboss drops + every-5-waves milestone drops. See §6.4a.
- **5 traversal verbs** shipped — `dash`, `blink`, `grapple`, `slide`,
  `phase`, bound to F with per-verb cooldowns + i-frames. See §6.3.
- **Signature drop system**: per-archetype `signature_primitives` +
  `signature_fire_mode` so Marksmen drop rails, Pinkies drop pumps,
  etc. Per-brand `signature_traversal` + `signature_perks` bias the
  perk + verb pool. See §6.2.3 and §9.2.

**Quality-of-life:**
- **Tab inventory overlay** showing HP/armor/sanity, consumable counts,
  perk list, per-slot loadout with rarity + mode + primitives.
- **Pickup toasts** — per-player multiplayer-aware notifications with
  fade-out.
- **Ground pickup labels** floating above each drop.
- **Auto-grab for consumables** — medkits / sanity / armor / perks /
  traversals pick up on collision; weapons + turret kits still need E.
- **Wave clear-timer** force-advance after 25–50s (wave-scaled) so a
  single straggler can't stall the run. Surfaced in the HUD.
- **Rubble shoot-through** — projectiles pass broken walls the same
  way the player does. AI sight still treats rubble as soft cover.
- **Player bleed FX** — damage taken triggers seconds-long blood
  emission at the player's position, scaled by hit severity.
- **Enemy soft separation** with a personal-space padding so a 10k
  zergling stampede spreads into a visible wall-of-teeth wave rather
  than stacking on one tile.
- **Self-healing install scripts** — stale HMACs after a host restart
  now re-fetch the installer from the server instead of throwing.

**Perf / tooling:**
- **Benchmark mode (`terminal_hell bench`)** — scripted scenarios,
  watch + headless render paths, Ctrl+C/Esc/F4 handling, JSON reports,
  CI smoke tests (`tests/bench_smoke.rs`). See §17.14.
- **Uniform spatial grid** (`src/spatial.rs`) with 3×3-neighborhood
  and radius-span queries. Used by enemy separation; available for
  chain / gravity / contagion etc. F4 overlays the grid with
  population heat-coloring.
- **Team-bucketed cross-faction targeting** — fixed a hidden O(N²)
  that was dominating tick cost at scale. Enemy loop at 10k enemies
  went from **2175ms/frame → 0.994ms** (~2000× speedup). Game runs
  zerg_tide_10k at ~30 fps sustained.
- `FrameProfile::take_totals()` — whole-scenario aggregate alongside
  the existing 1s rolling window.

Sections 6–16 of this document remain the design target. Use §19
Milestones to navigate what's done vs. ahead.

---

## 25. Legal & IP note

This project is a homage and parody intended for private play. Core
repository content uses original archetype names, silhouettes, and
audio. Brand-adjacent "fidelity" theme packs (e.g., anything named
explicitly after a trademarked property) live in user-authored content
packs distributed **outside** the shipping repository, modeled on how
the Doom/Quake modding and OpenRA communities operate.

The King in Yellow, Hastur, Carcosa, and the Yellow Sign derive from
Robert W. Chambers' 1895 *The King in Yellow* and are public domain.
They are ours to use freely; no legal concern.

If this project ever goes public beyond friends, a legal review is
required before distributing any brand-adjacent pack. References in
this document are design vocabulary only.
