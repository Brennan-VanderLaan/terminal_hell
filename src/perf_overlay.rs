//! Dedicated telemetry overlay. Toggled by F3; renders a compact
//! rolling-window table in the lower-right. Keeps the normal log
//! console clear of perf spam.
//!
//! Reads directly from the FrameProfile attached to Game — the overlay
//! itself is stateless except for the open flag. Stays rendered until
//! toggled off.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

use crate::telemetry::{FrameProfile, ReportLine};

pub struct PerfOverlay {
    pub open: bool,
    /// Latest rolling-window report. Replaced each time the profile
    /// refreshes its window (once per second via `pull_report`).
    cached: Vec<ReportLine>,
}

impl Default for PerfOverlay {
    fn default() -> Self {
        Self { open: false, cached: Vec::new() }
    }
}

impl PerfOverlay {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Pull a fresh report from the profile into the overlay's cache
    /// if one is available. Call once per frame; the profile itself
    /// gates to REPORT_PERIOD so this is cheap.
    pub fn pull_report(&mut self, perf: &mut FrameProfile) {
        if let Some(lines) = perf.maybe_report() {
            self.cached = lines;
        }
    }

    pub fn render<W: Write>(
        &self,
        out: &mut W,
        cols: u16,
        rows: u16,
        perf: &FrameProfile,
    ) -> Result<()> {
        if !self.open {
            return Ok(());
        }
        let counts = perf.counts_snapshot();

        // Panel geometry — bottom-right, width clamped so the arena
        // is still mostly visible.
        let panel_w: u16 = 48;
        let top_rows: usize = 8; // how many timing rows to show
        let panel_h: u16 = (top_rows as u16) + (counts.len() as u16) + 4;
        let w = panel_w.min(cols);
        let h = panel_h.min(rows);
        let x0 = cols.saturating_sub(w);
        let y0 = rows.saturating_sub(h);

        let bg = Color::Rgb { r: 8, g: 8, b: 18 };
        let border = Color::Rgb { r: 255, g: 180, b: 60 };
        let fg = Color::Rgb { r: 200, g: 220, b: 255 };
        let hot = Color::Rgb { r: 255, g: 120, b: 120 };
        let warm = Color::Rgb { r: 255, g: 210, b: 110 };
        let cool = Color::Rgb { r: 140, g: 200, b: 255 };

        // Wipe panel.
        for row in 0..h {
            queue!(
                out,
                MoveTo(x0, y0 + row),
                SetBackgroundColor(bg),
                SetForegroundColor(fg),
                Print(" ".repeat(w as usize)),
            )?;
        }

        // Title bar.
        let title = " F3 · perf · top timings + counters ";
        let title_trim: String = title.chars().take(w as usize).collect();
        queue!(
            out,
            MoveTo(x0, y0),
            SetBackgroundColor(Color::Rgb { r: 30, g: 20, b: 45 }),
            SetForegroundColor(border),
            Print(format!(
                "{title_trim}{}",
                " ".repeat((w as usize).saturating_sub(title_trim.chars().count()))
            )),
        )?;

        // Timing rows.
        let mut row = y0 + 1;
        queue!(
            out,
            MoveTo(x0, row),
            SetBackgroundColor(bg),
            SetForegroundColor(warm),
            Print(pad(" label                  avg      calls/f ", w)),
        )?;
        row += 1;
        for line in self.cached.iter().take(top_rows) {
            let us = line.avg_per_frame.as_micros();
            let color = if us >= 2000 {
                hot
            } else if us >= 500 {
                warm
            } else {
                cool
            };
            let text = format!(
                " {:<20} {:>5}µs  {:>4}    ",
                truncate(line.label, 20),
                us,
                line.calls_per_frame
            );
            queue!(
                out,
                MoveTo(x0, row),
                SetBackgroundColor(bg),
                SetForegroundColor(color),
                Print(pad(&text, w)),
            )?;
            row += 1;
        }
        // Fill empty slots so the panel has a stable shape.
        for _ in self.cached.iter().take(top_rows).count()..top_rows {
            queue!(
                out,
                MoveTo(x0, row),
                SetBackgroundColor(bg),
                Print(" ".repeat(w as usize)),
            )?;
            row += 1;
        }

        // Separator.
        queue!(
            out,
            MoveTo(x0, row),
            SetBackgroundColor(bg),
            SetForegroundColor(border),
            Print(pad(" ───────── counters ──────────────── ", w)),
        )?;
        row += 1;

        // Counters.
        for (label, value) in &counts {
            let color = if *value > 5000 {
                hot
            } else if *value > 500 {
                warm
            } else {
                cool
            };
            let text = format!(" {:<20} {:>10} ", truncate(label, 20), value);
            queue!(
                out,
                MoveTo(x0, row),
                SetBackgroundColor(bg),
                SetForegroundColor(color),
                Print(pad(&text, w)),
            )?;
            row += 1;
            if row >= y0 + h {
                break;
            }
        }

        queue!(out, ResetColor)?;
        // Flushed by the outer render loop alongside every other
        // HUD overlay — keeping a single flush per frame avoids the
        // mid-frame terminal updates that show up as flicker.
        Ok(())
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn pad(s: &str, w: u16) -> String {
    let w = w as usize;
    let len = s.chars().count();
    if len >= w {
        s.chars().take(w).collect()
    } else {
        format!("{s}{}", " ".repeat(w - len))
    }
}
