//! In-game log console. Pull-down from the top half of the screen,
//! toggled by backtick — the traditional game-console key.
//!
//! The console is a passive overlay: it shows the last N lines of the
//! log buffer. It doesn't trap input, so the player can keep moving
//! while watching logs scroll. Stays rendered until backtick is pressed
//! again.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

pub struct Console {
    pub open: bool,
}

impl Default for Console {
    fn default() -> Self {
        Self { open: false }
    }
}

impl Console {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn render<W: Write>(&self, out: &mut W, cols: u16, rows: u16) -> Result<()> {
        if !self.open {
            return Ok(());
        }
        let log = match crate::log_buf::LogBuffer::global() {
            Some(l) => l,
            None => return Ok(()),
        };

        // Console covers the top ~55% of the screen. Leaves the bottom
        // (where the player is likely looking) visible through the log.
        let panel_h = ((rows as u32 * 55 / 100) as u16).max(8).min(rows);
        let y0: u16 = 0;
        let bg = Color::Rgb { r: 8, g: 6, b: 18 };
        let fg = Color::Rgb { r: 200, g: 220, b: 255 };
        let border = Color::Rgb { r: 255, g: 180, b: 60 };
        let err_fg = Color::Rgb { r: 255, g: 120, b: 120 };
        let warn_fg = Color::Rgb { r: 255, g: 210, b: 110 };
        let info_fg = Color::Rgb { r: 180, g: 220, b: 255 };
        let debug_fg = Color::Rgb { r: 140, g: 150, b: 180 };

        // Wipe the panel area first so log lines sit on a solid bg.
        for row in 0..panel_h {
            queue!(
                out,
                MoveTo(0, y0 + row),
                SetBackgroundColor(bg),
                SetForegroundColor(fg),
                Print(" ".repeat(cols as usize)),
            )?;
        }

        // Title bar.
        let title = format!(" ` logs ({} lines buffered)  — backtick to close ", log.len());
        let title_trim: String = title.chars().take(cols as usize).collect();
        queue!(
            out,
            MoveTo(0, y0),
            SetBackgroundColor(Color::Rgb { r: 30, g: 20, b: 45 }),
            SetForegroundColor(border),
            Print(format!(
                "{title_trim}{}",
                " ".repeat((cols as usize).saturating_sub(title_trim.chars().count()))
            )),
        )?;

        // Scrollback body: render the last (panel_h - 1) lines; oldest
        // line sits just under the title, newest line at the bottom.
        let body_rows = panel_h.saturating_sub(1);
        if body_rows == 0 {
            queue!(out, ResetColor)?;
            return Ok(());
        }
        // Wrap long log lines onto multiple terminal rows so they
        // don't vanish off the right side. Each source line yields
        // 1+ display rows; we then take the last `body_rows` display
        // rows so the most-recent wrapped chunks stay visible.
        let cw = cols as usize;
        let content_w = cw.saturating_sub(2).max(8);
        let raw_lines = log.recent(body_rows as usize * 4);
        let mut wrapped: Vec<(String, Color)> = Vec::with_capacity(raw_lines.len() * 2);
        for line in &raw_lines {
            let color = level_color(line, err_fg, warn_fg, info_fg, debug_fg);
            // Chunk the line into content_w-wide segments. Indented
            // continuation segments get a '  …' prefix so wrapped
            // continuations read as "same log line."
            let chars: Vec<char> = line.chars().collect();
            if chars.is_empty() {
                wrapped.push((String::new(), color));
                continue;
            }
            let mut i = 0;
            let mut first = true;
            while i < chars.len() {
                let chunk_end = (i + content_w).min(chars.len());
                let chunk: String = chars[i..chunk_end].iter().collect();
                if first {
                    wrapped.push((chunk, color));
                    first = false;
                } else {
                    wrapped.push((format!(" … {chunk}"), color));
                }
                i = chunk_end;
            }
        }
        // Take the tail so the newest lines (possibly multi-row)
        // land at the bottom.
        let start = wrapped.len().saturating_sub(body_rows as usize);
        for (i, (text, color)) in wrapped[start..].iter().enumerate() {
            let row = y0 + 1 + i as u16;
            let padded = format!(
                " {text}{}",
                " ".repeat(cw.saturating_sub(text.chars().count() + 1))
            );
            queue!(
                out,
                MoveTo(0, row),
                SetBackgroundColor(bg),
                SetForegroundColor(*color),
                Print(padded),
            )?;
        }

        queue!(out, ResetColor)?;
        // Outer loop does a single flush per frame.
        Ok(())
    }
}

/// Heuristically pick a color based on the level token embedded in the
/// tracing line (default fmt layer writes `INFO`, `WARN`, `ERROR` etc.).
fn level_color(
    line: &str,
    err: Color,
    warn: Color,
    info: Color,
    debug: Color,
) -> Color {
    if line.contains("ERROR") {
        err
    } else if line.contains(" WARN") {
        warn
    } else if line.contains("DEBUG") || line.contains("TRACE") {
        debug
    } else {
        info
    }
}
