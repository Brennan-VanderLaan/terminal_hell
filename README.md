# terminal_hell

A terminal-native top-down multiplayer shooter. Every shooter you have ever
loved is leaking through the tear. Hold the line.

> *v0.2 MVP — shooter + destruction + waves + 2-player LAN, rendered at
> sextant-pixel density with multi-pixel pixel-art sprites. The Yellow Sign
> is coming in later milestones. Full design lives in [`Spec.md`](Spec.md).*

---

## What you need before launching

**Terminal font.** The game renders via Unicode sextants (U+1FB00…U+1FB3B)
and braille dots. Your terminal font must have these glyphs. Known good:

- **Windows:** Windows Terminal with **Cascadia Code** (the default).
- **macOS / Linux:** **WezTerm** (built-in glyphs), **kitty** + any Nerd
  Font, or Alacritty/iTerm2 with **JetBrains Mono Nerd Font**, **Iosevka**,
  **Fira Code Nerd Font**, or **Cascadia Code**.

If you see tofu boxes (□) instead of solid pixels when you launch, switch
fonts. No amount of `cargo run` will fix a missing glyph.

**Font size.** Smaller is better. This is a *game* — you want density.
Drop your terminal font to ~9–11pt and maximize the window. At 9pt on a
1080p display you'll have ~250 × 70 cells, which the game treats as ~500
× 210 pixels with sprites.

**Terminal size.** Minimum 80 × 30 cells. The arena auto-scales to fill the
terminal; bigger = more arena.

## Build

Requires Rust 1.94 or later.

```bash
cargo build --release
```

Binary is at `target/release/terminal_hell[.exe]`.

## Run

### Solo

```bash
cargo run --release -- solo
```

### Host a session

```bash
cargo run --release -- serve                 # default port 4646
cargo run --release -- serve --port 4646
```

The host window is both the server and the host's client — host plays too.

### Join a session

```bash
cargo run --release -- connect 192.168.1.42
cargo run --release -- connect 192.168.1.42:4646
```

Omitting the port uses `4646`. UDP must be open on the host; works fine over
[Tailscale](https://tailscale.com) if you want to play with remote friends
without port forwarding.

## Controls

| Action | Input             |
|--------|-------------------|
| Move   | WASD / arrow keys |
| Aim    | Mouse             |
| Fire   | LMB / Space       |
| Exit   | Esc / q / Ctrl+C  |

## What's rendered

- **Walls** — neon magenta concrete, 3-pixel-thick perimeter, destructible
  interior cover that fragments into glyph-confetti when shot or breached.
- **Floor** — black. The void shows through everywhere the arena hasn't
  built something. Braille dust trails are visible here.
- **Player** — cyan humanoid with head, body, legs, and a yellow-tipped
  barrel that tracks your mouse aim.
- **Rusher** — small red-horned melee creature. Dies fast.
- **Pinkie** — wide armored brute with tusks. Tanky.
- **Miniboss** — large spiked beast every 5th wave. Priority target.
- **Projectiles** — bright 2×2 cores with a sextant smoke trail and finer
  braille dust particles stretching behind.

## Status

MVP-1 per the plan in `Spec.md`. Next up: primitive bus, Carcosa / Yellow
Sign, audio pillar, Director mode. See the milestones section of the spec.
