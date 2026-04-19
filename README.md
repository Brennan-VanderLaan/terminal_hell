# terminal_hell

A terminal-native top-down multiplayer shooter. Every shooter you have ever
loved is leaking through the tear. Hold the line.

> *v0.1 MVP — shooter + destruction + waves + 2-player LAN. The Yellow Sign is
> coming in later milestones. Full design lives in [`Spec.md`](Spec.md).*

---

## What's in this build

- Solo and multiplayer modes over UDP (`renet`).
- WASD movement, mouse aim, LMB fire (or `Space`).
- Tile-chunked destructible walls with glyph-particle debris.
- Three enemy archetypes: Rusher, Pinkie, Miniboss (every 5 waves).
- Endless wave scheduler with wave banners and HUD.
- Host-authoritative simulation, 20 Hz snapshots, reliable tile deltas.
- Run ends when the last survivor dies.

## Build

Requires a recent Rust (1.94 tested). Clone, then:

```bash
cargo build --release
```

The release binary lands at `target/release/terminal_hell[.exe]`.

## Run

### Solo

```bash
cargo run --release -- solo
```

### Host a session

```bash
cargo run --release -- serve            # default port 4646
cargo run --release -- serve --port 4646
```

Leave that window running; it is both the server and the host's client.

### Join a session

On the other machine (same LAN):

```bash
cargo run --release -- connect 192.168.1.42
cargo run --release -- connect 192.168.1.42:4646
```

Omitting the port uses the default. Firewall / NAT: ensure UDP `4646` is
open on the host. Works fine over [Tailscale](https://tailscale.com) if
you want to play with friends outside your LAN without port forwarding.

## Controls

| Action       | Input             |
|--------------|-------------------|
| Move         | WASD / arrow keys |
| Aim          | Mouse             |
| Fire         | LMB or Space      |
| Exit         | Esc / q / Ctrl+C  |

The host and any connected clients all play as survivors. Dying ends your run
— multiplayer-turncoat "Director" mode, class pickups, primitives, and Carcosa
come in later milestones.

## Requirements

- Modern terminal with mouse support and truecolor. Windows Terminal,
  WezTerm, kitty, iTerm2, Alacritty all work.
- Terminal size: at least 80 × 30. Bigger is better.
- On Unix terminals without the kitty keyboard protocol, held-key movement
  falls back to a short decay timer, which may feel slightly chunky.
  Windows Terminal reports press + release natively — smoothest experience.

## Status

MVP-1 per the plan in `Spec.md`. Next up: primitive bus, Carcosa/Yellow
Sign, audio pillar, Director mode. See the milestones section of the spec.
