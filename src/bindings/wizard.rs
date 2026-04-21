//! Rebind wizard — modal overlay that walks the player through every
//! [`Action`] and captures fresh bindings for the chosen device.
//!
//! Flow: caller opens wizard via `start()`, sets capture mode on the
//! router, then delegates raw captures to [`handle_key_capture`] /
//! [`handle_gamepad_capture`]. The wizard returns a [`WizardOutcome`]
//! on every input; the caller saves on `Save`, discards on `Cancel`,
//! otherwise keeps going. While running, the wizard paints its own
//! modal panel on top of the framebuffer each frame.
//!
//! Controls (keyboard wizard):
//!   - Captured key → bound to current action, wizard advances.
//!   - `Space` → skip (leave existing binding alone, advance).
//!   - `Tab` → commit current capture without advancing (for adding
//!     multiple bindings to one action).
//!   - `Backspace` → clear all bindings for this action (explicitly
//!     unbind).
//!   - `Esc` → cancel the whole walkthrough, no save.
//!
//! Controls (gamepad wizard): same semantics, plus:
//!   - Any stick pushed past 65% magnitude → captures that stick
//!     direction as the binding.
//!   - Gamepad `Start` → cancels the walkthrough.
//!   - Keyboard `Space`/`Tab`/`Backspace`/`Esc` still work as meta
//!     commands so a player without a working D-pad can drive the
//!     flow from the keyboard.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::KeyCode,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

use crate::gamepad::GamepadButton;

use super::router::CaptureMode;
use super::{
    Action, Bindings, GamepadBinding, KeyBinding, RebindDevice, gamepad_binding_label, key_to_str,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WizardDevice {
    Keyboard,
    Gamepad,
}

impl WizardDevice {
    fn capture_mode(self) -> CaptureMode {
        match self {
            WizardDevice::Keyboard => CaptureMode::Keyboard,
            WizardDevice::Gamepad => CaptureMode::Gamepad,
        }
    }

    fn title(self) -> &'static str {
        match self {
            WizardDevice::Keyboard => "Rebind Keyboard",
            WizardDevice::Gamepad => "Rebind Gamepad",
        }
    }

    fn as_rebind_device(self) -> RebindDevice {
        match self {
            WizardDevice::Keyboard => RebindDevice::Keyboard,
            WizardDevice::Gamepad => RebindDevice::Gamepad,
        }
    }
}

#[derive(Debug, Clone)]
pub enum WizardOutcome {
    Continue,
    /// Player finished the walk. Caller should `bindings.save()` then
    /// hand the new map to the router via `set_bindings`.
    Save(Bindings),
    /// Player hit Esc / Start. Discard pending edits.
    Cancel,
}

/// Stateful wizard. Walks the device-specific action list in order;
/// players step through by capturing a key/button or hitting Space
/// to skip.
pub struct RebindWizard {
    pub device: WizardDevice,
    pub bindings: Bindings,
    /// Filtered list of actions to walk — overlays and dev toggles
    /// are omitted from the gamepad walk.
    actions: Vec<Action>,
    index: usize,
    /// Captures staged for the current action. The first capture on
    /// an action *replaces* the prior bindings; subsequent ones
    /// (triggered by Tab/"add another") append.
    pending_keys: Vec<KeyBinding>,
    pending_pad: Vec<GamepadBinding>,
    /// Replaced-flag per action: on the first capture we blow away
    /// the existing list; later presses append. Tab keeps
    /// `replaced=true` so each Tab-then-capture appends instead of
    /// replacing again.
    replaced: bool,
    status: String,
}

impl RebindWizard {
    /// Start a fresh walkthrough of the first rebindable action for
    /// the chosen device. Also includes any action that already has
    /// bindings for this device even if it isn't nominally rebindable
    /// there — lets the player un-bind stale orphans captured by an
    /// earlier build whose wizard didn't filter action lists.
    pub fn start(device: WizardDevice, bindings: Bindings) -> Self {
        let rebind_device = device.as_rebind_device();
        let actions: Vec<Action> = Action::all()
            .iter()
            .copied()
            .filter(|a| {
                if a.rebindable_on(rebind_device) {
                    return true;
                }
                match device {
                    WizardDevice::Keyboard => bindings
                        .keyboard
                        .get(a)
                        .map(|v| !v.is_empty())
                        .unwrap_or(false),
                    WizardDevice::Gamepad => bindings
                        .gamepad
                        .get(a)
                        .map(|v| !v.is_empty())
                        .unwrap_or(false),
                }
            })
            .collect();
        Self {
            device,
            bindings,
            actions,
            index: 0,
            pending_keys: Vec::new(),
            pending_pad: Vec::new(),
            replaced: false,
            status: String::new(),
        }
    }

    pub fn capture_mode(&self) -> CaptureMode {
        self.device.capture_mode()
    }

    pub fn current(&self) -> Option<Action> {
        self.actions.get(self.index).copied()
    }

    /// Keyboard-path input. Handles meta keys (`Esc`, `Space`, `Tab`,
    /// `Backspace`) before treating anything else as a capture. The
    /// Enter key is also treated as a skip for backwards-compat with
    /// the previous build and because kitty keyboard protocol can
    /// route it as `Char('\r')` in some terminals.
    pub fn handle_key_capture(&mut self, code: KeyCode) -> WizardOutcome {
        if self.is_cancel_key(code) {
            return WizardOutcome::Cancel;
        }
        if self.is_skip_key(code) {
            return self.skip();
        }
        if self.is_add_another_key(code) {
            // Tab: commit pending captures but stay on the current
            // action so more can be added. Useful for multi-bind.
            self.commit_pending();
            self.replaced = true;
            self.status = "add another or press Space to move on".into();
            return WizardOutcome::Continue;
        }
        if self.is_clear_key(code) {
            if let Some(action) = self.current() {
                self.bindings.keyboard.insert(action, Vec::new());
                self.bindings.gamepad.insert(action, Vec::new());
            }
            self.pending_keys.clear();
            self.pending_pad.clear();
            self.replaced = true;
            self.status = "unbound".into();
            return WizardOutcome::Continue;
        }
        // Gamepad wizard: keyboard ONLY sees meta keys, not captures.
        if self.device != WizardDevice::Keyboard {
            return WizardOutcome::Continue;
        }
        self.pending_keys.push(KeyBinding(code));
        self.status = format!("captured {}", key_to_str(code));
        self.advance()
    }

    /// Gamepad-path input. `Start` is treated as cancel so a player
    /// without a working keyboard can still back out of the wizard.
    pub fn handle_gamepad_capture(&mut self, bind: GamepadBinding) -> WizardOutcome {
        if self.device != WizardDevice::Gamepad {
            return WizardOutcome::Continue;
        }
        if let GamepadBinding::Button(GamepadButton::Start) = bind {
            return WizardOutcome::Cancel;
        }
        self.pending_pad.push(bind);
        self.status = format!("captured {}", gamepad_binding_label(bind));
        self.advance()
    }

    fn skip(&mut self) -> WizardOutcome {
        self.pending_keys.clear();
        self.pending_pad.clear();
        self.status = "skipped (kept existing)".into();
        self.advance_index()
    }

    fn advance(&mut self) -> WizardOutcome {
        self.commit_pending();
        self.advance_index()
    }

    fn advance_index(&mut self) -> WizardOutcome {
        self.index += 1;
        self.replaced = false;
        if self.index >= self.actions.len() {
            return WizardOutcome::Save(self.bindings.clone());
        }
        WizardOutcome::Continue
    }

    fn commit_pending(&mut self) {
        let Some(action) = self.current() else {
            self.pending_keys.clear();
            self.pending_pad.clear();
            return;
        };
        match self.device {
            WizardDevice::Keyboard => {
                if self.pending_keys.is_empty() {
                    return;
                }
                let drained: Vec<_> = self.pending_keys.drain(..).collect();
                if !self.replaced {
                    self.bindings.keyboard.insert(action, drained);
                    self.replaced = true;
                } else {
                    let slot = self.bindings.keyboard.entry(action).or_default();
                    slot.extend(drained);
                }
            }
            WizardDevice::Gamepad => {
                if self.pending_pad.is_empty() {
                    return;
                }
                let drained: Vec<_> = self.pending_pad.drain(..).collect();
                if !self.replaced {
                    self.bindings.gamepad.insert(action, drained);
                    self.replaced = true;
                } else {
                    let slot = self.bindings.gamepad.entry(action).or_default();
                    slot.extend(drained);
                }
            }
        }
    }

    fn is_cancel_key(&self, code: KeyCode) -> bool {
        matches!(code, KeyCode::Esc)
    }

    fn is_skip_key(&self, code: KeyCode) -> bool {
        matches!(
            code,
            KeyCode::Char(' ')
                | KeyCode::Enter
                | KeyCode::Char('\r')
                | KeyCode::Char('\n')
        )
    }

    fn is_add_another_key(&self, code: KeyCode) -> bool {
        matches!(code, KeyCode::Tab)
    }

    fn is_clear_key(&self, code: KeyCode) -> bool {
        matches!(code, KeyCode::Backspace | KeyCode::Delete)
    }

    /// Paint the wizard overlay. Called each frame by the entry
    /// points' render pass — mirrors the `Menu::render` pattern.
    pub fn render<W: Write>(&self, out: &mut W, cols: u16, rows: u16) -> Result<()> {
        let panel_w: u16 = 60;
        let panel_h: u16 = 16;
        let x = cols.saturating_sub(panel_w) / 2;
        let y = rows.saturating_sub(panel_h) / 2;

        let fg = Color::Rgb { r: 255, g: 240, b: 150 };
        let bg = Color::Rgb { r: 20, g: 10, b: 30 };
        let border = Color::Rgb { r: 255, g: 180, b: 60 };
        let dim = Color::Rgb { r: 180, g: 180, b: 200 };
        let accent = Color::Rgb { r: 120, g: 255, b: 200 };
        let strong = Color::Rgb { r: 255, g: 220, b: 100 };

        for row in 0..panel_h {
            queue!(
                out,
                MoveTo(x, y + row),
                SetBackgroundColor(bg),
                SetForegroundColor(fg),
                Print(" ".repeat(panel_w as usize)),
            )?;
        }
        let title = self.device.title();
        let head = format!(
            "┌{} {} {}┐",
            "─".repeat(6),
            title,
            "─".repeat((panel_w as usize).saturating_sub(10 + title.len()))
        );
        queue!(out, MoveTo(x, y), SetForegroundColor(border), Print(head))?;
        queue!(out, MoveTo(x, y + panel_h - 1))?;
        queue!(out, Print(format!("└{}┘", "─".repeat(panel_w as usize - 2))))?;
        for row in 1..panel_h - 1 {
            queue!(out, MoveTo(x, y + row), Print("│"))?;
            queue!(out, MoveTo(x + panel_w - 1, y + row), Print("│"))?;
        }

        let total = self.actions.len();
        let progress = format!("{}/{}", (self.index + 1).min(total), total.max(1));
        queue!(
            out,
            MoveTo(x + 2, y + 2),
            SetForegroundColor(dim),
            Print(format!("Step {progress}")),
        )?;

        let (action_label, prompt_leader) = if let Some(action) = self.current() {
            (
                action.label(),
                match self.device {
                    WizardDevice::Keyboard => "Press a key for:",
                    WizardDevice::Gamepad => "Press a button or push a stick for:",
                },
            )
        } else {
            ("All done — saving…", "")
        };
        queue!(
            out,
            MoveTo(x + 2, y + 4),
            SetForegroundColor(fg),
            Print(prompt_leader),
        )?;
        queue!(
            out,
            MoveTo(x + 2, y + 5),
            SetForegroundColor(strong),
            Print(format!("▸ {action_label}")),
        )?;

        let current_binds = self.describe_current();
        if !current_binds.is_empty() {
            queue!(
                out,
                MoveTo(x + 2, y + 7),
                SetForegroundColor(dim),
                Print(format!("current: {current_binds}")),
            )?;
        }
        let pending = self.describe_pending();
        if !pending.is_empty() {
            queue!(
                out,
                MoveTo(x + 2, y + 8),
                SetForegroundColor(accent),
                Print(format!("pending: {pending}")),
            )?;
        }

        let hint1 = match self.device {
            WizardDevice::Keyboard => {
                "Space = skip   Tab = add another   Backspace = unbind"
            }
            WizardDevice::Gamepad => {
                "Space = skip   Tab = add another   Backspace = unbind"
            }
        };
        let hint2 = match self.device {
            WizardDevice::Keyboard => "Esc = cancel wizard (no save)",
            WizardDevice::Gamepad => "Esc / Start = cancel wizard (no save)",
        };
        queue!(
            out,
            MoveTo(x + 2, y + panel_h - 4),
            SetForegroundColor(dim),
            Print(hint1),
        )?;
        queue!(
            out,
            MoveTo(x + 2, y + panel_h - 3),
            SetForegroundColor(dim),
            Print(hint2),
        )?;

        if !self.status.is_empty() {
            let trimmed: String = self.status.chars().take(panel_w as usize - 4).collect();
            queue!(
                out,
                MoveTo(x + 2, y + panel_h - 2),
                SetForegroundColor(accent),
                Print(trimmed),
            )?;
        }

        queue!(out, ResetColor)?;
        Ok(())
    }

    fn describe_current(&self) -> String {
        let Some(action) = self.current() else {
            return String::new();
        };
        match self.device {
            WizardDevice::Keyboard => self
                .bindings
                .keyboard
                .get(&action)
                .map(|v| {
                    if v.is_empty() {
                        "(unbound)".to_string()
                    } else {
                        v.iter().map(|b| key_to_str(b.0)).collect::<Vec<_>>().join(", ")
                    }
                })
                .unwrap_or_default(),
            WizardDevice::Gamepad => self
                .bindings
                .gamepad
                .get(&action)
                .map(|v| {
                    if v.is_empty() {
                        "(unbound)".to_string()
                    } else {
                        v.iter()
                            .map(|b| gamepad_binding_label(*b))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                })
                .unwrap_or_default(),
        }
    }

    fn describe_pending(&self) -> String {
        match self.device {
            WizardDevice::Keyboard => self
                .pending_keys
                .iter()
                .map(|b| key_to_str(b.0))
                .collect::<Vec<_>>()
                .join(", "),
            WizardDevice::Gamepad => self
                .pending_pad
                .iter()
                .map(|b| gamepad_binding_label(*b))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}
