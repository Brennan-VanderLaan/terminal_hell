# Terminal Hell — Design & Technical Specification (v1)

> *"Every shooter you've ever loved is trying to kill you. Hold the line."*

A terminal-native, top-down multiplayer roguelike shooter where players defend a
position against escalating waves drawn from iconic games across history. Dead
players don't leave — they ascend to become **small gods**, directing the horde
that just killed them.

---

## 1. Elevator pitch

You and 1–7 friends are queued for a game that won't load for another 20
minutes. Someone runs `terminal_hell serve`. Everyone else runs
`terminal_hell connect <code>`. You pick a class, roll some modifiers, and hold
a bunker on a procedurally-generated map while waves of Halo Grunts, Doom
Imps, Zerglings, Arma riflemen, and Tarkov bosses try to grind you down. When
you die, you don't sit out — you become a **Director**: a small god who
controls enemy units, places hazards, and shepherds the horde against your
former teammates. The run ends when everyone's dead. There is no win state,
only a leaderboard of how long you lasted.

---

## 2. Design pillars

1. **Plays in the gaps.** Runnable in the time between queue pop and match
   start. Instant join (nickname + code), no launcher, no updates, no menus
   deeper than two screens.
2. **Meta-homage, not IP theft.** Each wave is a *tonal* homage —
   silhouettes, weapon feel, sound, banter — rendered in half-block ASCII.
   No trademarked names in shipped content; references live in user-authored
   content packs.
3. **Death is a promotion.** Dying is never the end of the fun. The Director
   mode is a first-class game in its own right.
4. **Data-driven everything.** Classes, enemies, weapons, modifiers, waves,
   and themes all load from TOML/RON content packs. Adding "the Arma wave" is
   a content task, not a code task.
5. **Terminal-first, not terminal-tolerated.** Feels *good* in a modern
   terminal. Mouse aim, truecolor, 60fps half-block rendering, real audio.
6. **Endless, but curved.** No run cap. Difficulty follows a scripted curve
   for the first ~30 waves, then enters a pure escalation loop with seeded
   surprise spikes.
7. **Friends-first infra.** One player hosts, others connect. No accounts, no
   matchmaker, no servers to pay for. Unlocks are portable per host.

### Non-goals (for v1)

- Public matchmaking, lobbies, or a central account service.
- Mobile, web, or non-terminal clients.
- Anti-cheat beyond "the host is authoritative." This is a friends game.
- PvP competitive modes. Turncoat Director is PvE-with-a-twist, not arena PvP.
- Shipping trademarked assets or names. (See §12.)

---

## 3. User stories

### 3.1 Host (Hana)
- *As a host*, I want to `terminal_hell serve` and get a 6-character room
  code I can paste into Discord, so friends can join in under 30 seconds.
- *As a host*, I want to configure player cap, run modifiers, and enabled
  content packs before starting, but never be forced into setup screens.
- *As a host*, I want the run to continue if someone's connection drops, and
  for them to rejoin seamlessly into the same session.
- *As a host*, I want unlock progress persisted locally per nickname so my
  regular group's unlocks carry over between nights.

### 3.2 Surviving player (Sam)
- *As a player*, I want to aim with the mouse and move with WASD — it should
  feel like a real top-down shooter, not nethack.
- *As a player*, I want to see my teammates' health, ammo, and status at a
  glance so I can decide when to revive vs. hold position.
- *As a player*, I want weapon pickups to matter — picking up Killa's RPK
  should feel like a power spike.
- *As a player*, I want to play a full, fun run in 20–40 minutes on average,
  with the option to keep going forever if the team is clutch.

### 3.3 Late joiner (Lin)
- *As a late joiner*, I want to join mid-run with a minimal kit and scavenge
  my way up — not be handed a power tier I didn't earn.
- *As a late joiner*, I want enough on-screen context (current wave, team
  status, map) to orient in under 10 seconds.

### 3.4 Turncoat Director (Dee)
- *As a dead player*, I want to immediately do something fun — not sit in a
  spectator window for 20 minutes.
- *As a Director*, I want meaningful choices: which enemy archetype to
  spawn, where to place hazards, which of my former teammates to hunt first.
- *As a Director*, I want the power to matter but not trivially win — it
  should feel like stacking pressure, not one-shotting the survivors.
- *As a Director*, I want to possess individual units and control them
  directly when I want to be personally menacing.

### 3.5 Content author (Cam)
- *As a modder/author*, I want to add a new themed wave (say, "S.T.A.L.K.E.R.
  Zone Emission") by writing one TOML file with components referencing
  existing abilities — no Rust required for basic content.
- *As a content author*, I want to provide sprites (glyph + color pairs),
  sounds, voice lines, and balance numbers declaratively.

### 3.6 Audio contributor (you)
- *As the audio author*, I want to drop WAV/OGG samples into a content-pack
  folder and reference them by name from enemy/weapon/ambient TOML. Hot-reload
  during dev is table stakes.

---

## 4. Goals & success criteria

### 4.1 Experience goals
- **Feel:** Responsive, legible combat. Input-to-render latency under 16ms
  local; under 80ms over LAN; under 150ms over internet.
- **Legibility:** A new player can identify friends, enemies, projectiles,
  hazards, and pickups within 5 seconds of their first wave.
- **Session shape:** Median run 20–30 minutes, 95th percentile under 90
  minutes. No cap.
- **Replay:** After 10 runs, a player should have seen only ~50% of waves,
  modifiers, and enemy archetypes.

### 4.2 Technical goals
- **Frame budget:** 60fps render, 30Hz simulation tick, 20Hz network tick.
- **Player count:** 2–8 players, target comfort 4–6.
- **Footprint:** Single binary under 20MB. Content packs optional extras.
- **Platforms:** Linux, macOS, Windows (Windows Terminal / kitty / WezTerm /
  iTerm2 / Alacritty). Degrades gracefully on dumb terminals (no mouse →
  keyboard-aim fallback).
- **Build:** `cargo run` starts a local solo practice game in under 5 seconds.

---

## 5. Gameplay specification

### 5.1 Core loop

```
┌─ Connect (code or LAN) ─┐
│                          │
▼                          │
Pick class ─► Roll modifiers ─► Drop into arena ─► Waves escalate ─┐
                                                                   │
                                       ┌──── Survive ──────────────┘
                                       │
                                       ▼
                                Down → revive window → dead
                                                        │
                                                        ▼
                                                  Director mode
                                                        │
                                            team wipes ─┴── run ends
                                                        │
                                                        ▼
                                           Leaderboard + unlock tally
                                                        │
                                                        ▼
                                                      Lobby
```

### 5.2 Run structure

- **Wave:** 90–180 seconds of escalating pressure. Has a *theme* (e.g.
  "Covenant Strike," "Zone Emission," "Zerg Rush"), which selects the enemy
  palette, ambient audio bed, and possibly environmental effects.
- **Intermission:** 20–40 seconds between waves. Loot, revive downed
  teammates, refit, move position if the arena permits.
- **Miniboss wave:** Every 5 waves, a named unit shows up ("Killa," "Pyramid
  Head–shaped silhouette," "a very large zergling"). Minibosses drop
  guaranteed rare loot.
- **Ambient pressure:** Between waves, 0.3x–0.5x baseline spawns continue,
  so downtime is never total peace.
- **Endless curve:** Difficulty scales on wave index `w`. Enemy hp, damage,
  and spawn budget grow on piecewise curves tuned so wave 30 is "hard for a
  competent team," wave 60 is "legend territory," wave 100+ is "nobody
  should have survived this long."

### 5.3 Controls

**Default bindings (rebindable):**

| Action            | Input                       |
|-------------------|-----------------------------|
| Move              | WASD                        |
| Aim               | Mouse                       |
| Fire              | LMB or Space                |
| Alt-fire / aim    | RMB or Shift                |
| Reload            | R                           |
| Swap weapon       | Q / 1–4                     |
| Use / revive      | E                           |
| Dash              | LShift tap                  |
| Ping / mark       | Middle-click or G           |
| Team chat         | Enter (line-buffered)       |
| Scoreboard        | Tab (hold)                  |
| Menu              | Esc                         |

**Fallback (no-mouse terminal):** Arrow keys aim in 8 directions; Space
fires. Detected at connect time via terminal capability probe.

**Director mode (see §5.8):** Full rebinding set — cursor-driven command
mode with keyboard hotkeys for spawn/possess/hazard.

### 5.4 Classes

Each class has a **passive**, an **active ability** (cooldown), and starting
kit. Classes are data-driven (TOML); v1 ships with 6:

| Class       | Passive                              | Active                          |
|-------------|--------------------------------------|---------------------------------|
| Grunt       | +20% mag size                        | Adrenaline: 3s damage resist    |
| Medic       | Auto-regen out of combat             | Drop a revive beacon            |
| Engineer    | Can deploy walls/turrets from scrap  | Drop auto-turret (60s lifespan) |
| Scout       | Faster move, can see further in dark | Smoke + short teleport          |
| Heavy       | Carry LMG-class weapons at full move | Bullet shield (directional)     |
| Pyromancer  | Ignite on crit                       | Molotov arc                     |

Additional classes unlocked via meta-progression (§5.10).

### 5.5 Weapons & items

Weapons are **component-composed**: `fire_mode`, `projectile`, `spread`,
`mag`, `reload_s`, `damage_profile`, `on_hit_effects`, `audio_ref`,
`glyph`. A weapon is just a row in a TOML table referencing these.

Examples of shipped (non-branded) archetypes:
- **Semi-auto pistol, SMG, auto-rifle, pump shotgun, DMR, LMG**
- **Plasma rifle (homage slot), bolt-action (homage slot), RPG**
- **Grenades:** frag, smoke, flash, incendiary

Pickups drop from enemies and themed wave crates. Rarity tiers:
Common / Uncommon / Rare / Elite / Legendary. Higher rarities add
**perk slots** (on-hit procs, utility effects — drawn from a shared perk
pool). This gives drop RNG a strong build-identity axis without a separate
perk tree.

### 5.6 The wave/theme system (the central abstraction)

This is the heart of the project. Every "enemy" in the game is an entity
composed of **components** (à la Warcraft III abilities, or ECS
archetypes). A **theme** is a bundle of enemy compositions, an ambient audio
bed, weather/effects, and optional environmental modifiers.

**Component categories:**
- `Locomotion` — walker / flyer / crawler / teleporter / hulk
- `Weapon` — melee / hitscan / projectile / AoE / beam / summon
- `AI` — rusher / sniper / flanker / artillery / suicide / horde
- `Vitals` — hp, armor, armor type (light / kinetic / plasma / energy)
- `Senses` — sight range, hearing, requires line-of-sight
- `Modifiers` — regen / shielded / cloaked / explosive-on-death / spawns-on-death
- `Aesthetic` — glyph, palette, corpse glyph, sound bundle

A themed wave is then declared like:

```toml
[waves.covenant_strike]
ambient_audio  = "themes/covenant/ambient.ogg"
music_intensity = "medium"
enemies = [
  { archetype = "grunt_plasma",  weight = 0.6, min_wave = 3 },
  { archetype = "jackal_shield", weight = 0.25, min_wave = 5 },
  { archetype = "elite_sword",   weight = 0.15, min_wave = 8 },
]
miniboss = "zealot"
environmental = ["low_light"]

[archetypes.grunt_plasma]
base = "rusher_small"
weapon = "plasma_smg"
vitals = { hp = 40, armor = "light" }
modifiers = ["explosive_on_death_small"]
aesthetic = { glyph = "g", palette = "covenant_purple", sound_bundle = "grunt_vox" }
```

Adding a new themed wave is a pure content task.

### 5.7 Enemy archetypes (shipped examples, non-branded labels)

| Archetype     | Role           | Notes                                |
|---------------|----------------|--------------------------------------|
| Rusher        | Melee zerg     | Cheap, fast, dies quick              |
| Shield trooper| Frontline      | Frontal armor, flanker bait          |
| Plasma grunt  | Light ranged   | Lobs slow projectiles                |
| Sniper        | Heavy ranged   | Long wind-up, punishing damage       |
| Bomber        | Suicide        | Charges and detonates                |
| Spawner       | Summoner       | Stationary, births cheap minions     |
| Armored APC   | Miniboss mech  | Slow, high HP, machine gun turret    |
| Named elite   | Miniboss       | Unique kit per theme                 |

### 5.8 Director (turncoat) mode

The **jewel feature.** When a player dies and their revive window expires,
they don't spectate — they become a Director. There can be up to N
Directors simultaneously (N = dead players).

**Director capabilities:**

- **Commander view:** Top-down fog-of-war-free view of the arena. Can see
  everything, including surviving players.
- **Spawn budget:** Directors accrue **Influence** over time and from events
  (survivor kills minions = +Influence, survivor takes damage = +Influence).
  Spawning units, placing hazards, and possessing cost Influence.
- **Spawn:** Select from a menu of unlocked enemy archetypes weighted by
  current wave theme. Place spawn point anywhere outside players' direct
  line of sight.
- **Possess:** Director takes direct control of an existing AI unit (or one
  they just spawned) — they become that unit until it dies or they release.
  While possessing: unit has +stats and access to the possessed player's
  class-derived "divine" ability (e.g. a possessed Plasma Grunt from a
  former Engineer gets a mini-turret drop).
- **Hazards:** Drop environmental effects — gas clouds, caltrops, warp rifts,
  collapsing ceilings, flare darkness. Costs Influence, scaled by power.
- **Command:** Issue "focus fire," "rush," "hold" orders to nearby AI units.
  Applies a temporary AI override.

**Balance philosophy:** Directors should *raise the ceiling* of pressure,
not provide a direct I-win button. A team of survivors playing well should
beat two Directors playing well; three+ Directors (half the team dead) is
when it gets grim. Influence costs and Director count scaling are the
primary tuning knobs.

**Director UI:** Different screen layout — minimap takes center, spawn/
possess/hazard menus are keyboard-driven chord menus (like nethack
extended commands). Mouse places cursor for spawn/hazard targeting.

**Griefing guard:** Directors cannot damage themselves. Friendly fire off
between Directors. Maximum Influence budget capped per Director to prevent
hoarding 10 minutes of passive income into a single megaspawn.

### 5.9 Run modifiers

Rolled at run start, visible to all players. 1–3 modifiers per run from a
weighted pool. Examples:

- *Acid blood* — enemies leak damaging pools on death.
- *Double loot* — +100% drops, +30% enemy HP.
- *Brownout* — reduced vision range, flashlight required.
- *Telefrag* — occasional enemy teleport-ins behind the team.
- *Plasma Sunday* — all enemy hitscan damage becomes plasma (different
  armor interaction).
- *Bring Your Own Army* — one extra Director seat, seeded with AI if no
  dead player yet.

Modifiers interact with themes (some themes force/ban certain modifiers).

### 5.10 Meta-progression (unlocks only)

No stat inflation. Unlocks are **options**, not power.

Tracked per-nickname in a host-local file (`~/.terminal_hell/unlocks/<nick>.ron`).

Unlock categories:
- **Classes** (6 starter, 6 more unlockable)
- **Starter weapons** (pick loadout at class-select)
- **Modifier slots** (start the run with 1 free chosen modifier)
- **Cosmetic glyph/palette sets** (your `@` can be green)
- **Director archetypes** (new unit types you can spawn as a Director)
- **Themed wave variants** (earn by surviving a specific wave N times)

Unlock triggers are *achievements*, not just grind: "Survive to wave 20
with no healing items," "Solo a miniboss," "Revive 3 teammates in one
wave," "Win a run as sole survivor."

### 5.11 Join-in-progress

When a new player connects to a run in progress:
- They enter a 10-second spectator orientation (arena overview, team list,
  current wave).
- Spawn with **baseline kit only** (class choice allowed, but starting gear
  is default — no catch-up loot).
- Enemies already scaled to wave N; catching up requires scavenging pickups
  and kills. Deliberately asymmetric; rewards surviving teammates.
- Their unlocks (class/starter loadout choices) still apply.

If all survivors die before the new player can contribute, they immediately
enter Director mode with a minimal Influence grant (not nothing, but not a
free kill).

### 5.12 Death, revive, Director transition

```
Alive ─ hp → 0 ─► Downed (crawling, can't shoot, bleedout 30s)
         │                 │
         │                 ├─ teammate finishes revive (5s channel) → Alive with 40% hp
         │                 │
         │                 └─ bleedout expires → Dead → Director mode
         │
         └─ instakill sources (explosions > 2x hp, etc.) skip downed → Director
```

- Revive interruption: taking damage as the reviver cancels the channel.
- Downed players can crawl slowly and mark enemies with pings.
- Director transition is *immediate* when bleedout expires; no "return to
  lobby" screen.
- If a Director's team wins the round (all remaining survivors die), the
  run ends — Directors are on the credits screen, not a separate reward
  screen. Everyone's achievements tally together.

### 5.13 Arena / map

- **Single procedural arena per run.** Generated at run start from a seeded
  RNG; seed is shared with all players and displayed (nice for
  "seed of the day" challenges).
- **Structure:** A defensible *core* (the spawn / objective area), 2–4
  chokepoints leading in, cover clutter scattered, destructible elements
  (crates, thin walls), and optional *outposts* — sub-objectives that grant
  buffs if held.
- **Size:** Roughly 120x40 cells in half-block space (so ~120x80 logical
  rendered pixels). Fits in one terminal screen at standard sizes.
- **Day/night shift:** Optional per-theme; affects vision radius.
- **Destructibility:** Some walls yield to explosives. Creates emergent
  defensive decay — the fortress you started in is not the one you end in.

---

## 6. Technical architecture

### 6.1 Overview

```
┌──────────────────────────┐            ┌──────────────────────────┐
│  terminal_hell (client)  │ ◄─ UDP ──► │  terminal_hell (server)  │
│                          │   renet    │  (hosted by one player)  │
│  ratatui render @60fps   │            │  sim @30Hz (authoritative)│
│  crossterm input         │            │  netcode @20Hz snapshot   │
│  rodio audio             │            │  content-pack loader     │
│  local input prediction  │            │  persistence: unlocks     │
│  interpolation buffer    │            │                          │
└──────────────────────────┘            └──────────────────────────┘
```

The host runs **both** client and server in the same process (they can play
too). The server is the source of truth; clients render an interpolated,
slightly-in-the-past snapshot.

### 6.2 Crate stack

| Concern            | Crate                  |
|--------------------|------------------------|
| TUI / rendering    | `ratatui` + `crossterm`|
| Async runtime      | `tokio`                |
| Networking         | `renet` (UDP + channels) |
| Serialization      | `bincode` (wire), `serde` |
| Content / config   | `toml`, `ron`, `serde` |
| RNG (seeded, det.) | `rand`, `rand_pcg`     |
| Audio              | `rodio` (+ `cpal`)     |
| Logging            | `tracing` + `tracing-subscriber` |
| CLI                | `clap`                 |
| Time               | `std::time::Instant`   |
| ECS (internal)     | hand-rolled archetype store OR `hecs` (lightweight) |

No Bevy for v1; headless Bevy ECS is overkill and pulls a lot of deps. Revisit
if hand-rolled ECS becomes painful.

### 6.3 Simulation

- **Tick rate:** 30Hz (33.3ms). Fixed timestep. Deterministic given same seed
  and same input stream (useful for replays and testing, not required for
  networking correctness).
- **Authoritative server:** Clients send input commands; server simulates;
  server broadcasts snapshots.
- **Client prediction:** Movement only. Shooting is server-arbitrated
  (prevents "my pistol fires faster than server allows" cheats even in a
  friends game — keeps the system clean).
- **Interpolation buffer:** 100ms. Remote entity rendering is 100ms behind
  server authoritative state, interpolated smoothly.
- **Lag compensation:** For hitscan, server rewinds remote entities 0–200ms
  based on reporter's measured RTT. Capped to prevent abuse.

### 6.4 Netcode

- **Transport:** UDP via renet. Reliability channels:
  - `0` reliable-ordered: lobby events, run start, unlock grants, chat.
  - `1` unreliable-unordered: input commands (client→server), snapshots
    (server→client).
  - `2` reliable-unordered: damage events, pickup acks.
- **Snapshot:** Server sends delta-compressed state snapshot @20Hz.
- **Input:** Client sends input packet every simulation tick (30Hz).
  Includes last N input frames for redundancy.
- **Bandwidth target:** < 30 KB/s per client sustained, < 80 KB/s peak.
- **Join handshake:** Nickname → unlock payload → arena seed → initial
  snapshot → go.
- **NAT traversal:** None in v1. Host forwards a port or uses Tailscale /
  ZeroTier / direct LAN. Document in the README.

### 6.5 Rendering

- **Framebuffer:** Custom layer above ratatui. Internal buffer of
  `(fg_rgb, bg_rgb, char)` cells at half-block resolution (each terminal
  cell = 2 logical pixels stacked).
- **Draw pipeline:**
  1. Clear virtual pixel buffer.
  2. Render terrain (static, cached per map seed).
  3. Render entities (interpolated positions).
  4. Render projectiles and effects.
  5. Render HUD overlay via ratatui widgets.
  6. Resolve pairs of vertical pixels into `▀`/`▄`/` `/`█` with FG/BG colors.
  7. Diff against previous frame; emit only changed cells to terminal.
- **Target:** 60fps render; measured input→frame latency < 20ms local.
- **Truecolor assumed**; 256-color fallback detected via `TERM` / `COLORTERM`.
- **Fallbacks:**
  - No truecolor → 256-color palette mapping.
  - No mouse → keyboard aim mode, detected and announced at connect.
  - Small terminal → warning + gameplay UI gracefully reflows; below a
    minimum size, refuse to start and show a resize message.

### 6.6 Audio

- **Engine:** `rodio` over `cpal`. Music + SFX mixers with independent volume.
- **Spatialization:** 2D positional — stereo pan and attenuation based on
  source distance from listener (player camera center).
- **Music:** Layered intensity system à la Left 4 Dead / Hotline Miami —
  base theme bed + high-intensity overlay fades in during wave peaks and
  miniboss encounters.
- **Content-pack audio:** Each pack bundles `audio/{music,sfx,vox}/*.ogg`.
  Referenced by symbolic name from entity/weapon/wave TOML.
- **Hot-reload:** In dev mode, audio refs reload on file change. Behind a
  feature flag.
- **Samples:** Placeholder set shipped; real samples authored externally
  (eurorack + FL Studio) and dropped into `assets/audio/`.

### 6.7 Content packs

```
content/
  core/                      # always loaded
    classes.toml
    weapons.toml
    perks.toml
    modifiers.toml
    archetypes.toml          # base enemy components
    themes/
      generic_horde.toml
    audio/
      sfx/
      music/
      vox/
  homage_pack_example/       # user-contributed; named generically in repo
    themes/
      midnight_covenant.toml
    archetypes/
      plasma_grunt.toml
      shield_jackal.toml
    audio/
      sfx/
      music/
      vox/
```

Packs declare which themes they expose. Host selects active packs at server
start. Clients auto-download pack metadata (not audio assets) to render
correctly — if a client is missing a pack's audio, they play silent SFX but
render fine. (Asset sync is a v1.1 concern; v1 says "install the same pack
on all clients.")

### 6.8 Persistence

- **Unlocks:** `~/.terminal_hell/unlocks/<nickname>.ron`. RON for
  human-readability. Keyed by nickname + host pubkey (so your "Sam" unlocks
  on Hana's host don't bleed into "Sam" on another host's game — avoids
  identity collisions without accounts).
- **Leaderboard:** Local, per-host. `~/.terminal_hell/leaderboard.ron`.
  Top 100 runs: wave reached, duration, kills, team composition, seed.
- **Replay (stretch):** If simulation is deterministic, recording the input
  stream is sufficient for replay. v1.1 feature.

### 6.9 Build, distribution, runtime

- **Single binary:** `cargo build --release` produces `terminal_hell` (or
  `.exe`). Content is embedded for core pack via `include_dir!`; external
  packs loaded from `content/` next to the binary.
- **CLI:**
  - `terminal_hell serve [--port P] [--pack PACKNAME]...`
  - `terminal_hell connect <code>` or `connect <addr>:<port>`
  - `terminal_hell solo` — offline practice mode
  - `terminal_hell --list-packs`
- **Room codes:** 6-character base32 codes map to `ip:port`. v1 does this
  via a tiny public relay for code→endpoint lookup (opt-in; users can
  always paste raw IP). Relay is optional infra; initially skipped in favor
  of raw IP + Discord.

---

## 7. Data model (sketch)

### 7.1 Entities

Internal ECS-ish. Entity is an index into component tables.

```
Entity {
  Position { x: f32, y: f32 }
  Velocity { vx: f32, vy: f32 }
  Collider { shape, radius }
  Vitals { hp, hp_max, armor, armor_type }
  Faction { player | ai | director_spawned | neutral }
  Renderable { glyph, palette, layer }
  Weapon { archetype_id, ammo, mag, cooldown }
  AI { behavior_tree_id, target, state }
  Audio { footstep_bundle, vox_bundle }
  Modifiers { Vec<ModifierInstance> }
  Networked { entity_id, owner }
  ...
}
```

### 7.2 Network messages (shape)

```
ClientToServer:
  Handshake { nickname, unlocks_hash, client_version }
  Input { tick, move_x, move_y, aim_x, aim_y, buttons, seq }
  DirectorCommand { kind, target_or_pos, payload }
  Chat { text }
  Ping

ServerToClient:
  JoinAck { session_id, seed, pack_manifest, initial_snapshot }
  Snapshot { tick, delta_from_tick, entities: [...], events: [...] }
  DamageEvent { source, target, amount, kind }
  UnlockGranted { unlock_id }
  RunEnd { summary }
  Chat { from, text }
  Pong
```

---

## 8. Milestones (toward "full feature spec v1")

Even though the target is the full spec, we need a real build order. Each
milestone should end in something runnable.

**M0 — Hello, terminal (1 week).** Single binary, ratatui draws a moving
`@` on a static map. WASD moves it. No networking. Proof of render +
input pipeline.

**M1 — Local shooter (2–3 weeks).** Mouse aim, bullets, one weapon, one
enemy type with dumb chase AI, collision, death/respawn, half-block
rendering. Solo mode only. Proof of combat feel.

**M2 — Two-player LAN (2–3 weeks).** Add renet, authoritative host,
interpolation, lag compensation. Two players can shoot the same enemies on
one LAN. Proof of netcode.

**M3 — Wave & content-pack scaffolding (2 weeks).** Move classes, weapons,
enemies, waves into TOML. Load core pack. Wave scheduler with
intermission/miniboss rhythm. No themes yet, just functional waves.

**M4 — First themed wave (1 week).** Author one homage-themed wave using
the content system. Validates the abstraction.

**M5 — Classes, perks, modifiers (3 weeks).** All v1 classes. Perk system.
Run-modifier rolls. Full drop RNG with rarities.

**M6 — Procedural arena (2 weeks).** Map generator with core / chokepoints /
cover / outposts. Seed sharing, day/night, destructibility.

**M7 — Death, revive, Director mode (4 weeks).** The centerpiece. Downed
state, revive channel, Director UI, spawn/possess/hazard/command, Influence
economy. Playtest extensively; this is the highest-risk system.

**M8 — Audio layer (2 weeks).** rodio integration, spatial audio, music
intensity, content-pack audio loading, hot-reload dev mode.

**M9 — Unlocks & persistence (1–2 weeks).** Per-nickname unlock files, local
leaderboard, achievement triggers.

**M10 — Join-in-progress + polish (2 weeks).** JIP flow, connection drop
recovery, terminal capability probing, fallbacks, settings UI.

**M11 — Content push (2+ weeks).** Author 4–6 themed waves to ship with v1
(all non-branded names in-repo, flavor via flavorful-but-original labels).

**M12 — Closed playtest, balance, ship.** Pass.

Rough total: **5–7 months of solo nights-and-weekends work** for a full v1.
Shorter if any stage reveals a dead end early; longer if Director mode takes
additional iteration (it probably will).

---

## 9. Open design questions

- **Director Influence numbers:** How much influence per second? What does
  a "small" spawn cost vs. a "huge" hazard? Needs playtesting; start with
  a tuning spreadsheet.
- **How do Directors coordinate?** Shared Influence pool vs. individual?
  Voice chat only vs. a ping/order system? Probably individual + pings.
- **PvP friendly fire during Director possession:** If a Director-possessed
  unit is killed by a surviving player, does the Director pay a cooldown or
  just lose the unit? Start with: lose the unit, short repossess cooldown.
- **Pack asset sync:** In v1 we require identical packs on all clients. Is
  this acceptable, or do we need a first-run sync? Probably v1.1.
- **Terminal minimum size:** Hard-floor at 100x30? Warn + degrade or
  refuse? Needs UX pass.
- **Seed-of-the-day:** Daily shared seed + daily leaderboard? Nice stretch.
- **Solo mode parity:** Should solo be balanced like 2P or distinct? Start
  with: solo is 2P balance minus Director mode (AI Director fills in).

---

## 10. Risks

| Risk                                              | Likelihood | Mitigation                                  |
|---------------------------------------------------|------------|---------------------------------------------|
| Director mode is unbalanceable                    | High       | Playtest early (M7); have AI-Director fallback. |
| Mouse input varies too much across terminals      | Medium     | Detect + fall back to keyboard aim cleanly. |
| Netcode lag compensation feels bad                | Medium     | Start with generous interp buffer; tune down. |
| Content-pack abstraction too rigid or too loose   | Medium     | Author one theme *before* finalizing schema. |
| Scope kills the project                           | High       | Milestones M0–M4 are shippable standalone.  |
| Audio production is a rabbit hole                 | Medium     | Ship with CC0 placeholders; real audio is a trailing track. |
| Terminal sizes fragment rendering                 | Low        | Min-size gate + generous HUD reflow.        |

---

## 11. Glossary

- **Arena:** The procedural map for a run.
- **Director:** A dead player playing the antagonist side.
- **Horde:** All AI-faction entities currently alive.
- **Influence:** Director currency for spawning and abilities.
- **Miniboss:** Named, themed powerful enemy every 5 waves.
- **Pack:** A content bundle of themes, archetypes, audio, and assets.
- **Run:** A single session from spawn to team wipe.
- **Theme:** A tonal/aesthetic wave group (e.g. "Covenant Strike").
- **Turncoat:** Colloquial for a player who transitioned to Director.
- **Wave:** A 90–180s pressure period with a theme.

---

## 12. Legal & IP note

This project is a **homage and parody** intended for private play among
friends. Shipped core content uses original archetype names, silhouettes,
and audio. Crossover "theme" content that leans on identifiable third-party
properties lives in user-authored content packs distributed separately, not
in the shipping repository. This mirrors how the Doom/Quake modding and OpenRA
communities have long operated. If this project ever goes public beyond
friends, a legal pass is required before distributing any brand-adjacent
theme pack, and references in this document are for design vocabulary only.

---

## 13. What this document is *not*

- Not a balance spreadsheet. All numbers are placeholder until playtested.
- Not an API contract. Data model sketch is directional.
- Not a backlog. Milestones are targets, not tickets.
- Not a promise. Scope will change after M1 when we learn what the game
  *actually* feels like.
