//! Music transition API — stub.
//!
//! Design lives in `audio-spec.md §11` + session-captured boss-phase
//! notes. The real scheduler lands with the music pipeline; this
//! module exists now so gameplay code can wire up `boss.death →
//! music_transition(...)` against a stable surface that doesn't
//! ghost-change when the scheduler arrives.

/// How the next stem enters relative to the current one.
#[derive(Debug, Clone, Copy)]
pub enum Crossfade {
    /// No crossfade — the new stem starts immediately, the old one
    /// cuts on the next sample. Rare in practice; used for stings.
    Immediate,
    /// Crossfade over N bars, aligned to the next bar boundary of the
    /// current stem. The common case.
    Bars(u32),
    /// Free-time crossfade over N seconds. Falls back to this when
    /// neither stem has a bar grid declared.
    Seconds(f32),
}

/// Request a music-stem transition. No-op today beyond a tracing
/// log; the intent is to get callers threaded through a stable
/// surface so wiring boss-phase chains doesn't wait on the
/// scheduler.
pub fn music_transition(stem_id: &str, xf: Crossfade) {
    tracing::info!(stem_id, xf = ?xf, "audio: music_transition (stub — scheduler pending)");
}
