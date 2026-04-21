//! Input remapping layer.
//!
//! Maps abstract [`Action`]s (Fire, Interact, MenuUp, …) to one or
//! more physical inputs per device (keyboard, gamepad). Every entry
//! point — solo, host, client — consumes the same stream of action
//! events from [`router::InputRouter`]; the only thing that differs
//! between them is how each verb is dispatched (local method call
//! vs. network packet).
//!
//! Hardcoded, never-rebindable:
//!   - `ESC` → toggle in-game menu
//!   - `Ctrl+C` → quit
//!   - Gamepad `Start` → toggle in-game menu
//!
//! Everything else is bindable. The rebinder wizard (see
//! [`wizard`]) lets the player capture fresh bindings per device at
//! runtime; bindings persist to `$XDG_CONFIG_HOME/terminal_hell/
//! bindings.toml` (or `~/.config/terminal_hell/bindings.toml`).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use crossterm::event::KeyCode;
use serde::{Deserialize, Serialize};

use crate::gamepad::GamepadButton;

pub mod router;
pub mod wizard;

/// Stable catalog of abstract game actions. The order here defines
/// the order the rebind wizard walks, so keep movement first, then
/// fire, then verbs, then menu nav, then overlays.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Fire,
    Interact,
    CycleWeapon,
    DeployTurret,
    Traversal,
    AimZoom,
    ZoomIn,
    ZoomOut,
    MenuUp,
    MenuDown,
    MenuActivate,
    ConsoleToggle,
    InventoryToggle,
    PerfOverlay,
    DebugSpatial,
    AudioOverlay,
}

impl Action {
    /// Stable display label used in the wizard prompt. Separate from
    /// the Debug repr so we can tweak copy without breaking saved
    /// bindings files.
    pub fn label(self) -> &'static str {
        match self {
            Action::MoveUp => "Move Up",
            Action::MoveDown => "Move Down",
            Action::MoveLeft => "Move Left",
            Action::MoveRight => "Move Right",
            Action::Fire => "Fire",
            Action::Interact => "Interact",
            Action::CycleWeapon => "Cycle Weapon",
            Action::DeployTurret => "Deploy Turret",
            Action::Traversal => "Traversal / Dash",
            Action::AimZoom => "Tactical View (hold to see more)",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
            Action::MenuUp => "Menu Up",
            Action::MenuDown => "Menu Down",
            Action::MenuActivate => "Menu Activate",
            Action::ConsoleToggle => "Toggle Console",
            Action::InventoryToggle => "Toggle Inventory",
            Action::PerfOverlay => "Toggle Perf Overlay",
            Action::DebugSpatial => "Toggle Debug Grid",
            Action::AudioOverlay => "Toggle Audio Overlay",
        }
    }

    /// The full list, in wizard walk order.
    pub fn all() -> &'static [Action] {
        use Action::*;
        &[
            MoveUp,
            MoveDown,
            MoveLeft,
            MoveRight,
            Fire,
            Interact,
            CycleWeapon,
            DeployTurret,
            Traversal,
            AimZoom,
            ZoomIn,
            ZoomOut,
            MenuUp,
            MenuDown,
            MenuActivate,
            ConsoleToggle,
            InventoryToggle,
            PerfOverlay,
            DebugSpatial,
            AudioOverlay,
        ]
    }

    /// Subset of actions that make sense to rebind for the given
    /// device. The gamepad wizard skips dev-only overlays and the
    /// log console (no reason to bind them to a stick/bumper).
    pub fn rebindable_on(self, device: RebindDevice) -> bool {
        match device {
            RebindDevice::Keyboard => true,
            RebindDevice::Gamepad => !matches!(
                self,
                Action::ConsoleToggle
                    | Action::PerfOverlay
                    | Action::DebugSpatial
                    | Action::AudioOverlay
            ),
        }
    }

    /// True for actions held over time (movement, fire, aim-zoom).
    /// Zoom bumpers stick so ZoomIn/Out are press-edge, not held.
    pub fn is_held_style(self) -> bool {
        matches!(
            self,
            Action::MoveUp
                | Action::MoveDown
                | Action::MoveLeft
                | Action::MoveRight
                | Action::Fire
                | Action::AimZoom
        )
    }
}

/// Which device a binding targets. Used by the wizard to filter the
/// action walk and to pick the right overlay glyphs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RebindDevice {
    Keyboard,
    Gamepad,
}

/// Serializable keyboard binding — serde round-trips through a short
/// string (`"W"`, `"Space"`, `"F3"`, …) for readability in the TOML file.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyBinding(pub KeyCode);

impl Serialize for KeyBinding {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&key_to_str(self.0))
    }
}

impl<'de> Deserialize<'de> for KeyBinding {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        key_from_str(&s)
            .map(KeyBinding)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown key: {s:?}")))
    }
}

/// Which analog stick on the gamepad.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Stick {
    Left,
    Right,
}

/// Cardinal direction on a stick.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StickDir {
    Up,
    Down,
    Left,
    Right,
}

/// Binding for a gamepad input. Buttons + analog triggers round-trip
/// via gilrs's button press events. Stick directions read the analog
/// axes directly: `Stick(Left, Up)` is "held" while left-stick Y is
/// pushed past the capture threshold toward negative (up, in gilrs'
/// inverted-y convention).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GamepadBinding {
    Button(GamepadButton),
    Stick(Stick, StickDir),
}

impl Serialize for GamepadBinding {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&gamepad_binding_to_str(*self))
    }
}

impl<'de> Deserialize<'de> for GamepadBinding {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        gamepad_binding_from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown gamepad binding: {s:?}")))
    }
}

/// Per-device binding maps. `BTreeMap` so the on-disk TOML is sorted
/// deterministically (diff-friendly, easy to hand-edit).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bindings {
    #[serde(default)]
    pub keyboard: BTreeMap<Action, Vec<KeyBinding>>,
    #[serde(default)]
    pub gamepad: BTreeMap<Action, Vec<GamepadBinding>>,
}

impl Bindings {
    /// Baked-in defaults — the shipped control scheme. Restored by the
    /// wizard's "reset" path and used when no on-disk file exists.
    pub fn defaults() -> Self {
        let mut keyboard: BTreeMap<Action, Vec<KeyBinding>> = BTreeMap::new();
        let mut add_key = |a: Action, codes: &[KeyCode]| {
            keyboard.insert(a, codes.iter().copied().map(KeyBinding).collect());
        };
        add_key(Action::MoveUp, &[KeyCode::Char('w'), KeyCode::Up]);
        add_key(Action::MoveDown, &[KeyCode::Char('s'), KeyCode::Down]);
        add_key(Action::MoveLeft, &[KeyCode::Char('a'), KeyCode::Left]);
        add_key(Action::MoveRight, &[KeyCode::Char('d'), KeyCode::Right]);
        add_key(Action::Fire, &[KeyCode::Char(' ')]);
        add_key(Action::Interact, &[KeyCode::Char('e')]);
        add_key(Action::CycleWeapon, &[KeyCode::Char('q')]);
        add_key(Action::DeployTurret, &[KeyCode::Char('t')]);
        add_key(Action::Traversal, &[KeyCode::Char('f')]);
        add_key(Action::AimZoom, &[]);
        add_key(Action::ZoomIn, &[KeyCode::Char('='), KeyCode::Char('+')]);
        add_key(Action::ZoomOut, &[KeyCode::Char('-')]);
        add_key(Action::MenuUp, &[KeyCode::Up]);
        add_key(Action::MenuDown, &[KeyCode::Down]);
        add_key(Action::MenuActivate, &[KeyCode::Enter]);
        add_key(Action::ConsoleToggle, &[KeyCode::Char('`')]);
        add_key(Action::InventoryToggle, &[KeyCode::Tab]);
        add_key(Action::PerfOverlay, &[KeyCode::F(3)]);
        add_key(Action::DebugSpatial, &[KeyCode::F(4)]);
        add_key(Action::AudioOverlay, &[KeyCode::F(5)]);

        let mut gamepad: BTreeMap<Action, Vec<GamepadBinding>> = BTreeMap::new();
        let mut add_pad_btns = |a: Action, binds: &[GamepadBinding]| {
            gamepad.insert(a, binds.to_vec());
        };
        use GamepadBinding::*;
        add_pad_btns(Action::MoveUp, &[Stick(self::Stick::Left, StickDir::Up)]);
        add_pad_btns(Action::MoveDown, &[Stick(self::Stick::Left, StickDir::Down)]);
        add_pad_btns(Action::MoveLeft, &[Stick(self::Stick::Left, StickDir::Left)]);
        add_pad_btns(Action::MoveRight, &[Stick(self::Stick::Left, StickDir::Right)]);
        add_pad_btns(Action::Fire, &[Button(GamepadButton::RightTrigger)]);
        add_pad_btns(Action::Interact, &[Button(GamepadButton::South)]);
        add_pad_btns(Action::CycleWeapon, &[Button(GamepadButton::West)]);
        add_pad_btns(Action::DeployTurret, &[Button(GamepadButton::North)]);
        // Traversal on LB; LT becomes AimZoom (hold-to-focus).
        add_pad_btns(Action::Traversal, &[Button(GamepadButton::LeftBumper)]);
        add_pad_btns(Action::AimZoom, &[Button(GamepadButton::LeftTrigger)]);
        add_pad_btns(
            Action::ZoomIn,
            &[
                Button(GamepadButton::RightBumper),
                Button(GamepadButton::DpadRight),
            ],
        );
        add_pad_btns(Action::ZoomOut, &[Button(GamepadButton::DpadLeft)]);
        add_pad_btns(Action::MenuUp, &[Button(GamepadButton::DpadUp)]);
        add_pad_btns(Action::MenuDown, &[Button(GamepadButton::DpadDown)]);
        add_pad_btns(Action::MenuActivate, &[Button(GamepadButton::South)]);
        add_pad_btns(Action::InventoryToggle, &[Button(GamepadButton::Select)]);

        Self { keyboard, gamepad }
    }

    /// Load bindings from the user's config file. Returns defaults if
    /// the file is missing. Errors propagate only on malformed TOML,
    /// so the caller can warn and fall back.
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            // First-run migration: if an older build left bindings
            // under `$XDG_CONFIG_HOME`, copy to the new location
            // before we fall through to defaults.
            if let Some(legacy) = legacy_config_path() {
                if legacy.exists() {
                    if let Some(parent) = path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if fs::copy(&legacy, &path).is_ok() {
                        tracing::info!(
                            from = %legacy.display(),
                            to = %path.display(),
                            "migrated bindings to new location"
                        );
                    }
                }
            }
        }
        if !path.exists() {
            return Ok(Self::defaults());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let parsed: Self = toml::from_str(&text)
            .with_context(|| format!("parse {}", path.display()))?;
        // Fold in any actions that the on-disk file is missing (e.g.
        // new Action variants added after the file was last written).
        // Actions that are explicitly empty in the file (unbound)
        // are preserved — backfill only touches missing keys.
        Ok(parsed.backfill_defaults())
    }

    fn backfill_defaults(mut self) -> Self {
        let defaults = Self::defaults();
        // For each action that's missing from the on-disk file, copy
        // in its default bindings — but filter out any that collide
        // with bindings already claimed by a DIFFERENT action in the
        // file. Without this filter, a newly-shipped Action (say,
        // AimZoom=[LT]) can end up dual-bound alongside the user's
        // existing Traversal=[LT] from an earlier build's defaults.
        // If every default collides, the action ends up with an
        // empty Vec — visible as "(unbound)" in the wizard so the
        // player knows to rebind.
        for (action, default_binds) in defaults.keyboard {
            if self.keyboard.contains_key(&action) {
                continue;
            }
            let used: std::collections::HashSet<KeyCode> = self
                .keyboard
                .values()
                .flat_map(|v| v.iter().map(|b| b.0))
                .collect();
            let filtered: Vec<KeyBinding> = default_binds
                .into_iter()
                .filter(|b| !used.contains(&b.0))
                .collect();
            self.keyboard.insert(action, filtered);
        }
        for (action, default_binds) in defaults.gamepad {
            if self.gamepad.contains_key(&action) {
                continue;
            }
            let used: std::collections::HashSet<GamepadBinding> = self
                .gamepad
                .values()
                .flat_map(|v| v.iter().copied())
                .collect();
            let filtered: Vec<GamepadBinding> = default_binds
                .into_iter()
                .filter(|b| !used.contains(b))
                .collect();
            self.gamepad.insert(action, filtered);
        }
        self
    }

    /// Write the current bindings to disk, creating parent dirs as
    /// needed.
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serialize bindings")?;
        fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    /// Set the keyboard bindings for an action, replacing any prior
    /// bindings. Used by the wizard when a player recaptures a key.
    pub fn set_keyboard(&mut self, action: Action, binds: Vec<KeyBinding>) {
        self.keyboard.insert(action, binds);
    }

    /// Append a keyboard binding if the action already has some. Used
    /// by the wizard's "add another" flow.
    pub fn push_keyboard(&mut self, action: Action, bind: KeyBinding) {
        self.keyboard.entry(action).or_default().push(bind);
    }

    pub fn set_gamepad(&mut self, action: Action, binds: Vec<GamepadBinding>) {
        self.gamepad.insert(action, binds);
    }

    pub fn push_gamepad(&mut self, action: Action, bind: GamepadBinding) {
        self.gamepad.entry(action).or_default().push(bind);
    }

    /// Every action bound to the given keyboard key. One key can drive
    /// multiple actions at once — e.g. `Up` is both `MoveUp`
    /// (gameplay) and `MenuUp` (menu), and both fire on press. The
    /// dispatch layer decides which to act on based on UI state
    /// (menu-open ignores gameplay verbs, and vice versa), so the
    /// router stays context-free.
    pub fn actions_for_key(&self, code: KeyCode) -> Vec<Action> {
        let mut out = Vec::new();
        for (action, binds) in &self.keyboard {
            if binds.iter().any(|b| b.0 == code) {
                out.push(*action);
            }
        }
        out
    }

    /// Every action bound to the given gamepad button. Same fan-out
    /// semantics as [`actions_for_key`]. Stick bindings are read
    /// per-frame through [`actions_for_stick_state`] instead.
    pub fn actions_for_gamepad_button(&self, button: GamepadButton) -> Vec<Action> {
        let mut out = Vec::new();
        for (action, binds) in &self.gamepad {
            if binds
                .iter()
                .any(|b| matches!(b, GamepadBinding::Button(x) if *x == button))
            {
                out.push(*action);
            }
        }
        out
    }

    /// Collect every action that is currently "held" by stick state.
    /// Caller supplies the deadzoned stick positions (gilrs frame-y is
    /// already sign-flipped so up is negative in our convention).
    /// Threshold is 0.5 for directional bindings so a gentle push
    /// doesn't trigger verbs, but movement reads the raw analog
    /// vector through `movement_stick` instead of this threshold.
    pub fn actions_for_stick_state(
        &self,
        left: (f32, f32),
        right: (f32, f32),
    ) -> Vec<Action> {
        const DIR_THRESHOLD: f32 = 0.5;
        let mut out = Vec::new();
        for (action, binds) in &self.gamepad {
            for b in binds {
                if let GamepadBinding::Stick(stick, dir) = b {
                    let (x, y) = match stick {
                        Stick::Left => left,
                        Stick::Right => right,
                    };
                    let active = match dir {
                        StickDir::Up => y < -DIR_THRESHOLD,
                        StickDir::Down => y > DIR_THRESHOLD,
                        StickDir::Left => x < -DIR_THRESHOLD,
                        StickDir::Right => x > DIR_THRESHOLD,
                    };
                    if active {
                        out.push(*action);
                        break;
                    }
                }
            }
        }
        out
    }

    /// Which stick (if any) is bound to the movement actions. Used
    /// by the router to pull the analog movement vector directly
    /// from that stick for continuous speed control, instead of
    /// thresholded ±1 values.
    pub fn movement_stick(&self) -> Option<Stick> {
        for action in [
            Action::MoveUp,
            Action::MoveDown,
            Action::MoveLeft,
            Action::MoveRight,
        ] {
            if let Some(binds) = self.gamepad.get(&action) {
                for b in binds {
                    if let GamepadBinding::Stick(s, _) = b {
                        return Some(*s);
                    }
                }
            }
        }
        None
    }
}

/// Where bindings live. Per-platform, following each OS's user-data
/// convention so the file is easy to find + edit. Falls through to
/// `./bindings.toml` only if every env-var probe fails.
///
///  - Linux/*BSD: `$XDG_DATA_HOME/terminal_hell/bindings.toml`
///    or `$HOME/.local/share/terminal_hell/bindings.toml`.
///  - macOS: `$HOME/Library/Application Support/terminal_hell/bindings.toml`.
///  - Windows: `%APPDATA%\terminal_hell\bindings.toml`.
pub fn config_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata)
                .join("terminal_hell")
                .join("bindings.toml");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("terminal_hell")
                .join("bindings.toml");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("terminal_hell").join("bindings.toml");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("terminal_hell")
                .join("bindings.toml");
        }
    }
    PathBuf::from("bindings.toml")
}

/// Legacy path used by builds prior to the XDG-data migration. If we
/// find a file there and the new path doesn't exist yet, copy it
/// over and leave the old one so rolling back to an earlier build
/// still works.
fn legacy_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("terminal_hell").join("bindings.toml"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("terminal_hell")
                .join("bindings.toml"),
        );
    }
    None
}

/// Encode a `KeyCode` as a short display-friendly string. Inverse of
/// [`key_from_str`].
pub fn key_to_str(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char('`') => "Backtick".into(),
        KeyCode::Char(c) => c.to_ascii_lowercase().to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "BackTab".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::Esc => "Esc".into(),
        other => format!("{other:?}"),
    }
}

fn key_from_str(s: &str) -> Option<KeyCode> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    Some(match s {
        "Space" => KeyCode::Char(' '),
        "Backtick" => KeyCode::Char('`'),
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "BackTab" => KeyCode::BackTab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Esc" => KeyCode::Esc,
        other if other.starts_with('F') && other[1..].parse::<u8>().is_ok() => {
            KeyCode::F(other[1..].parse().unwrap())
        }
        other if other.chars().count() == 1 => {
            KeyCode::Char(other.chars().next().unwrap().to_ascii_lowercase())
        }
        _ => return None,
    })
}

/// Display-friendly glyph for a gamepad binding. Used by the wizard
/// and by the on-HUD hint swap (e.g. `[Ⓐ]` for the interact prompt).
pub fn gamepad_binding_label(b: GamepadBinding) -> String {
    match b {
        GamepadBinding::Button(btn) => format!("{btn:?}"),
        GamepadBinding::Stick(s, d) => {
            let stick = match s {
                Stick::Left => "LStick",
                Stick::Right => "RStick",
            };
            let dir = match d {
                StickDir::Up => "↑",
                StickDir::Down => "↓",
                StickDir::Left => "←",
                StickDir::Right => "→",
            };
            format!("{stick}{dir}")
        }
    }
}

fn gamepad_binding_to_str(b: GamepadBinding) -> String {
    match b {
        GamepadBinding::Button(btn) => format!("{btn:?}"),
        GamepadBinding::Stick(s, d) => {
            let stick = match s {
                Stick::Left => "LeftStick",
                Stick::Right => "RightStick",
            };
            let dir = match d {
                StickDir::Up => "Up",
                StickDir::Down => "Down",
                StickDir::Left => "Left",
                StickDir::Right => "Right",
            };
            format!("{stick}{dir}")
        }
    }
}

fn gamepad_binding_from_str(s: &str) -> Option<GamepadBinding> {
    // Stick bindings first (unique prefixes).
    if let Some(rest) = s.strip_prefix("LeftStick") {
        let d = parse_stick_dir(rest)?;
        return Some(GamepadBinding::Stick(Stick::Left, d));
    }
    if let Some(rest) = s.strip_prefix("RightStick") {
        let d = parse_stick_dir(rest)?;
        return Some(GamepadBinding::Stick(Stick::Right, d));
    }
    Some(GamepadBinding::Button(match s {
        "South" => GamepadButton::South,
        "East" => GamepadButton::East,
        "West" => GamepadButton::West,
        "North" => GamepadButton::North,
        "LeftBumper" => GamepadButton::LeftBumper,
        "RightBumper" => GamepadButton::RightBumper,
        "LeftTrigger" => GamepadButton::LeftTrigger,
        "RightTrigger" => GamepadButton::RightTrigger,
        "Start" => GamepadButton::Start,
        "Select" => GamepadButton::Select,
        "DpadUp" => GamepadButton::DpadUp,
        "DpadDown" => GamepadButton::DpadDown,
        "DpadLeft" => GamepadButton::DpadLeft,
        "DpadRight" => GamepadButton::DpadRight,
        _ => return None,
    }))
}

fn parse_stick_dir(s: &str) -> Option<StickDir> {
    Some(match s {
        "Up" => StickDir::Up,
        "Down" => StickDir::Down,
        "Left" => StickDir::Left,
        "Right" => StickDir::Right,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_cover_every_action_with_keyboard() {
        let b = Bindings::defaults();
        for a in Action::all() {
            assert!(
                b.keyboard.contains_key(a),
                "default keyboard binding missing for {a:?}"
            );
        }
    }

    #[test]
    fn default_gamepad_movement_is_bound_to_left_stick() {
        let b = Bindings::defaults();
        assert_eq!(b.movement_stick(), Some(Stick::Left));
    }

    #[test]
    fn stick_binding_codec_roundtrip() {
        for b in [
            GamepadBinding::Stick(Stick::Left, StickDir::Up),
            GamepadBinding::Stick(Stick::Right, StickDir::Down),
            GamepadBinding::Button(GamepadButton::South),
            GamepadBinding::Button(GamepadButton::RightTrigger),
        ] {
            let s = gamepad_binding_to_str(b);
            assert_eq!(gamepad_binding_from_str(&s), Some(b), "roundtrip {s}");
        }
    }

    #[test]
    fn stick_dir_threshold_respected() {
        let b = Bindings::defaults();
        // Gentle push (past capture deadzone but under directional
        // threshold) should NOT register movement actions as held.
        let gentle = b.actions_for_stick_state((0.3, 0.0), (0.0, 0.0));
        assert!(gentle.is_empty(), "gentle push fired actions: {gentle:?}");
        // Full right-stick up on Left stick = MoveUp held.
        let strong = b.actions_for_stick_state((0.0, -0.9), (0.0, 0.0));
        assert!(strong.contains(&Action::MoveUp));
    }

    #[test]
    fn gamepad_wizard_filters_dev_overlays() {
        assert!(Action::PerfOverlay.rebindable_on(RebindDevice::Keyboard));
        assert!(!Action::PerfOverlay.rebindable_on(RebindDevice::Gamepad));
        assert!(!Action::DebugSpatial.rebindable_on(RebindDevice::Gamepad));
        assert!(!Action::AudioOverlay.rebindable_on(RebindDevice::Gamepad));
        assert!(!Action::ConsoleToggle.rebindable_on(RebindDevice::Gamepad));
        // Gameplay verbs are always rebindable on both.
        assert!(Action::Interact.rebindable_on(RebindDevice::Gamepad));
        assert!(Action::MoveUp.rebindable_on(RebindDevice::Gamepad));
    }

    #[test]
    fn roundtrip_through_toml() {
        let before = Bindings::defaults();
        let s = toml::to_string_pretty(&before).unwrap();
        let after: Bindings = toml::from_str(&s).unwrap();
        assert_eq!(before.keyboard, after.keyboard);
        assert_eq!(before.gamepad, after.gamepad);
    }

    #[test]
    fn key_codec_roundtrip() {
        for c in ['a', 'z', ' ', '`'] {
            let code = KeyCode::Char(c);
            let s = key_to_str(code);
            assert_eq!(key_from_str(&s), Some(code));
        }
        for n in [3u8, 4, 5] {
            assert_eq!(key_from_str(&key_to_str(KeyCode::F(n))), Some(KeyCode::F(n)));
        }
        for c in [KeyCode::Up, KeyCode::Tab, KeyCode::Enter, KeyCode::Esc] {
            assert_eq!(key_from_str(&key_to_str(c)), Some(c));
        }
    }

    #[test]
    fn action_for_key_finds_default_bindings() {
        let b = Bindings::defaults();
        assert_eq!(b.actions_for_key(KeyCode::Char('e')), vec![Action::Interact]);
        assert_eq!(b.actions_for_key(KeyCode::Char(' ')), vec![Action::Fire]);
        assert_eq!(b.actions_for_key(KeyCode::F(3)), vec![Action::PerfOverlay]);
    }

    /// Up arrow must fire BOTH MoveUp and MenuUp so the same key drives
    /// movement in-game AND menu nav when the menu is open. Regression
    /// test for the bug where BTreeMap iteration order hid MenuUp.
    #[test]
    fn overlapping_keys_fan_out_to_all_actions() {
        let b = Bindings::defaults();
        let up = b.actions_for_key(KeyCode::Up);
        assert!(up.contains(&Action::MoveUp), "Up should drive MoveUp: {up:?}");
        assert!(up.contains(&Action::MenuUp), "Up should drive MenuUp: {up:?}");
        let enter = b.actions_for_key(KeyCode::Enter);
        assert!(enter.contains(&Action::MenuActivate));
    }

    /// Same fan-out requirement for the gamepad: A-button is both
    /// Interact (in gameplay) and MenuActivate (in menu).
    #[test]
    fn overlapping_gamepad_buttons_fan_out() {
        let b = Bindings::defaults();
        let a = b.actions_for_gamepad_button(GamepadButton::South);
        assert!(a.contains(&Action::Interact));
        assert!(a.contains(&Action::MenuActivate));
    }

    #[test]
    fn backfill_restores_missing_actions_from_disk() {
        let mut partial = Bindings::default();
        partial.keyboard.insert(Action::Fire, vec![KeyBinding(KeyCode::Enter)]);
        let filled = partial.backfill_defaults();
        // Explicit entry survives.
        assert_eq!(
            filled.keyboard.get(&Action::Fire),
            Some(&vec![KeyBinding(KeyCode::Enter)])
        );
        // Missing action gets the default.
        assert!(filled.keyboard.contains_key(&Action::Interact));
    }

    /// Regression: a user whose saved file still holds a pre-refactor
    /// Traversal=[LT] from an old build shouldn't end up with LT
    /// dual-bound to Traversal AND the newly-added AimZoom's default.
    /// Backfill must filter out collisions.
    #[test]
    fn backfill_skips_default_bindings_that_collide_with_existing() {
        let mut stale = Bindings::default();
        stale.gamepad.insert(
            Action::Traversal,
            vec![GamepadBinding::Button(GamepadButton::LeftTrigger)],
        );
        let filled = stale.backfill_defaults();
        // AimZoom gets added by backfill, but NOT with the default LT —
        // that's already claimed.
        let aim = filled.gamepad.get(&Action::AimZoom).unwrap();
        assert!(
            !aim.iter().any(|b| matches!(
                b,
                GamepadBinding::Button(GamepadButton::LeftTrigger)
            )),
            "AimZoom should not inherit LT while Traversal still owns it: {aim:?}"
        );
        // Traversal's legacy binding is preserved.
        assert_eq!(
            filled.gamepad.get(&Action::Traversal),
            Some(&vec![GamepadBinding::Button(GamepadButton::LeftTrigger)])
        );
    }
}
