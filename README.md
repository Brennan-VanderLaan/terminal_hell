# terminal_hell

A terminal-native top-down multiplayer shooter. Every shooter you have ever
loved is leaking through the tear. Hold the line.

> *v0.7 — shooter + destruction + waves + 2-player LAN, rendered at
> sextant-pixel density with multi-pixel pixel-art sprites. Scripted
> benchmark mode for perf work. Audio pipeline + recorder tool wired end
> to end; real samples land as they get produced. Carcosa, Director mode,
> and primitive bus are the next milestones. Full design lives in
> [`Spec.md`](Spec.md); audio design lives in
> [`audio-spec.md`](audio-spec.md).*

---

## Quickstart — playing

```bash
# once per machine
cargo build --release                            # binary at target/release/terminal_hell

# solo
cargo run --release -- solo

# host a LAN session
cargo run --release -- serve                     # default port 4646

# join a session (ip:port or ip; share-code strings also accepted)
cargo run --release -- connect 192.168.1.42
```

UDP on the host port must be open. Works fine over
[Tailscale](https://tailscale.com) for remote friends without port forwarding.

### Terminal setup

The game renders via Unicode sextants (U+1FB00…U+1FB3B) + braille dots.
Your terminal font must have these glyphs or you'll see tofu boxes (□)
instead of sprites.

| platform | terminal | font |
|---|---|---|
| Windows | Windows Terminal | Cascadia Code (default) |
| macOS | WezTerm / iTerm2 / kitty | JetBrains Mono Nerd, Iosevka, Fira Code Nerd |
| Linux | kitty / Alacritty / foot / WezTerm | JetBrains Mono Nerd, Iosevka, Cascadia Code |

Drop your font to ~9–11pt and maximize the window. At 9pt on 1080p
you'll get ~250 × 70 cells — the game treats those as ~500 × 210 pixels.
Minimum useful size is 80 × 30 cells.

### Controls

| action | input |
|---|---|
| move | WASD / arrow keys |
| aim | mouse |
| fire | LMB / space |
| interact | E |
| cycle weapon | Q |
| deploy turret | T |
| traversal verb (dash/blink/…) | F |
| inventory | Tab |
| pause menu | Esc |
| log console | backtick ` |
| perf overlay | F3 |
| spatial-grid overlay | F4 |
| audio debug overlay | F5 |
| exit | Ctrl+C |

---

## Quickstart — dev

### Prerequisites

- **Rust 1.94 or later** (stable).
- **Platform deps** — see [platform setup](#platform-setup) below for
  system packages. The audio stack (`cpal`), the recorder GUI
  (`eframe` + `egui`), and clipboard (`arboard`) all need matching
  `-dev` packages on Linux.

### Build everything

```bash
git clone <repo>
cd terminal_hell
cargo check                    # fastest way to surface missing deps
cargo test --lib               # runs the 19 audio-pipeline tests
cargo build                    # debug builds of both binaries
cargo build --release          # release builds of both binaries
```

Two binaries land in `target/<profile>/`:

- `terminal_hell[.exe]` — the game.
- `th_record[.exe]` — the recorder GUI (dev tool; never shipped to players).

### Running modes

```bash
# solo practice
cargo run -- solo                                        # defaults to --audition off
cargo run -- solo --audition mix                         # play audition-pool samples too

# benchmark scenarios (scripted spawns, telemetry capture)
cargo run -- bench --headless                            # runs the full catalogue; CI-friendly
cargo run -- bench --scenario rusher_500 --watch         # render mode, 60 fps
cargo run -- bench --scenario rusher_50 --watch --loop   # screensaver mode for audio iteration
cargo run -- bench --loop --playlist rusher_50,rusher_500,turret_wall_vs_zerg

# audio coverage report — what's been recorded, what still needs work
cargo run -- audio audit
cargo run -- audio audit --write AUDIO_STATUS.md

# the recorder GUI (separate bin so a GUI crash can't kill a live game)
cargo run --bin th_record
```

All `--loop` + `--audition` combinations rescan the content tree between
iterations, so samples saved by `th_record` land inside a running
`bench --loop` session without restart.

### Audio production workflow

The engine + recorder are designed to run side-by-side so you can hear
new samples in context within seconds of saving them.

```
┌─ term 1 ─────────────────────────────────────┐  ┌─ term 2 ──────────────┐
│ cargo run -- bench \                         │  │ cargo run --bin       │
│     --scenario rusher_50 --watch --loop \    │  │     th_record         │
│     --audition mix                           │  │                       │
│                                              │  │ pick input device →   │
│ game loops forever; notify watcher picks     │  │ pick session type →   │
│ up any new WAV landing under                 │  │ arm / REC / STOP →    │
│ content/core/audio/samples/                  │  │ trim / assign / SAVE  │
└──────────────────────────────────────────────┘  └───────────────────────┘
```

The recorder writes WAVs to the exact paths the engine scans (`audio::paths`
is the single source of truth both bins share). Hot-reload latency is
≤200ms debounced; the next scenario loop hears the new sample.

Full audio design: [`audio-spec.md`](audio-spec.md). Recording briefs
and motif files live under `content/core/audio/`.

### Repo layout

```
src/
  bin/th_record/         # recorder GUI (egui + cpal + hound)
  audio/                 # engine, audit, watcher, params, schema, cv, transitions
  bench/                 # scenarios + runner + scripted spawns
  net/                   # renet client/server
  game.rs, arena.rs, …   # sim
content/core/
  archetypes.toml        # enemy stats
  brands/                # brand TOMLs (17 brands)
  substances/            # substance effects
  audio/                 # buses, motifs, unit briefs, reactions, samples/
Spec.md                  # full game design
audio-spec.md            # full audio pipeline + recorder design
```

### Testing

```bash
cargo test --lib                              # all unit tests
cargo test --lib audio::                      # audio-only (fast)
cargo test --test bench_smoke                 # CI perf regression gates
```

The bench's three CI smoke scenarios assert tick-time thresholds — if
those fail in a PR you've regressed perf, not flakiness.

---

## Platform setup

### Windows

Nothing beyond Rust. `cpal` uses WASAPI, `eframe` uses glow, `arboard`
uses the Win32 clipboard — all bundled with the OS.

### Linux

Install the system deps before `cargo build`:

**Debian / Ubuntu:**

```bash
sudo apt install build-essential pkg-config \
    libasound2-dev \
    libudev-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libwayland-dev \
    libgl1-mesa-dev
```

**Fedora / RHEL:**

```bash
sudo dnf install gcc pkgconf-pkg-config \
    alsa-lib-devel \
    systemd-devel \
    libxcb-devel libxkbcommon-devel wayland-devel \
    mesa-libGL-devel
```

**Arch:**

```bash
sudo pacman -S base-devel pkgconf alsa-lib systemd-libs libxcb libxkbcommon wayland mesa
```

The #1 failure mode is a missing `libasound2-dev` / `alsa-lib-devel` —
produces a cryptic linker error about `-lasound`. If you see that, it's
ALSA.

`libudev-dev` (Debian/Ubuntu) / `systemd-devel` (Fedora/RHEL) /
`systemd-libs` (Arch) is required by `gilrs` for Xbox controller input +
rumble. If it's missing, `libudev-sys` panics during the build with a
pkg-config error pointing at `libudev.pc`.

**Expected Linux quirks:**

- Input device names in `th_record` look like `default` / `pulse` /
  `hw:CARD=ES8,DEV=0` rather than the Windows product name. USB
  class-compliant devices (the ES-8 is one) usually still surface a
  readable name.
- Clipboard copy ("copy connect command" in the menu) works on X11 and
  XWayland. On pure Wayland it silently fails until `arboard` is rebuilt
  with its `wayland-data-control` feature. Low impact — nothing in the
  audio path touches clipboard.
- PipeWire presents as an ALSA device automatically; no extra config
  needed. JACK would require recompiling `cpal` with the `jack` feature.

### macOS

Untested on macOS for v0.7. Should work — all crates support it — but
expect one or two `-sys` crate surprises. If you try it, the deps that
could bite are `cpal` (wants CoreAudio, bundled) and `eframe` (wants
Metal / OpenGL). No extra Homebrew packages should be required.

---

## Status

- **Shipped (v0.7):** M0–M2.5 + benchmark mode (§17.14). Playable solo
  and 2-player LAN. Sextant+braille rendering, procedural sprites,
  tile-chunked destruction, wave scheduler with minibosses, HUD,
  auto-scaling arena, 17 brand TOMLs, substances, gore, audio engine +
  recorder pipeline end-to-end.
- **Design locked, not yet shipped:** primitive bus (M3–M5), Carcosa /
  Yellow Sign system (M7), Director mode (M8), real audio content
  (pipeline ready, samples pending).
- **In active work elsewhere:** new archetypes (Phaser and friends),
  content expansion.

See `Spec.md` milestones for the full roadmap, `audio-spec.md` for the
audio pipeline design, and `audio audit` for live coverage.
