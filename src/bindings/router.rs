//! Central translation layer: raw crossterm + gamepad events in,
//! [`InputEvent`]s out. Every entry point (solo, host, client) feeds
//! the same router with whatever came off its terminal / gamepad
//! thread and then consumes the emitted event stream uniformly.
//!
//! The router also owns held-state for movement + fire (with the same
//! decay fallback the old `input::Input` struct had, so terminals that
//! don't report key-release still feel responsive).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};

use crate::audio::AudioOverlay;
use crate::game::Game;
use crate::gamepad::{self, GamepadButton, GamepadEvent, GamepadSnapshot};
use crate::menu::MenuAction;

use super::{Action, Bindings, GamepadBinding, Stick, StickDir};

/// Stick magnitude past which the wizard accepts a direction as the
/// player's intended capture. Higher than runtime movement deadzone
/// so a gentle drift doesn't accidentally bind to a stick.
const CAPTURE_STICK_THRESHOLD: f32 = 0.65;

/// Short decay window that keeps movement feeling fluid on legacy
/// terminals that swallow release events. Mirrors the old behaviour
/// in the hand-rolled `input::Input` struct.
const HELD_DECAY: Duration = Duration::from_millis(120);

/// Events the router emits. Each frame, entry points drain these
/// from the router and dispatch them per their context (local call,
/// network packet, menu nav, …).
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// An action binding's key/button went down. Consumers typically
    /// act on this for edge-press verbs (Interact, CycleWeapon, menu
    /// nav, overlay toggles).
    ActionPress(Action),
    /// An action binding's key/button went up. Mostly informational —
    /// held-style actions update their held-state automatically.
    ActionRelease(Action),
    /// Hardcoded: ESC (or gamepad Start) toggles the in-game menu.
    /// Not bindable.
    MenuToggle,
    /// Hardcoded: Ctrl+C quits. Not bindable.
    Quit,
    /// Terminal mouse event (cursor move, click, scroll). Pass-through
    /// since mouse handling is context-specific.
    Mouse(MouseEvent),
    /// Terminal resize; needs to reach whichever framebuffer owns
    /// viewport state.
    Resize(u16, u16),
    /// Raw capture for the rebind wizard — only emitted while the
    /// router is in `capture_mode`. In that mode, ActionPress/Release
    /// for that device are suppressed so the wizard can see the raw
    /// press before it gets filed against a binding.
    RawKeyboardCapture(KeyCode),
    /// Wizard capture for gamepad input — button, trigger, or stick
    /// direction. The wizard treats all three as "captures".
    RawGamepadCapture(GamepadBinding),
}

/// Which device the wizard is asking the player to press a key on.
/// `None` means normal dispatch.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    Keyboard,
    Gamepad,
}

/// Per-action held state. Populated on ActionPress from either device,
/// cleared on ActionRelease. Movement uses the `is_held` read; the
/// sim layer hashes it into a move vector.
#[derive(Default)]
struct HeldState {
    held: HashSet<Action>,
    /// Monotonic last-press timestamp per action — drives the decay
    /// fallback for terminals that never emit release events.
    last_press: std::collections::HashMap<Action, Instant>,
}

impl HeldState {
    fn press(&mut self, a: Action) {
        self.held.insert(a);
        self.last_press.insert(a, Instant::now());
    }
    fn release(&mut self, a: Action) {
        self.held.remove(&a);
        self.last_press.remove(&a);
    }
    fn is_held(&self, a: Action) -> bool {
        if self.held.contains(&a) {
            return true;
        }
        // Decay fallback: only applied to movement (not Fire) so a
        // stuck trigger doesn't keep shooting forever on a terminal
        // that swallows releases.
        if matches!(
            a,
            Action::MoveUp | Action::MoveDown | Action::MoveLeft | Action::MoveRight
        ) {
            if let Some(t) = self.last_press.get(&a) {
                return t.elapsed() < HELD_DECAY;
            }
        }
        false
    }
}

/// Which kind of device most-recently sent input. Drives the
/// UI-glyph swap (mouse/keyboard vs. controller icons) and which
/// source the sim reads aim from each frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LastInput {
    MouseKeyboard,
    Gamepad,
}

/// Owns the bindings table, held-action state, and the event queue.
/// One router per entry point.
pub struct InputRouter {
    pub bindings: Bindings,
    held: HeldState,
    /// Set of actions currently driven "held" by stick position.
    /// Recomputed from the analog snapshot each frame; unioned with
    /// `held` when `is_held` is queried. Kept separate so
    /// [`observe_gamepad_analog`] can clear them cleanly without
    /// interfering with button-driven held state.
    stick_held: HashSet<Action>,
    events: Vec<InputEvent>,
    capture: Option<CaptureMode>,
    /// Last device that fired a non-hardcoded event. Used by the
    /// entry points to flip `Game::input_mode` without duplicating
    /// the detection heuristic. Defaults to mouse/keyboard.
    last_input: LastInput,
    /// Track whether a stick was already past the capture threshold
    /// last frame, so we only emit one RawGamepadCapture per push
    /// (edge-detection on the stick).
    capture_sticks_latched: [[bool; 4]; 2],
}

impl InputRouter {
    /// Build a router from a pre-loaded [`Bindings`] table.
    pub fn new(bindings: Bindings) -> Self {
        Self {
            bindings,
            held: HeldState::default(),
            stick_held: HashSet::new(),
            events: Vec::new(),
            capture: None,
            last_input: LastInput::MouseKeyboard,
            capture_sticks_latched: [[false; 4]; 2],
        }
    }

    pub fn last_input(&self) -> LastInput {
        self.last_input
    }

    /// Replace the binding table — called by the wizard after a save.
    pub fn set_bindings(&mut self, bindings: Bindings) {
        self.bindings = bindings;
        // Held state references actions, not keys; a rebind doesn't
        // need to clear it, but pressing a key that was formerly
        // bound will still "release" the old action on next key
        // release. That's the right semantics.
    }

    /// Enter / leave wizard capture mode. In capture mode, raw key
    /// and gamepad events emit `RawKeyboardCapture` / `RawGamepadCapture`
    /// *instead* of firing their normal ActionPress, so the wizard
    /// can record the press without also triggering the old binding.
    pub fn set_capture(&mut self, mode: Option<CaptureMode>) {
        self.capture = mode;
    }

    /// True when the wizard is actively capturing the given device.
    pub fn is_capturing(&self, mode: CaptureMode) -> bool {
        self.capture == Some(mode)
    }

    pub fn is_held(&self, action: Action) -> bool {
        self.held.is_held(action) || self.stick_held.contains(&action)
    }

    /// Compute the canonical [-1,1] movement vector from currently-held
    /// movement actions. Used by every entry point that wants
    /// keyboard/digital movement.
    pub fn move_vec(&self) -> (f32, f32) {
        let mut dx = 0.0_f32;
        let mut dy = 0.0_f32;
        if self.is_held(Action::MoveLeft) {
            dx -= 1.0;
        }
        if self.is_held(Action::MoveRight) {
            dx += 1.0;
        }
        if self.is_held(Action::MoveUp) {
            dy -= 1.0;
        }
        if self.is_held(Action::MoveDown) {
            dy += 1.0;
        }
        let len2 = dx * dx + dy * dy;
        if len2 > 0.0 {
            let inv = len2.sqrt().recip();
            dx *= inv;
            dy *= inv;
        }
        (dx, dy)
    }

    /// True if the configured Fire action is held. Callers OR this
    /// with any analog-trigger / mouse-button check they already do.
    pub fn firing(&self) -> bool {
        self.is_held(Action::Fire)
    }

    /// Feed one crossterm event. Emits InputEvents into the internal
    /// queue; caller drains with [`drain`] once per frame.
    pub fn push_crossterm(&mut self, ev: CtEvent) {
        if matches!(ev, CtEvent::Key(_)) {
            self.last_input = LastInput::MouseKeyboard;
        }
        if let CtEvent::Mouse(m) = &ev {
            if mouse_is_intentional(m) {
                self.last_input = LastInput::MouseKeyboard;
            }
        }
        match ev {
            CtEvent::Key(k) => {
                let is_press = matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat);
                let is_release = matches!(k.kind, KeyEventKind::Release);
                let is_first_press = matches!(k.kind, KeyEventKind::Press);
                // Ctrl+C is the one escape hatch that trumps every
                // other path — a misguided rebind can never trap the
                // player.
                if k.code == KeyCode::Char('c')
                    && is_press
                    && k.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.events.push(InputEvent::Quit);
                    return;
                }
                // In ANY capture mode, keyboard events route to the
                // wizard — it owns Esc (cancel), Space/Enter (skip),
                // Tab (add-another), Backspace (clear), and every
                // other key as a captured binding. Routing Esc this
                // way instead of to MenuToggle means the wizard gets
                // to control its own cancel semantics without racing
                // a stray menu toggle.
                if self.capture.is_some() {
                    if is_first_press {
                        self.events.push(InputEvent::RawKeyboardCapture(k.code));
                    }
                    return;
                }
                // Outside capture, Esc is the hardcoded menu toggle.
                if k.code == KeyCode::Esc && is_first_press {
                    self.events.push(InputEvent::MenuToggle);
                    return;
                }
                // Normal binding dispatch. One key can fire multiple
                // actions at once (e.g. `Up` → MoveUp + MenuUp), so we
                // iterate every match and press/release all of them.
                // The dispatch layer filters by UI state.
                let actions = self.bindings.actions_for_key(k.code);
                for action in actions {
                    if is_press {
                        self.held.press(action);
                        if is_first_press {
                            self.events.push(InputEvent::ActionPress(action));
                        }
                    } else if is_release {
                        self.held.release(action);
                        self.events.push(InputEvent::ActionRelease(action));
                    }
                }
            }
            CtEvent::Mouse(m) => {
                self.events.push(InputEvent::Mouse(m));
            }
            CtEvent::Resize(w, h) => {
                self.events.push(InputEvent::Resize(w, h));
            }
            _ => {}
        }
    }

    /// Feed one gamepad button event. Stick axes are read per-frame
    /// via [`observe_gamepad_analog`] and don't go through here.
    pub fn push_gamepad(&mut self, ev: GamepadEvent) {
        if matches!(ev, GamepadEvent::Press(_) | GamepadEvent::Release(_)) {
            self.last_input = LastInput::Gamepad;
        }
        match ev {
            GamepadEvent::Press(btn) => {
                // Gamepad capture eats every button — including Start.
                // The wizard treats Start as cancel, so routing it
                // through the capture path (instead of hardcoding
                // MenuToggle) keeps the wizard in control and can't
                // double-fire a menu open behind the overlay.
                if self.capture == Some(CaptureMode::Gamepad) {
                    self.events
                        .push(InputEvent::RawGamepadCapture(GamepadBinding::Button(btn)));
                    return;
                }
                // Keyboard wizard open — swallow gamepad presses so
                // they can't fire gameplay verbs behind the overlay.
                if self.capture == Some(CaptureMode::Keyboard) {
                    return;
                }
                // Outside capture, Start is the hardcoded menu toggle.
                if btn == GamepadButton::Start {
                    self.events.push(InputEvent::MenuToggle);
                    return;
                }
                for action in self.bindings.actions_for_gamepad_button(btn) {
                    self.held.press(action);
                    self.events.push(InputEvent::ActionPress(action));
                }
            }
            GamepadEvent::Release(btn) => {
                // During any capture mode, swallow releases too so
                // the player's held state doesn't drift out of sync
                // — only presses fire during capture.
                if self.capture.is_some() || btn == GamepadButton::Start {
                    return;
                }
                for action in self.bindings.actions_for_gamepad_button(btn) {
                    self.held.release(action);
                    self.events.push(InputEvent::ActionRelease(action));
                }
            }
            GamepadEvent::Connected | GamepadEvent::Disconnected => {}
        }
    }

    /// Drain the pending event queue; returns all events accumulated
    /// since the last drain.
    pub fn drain(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events)
    }

    /// Per-frame stick + trigger read. Does three things:
    ///
    /// 1. Flips `last_input` to Gamepad if the analog snapshot is
    ///    above the activity floor (catches "player only pushed the
    ///    stick" cases that wouldn't fire a gilrs button event).
    /// 2. Rebuilds `stick_held` from the current stick positions and
    ///    the configured stick bindings.
    /// 3. While the wizard is in gamepad capture mode, emits a
    ///    `RawGamepadCapture(Stick…)` the instant a stick crosses
    ///    [`CAPTURE_STICK_THRESHOLD`] — latched so one push = one
    ///    capture.
    pub fn observe_gamepad_analog(&mut self, snap: &GamepadSnapshot) {
        if gamepad::is_active(snap) {
            self.last_input = LastInput::Gamepad;
        }

        self.stick_held.clear();
        for a in self
            .bindings
            .actions_for_stick_state(snap.left_stick, snap.right_stick)
        {
            self.stick_held.insert(a);
        }

        if self.capture == Some(CaptureMode::Gamepad) {
            self.observe_capture_sticks(snap);
        } else {
            // Reset latches when not capturing so a stick held during
            // capture start doesn't immediately re-fire on next open.
            self.capture_sticks_latched = [[false; 4]; 2];
        }
    }

    fn observe_capture_sticks(&mut self, snap: &GamepadSnapshot) {
        for (stick_idx, stick) in [Stick::Left, Stick::Right].iter().enumerate() {
            let (x, y) = match stick {
                Stick::Left => snap.left_stick,
                Stick::Right => snap.right_stick,
            };
            for (dir_idx, (dir, active)) in [
                (StickDir::Up, y < -CAPTURE_STICK_THRESHOLD),
                (StickDir::Down, y > CAPTURE_STICK_THRESHOLD),
                (StickDir::Left, x < -CAPTURE_STICK_THRESHOLD),
                (StickDir::Right, x > CAPTURE_STICK_THRESHOLD),
            ]
            .iter()
            .enumerate()
            {
                let latched = &mut self.capture_sticks_latched[stick_idx][dir_idx];
                if *active && !*latched {
                    *latched = true;
                    self.events.push(InputEvent::RawGamepadCapture(
                        GamepadBinding::Stick(*stick, *dir),
                    ));
                } else if !*active {
                    *latched = false;
                }
            }
        }
    }
}

/// Shared dispatch for gameplay actions that are handled *locally*
/// on the authoritative host (solo, host). Clients handle verbs by
/// sending network packets instead — see `dispatch_client_verb`.
/// Returns true if the action was consumed.
pub fn dispatch_local_verb(game: &mut Game, local_id: u32, action: Action) -> bool {
    match action {
        Action::Interact => {
            game.try_interact(local_id);
            true
        }
        Action::CycleWeapon => {
            game.try_cycle_weapon(local_id);
            true
        }
        Action::DeployTurret => {
            game.try_deploy_turret(local_id);
            true
        }
        Action::Traversal => {
            game.try_activate_traversal(local_id);
            true
        }
        _ => false,
    }
}

/// Shared dispatch for overlay toggle + camera zoom actions. Runs
/// on edge-press from the main event loop; for the held-style
/// AimZoom action the caller polls [`InputRouter::is_held`] each
/// frame and drives [`crate::camera::Camera::tick_zoom`] directly.
pub fn dispatch_overlay(
    game: &mut Game,
    audio_overlay: &mut AudioOverlay,
    action: Action,
) -> bool {
    match action {
        Action::ConsoleToggle => {
            game.console.toggle();
            true
        }
        Action::InventoryToggle => {
            game.inventory_open = !game.inventory_open;
            true
        }
        Action::PerfOverlay => {
            game.perf_overlay.toggle();
            true
        }
        Action::DebugSpatial => {
            game.debug_spatial_grid = !game.debug_spatial_grid;
            true
        }
        Action::AudioOverlay => {
            audio_overlay.toggle();
            true
        }
        Action::ZoomIn => {
            game.camera.adjust_zoom(crate::camera::ZOOM_STEP);
            true
        }
        Action::ZoomOut => {
            game.camera.adjust_zoom(1.0 / crate::camera::ZOOM_STEP);
            true
        }
        _ => false,
    }
}

/// Shared dispatch for menu-nav actions while the in-game menu is
/// open. Returns the [`MenuAction`] selected by Enter / MenuActivate,
/// or `MenuAction::None` for Up/Down and unhandled keys.
pub fn dispatch_menu_nav(game: &mut Game, action: Action) -> MenuAction {
    match action {
        Action::MenuUp => {
            game.menu.move_up();
            MenuAction::None
        }
        Action::MenuDown => {
            let max = game.menu.items(game.is_authoritative).len();
            game.menu.move_down(max);
            MenuAction::None
        }
        Action::MenuActivate => {
            let connect_cmd = game
                .share_code
                .as_ref()
                .map(|c| c.connect_command());
            let install = game.install_one_liner.clone();
            game.menu.activate(
                game.is_authoritative,
                connect_cmd.as_deref(),
                install.as_deref(),
            )
        }
        _ => MenuAction::None,
    }
}

/// Analog gamepad snapshot with the main-loop's derived unit vectors.
/// Returned from [`pump_frame`] so entry points can feed the same
/// move/aim values into the game regardless of device.
#[derive(Copy, Clone, Debug, Default)]
pub struct FrameInput {
    pub move_vec: (f32, f32),
    pub aim: Option<(f32, f32)>,
    pub firing: bool,
    pub gamepad_active: bool,
}

/// Pull analog axes off the gamepad snapshot and blend with keyboard
/// held-action state into the per-frame inputs the sim needs. Every
/// entry point shares this so deadzone / aim-coast behave identically.
///
/// Movement source is bindings-driven: whichever stick the user has
/// bound to MoveUp/Down/Left/Right drives movement (analog magnitude
/// preserved). Aim uses the *other* stick by convention so a player
/// who remaps movement to RS gets aim on LS automatically.
pub fn resolve_frame_input(
    router: &InputRouter,
    prefer_gamepad: bool,
    last_pad_aim: &mut Option<(f32, f32)>,
) -> FrameInput {
    let pad: GamepadSnapshot = gamepad::snapshot();
    let kb_move = router.move_vec();

    // Movement: prefer the stick bound to move actions; otherwise
    // fall back to keyboard-held direction keys (which may still be
    // the right behavior on a pad with movement mapped to D-pad).
    let move_stick = router.bindings.movement_stick();
    let (gx, gy) = match move_stick {
        Some(Stick::Left) => pad.left_stick,
        Some(Stick::Right) => pad.right_stick,
        None => (0.0, 0.0),
    };
    let stick_move_active = gx != 0.0 || gy != 0.0;
    let move_vec = if prefer_gamepad && stick_move_active {
        let len2 = gx * gx + gy * gy;
        if len2 > 1.0 {
            let inv = len2.sqrt().recip();
            (gx * inv, gy * inv)
        } else {
            (gx, gy)
        }
    } else if prefer_gamepad {
        // Digital held (D-pad or stick-as-held) still applies when
        // the player uses those instead of a stick.
        let dx: f32 = if router.is_held(Action::MoveRight) {
            1.0
        } else if router.is_held(Action::MoveLeft) {
            -1.0
        } else {
            0.0
        };
        let dy: f32 = if router.is_held(Action::MoveDown) {
            1.0
        } else if router.is_held(Action::MoveUp) {
            -1.0
        } else {
            0.0
        };
        let len2 = dx * dx + dy * dy;
        if len2 > 0.0 {
            let inv = len2.sqrt().recip();
            (dx * inv, dy * inv)
        } else {
            (0.0, 0.0)
        }
    } else {
        kb_move
    };

    // Aim: the stick NOT bound to movement. Falls back to right
    // stick if nothing is bound.
    let aim_stick = match move_stick {
        Some(Stick::Left) => Stick::Right,
        Some(Stick::Right) => Stick::Left,
        None => Stick::Right,
    };
    let (rx, ry) = match aim_stick {
        Stick::Left => pad.left_stick,
        Stick::Right => pad.right_stick,
    };
    let stick_aim_active = rx != 0.0 || ry != 0.0;
    let aim = if prefer_gamepad {
        if stick_aim_active {
            let len2 = rx * rx + ry * ry;
            let (ax, ay) = if len2 > 1.0 {
                let inv = len2.sqrt().recip();
                (rx * inv, ry * inv)
            } else {
                (rx, ry)
            };
            *last_pad_aim = Some((ax, ay));
            Some((ax, ay))
        } else {
            *last_pad_aim
        }
    } else {
        *last_pad_aim = None;
        None
    };

    let firing = if prefer_gamepad {
        router.is_held(Action::Fire) || gamepad::fire_held(&pad)
    } else {
        router.firing()
    };

    FrameInput {
        move_vec,
        aim,
        firing,
        gamepad_active: pad.connected && gamepad::is_active(&pad),
    }
}

/// Whether a mouse event counts as "intentional activity" that should
/// flip the input mode back to MouseKeyboard. Raw movement on some
/// terminals fires spurious events; we only flip on buttons / scroll
/// or a genuine cursor move.
pub fn mouse_is_intentional(m: &MouseEvent) -> bool {
    matches!(
        m.kind,
        MouseEventKind::Down(_)
            | MouseEventKind::Up(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Moved
    )
}
