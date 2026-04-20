//! Gameplay parameter registry — the bridge between CV envelopes and
//! the sim. Each parameter is an atomic f32 the sim reads every tick;
//! CV envelopes (or any other modulator) write to it.
//!
//! Tier tagging (`ClientOnly` vs `Authoritative`) is organizational
//! — it documents which params can be freely driven from client-side
//! music without desync, and which would need a server-authoritative
//! music clock to be touched safely. For this game's casual posture
//! (2–4 player co-op, host-authoritative positions) the Authoritative
//! tier starts empty; knobs get promoted only on demonstrated pain.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

/// Lock-free f32 storage backed by AtomicU32 (bit-cast).
pub struct AtomicF32 {
    inner: AtomicU32,
}

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self {
            inner: AtomicU32::new(v.to_bits()),
        }
    }
    #[inline]
    pub fn load(&self) -> f32 {
        f32::from_bits(self.inner.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn store(&self, v: f32) {
        self.inner.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Which reader owns a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Read locally on every client — no sync pressure. Safe for
    /// visual + feel knobs even with drifting music clocks.
    ClientOnly,
    /// Would need a broadcast music clock (or server-side CV
    /// playback) to read safely in multiplayer. Starts empty; add
    /// knobs here only when a specific desync shows up.
    Authoritative,
}

/// Declaration of a named parameter.
pub struct ParamDef {
    pub name: &'static str,
    pub tier: Tier,
    pub default: f32,
}

/// The shipped parameter table. CV routes in music stems reference
/// these names; `emit::param(name)` reads them from gameplay code.
///
/// Start small — add knobs only when a sim site + a music stem both
/// want one. Unused entries are dead weight.
pub const PARAMS: &[ParamDef] = &[
    ParamDef { name: "enemy.repel_scale_visual",    tier: Tier::ClientOnly, default: 1.0 },
    ParamDef { name: "particle.density_scale",      tier: Tier::ClientOnly, default: 1.0 },
    ParamDef { name: "particle.drag_scale",         tier: Tier::ClientOnly, default: 1.0 },
    ParamDef { name: "fx.screen_shake_scale",       tier: Tier::ClientOnly, default: 1.0 },
    ParamDef { name: "fx.chromatic_aberration",     tier: Tier::ClientOnly, default: 0.0 },
    ParamDef { name: "hud.scanline_strength",       tier: Tier::ClientOnly, default: 0.0 },
    ParamDef { name: "bg.tint_bias",                tier: Tier::ClientOnly, default: 0.0 },
    ParamDef { name: "enemy.glyph_flicker_rate",    tier: Tier::ClientOnly, default: 0.0 },
];

struct Registry {
    values: HashMap<&'static str, AtomicF32>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let mut values = HashMap::with_capacity(PARAMS.len());
        for def in PARAMS {
            values.insert(def.name, AtomicF32::new(def.default));
        }
        Registry { values }
    })
}

/// Read a parameter's current value. Returns the default if the name
/// isn't registered so gameplay code can tolerate missing params
/// during early development.
pub fn get(name: &str) -> f32 {
    match registry().values.get(name) {
        Some(slot) => slot.load(),
        None => default_for(name).unwrap_or(0.0),
    }
}

/// Write a parameter. No-op on unknown names so mis-spelled CV
/// routes don't panic the audio worker — they just show up as silent
/// in the audit log.
pub fn set(name: &str, value: f32) {
    if let Some(slot) = registry().values.get(name) {
        slot.store(value);
    } else {
        tracing::warn!(name, "audio::params::set on unknown param (ignored)");
    }
}

/// Restore a parameter to its declared default.
pub fn reset(name: &str) {
    if let Some(def) = PARAMS.iter().find(|d| d.name == name) {
        set(name, def.default);
    }
}

/// Restore every parameter to its declared default. Called between
/// scenario iterations so a half-loaded CV pool can't carry state
/// across reloads.
pub fn reset_all() {
    for def in PARAMS {
        if let Some(slot) = registry().values.get(def.name) {
            slot.store(def.default);
        }
    }
}

pub fn default_for(name: &str) -> Option<f32> {
    PARAMS.iter().find(|d| d.name == name).map(|d| d.default)
}

pub fn tier(name: &str) -> Option<Tier> {
    PARAMS.iter().find(|d| d.name == name).map(|d| d.tier)
}

pub fn known_names() -> impl Iterator<Item = &'static str> {
    PARAMS.iter().map(|d| d.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_loaded() {
        assert_eq!(get("particle.density_scale"), 1.0);
        assert_eq!(get("fx.chromatic_aberration"), 0.0);
    }

    #[test]
    fn set_get_roundtrip() {
        set("fx.screen_shake_scale", 1.7);
        assert!((get("fx.screen_shake_scale") - 1.7).abs() < 1e-6);
        reset("fx.screen_shake_scale");
        assert_eq!(get("fx.screen_shake_scale"), 1.0);
    }

    #[test]
    fn unknown_name_is_zero_and_no_panic() {
        assert_eq!(get("bogus.param"), 0.0);
        set("bogus.param", 1.0); // must not panic
    }

    #[test]
    fn tier_lookup() {
        assert_eq!(tier("particle.density_scale"), Some(Tier::ClientOnly));
        assert_eq!(tier("bogus"), None);
    }
}
