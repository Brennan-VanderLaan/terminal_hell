# Audio Spec — Terminal Hell

**Status:** design target for M9. Not shipped in v0.6. `rodio` / `cpal` are
not yet in the crate stack. This document supersedes Spec.md §13 for the
audio system; the older section remains as the high-level pillar statement.

---

## 1. Principles

- **Placeholder-first.** Core pack ships tiny CC0 samples embedded via
  `include_dir!`. Binary stays small; real samples live outside and load
  at runtime.
- **Self-recorded.** All final samples produced on Eurorack + FL Studio.
  Brand-fidelity audio stays outside the shipping repo per the legal
  posture in Spec.md §17.
- **Motif-driven.** Narrative and shared sonic DNA live in *motif files*
  written once per family. Unit briefs inherit them and describe only
  what is unique to that unit.
- **Filesystem-as-matrix.** The corruption / sanity variant matrix is
  derived from filenames on disk, not enumerated in TOML.
- **Audit-driven production.** `terminal_hell audio audit` is the
  authoritative TODO list. Sessions are grouped by motif so one Eurorack
  patch covers multiple events per sit-down.
- **Three concerns, three files.** Unit TOMLs own samples. `buses.toml`
  owns static mix topology. `reactions.toml` owns live FX modulation.

---

## 2. Storage layout

```
content/core/
  audio/
    buses.toml                       # static mix topology (6 busses + aux)
    reactions.toml                   # live FX modulation rules
    motifs/
      carcosa_glyph_tear.toml
      mil_sim_mechanical.toml
      synthwave_warm.toml
    units/
      rusher.toml
      pinkie.toml
      miniboss.toml
    weapons/
      pulse.toml
      auto.toml
    primitives/
      ignite.toml
    ui/
      pickup.toml
      vote.toml
    carcosa/
      yellow_sign.toml
      thresholds.toml
    music/
      synthwave_base.toml
      carcosa_overlay.toml
    samples/
      units/rusher/spawn_baseline_01.ogg
      weapons/pulse/fire_01.ogg
      music/base/base_A_01.ogg
      ...

  brands/
    doom_inferno.toml                # brand def — unchanged
    doom_inferno/                    # sibling dir, OPTIONAL per brand
      audio/
        units/miniboss.toml          # brand-owned overrides
        samples/units/miniboss/...
        music/wave_theme.toml
```

---

## 3. Override resolution

At runtime, same-keyed paths override in this order (highest wins):

1. `~/.terminal_hell/audio/<path>` — user / modder overrides.
2. `./content/<pack>/audio/<path>` — sibling disk pack (ships alongside
   binary for production audio, since OGG bloat shouldn't ride in the
   embedded core pack).
3. `content/core/audio/<path>` — embedded CC0 placeholders.

The rule applies to **both** briefs and samples, so modders can add
entirely new events, new motifs, and override any existing sound.

---

## 4. Unit TOML schema

Per-unit brief. Under 30 lines typical. Inherits narrative from `motif`.

```toml
# content/core/audio/units/rusher.toml

[unit]
id            = "rusher"
description   = """
Light melee chaser. Comes through small glyph-tears in the arena wall —
a hungry silhouette, not a monster. The sound emerges rather than strikes.
"""
sonic_palette = "thin, papery, wet. pitched under player gunfire. no vocal cords."
motif         = "carcosa.glyph_tear"
bus           = "sfx"

[events.spawn]
description = "Fires once when a Rusher enters the arena."
filter_hint = "body 140–900Hz. leave 1.8–3.2kHz clear for gunfire."
spatial     = true

[events.footstep]
description = "Per-step, while moving. Light, quick."
filter_hint = "dry scuff, no low end, HPF @ 200Hz."
spatial     = true

[events.death]
description = "Glyph-tear collapses back on the thing. Wet, short."
filter_hint = "body + short splash tail, mid-focus."
spatial     = true

[runtime]
pitch_jitter_st = [-1.5, 1.5]
max_concurrent  = 3
```

### Fields

**`[unit]`**
- `id` — must match an in-code entity identifier.
- `description` — what this unit IS (fiction + role).
- `sonic_palette` — one-line summary of what it sounds like.
- `motif` — motif file id; inherits shared DNA + narrative.
- `bus` — default bus for all this unit's events (override per-event if needed).

**`[events.<name>]`**
- `description` — when and where it fires.
- `filter_hint` — producer-facing frequency guidance.
- `spatial` — 2D-positioned or global.
- `bus` — optional override of unit-level bus.

**`[runtime]`**
- `pitch_jitter_st` — random pitch shift range in semitones.
- `gain_jitter_db` — optional gain jitter.
- `max_concurrent` — polyphony cap for this unit.

---

## 5. Motif files

Written once per sonic family. Carries the narrative that would
otherwise duplicate across every unit.

```toml
# content/core/audio/motifs/carcosa_glyph_tear.toml

description = """
Something stepping through a glyph-tear in reality. Short, wet, thin.
Not a monster — a hungry silhouette. The sound emerges, never strikes.
"""
emotion     = "gut-drop, involuntary swivel, low-stakes dread"
antipattern = "no screams, roars, snarls, vibrato. no vocal-cord content."
shared_layers = [
  "pitched-down exhale (same source, pitched per unit)",
  "sub-flutter 40–70Hz — the tear opening",
]
```

`shared_layers` are recorded once and sit under every unit in the family.
Recording the motif layers is the first audit-surfaced task before any
sibling unit gets marked complete.

---

## 6. Variant matrix — filesystem convention

The corruption / sanity matrix is **not** enumerated in TOML. It lives
on disk. File naming:

```
samples/<category>/<id>/<event>_<variant>_<take>.<ext>
```

Variants:

- `baseline` — required. The default sound.
- `p25`, `p50`, `p75`, `p100` — optional Carcosa corruption-threshold variants.
- `phantom` — optional sanity-low hallucination variant.

Examples:

```
samples/units/rusher/
  spawn_baseline_01.ogg
  spawn_baseline_02.ogg
  spawn_p50_01.ogg
  spawn_p100_01.ogg
  spawn_phantom_01.ogg
  footstep_baseline_01.ogg
  death_baseline_01.ogg
```

At playback time, the engine:

1. Reads current `corruption %` — picks the highest `pN` bucket ≤ that
   value. Falls back to `baseline` if no `pN` sample exists.
2. Reads player sanity. If sanity < 30 AND the phantom variant has
   samples AND the phantom-trigger fires, plays from `phantom_*`.
3. Otherwise plays from `baseline_*` / the matching `pN` pool.
4. Round-robins within the resolved pool, skipping last-played.
5. Applies `pitch_jitter_st` and gain jitter.

Missing variants are **not** errors. They are TODO slots surfaced by
the audit tool.

---

## 7. Audit tool

### Command

```
terminal_hell audio audit [--write AUDIO_STATUS.md]
```

Walks every unit / weapon / primitive / ui / carcosa / music TOML,
cross-references declared events against `samples/` tree, reports.

### Mocked output

```
rusher    [3/18 slots filled]
  spawn         baseline  ✓ 2 takes
                p25       — missing
                p50       ✓ 1 take (want ≥3)
                p75       — missing
                p100      — missing
                phantom   — missing
  footstep      baseline  ✓ 1 take (want ≥3)
                p50+      — missing (all)
                phantom   — missing
  death         baseline  — missing     [BLOCKS m9]

motif: carcosa.glyph_tear   [shared_layers not yet recorded]
  ➜ record motif layers first; they unlock all sibling units

suggested next session (groups events that share your patch):
  - motif.carcosa.glyph_tear  (shared exhale + sub-flutter)
  - rusher.spawn.baseline     (extend to 3+ takes)
  - rusher.footstep.baseline  (extend to 3+ takes)
  - rusher.death.baseline     (m9 blocker)
```

### Grouping logic

Suggested-next-session lists events grouped by their parent motif so
one Eurorack patch covers multiple events. Motif-layer recording is
always surfaced first — no sibling unit is considered "done" until its
motif layers exist.

### In-game audit

`/audio audit` in the console (see `console.rs`) runs the same report
against the loaded content tree without quitting the game. Useful when
iterating samples with hot-reload on.

### Slot counting

For each event in a unit TOML, the audit tool counts 6 slots:
`baseline`, `p25`, `p50`, `p75`, `p100`, `phantom`. A slot is "filled"
if ≥1 matching file exists. `baseline` is the only mandatory slot;
missing `pN` / `phantom` slots are reported but do not block ship.

---

## 8. Buses — static topology

`buses.toml` defines the mix graph. Authored once; rarely touched.
Six busses ship:

| bus              | purpose                                              |
|------------------|------------------------------------------------------|
| `music`          | dual-stem synthwave base + per-brand wave themes     |
| `carcosa_overlay`| Corruption-weighted dissonance stem                  |
| `sfx`            | diegetic gameplay sounds (units, weapons, primitives)|
| `vox`            | King's voice, unit vox (deferred post-M9)            |
| `ambient`        | room tone, wind, Carcosa sub-drone                   |
| `ui`             | menu / pickup / vote / toast — non-diegetic          |

```toml
# content/core/audio/buses.toml

[bus.sfx]
gain_db = -3
chain = [
  { id = "lpf",  type = "lowpass",    cutoff_hz = 20000, resonance = 0.15 },
  { id = "peak", type = "bell",       hz = 2500, q = 0.8, gain_db = 1.5 },
  { id = "comp", type = "compressor", threshold_db = -12, ratio = 3 },
  { id = "verb", type = "send",       target = "reverb_small", amount = 0.15 },
]

[bus.music]
gain_db = -6
chain = [
  { id = "lpf",  type = "lowpass",    cutoff_hz = 20000, resonance = 0.0 },
  { id = "duck", type = "compressor", threshold_db = -18, ratio = 2 },
]

[bus.ui]
gain_db = -6
chain = [
  { id = "hpf",  type = "highpass", cutoff_hz = 120 },
]
ducks = "music:-3db:120ms"

[bus.carcosa_overlay]
gain_db = "corruption_curve"           # dynamic; driven by Corruption %
chain = [
  { id = "lpf",  type = "lowpass", cutoff_hz = 8000 },
  { id = "verb", type = "send",    target = "reverb_hall", amount = 0.35 },
]

# Aux reverb busses (send targets, not sources)
[bus.reverb_small]
type = "reverb"
size = 0.25
decay_s = 0.6
damping = 0.6
pre_delay_ms = 8

[bus.reverb_hall]
type = "reverb"
size = 0.85
decay_s = 2.4
damping = 0.2
pre_delay_ms = 40
```

Each effect node has an `id` so reactions can target individual
parameters at runtime (see §9).

---

## 9. Reactions — live FX modulation

`reactions.toml` declares how game events and game state modulate bus
effect parameters. Three modes:

- **one-shot** — fires on a gameplay event, plays an envelope, releases.
- **continuous** — parameter follows a game input in real time.
- **state** — persists while a condition holds; released when it clears.

```toml
# content/core/audio/reactions.toml

# one-shot: explosion ducks the mix for 120ms then releases over 600ms
[explosion_nearby]
trigger = "event:explosion"
actions = [
  { bus="sfx",   effect="lpf", param="cutoff_hz", to=800,  hold_ms=120, release_ms=600, curve="exp" },
  { bus="music", effect="lpf", param="cutoff_hz", to=1200, hold_ms=200, release_ms=800, curve="exp" },
]

# continuous: proximity to the Hastur Daemon pumps sfx-bus resonance
[near_daemon]
trigger = "proximity:hastur_daemon"
mode    = "continuous"
input   = { name = "distance", clamp = [2.0, 20.0] }
actions = [
  { bus="sfx", effect="lpf", param="resonance", map = "distance: 20→0.15, 2→0.85" },
  { bus="sfx", effect="lpf", param="cutoff_hz", map = "distance: 20→20000, 2→900"  },
]

# state-bound: holds while sanity is low
[sanity_low]
trigger = "state:sanity<30"
mode    = "continuous"
actions = [
  { bus="sfx",   effect="lpf",  param="cutoff_hz", to=4000 },
  { bus="music", effect="peak", param="gain_db",   to=-4   },
]

# one-shot: Hastur possession event
[possession]
trigger = "event:hastur_possession"
actions = [
  { bus="music", effect="lpf", param="cutoff_hz", to=600, hold_ms=800, release_ms=1500 },
]
```

### Engine API

Game code fires reactions through a narrow surface:

```rust
// one-shot, optionally at a world position
audio::reaction("explosion_nearby", Some(world_pos));

// continuous, with a live input value
audio::continuous("near_daemon", "distance", dist_to_daemon);

// state-bound (the audio system watches game state via a predicate
// registered at startup; no per-tick call needed)
```

Reactions compose. Multiple active reactions on the same (bus, effect,
param) combine by the rule declared in the reaction (default: last
writer wins; `combine = "min"` / `"max"` / `"sum"` overrides).

---

## 10. Brand overrides

Brands may ship their own audio folder as a sibling of their brand TOML.
The engine resolves per-event samples in this order (highest wins):

1. Active brand's `audio/<category>/<id>.toml` + samples.
2. Core pack's `audio/<category>/<id>.toml` + samples.

Brands may only override fields whitelisted by the core unit TOML
(typically `samples` + `pitch_jitter_st`). Motif and family remain
fixed — brand overrides preserve sonic identity while permitting
brand-flavored timbre.

### Fallback chain

When a brand's override references a fallback:

```toml
# content/core/brands/doom_inferno/audio/units/miniboss.toml
[events.spawn]
fallback = "miniboss.generic_heavy"    # named event in core pack
```

The engine walks the chain until samples resolve. Each hop is logged in
the audit report so fallbacks are debuggable.

---

## 11. Music — bar-synced chunks

Matches the solo producer's workflow (Oxi One Mk2 sequencing bar-length
chunks at a fixed tempo). Every music event declares tempo + bar grid;
the engine stitches chunks on bar boundaries and crossfades stems by
bar rather than by wall-clock.

```toml
# content/core/audio/music/synthwave_base.toml

[base_loop]
bpm         = 96
bars        = 8
swing       = 0.0
chunks      = [
  "samples/music/base/base_A_*.ogg",
  "samples/music/base/base_B_*.ogg",
]
sequence    = "ABAB"                   # or "random" / "markov"
bus         = "music"

[carcosa_overlay]
bpm         = 96                       # MUST match base — engine validates
bars        = 8
chunks      = ["samples/music/carcosa/overlay_*.ogg"]
bus         = "carcosa_overlay"
# gain driven by the bus's corruption_curve; not set here
```

Engine responsibilities:

- Sample-accurate bar-boundary scheduling via a look-ahead scheduler.
- ≤ 1-bar crossover when switching sequences (e.g. brand theme entering
  on wave start).
- Tempo lock across every stem on the music bus. Mismatches are logged
  loudly and the offending stem is muted.
- Pre-roll a chunk's first 50ms through the bus EQ/reverb tails so
  transients do not clip on the join.

---

## 12. Deferred

Out of M9 scope, in spec for later:

- **King's voice.** No stub, no placeholder. Corruption-threshold events
  fire without vox until post-M9.
- **Per-event narrative fields** (emotion, antipattern, teaches) — lives
  in motif files instead.
- **Explicit session_target field** — replaced by motif-grouped audit
  suggestions.
- **Dynamics / LUFS budgets per event** — rolled into bus compressor +
  limiter; authors conform by mastering into the bus chain.
- **Audio hallucination line-by-line tells catalogue** — surfaces when
  M7 Carcosa polish begins.

---

## 13. M9 implementation checklist (engine)

- [ ] Add `rodio`, `cpal`, `hound`, `arc-swap`, `notify` to Cargo.toml.
- [ ] `src/audio/` module: engine init, bus graph, effect nodes,
      reaction scheduler, sample pool + round-robin, 2D spatial pan +
      attenuation, bar-aligned music scheduler.
- [ ] Schema loaders: buses.toml, reactions.toml, motifs/, units/,
      weapons/, primitives/, music/.
- [ ] Content-pack external loader (`~/.terminal_hell/audio/` +
      `./content/<pack>/audio/`) with override resolution per §3.
- [ ] Sample-pool hot-reload: `notify` watcher + `ArcSwap<Vec<Sample>>`
      atomic swap so in-flight voices finish on the old pool.
- [ ] `terminal_hell audio audit [--write AUDIO_STATUS.md]` CLI.
- [ ] `/audio ...` console commands: `audit`, `play <ref>`,
      `loop <ref> <duration>`, `solo <bus>`, `mute <bus>`, `ab`, `dump`.
- [ ] `--audition off|mix|only` flag on `solo` + `bench`. Default off;
      must be explicit to load audition-pool samples.
- [ ] `bench --loop` + `bench --playlist <csv>` — lets the recorder
      workflow in §14 run a continuous screensaver soundstage.
- [ ] Wire gameplay emitters: unit spawn / death / footstep / hit,
      weapon fire, primitive tells, pickup, vote, Carcosa threshold
      crossings, possession events, proximity-to-daemon continuous,
      sanity-low state, explosion nearby event.
- [ ] Ship CC0 placeholder set in core pack for every required
      baseline slot so `audio audit` starts with a real floor.

---

## 14. Recorder utility — `th_record`

Dev tool, separate binary (`src/bin/th_record.rs`), never shipped to
end users. Purpose: capture raw audio from the user's Eurorack via an
audio interface, slice it visually, assign it to a slot or an audition
pool, and iterate inside a live `bench --loop` session that picks up
the new file on hot-reload.

### 14.1 Architecture

- `egui` via `eframe` for the GUI window.
- `cpal` for input capture — same crate the engine uses for output.
- `hound` for WAV write — lossless, pure-Rust, zero encoder complexity.
- Shares the `terminal_hell` lib crate: audit queue, content schemas,
  filesystem naming.
- Separate bin = separate crash domain. A recorder bug cannot kill the
  running game session.

### 14.2 Capture format

- **WAV only for now.** Raw captures ship uncompressed. Disk cost is
  accepted until the compression step lands post-M10.
- The engine accepts both WAV and (future) OGG transparently — the
  pool loader sniffs the extension.
- Device-native sample rate and bit depth; user selects both in the
  recorder UI. Engine resamples at load time if needed.

### 14.3 Multichannel workflow (ES-8)

- Recorder arms **all channels the device exposes** on every take
  (typically 4 from an ES-8). One recording pass captures up to four
  candidate sounds in parallel.
- Per-channel assignment happens **after** the take, inside the GUI:
  route channel N to a named slot, route to the audition pool, or
  discard.
- Mono-vs-stereo is a per-destination decision — two channels can be
  grouped into one stereo sample at assignment time; unpaired channels
  print mono. Only mono or stereo samples are written to disk today.

### 14.4 Audition pools

Alongside locked samples, each slot may carry an `_audition/`
subfolder holding candidate takes produced by the recorder. Engine
behavior is controlled by an **explicit** CLI flag on `solo` / `bench`:

- `--audition off` (default) — locked samples only. What players hear
  in a real run. CI and release play under this mode.
- `--audition mix` — engine draws from locked + audition pools
  round-robin. A/B candidates against committed takes.
- `--audition only` — audition pool only. Soak candidates in isolation.

The flag is explicit so a release or benchmarking session never
silently plays in-progress takes.

### 14.5 Audition lifecycle

From inside the recorder GUI:
- **Promote** — move file from `_audition/` to the slot directory,
  rename to the next free `<event>_<variant>_<NN>.wav`.
- **Discard** — delete the audition file.
- **Keep auditioning** — leave it in the pool; game picks it up under
  `--audition mix` / `only`.

The audit tool reports audition counts alongside locked counts:

```
rusher.spawn.baseline  locked: 2 · audition: 3 candidates
```

### 14.6 Hot-reload

The engine watches `content/**/audio/samples/` via `notify`. On any
file change the affected event's sample pool is rebuilt and atomically
swapped via `ArcSwap<Vec<SampleRef>>`. Voices already playing finish
on their old samples; new triggers pick the new pool. Sub-second
latency, no glitches.

### 14.7 UI sketch

```
┌─ Terminal Hell — Audio Recorder ────────────────────────────┐
│ input: ES-8 · 48000Hz 24-bit · 4 channels armed              │
│ ┌ meters ─────────────┐  ┌ queue (from audit) ────────────┐ │
│ │ 1 ▮▮▮▮▮▮▮▮░ -12dB  │  │ NEXT  motif.glyph_tear.exhale  │ │
│ │ 2 ▮▮▮▮▮▮░░░ -18dB  │  │        (11 more)               │ │
│ │ 3 ▮▮▮▮▮▮▮▮▮ -6dB   │  └────────────────────────────────┘ │
│ │ 4 ▮▮░░░░░░░ -36dB  │  brief: "something stepping through │
│ └─────────────────────┘  a glyph-tear… no vocal cords."    │
│                                                             │
│ [● REC]  [■ STOP]  [▶ PLAY]                                 │
│                                                             │
│ waveform ch1:  ─▬▬▁▄▇█▇▅▃▁──  [trim_in]───[trim_out]        │
│ waveform ch2:  ──▁▃▅▇█▇▅▃▁─   [trim_in]───[trim_out]        │
│ waveform ch3:  collapsed                                    │
│ waveform ch4:  collapsed                                    │
│                                                             │
│ assign:                                                     │
│   ch1 → ○ slot [__________]  ● audition   ○ discard         │
│   ch2 → ● slot [rusher.spawn.baseline]                      │
│   ch3 → ○ slot               ○ audition   ● discard         │
│   ch4 → ○ slot               ● audition   ○ discard         │
│                                         [SAVE ALL]          │
│                                                             │
│ current pool: rusher.spawn.baseline                         │
│   LOCKED:   take_01.wav ▶ 🗑    take_02.wav ▶ 🗑            │
│   AUDITION: _a_01.wav ▶ ⇧ 🗑    _a_02.wav ▶ ⇧ 🗑            │
└─────────────────────────────────────────────────────────────┘
```

### 14.8 Deferred (post-M10)

- **OGG compression step.** Batch re-encode WAV → OGG to reclaim disk.
- Automatic silence-trim / peak-limit during save.
- Bus-preview monitor (route live input through the engine bus graph).
- Batch take detection (silence-split one pass into N takes).
- Recorder → game IPC (filesystem watcher is enough for now).

---

## 15. M10 implementation checklist (recorder)

- [ ] New bin target `src/bin/th_record.rs`; eframe/egui skeleton.
- [ ] `cpal` input capture with user-selectable device / rate / depth.
- [ ] Record all available input channels in parallel; expose them in
      the GUI for post-hoc assignment.
- [ ] Per-channel waveform renderer + drag-handle trim-in/trim-out.
- [ ] Per-channel assignment: slot picker (drawn from audit queue),
      audition pool, or discard.
- [ ] `hound` WAV writer; files land at the exact paths §2 / §6
      define so the engine picks them up on the next reload.
- [ ] Current-slot pool viewer: list locked + audition takes, play
      buttons, promote / discard actions.
- [ ] Stereo pairing at assign time — two channels → one stereo WAV.
- [ ] Session log — every take + its destination / discard decision
      written to `~/.terminal_hell/recorder/sessions/<stamp>.log`.
