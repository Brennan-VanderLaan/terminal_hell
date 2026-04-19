//! Held-key input state driven by crossterm press/release events. Windows
//! Terminal reports both natively. Unix kitty keyboard enhancement flags are
//! pushed in the terminal guard for modern terminals that support it;
//! everywhere else, we fall back to "auto-repeat as held" using a short decay
//! timer per key.

use std::time::{Duration, Instant};

const HELD_DECAY: Duration = Duration::from_millis(120);

#[derive(Default)]
pub struct Input {
    up: KeyState,
    down: KeyState,
    left: KeyState,
    right: KeyState,
}

#[derive(Default)]
struct KeyState {
    held: bool,
    /// Last-press timestamp; used for decay fallback on terminals that never
    /// send release events.
    last_press: Option<Instant>,
}

impl KeyState {
    fn press(&mut self) {
        self.held = true;
        self.last_press = Some(Instant::now());
    }
    fn release(&mut self) {
        self.held = false;
        self.last_press = None;
    }
    fn is_active(&self) -> bool {
        if self.held {
            return true;
        }
        // Decay fallback: if a press came in very recently but no release has
        // fired, keep treating it as held briefly so movement isn't choppy on
        // terminals that swallow release events.
        matches!(self.last_press, Some(t) if t.elapsed() < HELD_DECAY)
    }
}

impl Input {
    pub fn key_event(&mut self, code: crossterm::event::KeyCode, press: bool) {
        use crossterm::event::KeyCode;
        let slot = match code {
            KeyCode::Char('w') | KeyCode::Char('W') | KeyCode::Up => &mut self.up,
            KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Down => &mut self.down,
            KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Left => &mut self.left,
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Right => &mut self.right,
            _ => return,
        };
        if press {
            slot.press();
        } else {
            slot.release();
        }
    }

    pub fn up(&self) -> bool {
        self.up.is_active()
    }
    pub fn down(&self) -> bool {
        self.down.is_active()
    }
    pub fn left(&self) -> bool {
        self.left.is_active()
    }
    pub fn right(&self) -> bool {
        self.right.is_active()
    }

    pub fn move_vec(&self) -> (f32, f32) {
        let mut dx: f32 = 0.0;
        let mut dy: f32 = 0.0;
        if self.left() {
            dx -= 1.0;
        }
        if self.right() {
            dx += 1.0;
        }
        if self.up() {
            dy -= 1.0;
        }
        if self.down() {
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
}
