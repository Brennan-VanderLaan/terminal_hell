//! Filesystem watcher — re-scans sample pools when files land on
//! disk. Implements the hot-reload half of `audio-spec.md §14.6`.
//!
//! Single debounced thread: raw `notify` events come in bursts
//! (CREATE + MODIFY + CLOSE_WRITE for one file write), so we
//! coalesce them with a short quiet window before triggering a
//! rescan. The rescan itself is cheap — reads `std::fs::read_dir`
//! entries — and the `ArcSwap` replacement of pool contents keeps
//! in-flight voices from stuttering.

use anyhow::{Context, Result};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

/// Settle window before triggering a rescan. Long enough to absorb a
/// typical save burst, short enough that the recorder's save → hear
/// latency still feels immediate.
const DEBOUNCE_MS: u64 = 200;

/// Spawn the watcher thread. Returns once the watcher is armed; the
/// thread runs for the lifetime of the process. Safe to call more
/// than once — each extra call just adds another watcher thread,
/// which is wasteful but not broken. `main` invokes this once.
pub fn spawn_watcher(content_root: PathBuf) -> Result<()> {
    let watch_root = content_root.join("audio").join("samples");
    if !watch_root.is_dir() {
        tracing::info!(
            path = %watch_root.display(),
            "audio watch: samples dir missing — watcher idle until created"
        );
    }
    thread::Builder::new()
        .name("terminal-hell-audio-watch".into())
        .spawn(move || {
            if let Err(err) = watcher_main(content_root) {
                tracing::error!(?err, "audio watcher exited with error");
            }
        })
        .context("spawn audio watcher thread")?;
    Ok(())
}

fn watcher_main(content_root: PathBuf) -> Result<()> {
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
        .context("create notify watcher")?;

    let watch_root = content_root.join("audio").join("samples");
    if watch_root.is_dir() {
        watcher
            .watch(&watch_root, RecursiveMode::Recursive)
            .context("arm watcher on samples dir")?;
        tracing::info!(path = %watch_root.display(), "audio watch: armed");
    }

    let mut deadline: Option<Instant> = None;

    loop {
        let poll = Duration::from_millis(50);
        match rx.recv_timeout(poll) {
            Ok(Ok(event)) => {
                if relevant(event.kind) {
                    deadline = Some(Instant::now() + Duration::from_millis(DEBOUNCE_MS));
                }
            }
            Ok(Err(err)) => {
                tracing::warn!(?err, "audio watch: notify error");
            }
            Err(RecvTimeoutError::Timeout) => {
                // No event — check debounce.
                if let Some(at) = deadline {
                    if Instant::now() >= at {
                        if let Err(err) = crate::audio::scan(&content_root) {
                            tracing::warn!(?err, "audio watch: rescan failed");
                        }
                        deadline = None;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                tracing::info!("audio watch: channel disconnected, exiting");
                return Ok(());
            }
        }
    }
}

fn relevant(kind: EventKind) -> bool {
    // Ignore access-only events; we only care when content might
    // have changed. `Any` covers platform backends that don't
    // classify events precisely.
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}
