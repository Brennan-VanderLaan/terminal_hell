//! Audio engine — owns rodio's output stream on a dedicated worker
//! thread, scans the content tree into sample pools, and exposes a
//! send-safe `emit` API the rest of the codebase calls.
//!
//! Rodio's `OutputStream` is not `Send` on all platforms, so the
//! stream + handle live entirely inside `audio_worker`. Callers ship
//! paths across an mpsc channel; the worker decodes and plays. This
//! keeps the engine plant in one place and keeps every emit call a
//! non-blocking channel send.

use anyhow::{Context, Result, anyhow};
use arc_swap::ArcSwap;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::paths::{self, AUDITION_DIR, Category, VARIANTS};

/// How many emit events to retain for the debug overlay.
const EMIT_RING_CAPACITY: usize = 64;

/// Mode controlling whether the engine draws from locked samples,
/// audition-pool samples, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditionMode {
    Off,
    Mix,
    Only,
}

impl AuditionMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "mix" => Ok(Self::Mix),
            "only" => Ok(Self::Only),
            other => Err(anyhow!(
                "unknown audition mode `{other}` (expected off|mix|only)"
            )),
        }
    }
}

/// A round-robin sample pool keyed by "<unit>.<event>". The
/// `ArcSwap` is what the hot-reload watcher rebuilds into — in-flight
/// voices finish decoding on the old path, new `pick()` calls see the
/// new list.
pub struct Pool {
    paths: ArcSwap<Vec<PathBuf>>,
    cursor: AtomicUsize,
}

impl Pool {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths: ArcSwap::from_pointee(paths),
            cursor: AtomicUsize::new(0),
        }
    }

    fn pick(&self) -> Option<PathBuf> {
        let paths = self.paths.load();
        if paths.is_empty() {
            return None;
        }
        let idx = self.cursor.fetch_add(1, Ordering::Relaxed);
        Some(paths[idx % paths.len()].clone())
    }

    pub fn len(&self) -> usize {
        self.paths.load().len()
    }

    pub fn replace(&self, new: Vec<PathBuf>) {
        self.paths.store(Arc::new(new));
    }
}

enum AudioCmd {
    Play { path: PathBuf, volume: f32 },
    Shutdown,
}

/// Single emit event kept for the debug overlay / diagnostic panels.
#[derive(Debug, Clone)]
pub struct EmitRecord {
    /// Milliseconds since UNIX_EPOCH — stable enough for diff-since-now.
    pub at_ms: u64,
    /// Pool key: `<unit>.<event>`.
    pub key: String,
    /// World position if the caller provided one.
    pub pos: Option<(f32, f32)>,
    /// True when the pool was present and a path was selected; false when
    /// the emit was a no-op (missing pool / empty pool).
    pub resolved: bool,
}

/// Coarse per-pool snapshot for the overlay.
#[derive(Debug, Clone)]
pub struct PoolStat {
    pub key: String,
    pub file_count: usize,
}

pub struct AudioEngine {
    tx: Sender<AudioCmd>,
    pools: Arc<RwLock<HashMap<String, Arc<Pool>>>>,
    audition: AuditionMode,
    /// Running count of play-commands shipped to the worker. Used as
    /// a rough "voices triggered" meter in the overlay.
    plays_issued: Arc<AtomicU64>,
    /// Ring of recent emit events (newest at back). Oldest dropped
    /// when capacity is hit. Read by the overlay each refresh.
    emit_ring: Arc<Mutex<VecDeque<EmitRecord>>>,
}

impl AudioEngine {
    /// Spin up the engine. Starts the audio worker thread; if the
    /// default output device can't be opened the worker logs and
    /// exits, and every emit becomes a silent no-op.
    pub fn init(audition: AuditionMode) -> Result<Self> {
        let (tx, rx) = channel::<AudioCmd>();
        thread::Builder::new()
            .name("terminal-hell-audio".into())
            .spawn(move || audio_worker(rx))
            .context("spawn audio worker thread")?;
        Ok(Self {
            tx,
            pools: Arc::new(RwLock::new(HashMap::new())),
            audition,
            plays_issued: Arc::new(AtomicU64::new(0)),
            emit_ring: Arc::new(Mutex::new(VecDeque::with_capacity(EMIT_RING_CAPACITY))),
        })
    }

    /// Walk `<pack_root>/audio/samples/units/` and rebuild all pools.
    /// Call at startup, between bench iterations, or whenever the
    /// filesystem watcher signals a change.
    pub fn scan(&self, pack_root: &Path) -> Result<()> {
        let mut collected: HashMap<String, Vec<PathBuf>> = HashMap::new();

        let samples_root = paths::audio_root(pack_root).join("samples");
        if samples_root.is_dir() {
            // MVP: only scan unit samples. Weapons / primitives / ui /
            // carcosa / music get added as their emitters land.
            let cat_dir = samples_root.join(Category::Unit.dir_name());
            if cat_dir.is_dir() {
                for entry in std::fs::read_dir(&cat_dir)? {
                    let entry = entry?;
                    if !entry.file_type()?.is_dir() {
                        continue;
                    }
                    let id = entry.file_name().to_string_lossy().into_owned();
                    let id_dir = entry.path();

                    if self.audition != AuditionMode::Only {
                        collect_pool_files(&id_dir, &id, &mut collected)?;
                    }
                    if self.audition != AuditionMode::Off {
                        let aud = id_dir.join(AUDITION_DIR);
                        if aud.is_dir() {
                            collect_pool_files(&aud, &id, &mut collected)?;
                        }
                    }
                }
            }
        }

        let mut pools = self.pools.write().unwrap();
        pools.clear();
        let pool_count = collected.len();
        let file_count: usize = collected.values().map(|v| v.len()).sum();
        for (key, files) in collected {
            pools.insert(key, Arc::new(Pool::new(files)));
        }
        tracing::info!(
            pool_count,
            file_count,
            audition = ?self.audition,
            "audio: pools scanned"
        );
        Ok(())
    }

    /// Fire a one-shot sample for (`unit`, `event`). No-op if no pool
    /// exists or the pool is empty. Never blocks — sends a command to
    /// the worker and returns.
    pub fn emit(&self, unit: &str, event: &str, pos: Option<(f32, f32)>) {
        let key = format!("{unit}.{event}");
        let path = {
            let pools = self.pools.read().unwrap();
            pools.get(&key).and_then(|p| p.pick())
        };
        let resolved = path.is_some();
        if let Some(path) = path {
            self.plays_issued.fetch_add(1, Ordering::Relaxed);
            let _ = self.tx.send(AudioCmd::Play { path, volume: 1.0 });
        }
        self.push_emit_record(EmitRecord {
            at_ms: now_ms(),
            key,
            pos,
            resolved,
        });
    }

    fn push_emit_record(&self, rec: EmitRecord) {
        if let Ok(mut ring) = self.emit_ring.lock() {
            if ring.len() >= EMIT_RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(rec);
        }
    }

    /// Snapshot of currently-loaded pools, sorted by key. Used by the
    /// debug overlay — safe to call every frame.
    pub fn pool_stats(&self) -> Vec<PoolStat> {
        let pools = self.pools.read().unwrap();
        let mut out: Vec<PoolStat> = pools
            .iter()
            .map(|(k, p)| PoolStat {
                key: k.clone(),
                file_count: p.len(),
            })
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    }

    /// Last `n` emit events, newest first.
    pub fn recent_emits(&self, n: usize) -> Vec<EmitRecord> {
        let ring = match self.emit_ring.lock() {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        ring.iter().rev().take(n).cloned().collect()
    }

    /// Total play commands shipped to the worker since init.
    pub fn plays_issued(&self) -> u64 {
        self.plays_issued.load(Ordering::Relaxed)
    }

    pub fn pool_count(&self) -> usize {
        self.pools.read().unwrap().len()
    }

    pub fn audition_mode(&self) -> AuditionMode {
        self.audition
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCmd::Shutdown);
    }
}

/// Read a directory and register every `.wav` / `.ogg` file whose
/// name matches the `<event>_<variant>[_<take>]` convention into the
/// right pool key. Unknown / malformed filenames are skipped quietly.
fn collect_pool_files(
    dir: &Path,
    id: &str,
    collected: &mut HashMap<String, Vec<PathBuf>>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let os_name = entry.file_name();
        let name = os_name.to_string_lossy();
        let Some(dot) = name.rfind('.') else { continue };
        let ext = &name[dot + 1..];
        if !matches!(ext, "wav" | "ogg") {
            continue;
        }
        let stem = &name[..dot];
        let Some(event) = stem_to_event(stem) else { continue };
        let key = format!("{id}.{event}");
        collected.entry(key).or_default().push(entry.path());
    }
    Ok(())
}

/// From a file stem like `spawn_baseline_01` or `footstep_p50_02`,
/// extract the event portion (`spawn` / `footstep`) by looking for a
/// known variant. Returns None for unrecognized shapes so stray
/// files don't crash the scan.
fn stem_to_event(stem: &str) -> Option<String> {
    for variant in VARIANTS {
        let needle = format!("_{variant}_");
        if let Some(idx) = stem.find(&needle) {
            return Some(stem[..idx].to_string());
        }
        // Also accept `<event>_<variant>` with no take suffix.
        let tail = format!("_{variant}");
        if let Some(head) = stem.strip_suffix(&tail) {
            return Some(head.to_string());
        }
    }
    None
}

/// Worker thread: owns the rodio output stream for its lifetime and
/// handles play / shutdown commands. Decoding happens here so it
/// never touches the sim thread.
fn audio_worker(rx: std::sync::mpsc::Receiver<AudioCmd>) {
    let (stream, handle) = match OutputStream::try_default() {
        Ok(pair) => pair,
        Err(err) => {
            tracing::error!(?err, "audio: failed to open default output — audio disabled");
            // Drain the channel so senders don't block if the worker
            // never comes up.
            while rx.recv().is_ok() {}
            return;
        }
    };
    tracing::info!("audio: output stream opened");
    // Keep the stream alive for the worker's lifetime.
    let _stream = stream;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            AudioCmd::Play { path, volume } => {
                if let Err(err) = play_one(&handle, &path, volume) {
                    tracing::warn!(?err, ?path, "audio: play failed");
                }
            }
            AudioCmd::Shutdown => {
                tracing::info!("audio: worker shutdown");
                break;
            }
        }
    }
}

fn play_one(handle: &OutputStreamHandle, path: &Path, volume: f32) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {path:?}"))?;
    let decoder = Decoder::new(BufReader::new(file))
        .with_context(|| format!("decode {path:?}"))?;
    let sink = Sink::try_new(handle).context("create sink")?;
    sink.set_volume(volume);
    sink.append(decoder);
    sink.detach();
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// Tiny silencer so the unused `Instant` import in previous iterations
// wouldn't leak warnings on future refactors.
#[allow(dead_code)]
fn _instant_type_anchor() -> Instant {
    Instant::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audition_mode_parses() {
        assert_eq!(AuditionMode::parse("off").unwrap(), AuditionMode::Off);
        assert_eq!(AuditionMode::parse("MIX").unwrap(), AuditionMode::Mix);
        assert_eq!(AuditionMode::parse("only").unwrap(), AuditionMode::Only);
        assert!(AuditionMode::parse("nope").is_err());
    }

    #[test]
    fn stem_extracts_event_with_take() {
        assert_eq!(stem_to_event("spawn_baseline_01"), Some("spawn".into()));
        assert_eq!(stem_to_event("hit_reaction_p50_02"), Some("hit_reaction".into()));
        assert_eq!(stem_to_event("death_phantom_01"), Some("death".into()));
    }

    #[test]
    fn stem_extracts_event_without_take() {
        assert_eq!(stem_to_event("spawn_baseline"), Some("spawn".into()));
    }

    #[test]
    fn stem_rejects_garbage() {
        assert_eq!(stem_to_event("spawn"), None);
        assert_eq!(stem_to_event("random_file"), None);
    }
}
