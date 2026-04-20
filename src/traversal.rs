//! Traversal verbs — movement primitives. Each verb gives the
//! player a distinctive mobility option on LShift tap: dash short,
//! blink through a wall, grapple toward aim, etc. One verb slot per
//! player; pickups replace the current verb.
//!
//! Verbs are intentionally small: each is a single state transition
//! with a cooldown. Input is a press-edge so holding LShift doesn't
//! spam the dash.

use crate::fb::Pixel;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraversalVerb {
    /// Short directional burst in the movement direction + brief
    /// i-frames. Cooldown 1.4s.
    Dash,
    /// Short teleport in the aim direction. Passes through walls.
    /// No i-frames. Cooldown 2.5s + small sanity cost per use.
    Blink,
    /// Hook in the aim direction — rapid pull toward the first
    /// wall / enemy / corpse. Cooldown 3.5s.
    Grapple,
    /// Slide forward at speed for 0.6s — keeps moving even if you
    /// release keys. Brief damage resistance during the slide.
    Slide,
    /// Phase: pass through the next wall tile you walk into for
    /// 2s. Cooldown 5s + sanity cost.
    Phase,
}

impl TraversalVerb {
    pub fn all() -> &'static [TraversalVerb] {
        &[
            TraversalVerb::Dash,
            TraversalVerb::Blink,
            TraversalVerb::Grapple,
            TraversalVerb::Slide,
            TraversalVerb::Phase,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            TraversalVerb::Dash => "Dash",
            TraversalVerb::Blink => "Blink",
            TraversalVerb::Grapple => "Grapple",
            TraversalVerb::Slide => "Slide",
            TraversalVerb::Phase => "Phase",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            TraversalVerb::Dash => 'D',
            TraversalVerb::Blink => 'B',
            TraversalVerb::Grapple => 'G',
            TraversalVerb::Slide => 'S',
            TraversalVerb::Phase => 'P',
        }
    }

    pub fn color(self) -> Pixel {
        match self {
            TraversalVerb::Dash => Pixel::rgb(80, 255, 200),
            TraversalVerb::Blink => Pixel::rgb(180, 140, 255),
            TraversalVerb::Grapple => Pixel::rgb(255, 180, 100),
            TraversalVerb::Slide => Pixel::rgb(160, 220, 255),
            TraversalVerb::Phase => Pixel::rgb(230, 230, 150),
        }
    }

    pub fn cooldown(self) -> f32 {
        match self {
            TraversalVerb::Dash => 1.4,
            TraversalVerb::Blink => 2.5,
            TraversalVerb::Grapple => 3.5,
            TraversalVerb::Slide => 2.0,
            TraversalVerb::Phase => 5.0,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "dash" => TraversalVerb::Dash,
            "blink" => TraversalVerb::Blink,
            "grapple" => TraversalVerb::Grapple,
            "slide" => TraversalVerb::Slide,
            "phase" => TraversalVerb::Phase,
            _ => return None,
        })
    }

    pub fn flavor(self) -> &'static str {
        match self {
            TraversalVerb::Dash => "quick dodge burst",
            TraversalVerb::Blink => "teleport through cover",
            TraversalVerb::Grapple => "hook + pull toward aim",
            TraversalVerb::Slide => "locked-in slide, brief resist",
            TraversalVerb::Phase => "walk through one wall tile",
        }
    }
}

pub fn roll_verb(rng: &mut rand::rngs::SmallRng) -> TraversalVerb {
    use rand::seq::SliceRandom;
    *TraversalVerb::all().choose(rng).unwrap_or(&TraversalVerb::Dash)
}
