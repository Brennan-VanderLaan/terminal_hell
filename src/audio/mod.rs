//! Audio system — see `audio-spec.md`.
//!
//! Modules shipped:
//! - `paths` — filesystem conventions (sample naming, audition-pool
//!   locations, content-root resolution).
//! - `schema` — serde types for TOML files under `content/<pack>/audio/`.
//! - `engine` — rodio-backed worker thread, sample pools, send-safe
//!   `emit` API.
//! - `audit` — coverage report; walks the content tree, cross-references
//!   the filesystem-as-matrix.
//! - `watch` — `notify`-backed filesystem watcher that rebuilds pools
//!   on save so the recorder's workflow lands in real time.
//! - `params` — atomic-f32 gameplay parameter registry (CV bridge).
//! - `cv` — CV envelope loader (schema + stub playback).
//! - `transitions` — `music_transition` API stub for boss-phase chains.
//!
//! The global `ENGINE` is initialized once per process from `main`. The
//! free functions (`init`, `scan`, `emit`, `music_transition`, …)
//! forward to it; when the engine isn't live they are no-ops so
//! callers don't have to guard.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;

pub mod audit;
pub mod cv;
pub mod engine;
pub mod overlay;
pub mod params;
pub mod paths;
pub mod schema;
pub mod transitions;
pub mod watch;

pub use engine::{AudioEngine, AuditionMode, EmitRecord, PoolStat};
pub use overlay::AudioOverlay;
pub use transitions::{Crossfade, music_transition};

static ENGINE: OnceLock<AudioEngine> = OnceLock::new();

/// Initialize the global audio engine. Errors if already initialized;
/// most callers should use `ensure_init` instead.
pub fn init(audition: AuditionMode) -> Result<()> {
    let engine = AudioEngine::init(audition)?;
    ENGINE
        .set(engine)
        .map_err(|_| anyhow::anyhow!("audio engine already initialized"))
}

/// Idempotent startup — initializes only if not already done.
pub fn ensure_init(audition: AuditionMode) -> Result<()> {
    if ENGINE.get().is_some() {
        return Ok(());
    }
    init(audition)
}

/// Rebuild sample pools from `<pack_root>/audio/samples/`. No-op
/// when the engine isn't live.
pub fn scan(pack_root: &Path) -> Result<()> {
    match ENGINE.get() {
        Some(e) => e.scan(pack_root),
        None => Ok(()),
    }
}

/// Fire a one-shot sample for (unit, event) at an optional world
/// position. No-op if the engine isn't live or the pool is empty.
pub fn emit(unit: &str, event: &str, pos: Option<(f32, f32)>) {
    if let Some(e) = ENGINE.get() {
        e.emit(unit, event, pos);
    }
}

pub fn pool_count() -> usize {
    ENGINE.get().map(|e| e.pool_count()).unwrap_or(0)
}

pub fn audition_mode() -> AuditionMode {
    ENGINE
        .get()
        .map(|e| e.audition_mode())
        .unwrap_or(AuditionMode::Off)
}

/// Snapshot of every loaded pool. Empty when the engine isn't live.
pub fn pool_stats() -> Vec<PoolStat> {
    ENGINE.get().map(|e| e.pool_stats()).unwrap_or_default()
}

/// Most recent `n` emit events, newest first.
pub fn recent_emits(n: usize) -> Vec<EmitRecord> {
    ENGINE.get().map(|e| e.recent_emits(n)).unwrap_or_default()
}

/// Total play commands shipped since init. Used by the overlay's
/// play-rate calculation.
pub fn plays_issued() -> u64 {
    ENGINE.get().map(|e| e.plays_issued()).unwrap_or(0)
}

/// Resolve the best-guess on-disk path to the core content pack.
/// Prefers a sibling `./content/core` (dev workflow / shipped layout)
/// then falls back to the binary's parent. Scan is tolerant of
/// non-existent paths so a missing tree yields an empty pool set
/// rather than an error.
pub fn default_content_root() -> PathBuf {
    let cwd = PathBuf::from("./content/core");
    if cwd.is_dir() {
        return cwd;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let next_to = parent.join("content").join("core");
            if next_to.is_dir() {
                return next_to;
            }
        }
    }
    cwd
}
