//! WAV read/write, path resolution, rodio preview, and the session
//! log. Consolidated because these concerns all orbit the same
//! filesystem tree and splitting them across three files adds no
//! navigational value.

use anyhow::{Context, Result, anyhow};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use terminal_hell::audio::paths::{self, AUDITION_DIR, Category};

// ---------------------------------------------------------------------------
// destination vocabulary — one per captured channel

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Discard,
    /// Write this channel (mono) to a unit/event slot. `audition`
    /// routes into the `_audition/` pool instead of locked.
    Slot {
        unit_id: String,
        event: String,
        variant: String,
        audition: bool,
    },
    /// Write this channel to a named CV parameter folder. Stored
    /// mono. Always an audition by default — CV promotion is an
    /// explicit action (the same as sample slots).
    CvParam {
        param: String,
        stem: String,
        audition: bool,
    },
    /// Part of a stereo pair. `StereoLeft` carries the destination;
    /// `StereoRight` names its paired channel so save logic can
    /// interleave them into one WAV.
    StereoLeft {
        unit_id: String,
        event: String,
        variant: String,
        audition: bool,
        paired_right: usize,
    },
    StereoRight {
        paired_left: usize,
    },
}

impl Default for Destination {
    fn default() -> Self {
        Destination::Discard
    }
}

impl Destination {
    pub fn short_label(&self) -> String {
        match self {
            Destination::Discard => "discard".into(),
            Destination::Slot { unit_id, event, variant, audition } => {
                let tag = if *audition { "audition" } else { "locked" };
                format!("{unit_id}.{event}.{variant} [{tag}]")
            }
            Destination::CvParam { param, stem, audition } => {
                let tag = if *audition { "audition" } else { "locked" };
                format!("cv[{param}] @ {stem} [{tag}]")
            }
            Destination::StereoLeft { unit_id, event, variant, audition, paired_right } => {
                let tag = if *audition { "audition" } else { "locked" };
                format!(
                    "{unit_id}.{event}.{variant} [stereo L, R=ch{}] [{tag}]",
                    paired_right + 1
                )
            }
            Destination::StereoRight { paired_left } => {
                format!("stereo R (paired with ch{})", paired_left + 1)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// session-type hint drives which destinations are offered per channel

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    /// Treat every channel as a candidate unit/event sample. Default.
    Sfx,
    /// ch0+ch1 stereo music; remaining channels CV params.
    MusicCv,
    /// Every channel is a CV param. No audio written.
    CvOnly,
}

impl SessionType {
    pub fn label(self) -> &'static str {
        match self {
            SessionType::Sfx => "SFX",
            SessionType::MusicCv => "Music + CV",
            SessionType::CvOnly => "CV only",
        }
    }
}

// ---------------------------------------------------------------------------
// WAV write helpers — hound wraps libsndfile-free pure-Rust codec

pub fn write_mono_wav(path: &Path, samples: &[f32], rate: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("open {} for write", path.display()))?;
    for &s in samples {
        writer.write_sample(s).context("write sample")?;
    }
    writer.finalize().context("finalize wav")?;
    Ok(())
}

pub fn write_stereo_wav(path: &Path, left: &[f32], right: &[f32], rate: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    let n = left.len().min(right.len());
    for i in 0..n {
        writer.write_sample(left[i])?;
        writer.write_sample(right[i])?;
    }
    writer.finalize()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// path resolution — delegates to terminal_hell::audio::paths for
// anything that isn't a th_record-specific concept

pub fn resolve_slot_path(
    pack_root: &Path,
    unit_id: &str,
    event: &str,
    variant: &str,
    audition: bool,
) -> PathBuf {
    let base = paths::sample_dir(pack_root, Category::Unit, unit_id);
    if audition {
        base.join(AUDITION_DIR)
            .join(format!("{}.wav", paths::audition_stem(event, variant, monotonic_stamp())))
    } else {
        // Find next free locked take index.
        let next = next_locked_take(&base, event, variant);
        base.join(format!("{}.wav", paths::locked_stem(event, variant, next)))
    }
}

pub fn resolve_cv_path(
    pack_root: &Path,
    param: &str,
    stem: &str,
    audition: bool,
) -> PathBuf {
    let base = pack_root.join("audio").join("cv").join(param);
    if audition {
        base.join(AUDITION_DIR)
            .join(format!("{}_{}.wav", stem, monotonic_stamp()))
    } else {
        let next = next_locked_cv_take(&base, stem);
        base.join(format!("{}_{:02}.wav", stem, next))
    }
}

fn next_locked_take(dir: &Path, event: &str, variant: &str) -> u32 {
    if !dir.is_dir() {
        return 1;
    }
    let Ok(entries) = fs::read_dir(dir) else { return 1 };
    let mut max = 0u32;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        let stem = match s.rsplit_once('.') {
            Some((st, _)) => st,
            None => continue,
        };
        // Expected shape: <event>_<variant>_<nn>.
        let prefix = format!("{event}_{variant}_");
        if let Some(rest) = stem.strip_prefix(&prefix) {
            if let Ok(n) = rest.parse::<u32>() {
                if n > max {
                    max = n;
                }
            }
        }
    }
    max + 1
}

fn next_locked_cv_take(dir: &Path, stem: &str) -> u32 {
    if !dir.is_dir() {
        return 1;
    }
    let Ok(entries) = fs::read_dir(dir) else { return 1 };
    let mut max = 0u32;
    let prefix = format!("{stem}_");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        let Some((body, _)) = s.rsplit_once('.') else { continue };
        if let Some(rest) = body.strip_prefix(&prefix) {
            if let Ok(n) = rest.parse::<u32>() {
                if n > max {
                    max = n;
                }
            }
        }
    }
    max + 1
}

fn monotonic_stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// pool listing — reads the current locked + audition takes for a slot
// so the GUI can display and act on them

#[derive(Debug, Clone)]
pub struct PoolEntry {
    pub path: PathBuf,
    pub display: String,
    pub audition: bool,
}

pub fn list_slot_pool(
    pack_root: &Path,
    unit_id: &str,
    event: &str,
    variant: &str,
) -> Vec<PoolEntry> {
    let mut out = Vec::new();
    let base = paths::sample_dir(pack_root, Category::Unit, unit_id);
    collect_pool_entries(&base, event, variant, false, &mut out);
    collect_pool_entries(&base.join(AUDITION_DIR), event, variant, true, &mut out);
    out.sort_by(|a, b| a.audition.cmp(&b.audition).then(a.display.cmp(&b.display)));
    out
}

fn collect_pool_entries(
    dir: &Path,
    event: &str,
    variant: &str,
    audition: bool,
    out: &mut Vec<PoolEntry>,
) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let s = name.to_string_lossy().into_owned();
        let Some((stem, ext)) = s.rsplit_once('.') else { continue };
        if !matches!(ext, "wav" | "ogg") {
            continue;
        }
        if paths::file_matches(stem, event, variant) {
            out.push(PoolEntry {
                path: entry.path(),
                display: s,
                audition,
            });
        }
    }
}

/// Move a file from the `_audition/` pool to the locked pool with
/// the next free take index.
pub fn promote(
    pack_root: &Path,
    unit_id: &str,
    event: &str,
    variant: &str,
    audition_path: &Path,
) -> Result<PathBuf> {
    let locked_dir = paths::sample_dir(pack_root, Category::Unit, unit_id);
    fs::create_dir_all(&locked_dir)?;
    let next = next_locked_take(&locked_dir, event, variant);
    let dest = locked_dir.join(format!("{}.wav", paths::locked_stem(event, variant, next)));
    fs::rename(audition_path, &dest)
        .with_context(|| format!("rename {} → {}", audition_path.display(), dest.display()))?;
    Ok(dest)
}

pub fn trash(path: &Path) -> Result<()> {
    fs::remove_file(path).with_context(|| format!("delete {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// preview playback — rodio in-process so clicking a take plays it
// through the local output device

pub struct Preview {
    stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
}

impl Preview {
    pub fn new() -> Result<Self> {
        let (stream, handle) = rodio::OutputStream::try_default()
            .context("open default audio output for preview")?;
        Ok(Self { stream, handle })
    }
    pub fn play(&self, path: &Path) -> Result<()> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let source = rodio::Decoder::new(BufReader::new(file))
            .with_context(|| format!("decode {}", path.display()))?;
        let sink = rodio::Sink::try_new(&self.handle)?;
        sink.append(source);
        sink.detach();
        Ok(())
    }

    /// Play a slice of in-memory mono f32 samples at `rate` Hz. Used
    /// by the per-channel ▶ take-preview button so the recorder can
    /// hear what it captured before saving.
    pub fn play_samples(&self, samples: &[f32], rate: u32) -> Result<()> {
        let buf = rodio::buffer::SamplesBuffer::new(1, rate, samples.to_vec());
        let sink = rodio::Sink::try_new(&self.handle)?;
        sink.append(buf);
        sink.detach();
        Ok(())
    }
}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preview").finish_non_exhaustive()
    }
}

// hold stream alive by preventing it being dropped early; renaming
// the field to `_stream` would work too, but keeping the handle
// reachable + matching naming intent
impl Drop for Preview {
    fn drop(&mut self) {
        // The drop order here doesn't matter — rodio handles its own
        // shutdown — but leaving a hook here keeps `stream` read
        // and suppresses the dead_code warning.
        let _ = &self.stream;
    }
}

// ---------------------------------------------------------------------------
// session log — one line per save / promote / trash, per recorder run

pub struct SessionLog {
    path: PathBuf,
}

impl SessionLog {
    pub fn open() -> Result<Self> {
        let dir = user_home().join(".terminal_hell").join("recorder").join("sessions");
        fs::create_dir_all(&dir)?;
        let stamp = monotonic_stamp();
        let path = dir.join(format!("session-{stamp}.log"));
        // Touch the file so stale-session cleanup tools find it.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        tracing::info!(path = %path.display(), "session log opened");
        Ok(Self { path })
    }
    pub fn log(&self, line: impl AsRef<str>) {
        let Ok(mut f) = OpenOptions::new().append(true).open(&self.path) else {
            tracing::warn!("session log open failed; dropping line");
            return;
        };
        let ts = chrono_like_now();
        let _ = writeln!(f, "{ts}  {}", line.as_ref());
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn user_home() -> PathBuf {
    // `dirs` crate would be nicer but we prefer to avoid a dep for
    // this. Windows and *nix both respect `HOME` / `USERPROFILE`
    // env-vars with a fallback to `.` so an empty env doesn't crash.
    if let Ok(h) = std::env::var("USERPROFILE") {
        return PathBuf::from(h);
    }
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h);
    }
    PathBuf::from(".")
}

fn chrono_like_now() -> String {
    // ISO-ish timestamp without a chrono dep.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // epoch seconds is adequate for correlation — session logs are
    // short-lived.
    format!("{secs}.{millis:03}")
}

// ---------------------------------------------------------------------------
// save orchestrator — consumes a take + per-channel destinations +
// trim ranges and writes every non-discard file, returning lines for
// the session log

pub struct SaveResult {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<String>,
    pub log_lines: Vec<String>,
}

pub fn save_take(
    pack_root: &Path,
    samples: &[Vec<f32>],
    rate: u32,
    trims: &[(usize, usize)],
    destinations: &[Destination],
) -> Result<SaveResult> {
    if samples.len() != destinations.len() || samples.len() != trims.len() {
        return Err(anyhow!(
            "save_take: vectors mismatched ({} samples / {} trims / {} destinations)",
            samples.len(),
            trims.len(),
            destinations.len()
        ));
    }

    let mut written: Vec<PathBuf> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();

    for (ch, dest) in destinations.iter().enumerate() {
        let (in_idx, out_idx) = clamp_trim(trims[ch], samples[ch].len());
        let slice = &samples[ch][in_idx..out_idx];
        match dest {
            Destination::Discard => {
                skipped.push(format!("ch{}: discard", ch + 1));
            }
            Destination::Slot { unit_id, event, variant, audition } => {
                let path = resolve_slot_path(pack_root, unit_id, event, variant, *audition);
                write_mono_wav(&path, slice, rate)?;
                lines.push(format!(
                    "save  ch{}  {}  -> {}  ({} samples)",
                    ch + 1,
                    dest.short_label(),
                    path.display(),
                    slice.len()
                ));
                written.push(path);
            }
            Destination::CvParam { param, stem, audition } => {
                let path = resolve_cv_path(pack_root, param, stem, *audition);
                write_mono_wav(&path, slice, rate)?;
                lines.push(format!(
                    "save  ch{}  {}  -> {}  ({} samples)",
                    ch + 1,
                    dest.short_label(),
                    path.display(),
                    slice.len()
                ));
                written.push(path);
            }
            Destination::StereoLeft { unit_id, event, variant, audition, paired_right } => {
                let right_ch = *paired_right;
                if right_ch >= samples.len() {
                    skipped.push(format!(
                        "ch{}: stereo L pointed at missing ch{}",
                        ch + 1,
                        right_ch + 1
                    ));
                    continue;
                }
                let (r_in, r_out) = clamp_trim(trims[right_ch], samples[right_ch].len());
                let right_slice = &samples[right_ch][r_in..r_out];
                let path = resolve_slot_path(pack_root, unit_id, event, variant, *audition);
                write_stereo_wav(&path, slice, right_slice, rate)?;
                lines.push(format!(
                    "save  ch{}+ch{}  {}  -> {}  ({}+{} samples)",
                    ch + 1,
                    right_ch + 1,
                    dest.short_label(),
                    path.display(),
                    slice.len(),
                    right_slice.len(),
                ));
                written.push(path);
            }
            Destination::StereoRight { paired_left } => {
                // Owned by the left channel; skip here without
                // logging as a warning — this is expected.
                skipped.push(format!(
                    "ch{}: stereo R consumed by ch{}",
                    ch + 1,
                    paired_left + 1
                ));
            }
        }
    }

    Ok(SaveResult {
        written,
        skipped,
        log_lines: lines,
    })
}

fn clamp_trim((t_in, t_out): (usize, usize), total: usize) -> (usize, usize) {
    let a = t_in.min(total);
    let b = t_out.min(total);
    if a < b {
        (a, b)
    } else {
        (0, total)
    }
}
