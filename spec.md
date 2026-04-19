# Terminal Hell — Design & Technical Specification (v2)

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

#### 6.1.1 Starter primitive pool (v1 ship list — ~24)

**Combat primitives:**
- `ignite` — burns target; ticks damage; ignites adjacent flammables.
- `breach` — kinetic force; damages/destroys terrain tiles; knockback.
- `ricochet` — projectile bounces off terrain; preserves damage for N
  bounces.
- `chain` — on-hit, arcs to nearest N unlinked targets.
- `contagion` — on applied-effect, spreads to adjacent allies/enemies.
- `acid` — leaves damaging pool on hit; pools linger.
- `pierce` — projectile passes through target; preserves damage.
- `cryo` — slows target; stacking cryo eventually freezes for shatter
  bonus.
- `gravity-well` — hit creates brief attractor; pulls nearby entities.
- `overdrive` — next shot after kill is empowered; stacks to cap.

**Movement primitives (traversal slot; also composable with weapons):**
- `dash` — short directional burst; brief i-frames.
- `blink` — short teleport; no i-frames; ignores terrain.
- `slide` — maintain speed while crouched; under low cover; kicks debris.
- `grapple` — hook to terrain or enemy; rapid pull toward target.
- `rocket-jump` — self-damaging explosive-propelled launch; rewards HP
  economy.
- `phase` — briefly pass through one tile of wall; sanity cost.
- `afterburn` — sprint leaves brief trail (ignitable by other primitives).

**Utility / defensive:**
- `deployable` — primitive applied to an item makes it placeable as a
  stationary trap/turret version of itself.
- `sustained` — extends duration of any duration-bearing effect.
- `shield-break` — ignores shield/armor layer on hit.
- `marked` — on hit, target is highlighted and takes bonus damage from
  others.
- `reflect` — brief window where damage is returned.
- `phoenix-seed` *(removed for v1 per design)* — (placeholder: was
  considered, cut to preserve "no second chances")
- `feedback` — taking damage charges next outgoing shot.
- `siphon` — on kill, regenerate sanity (not HP).

**Carcosa primitives (rare, granted by Hastur events only):**
- `yellow-glyph` — projectile or hit leaves a Yellow Sign tile for 5s that
  drains sanity from enemies.
- `sign-bound` — while active, Carcosa terrain does not drain YOUR sanity.

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

### 6.2 Weapons

Weapons are **base fire-mode + primitive slots**.

#### 6.2.1 Base fire-modes (v1 ship list — 8)

| Fire mode      | Feel                                                    |
|----------------|---------------------------------------------------------|
| `pulse`        | Hitscan, fast cadence, forgiving spread                 |
| `auto`         | Full-auto projectile, medium cadence, spread on sustained|
| `burst`        | 3-round burst, high single-click damage                 |
| `pump`         | Shotgun cone; slow reload; pellet count                 |
| `rail`         | High-damage pierce line; long cooldown; tracer          |
| `lob`          | Grenade launcher; arcs, bounces                         |
| `cone`         | Continuous cone stream (flamer/plasma); no reload, heats|
| `beam`         | Continuous line; locks on held target; ramps damage     |

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

Most weapons are parameterized archetypes — two pulse-SMGs are identical
except for their slotted primitives. BUT a handful of **authored
signature** weapons exist in the pool as named exotics: the `Nail Gun`
(sticks nails that tick; over 3 nails = rupture), the `Bent Fork` (melee,
one shot, severs armor type), the `Harpoon` (grapples + chain-pulls + deals
damage), `Blood Rattle` (beam; damages *you* while it heats up enemy). The
authored signature weapons carry a unique base fire-mode variant and do
not spawn randomly — they unlock via achievement and appear in runs as
hand-placed Carcosa rewards (e.g., always in the Hastur-mark-survived
drop).

### 6.3 Movement

Movement verbs are primitives (§6.1). Each character has **1 traversal
slot**. Default is `walk` (baseline running speed). Picking up a traversal
primitive replaces your current one (or composes, depending on the verb).

A traversal primitive can be socketed with weapon-primitive compositions
that trigger on its use. `dash + ignite` = dashing leaves a burn trail.
`blink + contagion` = blink teleports nearby allies with you on a share
reaction. This is the *insane* stacking Risk of Rain promised but never
delivered.

Mouse aim is always mouse aim; movement is WASD; the traversal verb is
bound to **LShift tap** by default (rebindable).

### 6.4 Inventory

**Fixed slots, no weight:**

- 2 weapon slots (swap with Q or 1/2; picking up overflows → swap prompt)
- 1 traversal slot (replaced on pickup)
- 1 armor slot (replaced on pickup)
- 3 utility slots (grenades, deployables, consumables; cycled with mouse
  wheel or 3/4/5)
- 1 "sidearm" slot — a fixed cheap pistol that cannot be lost, the only
  guaranteed weapon you keep. Your lifeline when you've wasted your ammo.

No crafting. No combining. No drop-and-pick-up-again micro. Picking up is
swapping.

### 6.5 Controls (default, rebindable)

| Action              | Input                                     |
|---------------------|-------------------------------------------|
| Move                | WASD                                      |
| Aim                 | Mouse                                     |
| Fire                | LMB or Space                              |
| Alt-fire            | RMB or Shift+LMB                          |
| Reload              | R                                         |
| Swap weapon         | Q / 1–2                                   |
| Cycle utility       | MouseWheel / 3–5                          |
| Use / Loot corpse   | E (channel 0.7s)                          |
| Traversal verb      | LShift tap                                |
| Ping                | Middle-click or G                         |
| Vote at intermission| Walk up to vote terminal + press E        |
| Team chat           | Enter (line-buffered)                     |
| Scoreboard          | Tab (hold)                                |
| Menu                | Esc                                       |

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

**25% — First bleed.**
- Palette begins shifting: cool magenta/cyan picks up amber.
- Yellow Sign glyphs briefly flash in empty arena tiles (always 2s;
  always the same glyph family; players learn to tune them out
  mechanically, but tuning them out drains sanity).
- Sanity drain begins (see §8.3) at a low rate.
- Hastur Daemon moves from passive to active (see §8.6).

**50% — The King stirs.**
- Music bed fractures: the synthwave track gains a discordant overlay.
  Volume mix shifts over 5s. Audio-as-telegraph.
- **Hastur begins cycling marks.** One survivor at a time is *marked*
  (Yellow Sign over their glyph; also visible to them and all
  teammates). Marked players take +25% incoming damage and are
  prioritized by all AI. Mark rotates every 45s to a new survivor. Skill
  expression: the marked player plays defensively; the team screens for
  them.
- **Corrupted enemy variants** — a small fixed percentage (25%) of
  spawns from this point on gain ONE primitive rolled from a
  Corruption-specific sub-pool. The variants are *visually distinct*
  (different glyph palette, brief flash on spawn). Not a surprise; a
  telegraph. Skill: learn to identify them, prioritize accordingly.
- Audio hallucinations begin — but only in areas the player hasn't
  looked at in >3 seconds. They are tells, not traps.

**75% — Reality thins.**
- **Phantom enemies and false-friendlies** — the render layer begins to
  sometimes draw fake enemy glyphs in empty tiles, and to sometimes
  render teammates in enemy palettes. **Every hallucination has a
  tell**: phantom glyphs have a 1-frame flicker at spawn; false-friendly
  renders leave a faint cyan outline on the teammate's true glyph. A
  skilled player who knows the tells can always disambiguate. A player
  ignoring the tells will shoot their friends.
- **Snap-out-of-it:** when a player damages a false-friendly rendered as
  an enemy, they hear a distinctive bell tone and the hallucination
  collapses for that player for 5s (grace window). They see clearly
  again, but sanity drops.
- HUD corruption: ammo counters occasionally display with a 1-frame
  delay; health bar is rendered in Carcosa ochre. Both *consistent* —
  learnable, not random.
- **Destroyed walls begin regenerating as Carcosa terrain** (§8.5).
  Destruction decay is partially reversed into *unsafe* geometry.
- **Brand discipline breaks** — the wave mixes all currently-voted
  brands together freely (imp + scav + zergling in the same pack).
- Intermission compresses to 20s.

**100% — Carcosa proper.**
- **Arena safe space is gone.** Yellow Sign tiles propagate. Most of the
  arena is in Carcosa state. The horde composition is maximum
  multi-brand plus Carcosa originals (original cosmic-horror archetypes:
  the Pallid Mask, the Tatters of the King, sign-bearers).
- Hastur Daemon can **directly possess** enemies — but only in situations
  the team's positioning has invited. Possession is telegraphed: the
  possessed enemy gains the Yellow Sign halo for 1.5s before the
  Daemon's behavior kicks in. Players who watch for halos can react. A
  skilled team at 100% still has agency.
- Mark cadence doubles. Two marked players at any time.
- Corruption gains from kills drop to zero. The only way to push past
  100% is to continue surviving; the game will make sure you don't.

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
- Skill-based kills: headshots (critical hits), kills without taking
  damage during that engagement, pistol-only kills all trickle sanity
  back.
- Destroying Yellow Sign anchor tiles (rare; requires AoE response).
- Intermission breathe phase (small regen).
- The `siphon` primitive (on-kill regen).

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
- Carcosa terrain is **not destructible by normal means**. Exception: the
  rare `yellow-glyph` primitive on a weapon will temporarily suppress a
  Carcosa tile for 8s.
- Carcosa has *anchor* tiles — clusters of 3–5 adjacent Carcosa tiles
  that act as generators. Destroying an anchor (requires concentrated
  AoE plus a Carcosa-suppression primitive or similar) removes 10%
  Corruption and reverts nearby Carcosa terrain to normal.
- At 75%, destroyed walls regenerate *as* Carcosa terrain. Your safe
  bunker becomes hostile architecture.

### 8.6 The Hastur Daemon

An AI system that **always exists**. Before any player has died, it is
the sole pressure-source beyond scripted wave spawns. It is the
always-watching presence that makes the fiction feel real.

**Responsibilities:**
- Idle: observes. Does nothing but log pressure telemetry.
- Active (from Corruption 25%): triggers periodic *notice events*. Each
  notice event is a short, telegraphed pressure spike:
  - *"The King notices."* — Yellow Sign glyph lingers onscreen for 4s.
    During those 4s, one random enemy in the arena gains a temporary
    primitive. Survivors can see the target lighting up.
  - *"The King exhales."* — All ambient spawns in the next 8s are
    teleport-spawned in the peripheral vision of survivors (telegraphed
    by a 2s dust tell). This is a Director-ish move.
  - *"The King reaches."* — A random destroyed-wall tile converts to
    Carcosa terrain.
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
- 5–7 enemy archetypes (component-composed)
- 1–2 minibosses
- A music stem + ambient audio bed
- A vox bundle (2–3s stingers)
- Recognizable-but-non-trademarked palette

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

An enemy is a row in TOML referencing component categories:

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

Enemies can be **looted**. Killing a Scav leaves a corpse; pressing E on
the corpse for 0.7s transfers loot. The loot table is brand-specific.
Looting a Scav rifleman might give you a Mosin, a bandage, an armor
plate, or a rare tactical primitive like `sustained`.

Looting channels cost time (you can be shot). Decisions matter.

### 9.3 Wave composition

Waves are **theme-driven but vote-driven**:

- Wave 1 is always the *opening* brand (default: tactical). Cold open,
  recognizable, grounded.
- Waves 2+ are composed from currently-bled-in brands, weighted by the
  order they were voted in.
- As Corruption rises, brand composition rules relax (§8.2 at 75%: free
  mixing).

Miniboss waves (every 5) pull from the brand most recently voted in. A
team that just picked FPS arena on wave 9 gets a cacodemon-analogue on
wave 10.

### 9.4 Authoring a new wave

One TOML file. Declares enemy weights, ambient audio ref, miniboss
candidate, environmental modifiers (optional: low-light, weather, etc.),
and brand-identity palette.

No Rust. That is a design contract.

---

## 10. Director mode — pure chaos

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

- Every **arena tile** has:
  - `material` (metal / concrete / wood / bone / flesh / carcosa)
  - `hp` and `armor_type`
  - `chunk_count` (how many debris particles it spawns on destruction)
  - `collapses_adjacent` (bool; triggers a destruction-propagation check)
  - `regenerates_as_carcosa` (bool; true at 75%+ Corruption)

- Tiles are destroyed by accumulated damage (and by specific primitives:
  `breach` applies structural damage directly). On destruction, the
  tile:
  1. Spawns N glyph-particles with velocity (radial from damage source),
     ballistic fall, decay timer (typical 0.7–1.3s), and a brief
     collision-against-other-particles (loose, approximate).
  2. Leaves a rubble/debris tile behind (low-cover; passable; can be
     further destroyed).
  3. Kicks a dust-cloud audio-visual effect (slight visibility penalty,
     readable).
  4. Applies knockback to adjacent entities if the damage source was
     explosive (`breach`, `lob` with certain primitives, etc.).
  5. Possibly propagates (for `collapses_adjacent` tiles — ceilings,
     structural beams — adjacent tiles take a share of damage and can
     chain).

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
- `ignite`: flammable tiles (wood / flesh / carcosa at low level) ignite
  and tick.
- `breach`: structural damage multiplier ×3 vs terrain.
- `ricochet`: bounces off non-destroyed terrain; skipped over debris.
- `cryo`: freezes liquid gibs into floor obstacles.
- `gravity-well`: pulls loose debris; can funnel gore into a pile that
  becomes a low-cover tile.
- `acid`: slowly dissolves non-metal terrain over 8s.

### 11.5 Networking destruction

- Terrain state is authoritative on host.
- Delta-sync on tile state changes (intact / damaged / destroyed / rubble
  / carcosa).
- Particle effects are *client-side visual only* — the client spawns
  deterministic particle runs from a seeded RNG keyed on (host-tick,
  damage-source-id, tile-id). All clients see identical particle
  patterns without syncing particles over wire.

---

## 12. Arena

### 12.1 Procedural generation

- Single procedural arena per run. Seeded from a shared RNG; seed
  displayed at run start and persisted to the leaderboard for "seed of
  the day" sharing.
- Generator outputs: a *core* (defensive focal point), 2–4 chokepoints
  leading in, scattered cover, destructible architecture, and 2–3
  *brand-bleed vote kiosks* (fixed positions; see §7.3).
- Size: ~120×40 half-block cells (~120×80 logical pixels). Fits in one
  standard terminal.

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

Some arenas are night-variants (low-light; vision-range reduction).
Environmental modifiers can be layered per brand: fog (tactical), toxic
rain (chaos roguelike), zero-G pockets (Carcosa late-game), etc.

### 12.4 Arena themes

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

- Late joiners enter a 10s spectator orientation: arena overview, team
  list, current wave, current Corruption, who's a Director.
- Spawn as a survivor with **baseline kit only**. No catch-up loot.
- Enemies are scaled to wave N; the joiner scavenges or dies quickly.
- The joiner's unlocks still apply (primitive drop pool, palette).
- If all survivors die before the joiner can contribute, they enter
  Director mode with the reduced Influence cap per §10.6.
- Dropped connections: a disconnected survivor's character remains as an
  AI-controlled placeholder (idle, defensive) for 60s to allow rejoin.
  If they don't rejoin in 60s, the character goes inert (still a
  hitbox, not a combatant).

---

## 16. Solo mode

`terminal_hell solo` starts offline practice:

- Host and client in same process.
- Hastur Daemon is the only pressure (no human Directors possible).
- At first death, the run ends immediately. Solo is balanced accordingly
  (lower spawn budget, slower Corruption rise).
- Solo runs count for achievement progress but not co-op-specific
  achievements.
- Seed-of-the-day is available in solo.

---

## 17. Technical architecture

### 17.1 Overview

```
┌──────────────────────────┐            ┌──────────────────────────┐
│  terminal_hell (client)  │ ◄─ UDP ──► │  terminal_hell (server)  │
│                          │   renet    │  (hosted by one player)  │
│  ratatui render @60fps   │            │  sim @30Hz authoritative │
│  crossterm input         │            │  netcode @20Hz snapshot  │
│  rodio audio             │            │  content-pack loader     │
│  input prediction        │            │  persistence (unlocks)   │
│  interpolation buffer    │            │  corruption + Daemon AI  │
└──────────────────────────┘            └──────────────────────────┘
```

Host runs both client and server in the same process. Server is truth;
clients render an interpolated 100ms-past snapshot.

### 17.2 Crate stack

| Concern            | Crate                              |
|--------------------|------------------------------------|
| TUI / rendering    | `ratatui` + `crossterm`            |
| Async runtime      | `tokio`                            |
| Networking         | `renet` (UDP + channels)           |
| Serialization      | `bincode` (wire), `serde`          |
| Content / config   | `toml`, `ron`, `serde`             |
| RNG (deterministic)| `rand_pcg`                         |
| Audio              | `rodio` (+ `cpal`)                 |
| Logging            | `tracing` + `tracing-subscriber`   |
| CLI                | `clap`                             |
| ECS                | `hecs` (lightweight; reconsider if pain) |

No Bevy for v1. Headless Bevy ECS is overkill and pulls deps.

### 17.3 Simulation

- 30Hz fixed-step tick. Deterministic given seed + input stream. Useful
  for replays (stretch) and testing.
- Authoritative host. Clients send input commands; server simulates;
  server broadcasts snapshots.
- Client prediction for movement only. Shooting, looting, destruction
  are server-arbitrated.
- Interpolation buffer: 100ms. Lag compensation for hitscan rewinds
  remote entities 0–200ms based on reporter RTT; capped.

### 17.4 Netcode

- Transport: UDP via renet.
- Reliability channels:
  - `0` reliable-ordered — lobby, run start, unlocks, chat, votes.
  - `1` unreliable-unordered — input (client→server), snapshots
    (server→client).
  - `2` reliable-unordered — damage events, pickup acks, destruction
    events.
- Snapshots: delta-compressed @20Hz.
- Input packets: every sim tick @30Hz with last-N-frame redundancy.
- Bandwidth: <30 KB/s sustained per client; <80 KB/s peak (destruction
  events can spike).
- Join handshake: Nickname → unlock hash → arena seed → pack manifest →
  initial snapshot → go.
- NAT traversal: none in v1. Host forwards a port / uses Tailscale /
  ZeroTier / direct LAN. Document in README.

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
  (not yet implemented in v0.2).
- Keyboard-aim fallback if no mouse support detected (deferred).
- Minimum terminal size: 80×30. Below that: refuse with resize prompt.
- Required font glyph coverage: U+1FB00..U+1FB3B (sextants) and
  U+2800..U+28FF (braille). Cascadia Code, JetBrains Mono Nerd, Fira
  Code Nerd, Iosevka all work. Detection is deferred — users see tofu
  if they pick a bad font; the README documents known-good options.

### 17.6 Content packs

```
content/
  core/                     # always loaded; embedded in binary
    primitives.toml
    fire_modes.toml
    archetypes.toml
    brands/
      fps_arena.toml
      tactical.toml
      chaos_roguelike.toml
    audio/
      music/ sfx/ vox/
    arenas/
      server_room.toml ...
  carcosa_core/             # always loaded; Hastur + Sign mechanics
    daemon.toml
    sign.toml
    carcosa_primitives.toml
    audio/
  <user_pack>/              # optional; loaded if enabled at serve time
    brands/
    arenas/
    primitives/
    audio/
```

- Packs declare: brands they expose, primitives they add, arenas they
  add, audio they bundle, dependencies.
- Host selects active packs at `serve` start.
- Clients auto-sync pack *metadata* (not full audio assets) at connect
  so they can render correctly; missing-audio plays silently and logs a
  warning. Asset sync is a v1.1 concern.

### 17.7 Persistence

- Unlocks: `~/.terminal_hell/unlocks/<nick>@<host_pubkey_hash>.ron`
- Leaderboard: `~/.terminal_hell/leaderboard.ron`
- Replay (stretch): input-stream recording; deterministic sim makes it
  feasible for v1.1.

### 17.8 Build & distribution

- `cargo build --release` → single binary (`terminal_hell[.exe]`).
- Core content embedded via `include_dir!`.
- External packs live in `content/` next to binary.
- CLI:
  - `terminal_hell serve [--port P] [--pack NAME ...]`
  - `terminal_hell connect <code>` | `connect <addr>:<port>`
  - `terminal_hell solo`
  - `terminal_hell --list-packs`
  - `terminal_hell --seed-of-the-day`
- Room codes: base32-encoded (case-insensitive), 6 chars. Maps to
  `addr:port`. v1 skips the relay service — raw IP + Discord is fine.
  Relay is post-v1.

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

```
ClientToServer:
  Handshake { nickname, unlocks_hash, client_version, pack_manifest_hash }
  Input { tick, move_x, move_y, aim_x, aim_y, buttons, traversal, seq }
  DirectorCommand { kind, target_or_pos, payload }
  LootCorpse { target_entity }
  VoteBrand { kiosk_id }
  Chat { text }
  Ping

ServerToClient:
  JoinAck { session_id, seed, pack_manifest, initial_snapshot }
  Snapshot { tick, delta_from_tick, entities, corruption, sanity_self, events }
  DamageEvent { source, target, amount, kind, primitives_applied }
  DestructionEvent { tile_id, damage_source, chunk_seed }
  SanityEvent { target, delta, cause }
  CorruptionEvent { new_value, threshold_crossed? }
  HasturEvent { kind, target?, payload }   // notice / mark / possession
  UnlockGranted { unlock_id }
  RunEnd { summary }
  Chat { from, text }
  Pong
```

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

**M3 — Primitive bus scaffold (2 weeks).** Universal effect bus with a
starter set of ~8 primitives. Weapons are base fire-mode + slots.
Everything composes. Test multiple primitive stacks on both player
weapons and enemies.

**M4 — Content-pack plumbing + first brand (2 weeks).** Move archetypes,
fire-modes, primitives to TOML. Load core pack. Author and validate the
FPS arena brand fully (5+ archetypes, music stem, vox, audio-tells).

**M5 — Wave structure & intermission (2 weeks).** Wave scheduler,
miniboss every 5, intermission loop with brand-vote kiosks. Three brands
supported; voting resolves and bleed-through works.

**M6 — Procedural arena (2 weeks).** Arena generator (core,
chokepoints, kiosk placement, destructible structure). Seed sharing.
Day/night variants.

**M7 — Carcosa (4 weeks).** The jewel. Corruption %, sanity, marks,
Carcosa terrain, Hastur Daemon, Yellow Sign audio + visual integration,
threshold beats (25/50/75/100). Playtest extensively; this is the
highest-risk system.

**M8 — Director mode (3 weeks).** Death-to-Director transition.
Influence economy. Spawn / possess / hazard / breach / command. Director
UI. Co-op Director dynamics. Playtest.

**M9 — Audio pillar (2 weeks).** Full rodio integration. Spatial audio.
Dual-stem dynamic mix. Hot-reload dev mode. Carcosa audio layer.
Audio-tells across all enemy abilities.

**M10 — Unlocks & persistence (1–2 weeks).** Per-nickname unlock files,
local leaderboard, achievement triggers (survivor + Director).

**M11 — Join-in-progress + polish (2 weeks).** JIP, connection drop
recovery, terminal probing, fallbacks, settings UI, sanity audio/visual
polish.

**M12 — Content push (3 weeks).** Author the remaining two Tier-1
brands (tactical, chaos roguelike) + fill out primitive pool to 24 +
author 5 arena variants.

**M13 — Closed playtest, balance, ship.**

Rough total: **6–8 months of solo nights-and-weekends**, nearly double
the ambition of v1 because destruction + Carcosa are big. Milestones
M1–M5 are each shippable-feeling standalone for a solo-practice mode
before the spec fully lands.

---

## 20. Open design questions

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

---

## 22. Glossary

- **Arena** — the procedural map for a single run.
- **Braille layer** — the 2×4-dot overlay rendered beneath the sextant
  layer; shows through where sextants are empty. For fine detail: bullet
  dust, sparks.
- **Brand** — a category of shooter homage (FPS arena, tactical, chaos
  roguelike, etc.).
- **Brand bleed** — the vote-based introduction of a new brand into the
  current wave's enemy pool.
- **Carcosa** — the fictional city from the Yellow Mythos; metaphysically
  bleeding into the shell. Also the visual state of the arena at high
  Corruption.
- **Carcosa terrain** — Yellow-Sign-inlaid tiles that drain sanity and
  empower enemies crossing them.
- **Corruption %** — global arena metric (0–100+) tracking the advance of
  Carcosa into the run.
- **Director** — a dead player; plays the antagonist/chaos side.
- **Gibs** — glyph-particle remains of destroyed entities.
- **Hastur** — the King in Yellow. The game's antagonist presence.
- **Hastur Daemon** — the AI system running Hastur's passive/active
  pressure before (and alongside) human Directors.
- **Influence** — Director currency for spawning, possessing, hazards.
- **Mark** — Hastur's gaze on a specific survivor; bonus-damage and
  AI-priority target.
- **Miniboss** — named themed elite every 5 waves.
- **Pack** — content bundle (brands + archetypes + primitives + audio +
  arenas).
- **Primitive** — a named effect-component in the universal composition
  bus.
- **Run** — single session from drop-in to last-survivor-down.
- **Sanity** — per-player 0–100 metric; low sanity causes deterministic
  visual/audio/AI effects.
- **Sextant layer** — the 2×3 solid-fill pixel grid that is the primary
  rendering surface. Each terminal cell holds 6 sub-pixels resolved into
  one Unicode sextant glyph.
- **Sprite** — a procedurally-generated 2D `Option<Pixel>` grid stamped
  into the sextant layer to render an entity.
- **Traversal verb** — a movement primitive (dash, blink, grapple, etc.).
- **Wave** — 60–180s pressure period with a brand theme.
- **Yellow Sign** — Hastur's sigil; appears onscreen at Corruption
  thresholds and during notice events.

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

### 24.1 Shipped in v0.2 (as of this writing)

What's in the repo and playable today:

- Solo, `serve`, and `connect` modes over UDP. Host-authoritative sim.
- Sextant+braille framebuffer, procedural sprites, aim-tracking barrel.
- Tile-chunked destruction + persistent rubble + glyph-particle gibs.
- Three enemy archetypes (Rusher, Pinkie, Miniboss), wave scheduler,
  miniboss every 5 waves, run-end summary.
- One hand-crafted arena that auto-scales to the terminal size.
- HUD (wave / HP / kills), wave banner, game-over screen.

What the spec describes but **is not yet shipped**:

- Universal effect bus and primitives (M3).
- Classes-via-loot / inventory / corpse looting.
- Carcosa / Corruption / Sanity / Hastur Daemon (§8).
- Director mode (§10).
- Brands beyond "single hardcoded pool" (§9).
- Intermission brand-vote kiosks (§7.3).
- Audio pillar (§13).
- Meta-progression + unlocks (§14).
- Procedural arena generator (§12).
- Content-pack loader (§17.6).

Sections 6–16 of this document are the design target, not the current
build. Use §19 Milestones to navigate what's done vs. ahead.

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
