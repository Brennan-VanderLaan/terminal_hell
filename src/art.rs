//! ASCII-art sprite loader. Each `.art` file is a grid of whitespace-
//! separated characters — `.` is transparent, other printable chars are
//! palette keys that look up a color in the archetype's TOML palette
//! table. Width is the max char count across rows; height is row count.
//!
//! Example:
//!
//! ```text
//! . . H . H . .
//! . . B B B . .
//! . B B E B B .
//! . D B B B D .
//! . D D B D D .
//! . D . . . D .
//! D . . . . . D
//! ```
//!
//! ```toml
//! [rusher.palette]
//! H = "#c82828"
//! B = "#ff3c3c"
//! E = "#ffe678"
//! D = "#821428"
//! ```
//!
//! This path is what makes adding a new archetype a pure-TOML affair:
//! drop the stats block, the palette table, and an `.art` file; no
//! Rust code touched.

use crate::fb::Pixel;
use crate::sprite::Sprite;
use crate::substance::parse_hex;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Parse an ASCII-art grid plus a palette map into a Sprite. Each row
/// is split on whitespace; '.' is transparent; other chars look up a
/// color in `palette`. Unknown palette keys are silently skipped (so
/// typos fail soft — the sprite just has missing pixels).
pub fn parse_art(raw: &str, palette: &HashMap<char, Pixel>) -> Result<Sprite> {
    let rows: Vec<Vec<char>> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            line.split_whitespace()
                .filter_map(|tok| tok.chars().next())
                .collect()
        })
        .collect();
    if rows.is_empty() {
        anyhow::bail!("empty art file");
    }
    let h = rows.len();
    let w = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if w == 0 {
        anyhow::bail!("art file has no visible columns");
    }
    let mut sprite = Sprite::new(w as u16, h as u16);
    for (y, row) in rows.iter().enumerate() {
        for (x, &ch) in row.iter().enumerate() {
            if ch == '.' {
                continue;
            }
            if let Some(&color) = palette.get(&ch) {
                sprite.set(x as u16, y as u16, color);
            }
        }
    }
    Ok(sprite)
}

/// Convert a `HashMap<String, String>` palette (as parsed by serde
/// from TOML) into a `HashMap<char, Pixel>` suitable for `parse_art`.
/// Keys must be single-char palette letters; values are hex colors.
pub fn palette_from_toml(map: &HashMap<String, String>) -> Result<HashMap<char, Pixel>> {
    let mut out = HashMap::with_capacity(map.len());
    for (k, v) in map {
        let ch = k
            .chars()
            .next()
            .with_context(|| format!("empty palette key"))?;
        if k.chars().count() != 1 {
            anyhow::bail!("palette key `{k}` must be a single character");
        }
        let color = parse_hex(v)
            .with_context(|| format!("palette color for `{k}`"))?;
        out.insert(ch, color);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_art() {
        let art = ". X .\nX X X\n. X .";
        let mut palette = HashMap::new();
        palette.insert('X', Pixel::rgb(255, 0, 0));
        let sprite = parse_art(art, &palette).unwrap();
        assert_eq!(sprite.w, 3);
        assert_eq!(sprite.h, 3);
    }

    #[test]
    fn unknown_palette_key_skips_silent() {
        let art = "X Q X";
        let mut palette = HashMap::new();
        palette.insert('X', Pixel::rgb(0, 255, 0));
        let sprite = parse_art(art, &palette).unwrap();
        // Q isn't in the palette so its pixel stays transparent.
        assert_eq!(sprite.w, 3);
    }
}
