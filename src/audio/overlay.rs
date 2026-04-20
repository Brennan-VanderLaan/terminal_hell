//! F5 audio debug overlay — sibling to perf_overlay (F3) and the
//! spatial-grid debug (F4). Surfaces what the audio engine is
//! carrying, what it just played, which bus gains are declared, and
//! what the CV-driven gameplay parameters currently read.
//!
//! Stateless aside from the toggle + a periodic cache refresh. Reads
//! the global ENGINE and the live param registry; parses `buses.toml`
//! from the resolved content root once and caches the declared gains
//! so rendering every frame stays cheap.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::audio;

const REFRESH_EVERY: Duration = Duration::from_millis(250);
const POOLS_TO_SHOW: usize = 8;
const EMITS_TO_SHOW: usize = 8;
const PARAMS_TO_SHOW: usize = 8;

/// Parsed bus entry for the overlay — just the display-oriented bits.
#[derive(Debug, Clone)]
pub struct BusRow {
    pub name: String,
    pub gain_display: String,
    pub chain_summary: String,
}

pub struct AudioOverlay {
    pub open: bool,
    last_refresh: Option<Instant>,
    buses: Vec<BusRow>,
    plays_last_tick: u64,
    plays_per_sec: f32,
    last_plays_sample: Option<(Instant, u64)>,
}

impl Default for AudioOverlay {
    fn default() -> Self {
        Self {
            open: false,
            last_refresh: None,
            buses: Vec::new(),
            plays_last_tick: 0,
            plays_per_sec: 0.0,
            last_plays_sample: None,
        }
    }
}

impl AudioOverlay {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open && self.buses.is_empty() {
            self.refresh_buses();
        }
    }

    fn refresh_buses(&mut self) {
        let root = audio::default_content_root();
        self.buses = load_bus_rows(&root).unwrap_or_default();
    }

    fn refresh_rate(&mut self) {
        let total = audio::plays_issued();
        let now = Instant::now();
        if let Some((t, n)) = self.last_plays_sample {
            let dt = now.duration_since(t).as_secs_f32();
            if dt > 0.05 {
                let delta = total.saturating_sub(n) as f32;
                self.plays_per_sec = delta / dt;
            }
        }
        self.last_plays_sample = Some((now, total));
        self.plays_last_tick = total;
    }

    pub fn render<W: Write>(&mut self, out: &mut W, cols: u16, rows: u16) -> Result<()> {
        if !self.open {
            return Ok(());
        }

        let due_refresh = self
            .last_refresh
            .map(|t| t.elapsed() >= REFRESH_EVERY)
            .unwrap_or(true);
        if due_refresh {
            self.refresh_rate();
            self.last_refresh = Some(Instant::now());
            if self.buses.is_empty() {
                self.refresh_buses();
            }
        }

        // Panel geometry — top-right, deliberately distinct from F3's
        // bottom-right corner so both overlays can sit on screen at
        // once for a full diagnostic view.
        let panel_w: u16 = 60;
        let panel_h_target: u16 = (POOLS_TO_SHOW + EMITS_TO_SHOW + PARAMS_TO_SHOW) as u16
            + self.buses.len() as u16
            + 12; // headers, separators, summary rows
        let w = panel_w.min(cols);
        let h = panel_h_target.min(rows);
        let x0 = cols.saturating_sub(w);
        let y0 = 0u16;

        let bg = Color::Rgb { r: 12, g: 10, b: 22 };
        let title_bg = Color::Rgb { r: 38, g: 22, b: 56 };
        let border = Color::Rgb { r: 180, g: 140, b: 255 };
        let fg = Color::Rgb { r: 200, g: 220, b: 255 };
        let label = Color::Rgb { r: 150, g: 200, b: 180 };
        let value_ok = Color::Rgb { r: 140, g: 220, b: 180 };
        let value_warn = Color::Rgb { r: 255, g: 210, b: 110 };
        let value_err = Color::Rgb { r: 255, g: 120, b: 120 };
        let muted = Color::Rgb { r: 130, g: 130, b: 150 };

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
        let title = format!(
            " F5 · audio · {} pools · {:.1} plays/s ",
            audio::pool_count(),
            self.plays_per_sec
        );
        queue!(
            out,
            MoveTo(x0, y0),
            SetBackgroundColor(title_bg),
            SetForegroundColor(border),
            Print(pad(&title, w)),
        )?;
        let mut row = y0 + 1;

        // Engine summary.
        let mode = audio::audition_mode();
        let summary = format!(
            " audition={:?}  total plays issued: {}",
            mode,
            audio::plays_issued()
        );
        queue!(
            out,
            MoveTo(x0, row),
            SetBackgroundColor(bg),
            SetForegroundColor(muted),
            Print(pad(&summary, w)),
        )?;
        row += 1;

        // --- busses ----------------------------------------------------
        row = section_header(out, x0, row, w, " busses (from buses.toml) ", border, title_bg)?;
        if self.buses.is_empty() {
            queue!(
                out,
                MoveTo(x0, row),
                SetBackgroundColor(bg),
                SetForegroundColor(muted),
                Print(pad("  (buses.toml not loaded)", w)),
            )?;
            row += 1;
        } else {
            for bus in &self.buses {
                let text = format!(
                    " {:<18} {:<10}  {}",
                    truncate(&bus.name, 18),
                    truncate(&bus.gain_display, 10),
                    truncate(&bus.chain_summary, (w as usize).saturating_sub(32)),
                );
                queue!(
                    out,
                    MoveTo(x0, row),
                    SetBackgroundColor(bg),
                    SetForegroundColor(label),
                    Print(pad(&text, w)),
                )?;
                row += 1;
                if row >= y0 + h {
                    break;
                }
            }
        }

        // --- pools -----------------------------------------------------
        if row + 2 < y0 + h {
            row = section_header(out, x0, row, w, " loaded pools ", border, title_bg)?;
            let stats = audio::pool_stats();
            let shown = stats.iter().take(POOLS_TO_SHOW);
            for stat in shown {
                let color = if stat.file_count == 0 {
                    value_err
                } else if stat.file_count < 3 {
                    value_warn
                } else {
                    value_ok
                };
                let text = format!(
                    " {:<30} {:>3} take{}",
                    truncate(&stat.key, 30),
                    stat.file_count,
                    if stat.file_count == 1 { "" } else { "s" },
                );
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
            if stats.is_empty() {
                queue!(
                    out,
                    MoveTo(x0, row),
                    SetBackgroundColor(bg),
                    SetForegroundColor(muted),
                    Print(pad(
                        "  (no samples on disk — drop WAVs into content/core/audio/samples/)",
                        w,
                    )),
                )?;
                row += 1;
            } else if stats.len() > POOLS_TO_SHOW {
                queue!(
                    out,
                    MoveTo(x0, row),
                    SetBackgroundColor(bg),
                    SetForegroundColor(muted),
                    Print(pad(
                        &format!("  (+{} more)", stats.len() - POOLS_TO_SHOW),
                        w,
                    )),
                )?;
                row += 1;
            }
        }

        // --- recent emits ---------------------------------------------
        if row + 2 < y0 + h {
            row = section_header(out, x0, row, w, " recent emits ", border, title_bg)?;
            let emits = audio::recent_emits(EMITS_TO_SHOW);
            if emits.is_empty() {
                queue!(
                    out,
                    MoveTo(x0, row),
                    SetBackgroundColor(bg),
                    SetForegroundColor(muted),
                    Print(pad("  (no emits yet)", w)),
                )?;
                row += 1;
            } else {
                let now = now_ms();
                for rec in &emits {
                    let age_ms = now.saturating_sub(rec.at_ms);
                    let age_s = age_ms as f32 / 1000.0;
                    let flag = if rec.resolved { "✓" } else { "—" };
                    let color = if rec.resolved { value_ok } else { muted };
                    let pos = rec
                        .pos
                        .map(|(x, y)| format!("@({x:>5.0},{y:>4.0})"))
                        .unwrap_or_else(|| "        ".into());
                    let text = format!(
                        " {:>5.1}s {flag}  {:<24} {}",
                        age_s,
                        truncate(&rec.key, 24),
                        pos,
                    );
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
            }
        }

        // --- params ----------------------------------------------------
        if row + 2 < y0 + h {
            row = section_header(out, x0, row, w, " cv params (live) ", border, title_bg)?;
            let names: Vec<&'static str> = audio::params::known_names().collect();
            for name in names.iter().take(PARAMS_TO_SHOW) {
                let value = audio::params::get(name);
                let default = audio::params::default_for(name).unwrap_or(0.0);
                let tier = audio::params::tier(name)
                    .map(|t| format!("{:?}", t))
                    .unwrap_or_else(|| "?".into());
                let text = format!(
                    " {:<30} {:>6.3}  (default {:>5.2}, {})",
                    truncate(name, 30),
                    value,
                    default,
                    tier,
                );
                queue!(
                    out,
                    MoveTo(x0, row),
                    SetBackgroundColor(bg),
                    SetForegroundColor(label),
                    Print(pad(&text, w)),
                )?;
                row += 1;
                if row >= y0 + h {
                    break;
                }
            }
            if names.len() > PARAMS_TO_SHOW {
                queue!(
                    out,
                    MoveTo(x0, row),
                    SetBackgroundColor(bg),
                    SetForegroundColor(muted),
                    Print(pad(
                        &format!("  (+{} more)", names.len() - PARAMS_TO_SHOW),
                        w,
                    )),
                )?;
                // final row; no further advancement needed
                let _ = row;
            }
        }

        queue!(out, ResetColor)?;
        Ok(())
    }
}

fn section_header<W: Write>(
    out: &mut W,
    x0: u16,
    row: u16,
    w: u16,
    label: &str,
    fg: Color,
    bg: Color,
) -> Result<u16> {
    queue!(
        out,
        MoveTo(x0, row),
        SetBackgroundColor(bg),
        SetForegroundColor(fg),
        Print(pad(label, w)),
    )?;
    Ok(row + 1)
}

fn load_bus_rows(pack_root: &Path) -> Result<Vec<BusRow>> {
    let path = super::paths::audio_root(pack_root).join("buses.toml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)?;
    let parsed: super::schema::BusesFile = toml::from_str(&text)?;
    let mut rows: Vec<BusRow> = parsed
        .bus
        .into_iter()
        .map(|(name, def)| {
            let gain_display = format_gain(def.gain_db.as_ref());
            let chain_summary = if let Some(bus_type) = def.bus_type.as_ref() {
                format!("aux {bus_type}")
            } else if def.chain.is_empty() {
                "(no chain)".into()
            } else {
                def.chain
                    .iter()
                    .map(|n| n.node_type.as_str())
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            BusRow {
                name,
                gain_display,
                chain_summary,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

fn format_gain(v: Option<&toml::Value>) -> String {
    match v {
        None => "—".into(),
        Some(toml::Value::Integer(i)) => format!("{i} dB"),
        Some(toml::Value::Float(f)) => format!("{f:+.1} dB"),
        Some(toml::Value::String(s)) => format!("curve:{s}"),
        Some(other) => format!("{other}"),
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

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
