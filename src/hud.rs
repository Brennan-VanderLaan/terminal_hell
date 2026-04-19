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

pub fn draw_hud<W: Write>(out: &mut W, wave: u32, hp: i32, kills: u32) -> Result<()> {
    let text = format!(" WAVE {:>3}   HP {:>3}   KILLS {:>3} ", wave, hp.max(0), kills);
    queue!(
        out,
        MoveTo(0, 0),
        SetForegroundColor(Color::Rgb { r: 255, g: 255, b: 255 }),
        SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
        Print(&text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
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
