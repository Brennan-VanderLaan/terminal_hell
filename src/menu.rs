//! ESC menu overlay.
//!
//! A small centered panel rendered on top of the framebuffer via direct
//! ANSI writes (same pattern as the HUD). The menu is input-reactive:
//! when `open`, keyboard events are routed to `Menu::handle_key` instead
//! of the game, and the returned `MenuAction` tells the main loop what
//! to do (resume, copy a string, toggle pause, quit).
//!
//! Menu item list is dynamic: authoritative peers (solo + host) see the
//! Pause toggle; clients don't.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Resume,
    TogglePause,
    CopyConnect,
    CopyInstall,
    Quit,
}

impl MenuItem {
    pub fn label(self, paused: bool) -> &'static str {
        match self {
            MenuItem::Resume => "Resume",
            MenuItem::TogglePause => {
                if paused {
                    "Unpause run"
                } else {
                    "Pause run"
                }
            }
            MenuItem::CopyConnect => "Copy connect string",
            MenuItem::CopyInstall => "Copy install one-liner",
            MenuItem::Quit => "Quit",
        }
    }
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    None,
    Close,
    Quit,
    TogglePause,
    Copy(String),
}

pub struct Menu {
    pub open: bool,
    pub selected: usize,
    pub feedback: String,
    pub feedback_ttl: f32,
}

impl Default for Menu {
    fn default() -> Self {
        Self { open: false, selected: 0, feedback: String::new(), feedback_ttl: 0.0 }
    }
}

impl Menu {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.feedback.clear();
        self.feedback_ttl = 0.0;
        // Snap the cursor back to the top when (re)opening so the menu
        // doesn't land on a stale index pointing at a now-hidden item.
        if self.open {
            self.selected = 0;
        }
    }

    pub fn tick(&mut self, dt: f32) {
        if self.feedback_ttl > 0.0 {
            self.feedback_ttl -= dt;
            if self.feedback_ttl <= 0.0 {
                self.feedback.clear();
                self.feedback_ttl = 0.0;
            }
        }
    }

    /// Build the active item list for the current context. Authoritative
    /// peers get the Pause toggle; clients don't.
    pub fn items(&self, is_authoritative: bool) -> Vec<MenuItem> {
        let mut items = Vec::with_capacity(5);
        items.push(MenuItem::Resume);
        if is_authoritative {
            items.push(MenuItem::TogglePause);
        }
        items.push(MenuItem::CopyConnect);
        items.push(MenuItem::CopyInstall);
        items.push(MenuItem::Quit);
        items
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self, max: usize) {
        if self.selected + 1 < max {
            self.selected += 1;
        }
    }

    pub fn activate(
        &mut self,
        is_authoritative: bool,
        connect_string: Option<&str>,
        install_one_liner: Option<&str>,
    ) -> MenuAction {
        let items = self.items(is_authoritative);
        let Some(item) = items.get(self.selected).copied() else {
            return MenuAction::None;
        };
        match item {
            MenuItem::Resume => {
                self.open = false;
                MenuAction::Close
            }
            MenuItem::TogglePause => MenuAction::TogglePause,
            MenuItem::CopyConnect => match connect_string {
                Some(s) => MenuAction::Copy(s.to_string()),
                None => {
                    self.set_feedback("no share code yet", 2.0);
                    MenuAction::None
                }
            },
            MenuItem::CopyInstall => match install_one_liner {
                Some(s) => MenuAction::Copy(s.to_string()),
                None => {
                    self.set_feedback("install server not ready yet", 2.0);
                    MenuAction::None
                }
            },
            MenuItem::Quit => MenuAction::Quit,
        }
    }

    pub fn set_feedback(&mut self, text: impl Into<String>, ttl: f32) {
        self.feedback = text.into();
        self.feedback_ttl = ttl;
    }

    pub fn render<W: Write>(
        &self,
        out: &mut W,
        cols: u16,
        rows: u16,
        is_authoritative: bool,
        paused: bool,
    ) -> Result<()> {
        if !self.open {
            return Ok(());
        }
        let items = self.items(is_authoritative);
        let panel_w: u16 = 44;
        let panel_h: u16 = (items.len() as u16) + 6;
        let x = cols.saturating_sub(panel_w) / 2;
        let y = rows.saturating_sub(panel_h) / 2;

        let fg = Color::Rgb { r: 255, g: 240, b: 150 };
        let bg = Color::Rgb { r: 20, g: 10, b: 30 };
        let border = Color::Rgb { r: 255, g: 180, b: 60 };
        let selected_fg = Color::Rgb { r: 20, g: 10, b: 30 };
        let selected_bg = Color::Rgb { r: 255, g: 220, b: 100 };

        for row in 0..panel_h {
            queue!(
                out,
                MoveTo(x, y + row),
                SetBackgroundColor(bg),
                SetForegroundColor(fg),
                Print(" ".repeat(panel_w as usize)),
            )?;
        }

        queue!(
            out,
            MoveTo(x, y),
            SetForegroundColor(border),
            Print(format!("┌{} terminal_hell {}┐", "─".repeat(12), "─".repeat(14))),
        )?;
        queue!(out, MoveTo(x, y + panel_h - 1))?;
        queue!(out, Print(format!("└{}┘", "─".repeat(panel_w as usize - 2))))?;
        for row in 1..panel_h - 1 {
            queue!(out, MoveTo(x, y + row), Print("│"))?;
            queue!(out, MoveTo(x + panel_w - 1, y + row), Print("│"))?;
        }

        for (i, item) in items.iter().enumerate() {
            let row = y + 2 + i as u16;
            let marker = if i == self.selected { "▸" } else { " " };
            let text = format!(" {marker} {}", item.label(paused));
            let pad = (panel_w as usize).saturating_sub(text.len()).saturating_sub(2);
            let full = format!("{text}{}", " ".repeat(pad));
            queue!(out, MoveTo(x + 1, row))?;
            if i == self.selected {
                queue!(
                    out,
                    SetBackgroundColor(selected_bg),
                    SetForegroundColor(selected_fg),
                    Print(full),
                    SetBackgroundColor(bg),
                    SetForegroundColor(fg),
                )?;
            } else {
                queue!(out, Print(full))?;
            }
        }

        let hint = "↑/↓ move   Enter select   Esc close";
        let hint_row = y + panel_h - 3;
        queue!(
            out,
            MoveTo(x + 2, hint_row),
            SetForegroundColor(Color::Rgb { r: 180, g: 180, b: 200 }),
            Print(hint),
        )?;
        if !self.feedback.is_empty() {
            let feedback_row = y + panel_h - 2;
            let trimmed: String = self.feedback.chars().take(panel_w as usize - 4).collect();
            queue!(
                out,
                MoveTo(x + 2, feedback_row),
                SetForegroundColor(Color::Rgb { r: 120, g: 255, b: 200 }),
                Print(trimmed),
            )?;
        }

        queue!(out, ResetColor)?;
        // Outer loop does a single flush after all overlays queue.
        Ok(())
    }
}

/// Best-effort clipboard write. Returns a user-facing feedback string.
pub fn copy_to_clipboard(text: &str) -> String {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(_) => "copied to clipboard".into(),
        Err(e) => format!("clipboard unavailable: {e} — shown on stderr"),
    }
}
