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
    // Shifted to rows 4/5 so it doesn't collide with brand strip
    // (row 1), perk strip (row 2), intermission strip (row 3).
    for (i, slot) in loadout.slots.iter().enumerate() {
        let active = i as u8 == loadout.active_slot;
        let marker = if active { ">" } else { " " };
        let label = match slot {
            Some((rarity, prims, mode)) => {
                let color = rarity.color();
                let letters: String = if prims.is_empty() {
                    "—".to_string()
                } else {
                    prims.iter().map(|p| p.glyph()).collect::<String>()
                };
                let text = format!(
                    "{} S{} {} {} [{:<6}] ",
                    marker,
                    i + 1,
                    mode.glyph(),
                    mode.label(),
                    letters,
                );
                (color, text)
            }
            None => (
                crate::fb::Pixel::rgb(100, 100, 100),
                format!("{} S{} [ empty ] ", marker, i + 1),
            ),
        };
        let (color, text) = label;
        queue!(
            out,
            MoveTo(0, 4 + i as u16),
            SetForegroundColor(Color::Rgb { r: color.r, g: color.g, b: color.b }),
            SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
            Print(&text),
            ResetColor,
        )?;
    }
    out.flush()?;
    Ok(())
}

/// Big centered "⏸ PAUSED" banner rendered when the host has paused the
/// run. Visible to all peers. Uses the same layered-overlay pattern as
/// the wave banner.
/// Thin banner shown when the local player is dead but the run is
/// still going (other survivors are alive). No Director UI yet — the
/// dead player can just watch. Also visible to the local player as an
/// explicit death acknowledgement on top of HP=0.
pub fn draw_dead_banner<W: Write>(out: &mut W, cols: u16, rows: u16) -> Result<()> {
    let text = "  ✝ YOU ARE DEAD — watching the survivors  ";
    let text_len = text.chars().count() as u16;
    let x = cols.saturating_sub(text_len) / 2;
    let y = rows / 6;
    queue!(
        out,
        MoveTo(x, y),
        SetForegroundColor(Color::Rgb { r: 255, g: 220, b: 200 }),
        SetBackgroundColor(Color::Rgb { r: 100, g: 10, b: 20 }),
        Print(text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}

pub fn draw_paused_banner<W: Write>(
    out: &mut W,
    cols: u16,
    rows: u16,
    is_authoritative: bool,
) -> Result<()> {
    let text = if is_authoritative {
        "  ⏸ PAUSED — press ESC → Unpause  "
    } else {
        "  ⏸ HOST PAUSED  "
    };
    let text_len = text.chars().count() as u16;
    let x = cols.saturating_sub(text_len) / 2;
    let y = rows / 5;
    queue!(
        out,
        MoveTo(x, y),
        SetForegroundColor(Color::Rgb { r: 30, g: 10, b: 40 }),
        SetBackgroundColor(Color::Rgb { r: 255, g: 220, b: 120 }),
        Print(text),
        ResetColor,
    )?;
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
        "doom_inferno" => "DOOM",
        "quake_arena" => "QUAKE",
        "serious_sam" => "SAM",
        "tarkov_scavs" => "TARKOV",
        "stalker_zone" => "STALKER",
        "vampire_survivors" => "VAMP",
        "risk_of_rain" => "RoR",
        "left_4_dead" => "L4D",
        "helldivers" => "HELL",
        "deep_rock" => "DRG",
        "resident_evil" => "RE",
        "dead_space" => "DS",
        "zerg_tide" => "ZERG",
        "halo_flood" => "FLOOD",
        "rimworld_mechs" => "MECH",
        "factorio_biters" => "BITER",
        "soulsborne_invader" => "SOULS",
        other => other,
    }
}

/// RGB color per brand id — same palette used by the vote kiosks +
/// the persistent active-brand strip so a glance tells you what's
/// bleeding in.
fn brand_rgb(id: &str) -> (u8, u8, u8) {
    match id {
        "doom_inferno" => (255, 100, 40),
        "quake_arena" => (200, 80, 60),
        "serious_sam" => (255, 180, 60),
        "tarkov_scavs" => (150, 170, 100),
        "stalker_zone" => (180, 160, 90),
        "vampire_survivors" => (220, 140, 255),
        "risk_of_rain" => (255, 120, 180),
        "left_4_dead" => (120, 200, 80),
        "helldivers" => (255, 210, 70),
        "deep_rock" => (220, 150, 60),
        "resident_evil" => (160, 80, 60),
        "dead_space" => (180, 220, 180),
        "zerg_tide" => (160, 80, 200),
        "halo_flood" => (140, 240, 150),
        "rimworld_mechs" => (180, 180, 200),
        "factorio_biters" => (120, 200, 120),
        "soulsborne_invader" => (220, 200, 160),
        _ => (220, 220, 220),
    }
}

/// Perk strip on row 2 — one glyph per active perk, color-coded.
pub fn draw_perks<W: Write>(out: &mut W, perks: &[crate::perk::Perk]) -> Result<()> {
    if perks.is_empty() {
        return Ok(());
    }
    let bg = Color::Rgb { r: 20, g: 10, b: 30 };
    let label_fg = Color::Rgb { r: 180, g: 180, b: 210 };
    queue!(
        out,
        MoveTo(0, 2),
        SetBackgroundColor(bg),
        SetForegroundColor(label_fg),
        Print(" PERKS "),
    )?;
    for perk in perks {
        let c = perk.color();
        queue!(
            out,
            SetForegroundColor(Color::Rgb { r: c.r, g: c.g, b: c.b }),
            Print(format!("[{}] ", perk.glyph())),
        )?;
    }
    queue!(out, ResetColor)?;
    out.flush()?;
    Ok(())
}

/// Full-screen inventory overlay toggled with Tab. Shows perks with
/// names + flavor, consumable counts, armor + HP bars, and the
/// current weapon loadout + primitives.
pub fn draw_inventory<W: Write>(
    out: &mut W,
    cols: u16,
    rows: u16,
    player: &crate::player::Player,
    loadout: Option<&crate::game::LocalLoadoutView>,
) -> Result<()> {
    let bg = Color::Rgb { r: 10, g: 8, b: 20 };
    let border = Color::Rgb { r: 255, g: 200, b: 100 };
    let label = Color::Rgb { r: 200, g: 200, b: 230 };
    let val = Color::Rgb { r: 255, g: 255, b: 220 };
    let dim = Color::Rgb { r: 140, g: 140, b: 170 };

    // Panel covers the middle 60% of the screen.
    let pw = (cols as f32 * 0.7) as u16;
    let ph = (rows as f32 * 0.7) as u16;
    let x0 = (cols.saturating_sub(pw)) / 2;
    let y0 = (rows.saturating_sub(ph)) / 2;

    // Wipe panel.
    for row in 0..ph {
        queue!(
            out,
            MoveTo(x0, y0 + row),
            SetBackgroundColor(bg),
            SetForegroundColor(label),
            Print(" ".repeat(pw as usize)),
        )?;
    }

    // Title.
    let title = " ≡ INVENTORY · Tab to close ≡ ";
    let tx = x0 + (pw.saturating_sub(title.chars().count() as u16)) / 2;
    queue!(
        out,
        MoveTo(tx, y0),
        SetBackgroundColor(Color::Rgb { r: 40, g: 25, b: 55 }),
        SetForegroundColor(border),
        Print(title),
    )?;

    let mut row = y0 + 2;

    // Stats block.
    queue!(
        out,
        MoveTo(x0 + 2, row),
        SetBackgroundColor(bg),
        SetForegroundColor(val),
        Print(format!(
            "HP {:>3}/{:<3}   ARMOR {:>3}/{:<3}   SANITY {:>3}/100",
            player.hp.max(0),
            crate::player::PLAYER_MAX_HP,
            player.armor,
            player.max_armor(),
            player.sanity.round() as i32,
        )),
    )?;
    row += 2;

    // Consumables.
    queue!(
        out,
        MoveTo(x0 + 2, row),
        SetForegroundColor(label),
        Print("consumables:"),
    )?;
    row += 1;
    queue!(
        out,
        MoveTo(x0 + 4, row),
        SetForegroundColor(val),
        Print(format!("turret kits  × {}", player.turret_kits)),
    )?;
    row += 2;

    // Loadout.
    queue!(
        out,
        MoveTo(x0 + 2, row),
        SetForegroundColor(label),
        Print("loadout:"),
    )?;
    row += 1;
    if let Some(lo) = loadout {
        for (i, slot) in lo.slots.iter().enumerate() {
            let active = i as u8 == lo.active_slot;
            let marker = if active { ">" } else { " " };
            let text = match slot {
                Some((rarity, prims, mode)) => {
                    let prim_str: String = if prims.is_empty() {
                        "—".to_string()
                    } else {
                        prims
                            .iter()
                            .map(|p| p.glyph().to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    format!(
                        "{marker} slot {} · {:?} · {} {} [{}]",
                        i,
                        rarity,
                        mode.glyph(),
                        mode.label(),
                        prim_str,
                    )
                }
                None => format!("{marker} slot {} · empty", i),
            };
            queue!(
                out,
                MoveTo(x0 + 4, row),
                SetForegroundColor(val),
                Print(text),
            )?;
            row += 1;
        }
    }
    row += 1;

    // Perks block.
    queue!(
        out,
        MoveTo(x0 + 2, row),
        SetForegroundColor(label),
        Print(format!("perks ({}):", player.perks.len())),
    )?;
    row += 1;
    if player.perks.is_empty() {
        queue!(
            out,
            MoveTo(x0 + 4, row),
            SetForegroundColor(dim),
            Print("(none yet — kill a miniboss or survive to wave 5)"),
        )?;
        let _ = row;
    } else {
        for perk in &player.perks {
            let c = perk.color();
            queue!(
                out,
                MoveTo(x0 + 4, row),
                SetForegroundColor(Color::Rgb { r: c.r, g: c.g, b: c.b }),
                Print(format!("[{}] {:<14}", perk.glyph(), perk.name())),
                SetForegroundColor(dim),
                Print(format!(" {}", perk.flavor())),
            )?;
            row += 1;
            if row >= y0 + ph - 1 {
                break;
            }
        }
    }

    queue!(out, ResetColor)?;
    out.flush()?;
    Ok(())
}

/// Persistent strip listing every active brand, color-coded, always
/// visible during combat. Stacks left-to-right on row 1 under the
/// main HUD so a glance tells you what's bleeding in.
pub fn draw_active_brands<W: Write>(
    out: &mut W,
    active_brands: &[String],
) -> Result<()> {
    if active_brands.is_empty() {
        return Ok(());
    }
    let bg = Color::Rgb { r: 20, g: 10, b: 30 };
    let label_fg = Color::Rgb { r: 180, g: 180, b: 210 };
    queue!(
        out,
        MoveTo(0, 1),
        SetBackgroundColor(bg),
        SetForegroundColor(label_fg),
        Print(" BRANDS "),
    )?;
    for (i, id) in active_brands.iter().enumerate() {
        let (r, g, b) = brand_rgb(id);
        if i > 0 {
            queue!(
                out,
                SetForegroundColor(Color::Rgb { r: 120, g: 120, b: 140 }),
                Print("· "),
            )?;
        }
        queue!(
            out,
            SetForegroundColor(Color::Rgb { r, g, b }),
            Print(format!("{} ", short_brand(id))),
        )?;
    }
    queue!(out, ResetColor)?;
    out.flush()?;
    Ok(())
}

/// Label each active kiosk with its brand name + vote count, positioned
/// above the kiosk glyph in screen space. Only renders labels that fit
/// on-screen. Color is the brand signature color.
/// Floating text labels drawn above each pickup so players can
/// read what's on the ground at a glance. Only shown at the
/// Normal/Close/Hero mip where there's room to read them. Close-in
/// labels get a brighter tint since those are the ones the player
/// is actually standing next to.
pub fn draw_pickup_labels<W: Write>(
    out: &mut W,
    game: &crate::game::Game,
) -> Result<()> {
    use crate::camera::MipLevel;
    if game.pickups.is_empty() {
        return Ok(());
    }
    // Labels only legible at reasonable zoom levels — skip the
    // Blob/Dot mips where the symbol cluster is a single pixel.
    let mip = game.camera.mip_level();
    if matches!(mip, MipLevel::Blob | MipLevel::Dot) {
        return Ok(());
    }
    let camera = &game.camera;
    let total_cols = camera.viewport_w / 2;
    let total_rows = camera.viewport_h / 3;
    let (local_x, local_y) = game
        .local_player()
        .map(|p| (p.x, p.y))
        .unwrap_or((0.0, 0.0));
    for pickup in &game.pickups {
        let (sx, sy) = camera.world_to_screen((pickup.x, pickup.y));
        let col = (sx / 2.0) as i32;
        let row = ((sy / 3.0) as i32) - 2;
        if row < 0 || row >= total_rows as i32 {
            continue;
        }
        let text = pickup.kind.label();
        let half = (text.chars().count() as i32) / 2;
        let start_col = (col - half).max(0);
        if start_col >= total_cols as i32 {
            continue;
        }
        // Brighten labels on pickups the player is actually standing
        // on or next to, dim the rest so the "active" pickup pops.
        let dx = pickup.x - local_x;
        let dy = pickup.y - local_y;
        let near = dx * dx + dy * dy <= 16.0 * 16.0;
        let c = pickup.kind.halo_color();
        let fg = if near {
            Color::Rgb { r: c.r, g: c.g, b: c.b }
        } else {
            // Dimmed variant — still on-brand, just quieter.
            Color::Rgb {
                r: (c.r as u16 * 6 / 10) as u8,
                g: (c.g as u16 * 6 / 10) as u8,
                b: (c.b as u16 * 6 / 10) as u8,
            }
        };
        queue!(
            out,
            MoveTo(start_col as u16, row as u16),
            SetForegroundColor(fg),
            SetBackgroundColor(Color::Rgb { r: 15, g: 8, b: 25 }),
            Print(&text),
        )?;
        // Interact hint for equipment — only shown when the player
        // is close enough to press E on it.
        if near && pickup.kind.requires_interact() {
            let hint = " [E]";
            queue!(
                out,
                MoveTo((start_col + text.chars().count() as i32) as u16, row as u16),
                SetForegroundColor(Color::Rgb { r: 255, g: 220, b: 140 }),
                Print(hint),
            )?;
        }
    }
    queue!(out, ResetColor)?;
    out.flush()?;
    Ok(())
}

/// Stack of recent-pickup notifications at the right edge of the
/// screen. Newest at top, fading out as TTL drains.
pub fn draw_pickup_toasts<W: Write>(
    out: &mut W,
    cols: u16,
    toasts: &[crate::game::PickupToast],
) -> Result<()> {
    if toasts.is_empty() {
        return Ok(());
    }
    // Start below the zoom badge + loadout strip so we don't collide.
    let base_row: u16 = 7;
    for (i, t) in toasts.iter().enumerate() {
        let fade = (t.ttl / t.ttl_max).clamp(0.0, 1.0);
        // Fade the color toward black as TTL drains so expiring
        // toasts visibly dim before they disappear.
        let fg = Color::Rgb {
            r: (t.color.r as f32 * fade) as u8,
            g: (t.color.g as f32 * fade) as u8,
            b: (t.color.b as f32 * fade) as u8,
        };
        let text = format!(" + {} ", t.text);
        let len = text.chars().count() as u16;
        let col = cols.saturating_sub(len);
        let row = base_row + i as u16;
        queue!(
            out,
            MoveTo(col, row),
            SetForegroundColor(fg),
            SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
            Print(&text),
            ResetColor,
        )?;
    }
    out.flush()?;
    Ok(())
}

pub fn draw_kiosk_labels<W: Write>(
    out: &mut W,
    game: &crate::game::Game,
) -> Result<()> {
    if game.kiosks.is_empty() {
        return Ok(());
    }
    let camera = &game.camera;
    let total_cols = camera.viewport_w / 2;
    let total_rows = camera.viewport_h / 3;
    for kiosk in &game.kiosks {
        let (sx, sy) = camera.world_to_screen((kiosk.x, kiosk.y));
        let col = (sx / 2.0) as i32;
        // Draw 2 terminal rows above the kiosk.
        let row = ((sy / 3.0) as i32) - 3;
        if row < 0 || row >= total_rows as i32 {
            continue;
        }
        let text = format!("{} ({})", kiosk.brand_name, kiosk.votes);
        let half = (text.len() as i32) / 2;
        let start_col = (col - half).max(0);
        if start_col >= total_cols as i32 {
            continue;
        }
        let color = kiosk.brand_color;
        queue!(
            out,
            MoveTo(start_col as u16, row as u16),
            SetForegroundColor(Color::Rgb { r: color.r, g: color.g, b: color.b }),
            SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
            Print(&text),
            ResetColor,
        )?;
    }
    out.flush()?;
    Ok(())
}

/// Small badge in the upper-right showing current zoom level + a hint that
/// mouse-wheel adjusts it. Helpful when players are first getting used to
/// the camera.
pub fn draw_zoom_indicator<W: Write>(
    out: &mut W,
    cols: u16,
    zoom: f32,
) -> Result<()> {
    use crate::camera::{MIP_BLOB_MIN_ZOOM, MIP_CLOSE_MIN_ZOOM, MIP_HERO_MIN_ZOOM, MIP_NORMAL_MIN_ZOOM};
    let tier = if zoom >= MIP_HERO_MIN_ZOOM {
        "HERO"
    } else if zoom >= MIP_CLOSE_MIN_ZOOM {
        "CLOSE"
    } else if zoom >= MIP_NORMAL_MIN_ZOOM {
        "NORM"
    } else if zoom >= MIP_BLOB_MIN_ZOOM {
        "BLOB"
    } else {
        "DOT"
    };
    let text = format!(" ZOOM {:>4.2}x {:<5} ", zoom, tier);
    let col = cols.saturating_sub(text.len() as u16);
    queue!(
        out,
        MoveTo(col, 0),
        SetForegroundColor(Color::Rgb { r: 200, g: 220, b: 255 }),
        SetBackgroundColor(Color::Rgb { r: 20, g: 10, b: 30 }),
        Print(&text),
        ResetColor,
    )?;
    out.flush()?;
    Ok(())
}

/// Small countdown strip shown during the `Clearing` state so
/// survivors know the director will force the next wave soon.
/// Row 3 (below brand + perk strips, above loadout rows 4/5). Called
/// every frame from the render loop; early-outs when `timer <= 0`.
pub fn draw_clear_countdown<W: Write>(
    out: &mut W,
    cols: u16,
    timer: f32,
) -> Result<()> {
    if timer <= 0.0 {
        return Ok(());
    }
    // Flash color red as the deadline closes.
    let warn = timer <= 10.0;
    let fg = if warn {
        Color::Rgb { r: 255, g: 90, b: 80 }
    } else {
        Color::Rgb { r: 255, g: 220, b: 140 }
    };
    let text = format!(" next wave in {:>4.1}s (kill stragglers to skip) ", timer);
    let col = cols.saturating_sub(text.chars().count() as u16);
    queue!(
        out,
        MoveTo(col, 3),
        SetForegroundColor(fg),
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
