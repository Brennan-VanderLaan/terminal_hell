//! Xbox-style gamepad support with haptic rumble.
//!
//! Mirrors the `audio` module pattern: a `OnceLock` holds an optional
//! `GamepadManager`; a dedicated worker thread owns the `gilrs` handle
//! and pumps events / drives rumble. The main loop pulls a lockless
//! snapshot each frame and drains discrete button-press events through
//! an mpsc queue.
//!
//! Every public function is a no-op when the manager failed to start
//! (no controller, no permissions, missing udev, …) so callers don't
//! have to guard — same contract as `audio::emit`.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Repeat, Replay, Ticks};
use gilrs::{Axis, Button, EventType, Gilrs};

/// Absolute deadzone on stick axes. Below this, the stick reads as 0.
/// Xbox controller sticks idle at ~0.02; 0.18 kills drift without
/// eating intent.
const STICK_DEADZONE: f32 = 0.18;
/// Trigger threshold at which RT/LT count as "pressed" for fire / alt-fire.
const TRIGGER_THRESHOLD: f32 = 0.35;

/// Discrete button actions the main loop consumes per frame. Sticks
/// are read from the snapshot; triggers generate both discrete
/// press events (when crossing gilrs's internal threshold) and a
/// continuous axis read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    South,        // A
    East,         // B
    West,         // X
    North,        // Y
    LeftBumper,   // LB
    RightBumper,  // RB
    LeftTrigger,  // LT (analog, edge press)
    RightTrigger, // RT (analog, edge press)
    Start,
    Select,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamepadEvent {
    Press(GamepadButton),
    Release(GamepadButton),
    Connected,
    Disconnected,
}

/// Analog snapshot. Main loop reads this each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct GamepadSnapshot {
    pub connected: bool,
    pub left_stick: (f32, f32),
    pub right_stick: (f32, f32),
    pub lt: f32,
    pub rt: f32,
}

/// Kind of rumble event. The worker translates these into low/high
/// motor magnitudes and durations tuned for terminal-game feel.
#[derive(Clone, Copy, Debug)]
pub enum Rumble {
    /// Weak, brief tap — single weapon shot.
    Fire,
    /// Stronger, longer pulse scaled by damage magnitude (clamped).
    Damage { dmg: i32 },
}

struct Shared {
    snapshot: GamepadSnapshot,
}

struct Manager {
    rumble_tx: Sender<Rumble>,
    event_rx: Mutex<Receiver<GamepadEvent>>,
    shared: Mutex<Shared>,
}

static MANAGER: OnceLock<Manager> = OnceLock::new();

/// Idempotent startup. Spawns the worker thread if it hasn't started
/// and a `Gilrs` context can be constructed. Silent on failure —
/// absence of a controller is the normal case.
pub fn ensure_init() {
    if MANAGER.get().is_some() {
        return;
    }
    write_status("ensure_init: entering Gilrs::new()");
    let gilrs = match Gilrs::new() {
        Ok(g) => {
            write_status("ensure_init: Gilrs::new() succeeded");
            g
        }
        Err(e) => {
            let msg = format!(
                "ensure_init: Gilrs::new() failed: {e:?} — rumble + gamepad input disabled"
            );
            tracing::warn!("{msg}");
            write_status(&msg);
            return;
        }
    };
    // Enumerate whatever gilrs saw at startup so it's on record even
    // if the controller never reaches a Connected event.
    let mut enum_msg = String::from("ensure_init: initial gamepad enumeration:");
    let mut count = 0;
    for (id, gp) in gilrs.gamepads() {
        count += 1;
        enum_msg.push_str(&format!(
            "\n  id={id:?} name={:?} os_name={:?} ff={}",
            gp.name(),
            gp.os_name(),
            gp.is_ff_supported(),
        ));
    }
    if count == 0 {
        enum_msg.push_str(" (none — plug in the controller or check the kernel driver)");
    }
    tracing::info!("{enum_msg}");
    write_status(&enum_msg);

    let (rumble_tx, rumble_rx) = mpsc::channel::<Rumble>();
    let (event_tx, event_rx) = mpsc::channel::<GamepadEvent>();
    let manager = Manager {
        rumble_tx,
        event_rx: Mutex::new(event_rx),
        shared: Mutex::new(Shared { snapshot: GamepadSnapshot::default() }),
    };
    if MANAGER.set(manager).is_err() {
        return;
    }
    thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || worker_loop(gilrs, rumble_rx, event_tx))
        .ok();
    // Self-test: fire a gentle probe rumble shortly after the worker
    // boots. If the player feels a quick buzz at startup, the FF path
    // is healthy end-to-end. If not, the `/tmp/terminal_hell_gamepad.log`
    // lines will tell us whether gilrs refused the effect or silently
    // accepted it.
    thread::Builder::new()
        .name("gamepad-probe".into())
        .spawn(|| {
            thread::sleep(Duration::from_millis(500));
            rumble(Rumble::Fire);
            write_status("probe: queued Rumble::Fire self-test");
        })
        .ok();
}

/// Append one line to a status file on disk. Side-channel diagnostic
/// — doesn't depend on tracing filters, log levels, or stderr
/// redirection, so a user who says "I see no logs" can cat the file
/// and always get a signal about what gilrs reported. Best-effort:
/// silently noop on any fs error.
fn write_status(line: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;
    let path = std::env::temp_dir().join("terminal_hell_gamepad.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{:?}] {line}", std::time::SystemTime::now());
    }
}

/// Most recent analog state. `connected = false` when no controller is
/// attached (or gilrs failed to init).
pub fn snapshot() -> GamepadSnapshot {
    let Some(m) = MANAGER.get() else {
        return GamepadSnapshot::default();
    };
    m.shared.lock().map(|s| s.snapshot).unwrap_or_default()
}

/// Drain all pending button events. Call once per frame.
pub fn drain_events() -> Vec<GamepadEvent> {
    let Some(m) = MANAGER.get() else {
        return Vec::new();
    };
    let Ok(rx) = m.event_rx.lock() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(ev) => out.push(ev),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }
    out
}

/// Queue a rumble event. Best-effort; dropped if the worker isn't up.
pub fn rumble(kind: Rumble) {
    if let Some(m) = MANAGER.get() {
        let _ = m.rumble_tx.send(kind);
    }
}

fn worker_loop(mut gilrs: Gilrs, rumble_rx: Receiver<Rumble>, event_tx: Sender<GamepadEvent>) {
    // Which gamepad we're bound to. Rebuilt whenever a controller
    // connects/disconnects — FF effects are gamepad-scoped.
    let mut active: Option<gilrs::GamepadId> = gilrs.gamepads().next().map(|(id, _)| id);
    // Cached FF support for the active pad. When false, rumble
    // requests short-circuit to a no-op instead of hammering gilrs
    // with effect builds that will all return FfNotSupported. Kept in
    // sync with `active` on every (dis)connect.
    let mut ff_supported = active
        .and_then(|id| gilrs.connected_gamepad(id))
        .map(|gp| gp.is_ff_supported())
        .unwrap_or(false);
    // Linux fallback: when gilrs refuses the pad (its `test_ff`
    // requires the full periodic-waveform bundle, which xpadneo and
    // friends don't advertise), we open the matching evdev node
    // ourselves and talk FF_RUMBLE directly. On other platforms this
    // is a `()` zero-sized placeholder so the compile-time conditional
    // paths stay cheap.
    #[cfg(target_os = "linux")]
    let mut linux_fallback: Option<crate::gamepad_linux_ff::RumbleDevice> =
        maybe_open_linux_fallback(&gilrs, active, ff_supported);
    if let Some(id) = active {
        log_gamepad(&gilrs, id);
        #[cfg(target_os = "linux")]
        log_fallback(&linux_fallback, ff_supported);
        let _ = event_tx.send(GamepadEvent::Connected);
    }

    // Live FF effects that haven't expired yet. Gilrs refcounts effects
    // via the handle; dropping frees the server-side slot, but cancels
    // playback — so we retain handles until their deadline passes.
    let mut retained: Vec<(Instant, Effect)> = Vec::new();

    loop {
        while let Some(ev) = gilrs.next_event() {
            match ev.event {
                EventType::Connected => {
                    if active.is_none() {
                        active = Some(ev.id);
                        ff_supported = gilrs
                            .connected_gamepad(ev.id)
                            .map(|gp| gp.is_ff_supported())
                            .unwrap_or(false);
                        log_gamepad(&gilrs, ev.id);
                        #[cfg(target_os = "linux")]
                        {
                            linux_fallback =
                                maybe_open_linux_fallback(&gilrs, active, ff_supported);
                            log_fallback(&linux_fallback, ff_supported);
                        }
                        let _ = event_tx.send(GamepadEvent::Connected);
                    }
                }
                EventType::Disconnected => {
                    if active == Some(ev.id) {
                        active = gilrs.gamepads().find(|(id, _)| *id != ev.id).map(|(id, _)| id);
                        ff_supported = active
                            .and_then(|id| gilrs.connected_gamepad(id))
                            .map(|gp| gp.is_ff_supported())
                            .unwrap_or(false);
                        #[cfg(target_os = "linux")]
                        {
                            linux_fallback =
                                maybe_open_linux_fallback(&gilrs, active, ff_supported);
                        }
                        retained.clear();
                        let _ = event_tx.send(GamepadEvent::Disconnected);
                    }
                }
                EventType::ButtonPressed(btn, _) => {
                    if let Some(b) = map_button(btn) {
                        let _ = event_tx.send(GamepadEvent::Press(b));
                    }
                }
                EventType::ButtonReleased(btn, _) => {
                    if let Some(b) = map_button(btn) {
                        let _ = event_tx.send(GamepadEvent::Release(b));
                    }
                }
                _ => {}
            }
        }

        let snap = active
            .and_then(|id| gilrs.connected_gamepad(id))
            .map(read_snapshot)
            .unwrap_or_default();
        if let Some(m) = MANAGER.get() {
            if let Ok(mut s) = m.shared.lock() {
                s.snapshot = snap;
            }
        }

        // Drain rumble commands non-blocking. Coalesce so auto-fire
        // can't flood gilrs's effect slots — only the strongest
        // queued effect within one poll tick actually spawns.
        let mut best: Option<Rumble> = None;
        loop {
            match rumble_rx.try_recv() {
                Ok(r) => best = Some(merge_rumble(best, r)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        // Rumble routing:
        //   - gilrs supports FF for this pad → use gilrs's builder
        //     (XInput on Windows, IOKit on macOS, full-FF evdev nodes
        //     on Linux).
        //   - gilrs refuses (common on Linux with xpadneo + similar
        //     rumble-only drivers) → try the direct-evdev fallback.
        //   - Neither works → swallow silently (already logged once
        //     at connect).
        if let (Some(kind), Some(id)) = (best, active) {
            if ff_supported {
                if let Some((effect, deadline)) = spawn_rumble(&mut gilrs, id, kind) {
                    retained.push((deadline, effect));
                }
            } else {
                #[cfg(target_os = "linux")]
                if let Some(dev) = linux_fallback.as_mut() {
                    let (low, high, ms) = rumble_params(kind);
                    match dev.play_rumble(low, high, ms.min(u16::MAX as u32) as u16) {
                        Ok(_) => {
                            tracing::debug!(
                                ?kind, low, high, ms,
                                "rumble: linux fallback played"
                            );
                            write_status(&format!(
                                "rumble: linux fallback played kind={kind:?} \
                                 low={low:#x} high={high:#x} ms={ms}"
                            ));
                        }
                        Err(e) => {
                            tracing::warn!(error = ?e, ?kind, "rumble: linux fallback failed");
                            write_status(&format!(
                                "rumble: linux fallback FAILED kind={kind:?} err={e:?}"
                            ));
                        }
                    }
                }
                #[cfg(not(target_os = "linux"))]
                let _ = (kind, id);
            }
        }

        // Prune expired effects. Dropping the handle stops the
        // (already-ended) effect and releases the slab slot.
        let now = Instant::now();
        retained.retain(|(deadline, _)| *deadline > now);

        // Linux fallback path has its own stop-schedule because
        // xpadneo + similar HID drivers ignore `replay.length` and
        // run the motors until the host explicitly writes `value=0`.
        // `tick` walks the per-effect deadlines and commits the
        // stops that have come due.
        #[cfg(target_os = "linux")]
        if let Some(dev) = linux_fallback.as_mut() {
            dev.tick();
        }

        // ~500 Hz poll — tight enough for crisp FF timing, cheap on CPU.
        thread::sleep(Duration::from_millis(2));
    }
}

fn merge_rumble(a: Option<Rumble>, b: Rumble) -> Rumble {
    match (a, b) {
        (None, x) => x,
        // Damage always outranks Fire; higher-damage wins between damages.
        (Some(Rumble::Damage { dmg: d1 }), Rumble::Damage { dmg: d2 }) => {
            Rumble::Damage { dmg: d1.max(d2) }
        }
        (Some(Rumble::Damage { dmg }), Rumble::Fire) => Rumble::Damage { dmg },
        (Some(Rumble::Fire), Rumble::Damage { dmg }) => Rumble::Damage { dmg },
        (Some(Rumble::Fire), Rumble::Fire) => Rumble::Fire,
    }
}

fn spawn_rumble(
    gilrs: &mut Gilrs,
    id: gilrs::GamepadId,
    kind: Rumble,
) -> Option<(Effect, Instant)> {
    let (low, high, ms) = rumble_params(kind);
    let play_for = Ticks::from_ms(ms);
    let mut builder = EffectBuilder::new();
    if low > 0 {
        builder.add_effect(BaseEffect {
            kind: BaseEffectType::Strong { magnitude: low },
            scheduling: Replay {
                after: Ticks::from_ms(0),
                play_for,
                with_delay: Ticks::from_ms(0),
            },
            ..Default::default()
        });
    }
    if high > 0 {
        builder.add_effect(BaseEffect {
            kind: BaseEffectType::Weak { magnitude: high },
            scheduling: Replay {
                after: Ticks::from_ms(0),
                play_for,
                with_delay: Ticks::from_ms(0),
            },
            ..Default::default()
        });
    }
    let effect = match builder
        .gamepads(&[id])
        .repeat(Repeat::For(play_for))
        .finish(gilrs)
    {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = ?e, ?kind, "rumble: effect build failed");
            write_status(&format!(
                "rumble: effect build FAILED kind={kind:?} err={e:?}"
            ));
            return None;
        }
    };
    match effect.play() {
        Ok(_) => {
            tracing::debug!(?kind, low, high, ms, "rumble: playing");
            write_status(&format!(
                "rumble: playing kind={kind:?} low={low:#x} high={high:#x} ms={ms}"
            ));
        }
        Err(e) => {
            tracing::warn!(error = ?e, ?kind, "rumble: effect play failed");
            write_status(&format!(
                "rumble: effect play FAILED kind={kind:?} err={e:?}"
            ));
            return None;
        }
    }
    Some((effect, Instant::now() + Duration::from_millis(ms as u64 + 20)))
}

/// One-time log at connect: pad name, vendor/product if exposed, FF
/// capability. Helps diagnose "no rumble" reports on Linux where the
/// xpad driver doesn't always advertise FF for every 360-era pad.
fn log_gamepad(gilrs: &Gilrs, id: gilrs::GamepadId) {
    let Some(gp) = gilrs.connected_gamepad(id) else {
        return;
    };
    let ff = gp.is_ff_supported();
    let name = gp.name().to_string();
    let os_name = gp.os_name().to_string();
    if ff {
        tracing::info!(
            gamepad = %name,
            os_name = %os_name,
            "gamepad: connected, force-feedback supported"
        );
    } else {
        tracing::warn!(
            gamepad = %name,
            os_name = %os_name,
            "gamepad: connected, but force-feedback NOT supported — \
             rumble will be silent. On Linux this usually means the \
             driver (xpad) didn't advertise FF for this pad; try \
             xpadneo for Xbox 360/One controllers."
        );
    }
}

/// Convert a `Rumble` into (strong-motor, weak-motor, duration_ms).
/// Magnitudes are u16 (`0x0000..=0xFFFF`). Strong = low-frequency
/// motor (that "thud" feeling); weak = high-frequency motor (the
/// high-pitched buzz / "click").
fn rumble_params(kind: Rumble) -> (u16, u16, u32) {
    match kind {
        Rumble::Fire => {
            // Snappy tap — mostly weak-motor so it reads as a crisp
            // click under the finger rather than a low buzz. Short
            // enough that auto-fire doesn't blur into one long drone.
            (0x2800, 0x8800, 70)
        }
        Rumble::Damage { dmg } => {
            // Scale aggressively. Chip damage (~5hp) still jolts;
            // a 30+hp hit saturates both motors for most of half a
            // second — the "truck just hit me" feeling.
            let d = (dmg.clamp(1, 40) as f32) / 40.0;
            let strong = (0x5000 as f32 + d * (0xFFFF - 0x5000) as f32) as u16;
            let weak = (0x4000 as f32 + d * (0xE000 - 0x4000) as f32) as u16;
            let ms = 180 + (d * 320.0) as u32;
            (strong, weak, ms)
        }
    }
}

fn read_snapshot(gp: gilrs::Gamepad<'_>) -> GamepadSnapshot {
    let lx = deadzone(gp.value(Axis::LeftStickX));
    // Invert Y — gilrs reports up as +1, but our game uses screen-space
    // coordinates (up = -y) for movement/aim.
    let ly = -deadzone(gp.value(Axis::LeftStickY));
    let rx = deadzone(gp.value(Axis::RightStickX));
    let ry = -deadzone(gp.value(Axis::RightStickY));
    let lt = gp.value(Axis::LeftZ).clamp(0.0, 1.0);
    let rt = gp.value(Axis::RightZ).clamp(0.0, 1.0);
    GamepadSnapshot {
        connected: true,
        left_stick: (lx, ly),
        right_stick: (rx, ry),
        lt,
        rt,
    }
}

fn deadzone(v: f32) -> f32 {
    if v.abs() < STICK_DEADZONE {
        0.0
    } else {
        // Rescale so the usable range is still [-1, 1] past the
        // deadzone — otherwise full-push reads as ~0.82 and slow
        // intent feels clipped.
        let sign = v.signum();
        let mag = (v.abs() - STICK_DEADZONE) / (1.0 - STICK_DEADZONE);
        sign * mag.clamp(0.0, 1.0)
    }
}

fn map_button(b: Button) -> Option<GamepadButton> {
    Some(match b {
        Button::South => GamepadButton::South,
        Button::East => GamepadButton::East,
        Button::West => GamepadButton::West,
        Button::North => GamepadButton::North,
        // gilrs names follow xpad: LeftTrigger/RightTrigger are the
        // shoulder bumpers (LB/RB); LeftTrigger2/RightTrigger2 are
        // the analog triggers (LT/RT).
        Button::LeftTrigger => GamepadButton::LeftBumper,
        Button::RightTrigger => GamepadButton::RightBumper,
        Button::LeftTrigger2 => GamepadButton::LeftTrigger,
        Button::RightTrigger2 => GamepadButton::RightTrigger,
        Button::Start => GamepadButton::Start,
        Button::Select => GamepadButton::Select,
        Button::DPadUp => GamepadButton::DpadUp,
        Button::DPadDown => GamepadButton::DpadDown,
        Button::DPadLeft => GamepadButton::DpadLeft,
        Button::DPadRight => GamepadButton::DpadRight,
        _ => return None,
    })
}

/// True when the right trigger is pulled past the fire threshold.
pub fn fire_held(snap: &GamepadSnapshot) -> bool {
    snap.connected && snap.rt >= TRIGGER_THRESHOLD
}

/// True when any analog input on the gamepad is above its activity
/// floor (used to detect "player just picked up the controller"). A
/// higher threshold than the fire-deadzone so a resting hand doesn't
/// steal focus from the mouse.
pub fn is_active(snap: &GamepadSnapshot) -> bool {
    if !snap.connected {
        return false;
    }
    let (lx, ly) = snap.left_stick;
    let (rx, ry) = snap.right_stick;
    lx.abs() > 0.25
        || ly.abs() > 0.25
        || rx.abs() > 0.25
        || ry.abs() > 0.25
        || snap.lt > 0.25
        || snap.rt > 0.25
}

// ===== Linux direct-evdev FF fallback glue =====
//
// When gilrs refuses to expose force-feedback for a pad (its Linux
// capability probe requires the full periodic-waveform set; many
// rumble-only drivers like xpadneo advertise only FF_RUMBLE), we
// open the matching /dev/input/event* node ourselves and drive it
// via the kernel's FF ioctls. See `src/gamepad_linux_ff.rs`. The
// rest of the module stays platform-agnostic; these helpers are
// gated to Linux so Windows + macOS keep using gilrs's native FF
// paths unchanged.

#[cfg(target_os = "linux")]
fn maybe_open_linux_fallback(
    gilrs: &Gilrs,
    active: Option<gilrs::GamepadId>,
    ff_supported: bool,
) -> Option<crate::gamepad_linux_ff::RumbleDevice> {
    if ff_supported {
        return None;
    }
    let id = active?;
    let gp = gilrs.connected_gamepad(id)?;
    // os_name is typically the evdev name ("Xbox Wireless Controller"
    // for xpadneo), which is what we'll match substring-wise. We
    // also try the human-facing name as a fallback.
    let hint = {
        let os = gp.os_name().to_string();
        if !os.is_empty() {
            os
        } else {
            gp.name().to_string()
        }
    };
    crate::gamepad_linux_ff::RumbleDevice::find(&hint)
}

#[cfg(target_os = "linux")]
fn log_fallback(
    dev: &Option<crate::gamepad_linux_ff::RumbleDevice>,
    ff_supported: bool,
) {
    if ff_supported {
        return;
    }
    match dev {
        Some(d) => {
            tracing::info!(
                path = %d.path,
                "gamepad: opened Linux direct-evdev FF fallback — rumble routed there"
            );
            write_status(&format!(
                "fallback: opened direct-evdev FF device at {}",
                d.path
            ));
        }
        None => {
            tracing::warn!(
                "gamepad: gilrs refused FF and no direct-evdev fallback device \
                 found — rumble will be silent. Verify the user has r/w on \
                 /dev/input/event* and that the pad's FF_RUMBLE bit is set."
            );
            write_status("fallback: no direct-evdev FF device found");
        }
    }
}
