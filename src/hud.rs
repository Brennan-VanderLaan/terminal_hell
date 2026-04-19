//! Text overlays drawn directly to the terminal after the framebuffer blit.
//! The fb's diffing algorithm won't redraw cells that don't change beneath
//! the overlay, so the text stays put between frames until a particle or
//! player glyph repaints the cell underneath.

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::Write;

pub fn draw_hud<W: Write>(
    out: &mut W,
    wave: u32,
    hp: i32,
    kills: u32,
    corruption: f32,
    sanity: f32,
    marked: bool,
) -> Result<()> {
    let corrupt_bar = ascii_bar(corruption / 100.0, 10);
    let sanity_bar = ascii_bar(sanity / 100.0, 10);
    let mark_tag = if marked { " [MARKED]" } else { "" };
    let text = format!(
        " WAVE {:>3}   HP {:>3}   KILLS {:>3}   CORRUPT {:>3}% [{}]   SANITY {:>3}% [{}]{} ",
        wave,
        hp.max(0),
        kills,
        corruption.round() as i32,
        corrupt_bar,
        sanity.round() as i32,
        sanity_bar,
        mark_tag,
    );
    let fg = if marked {
        Color::Rgb { r: 255, g: 220, b: 100 }
    } else {
        Color::Rgb { r: 255, g: 235, b: 160 }
    };
    queue!(
        out,
        MoveTo(0, 0),
        SetForegroundColor(fg),
        SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
        Print(&text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}

fn ascii_bar(fraction: f32, len: usize) -> String {
    let filled = (fraction * len as f32).round().clamp(0.0, len as f32) as usize;
    (0..len).map(|i| if i < filled { '▓' } else { '▒' }).collect()
}

/// Render a weapon-loadout strip below the main HUD. Each slot shows its
/// rarity color and the letters of its primitive set; the active slot is
/// highlighted with a leading `>`.
pub fn draw_loadout<W: Write>(
    out: &mut W,
    loadout: &crate::game::LocalLoadoutView,
) -> Result<()> {
    for (i, slot) in loadout.slots.iter().enumerate() {
        let active = i as u8 == loadout.active_slot;
        let marker = if active { ">" } else { " " };
        let label = match slot {
            Some((rarity, prims)) => {
                let color = rarity.color();
                let letters: String = if prims.is_empty() {
                    "—".to_string()
                } else {
                    prims.iter().map(|p| p.glyph()).collect::<String>()
                };
                let text = format!("{} S{} [{:<6}] ", marker, i + 1, letters);
                (color, text)
            }
            None => (crate::fb::Pixel::rgb(100, 100, 100),
                     format!("{} S{} [ empty ] ", marker, i + 1)),
        };
        let (color, text) = label;
        queue!(
            out,
            MoveTo(0, 1 + i as u16),
            SetForegroundColor(Color::Rgb { r: color.r, g: color.g, b: color.b }),
            SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
            Print(&text),
            ResetColor,
        )?;
    }
    out.flush()?;
    Ok(())
}

/// Single-line intermission strip: phase label + time remaining + either
/// the active brand stack or the live vote tallies during the Vote phase.
pub fn draw_intermission<W: Write>(out: &mut W, game: &crate::game::Game) -> Result<()> {
    let (phase, timer) = game.display_phase();
    let Some(phase) = phase else {
        return Ok(());
    };
    let mut line = format!(" [{} {:>2}s] ", phase.label(), timer.ceil() as i32);

    use crate::waves::IntermissionPhase;
    if phase == IntermissionPhase::Vote && !game.kiosks.is_empty() {
        for k in &game.kiosks {
            line.push_str(&format!(
                "{}: {}  ",
                short_brand(&k.brand_id),
                k.votes
            ));
        }
        line.push_str("(walk + E to vote) ");
    } else {
        line.push_str("brands: ");
        for id in &game.active_brands {
            line.push_str(short_brand(id));
            line.push(' ');
        }
    }

    queue!(
        out,
        MoveTo(0, 3),
        SetForegroundColor(Color::Rgb { r: 255, g: 235, b: 150 }),
        SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
        Print(&line),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}

fn short_brand(id: &str) -> &str {
    match id {
        "fps_arena" => "FPS",
        "tactical" => "TAC",
        "chaos_roguelike" => "CHAOS",
        other => other,
    }
}

pub fn draw_wave_banner<W: Write>(
    out: &mut W,
    cols: u16,
    rows: u16,
    wave: u32,
    ttl: f32,
) -> Result<()> {
    if ttl <= 0.0 {
        return Ok(());
    }
    let text = format!("  WAVE {wave}  ");
    // Fade brightness from yellow-white in the first 0.4s, then hold.
    let bright = (ttl.min(1.0) * 255.0) as u8;
    let fg = Color::Rgb { r: 255, g: bright.max(150), b: 100 };
    let bg = Color::Rgb { r: 20, g: 0, b: 15 };
    let x = cols.saturating_sub(text.len() as u16) / 2;
    let y = rows / 3;
    queue!(
        out,
        MoveTo(x, y),
        SetForegroundColor(fg),
        SetBackgroundColor(bg),
        Print(&text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}

pub fn draw_connecting<W: Write>(out: &mut W) -> Result<()> {
    let text = "  CONNECTING...  ";
    queue!(
        out,
        MoveTo(0, 0),
        SetForegroundColor(Color::Rgb { r: 255, g: 240, b: 140 }),
        SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
        Print(text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}

pub fn draw_gameover<W: Write>(
    out: &mut W,
    cols: u16,
    rows: u16,
    wave: u32,
    kills: u32,
    elapsed_secs: u64,
) -> Result<()> {
    let mins = elapsed_secs / 60;
    let secs = elapsed_secs % 60;
    let lines: Vec<String> = vec![
        "                                  ".into(),
        "            YOU DIED              ".into(),
        "                                  ".into(),
        format!("    wave {:>3}    kills {:>3}       ", wave, kills),
        format!("    time {:>2}m {:02}s                ", mins, secs),
        "                                  ".into(),
        "     press any key to exit        ".into(),
        "                                  ".into(),
    ];
    let width = lines.iter().map(|l| l.len() as u16).max().unwrap_or(0);
    let height = lines.len() as u16;
    let x = cols.saturating_sub(width) / 2;
    let y = rows.saturating_sub(height) / 2;

    queue!(
        out,
        SetForegroundColor(Color::Rgb { r: 255, g: 240, b: 140 }),
        SetBackgroundColor(Color::Rgb { r: 30, g: 0, b: 15 }),
    )?;
    for (i, line) in lines.iter().enumerate() {
        queue!(out, MoveTo(x, y + i as u16), Print(line))?;
    }
    queue!(out, ResetColor)?;
    out.flush()?;
    Ok(())
}
