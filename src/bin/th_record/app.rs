//! Recorder egui app — owns device state, shared capture buffer,
//! per-channel UI state, audit queue, focused brief, preview player,
//! keyboard shortcuts, and the in-memory log tail.
//!
//! Layout (all panels resizable where egui allows):
//!
//! ┌───────────────────────────────────────────────────────────────┐
//! │ top bar — device / session / state card / transport buttons   │
//! ├────────────┬──────────────────────────────────┬───────────────┤
//! │ brief      │ channels (waveform + assign)     │ audit queue   │
//! │ viewer     │                                  │ (clickable)   │
//! │            │ current pool (play/promote/trash)│               │
//! ├────────────┴──────────────────────────────────┴───────────────┤
//! │ session log tail                                              │
//! ├───────────────────────────────────────────────────────────────┤
//! │ status bar                                                    │
//! └───────────────────────────────────────────────────────────────┘

use anyhow::Result;
use egui::{Color32, Frame, Key, Margin, Rect, RichText, Rounding, Stroke, Ui, Vec2};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::brief::{self, BriefView};
use crate::capture::{self, CaptureState, DeviceInfo};
use crate::io::{self, Destination, PoolEntry, Preview, SaveResult, SessionLog, SessionType};
use crate::queue::{self, Queue, QueueEntry};
use crate::waveform::WaveformView;

use terminal_hell::audio::params as audio_params;
use terminal_hell::audio::paths::Category;

// ---------------------------------------------------------------------------
// tunables

const PEAK_DECAY_PER_SECOND: f32 = 0.75;
const PEAK_HOLD_SECS: f32 = 0.9;
const STATUS_TTL: Duration = Duration::from_secs(6);
const LOG_TAIL_CAP: usize = 60;
const VARIANTS: &[&str] = &["baseline", "p25", "p50", "p75", "p100", "phantom"];

// palette — deep-charcoal background with synthwave accents that
// echo the game's own visual language.
const BG_BASE: Color32 = Color32::from_rgb(12, 10, 18);
const BG_CARD: Color32 = Color32::from_rgb(20, 18, 28);
const BG_CARD_HOT: Color32 = Color32::from_rgb(30, 22, 42);
const FG_TEXT: Color32 = Color32::from_rgb(220, 220, 235);
const FG_DIM: Color32 = Color32::from_rgb(140, 140, 160);
const FG_MUTE: Color32 = Color32::from_rgb(90, 90, 110);
const ACCENT_CYAN: Color32 = Color32::from_rgb(120, 220, 220);
const ACCENT_AMBER: Color32 = Color32::from_rgb(240, 200, 80);
const ACCENT_MAGENTA: Color32 = Color32::from_rgb(230, 110, 210);
const ACCENT_GREEN: Color32 = Color32::from_rgb(120, 220, 140);
const ACCENT_RED: Color32 = Color32::from_rgb(240, 110, 110);
const BORDER: Color32 = Color32::from_rgb(60, 48, 88);

// ---------------------------------------------------------------------------
// types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Err,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SlotKey {
    pub category: Category,
    pub unit_id: String,
    pub event: String,
    pub variant: String,
}

/// Wall-clock-driven preview of an in-memory channel take. Lives
/// while the samples are playing so the waveform can animate a
/// cursor; auto-clears once the elapsed time passes total duration.
#[derive(Debug, Clone)]
pub struct TakePreview {
    pub channel: usize,
    pub started: Instant,
    pub total_samples: usize,
    pub rate: u32,
}

impl TakePreview {
    /// Current sample index, or None if playback has finished.
    pub fn cursor_sample(&self) -> Option<usize> {
        if self.rate == 0 {
            return None;
        }
        let elapsed = self.started.elapsed().as_secs_f64();
        let idx = (elapsed * self.rate as f64) as usize;
        if idx >= self.total_samples {
            None
        } else {
            Some(idx)
        }
    }
}

/// Two-click trash confirmation window. A click within this many
/// seconds of the first click confirms the delete.
const TRASH_CONFIRM_WINDOW: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// app

pub struct RecorderApp {
    // device + capture -----------------------------------------------
    devices: Vec<DeviceInfo>,
    selected_device: Option<String>,
    capture: Arc<Mutex<CaptureState>>,
    stream: Option<cpal::Stream>,
    recording_start: Option<Instant>,

    // content + queue ------------------------------------------------
    pack_root: PathBuf,
    queue: Queue,
    queue_dirty_at: Option<Instant>,
    queue_search: String,
    queue_blockers_only: bool,

    // focused slot (drives brief panel + default ch0 destination) -----
    focused_slot: Option<SlotKey>,
    focused_brief: Option<BriefView>,

    // per-channel UI state -------------------------------------------
    trims: Vec<(usize, usize)>,
    destinations: Vec<Destination>,
    waveform_views: Vec<WaveformView>,
    take_samples: Option<Vec<Vec<f32>>>,
    take_rate: u32,
    peak_hold: Vec<(f32, Instant)>,
    /// Smoothed per-channel RMS (0..1) for the device-meter strip.
    /// Refreshed from `CaptureState::drain_rms` each UI frame then
    /// lerp-smoothed so the meter doesn't flicker on short spikes.
    live_rms: Vec<f32>,

    // session --------------------------------------------------------
    session_type: SessionType,

    // playback -------------------------------------------------------
    preview: Option<Preview>,
    selected_pool_entry: Option<PathBuf>,
    take_preview: Option<TakePreview>,

    // logging --------------------------------------------------------
    log: Option<SessionLog>,
    log_tail: VecDeque<String>,

    // status ---------------------------------------------------------
    status: Option<(String, Instant, Severity)>,
    last_frame: Instant,

    // session stats --------------------------------------------------
    session_start: Instant,
    takes_saved: u32,

    // two-click trash confirmation -----------------------------------
    pending_trash: Option<(PathBuf, Instant)>,

    // help overlay ---------------------------------------------------
    show_help: bool,

    // one-shot flags -------------------------------------------------
    theme_applied: bool,
}

impl RecorderApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let pack_root = terminal_hell::audio::default_content_root();
        let queue = Queue::rebuild(&pack_root).unwrap_or_else(|err| {
            tracing::warn!(?err, "initial queue rebuild failed");
            Queue::empty()
        });
        let devices = capture::list_input_devices();
        let selected_device = devices.first().map(|d| d.name.clone());
        let preview = Preview::new().ok();
        let log = SessionLog::open().ok();

        let mut app = Self {
            devices,
            selected_device,
            capture: Arc::new(Mutex::new(CaptureState::new())),
            stream: None,
            recording_start: None,
            pack_root,
            queue,
            queue_dirty_at: None,
            queue_search: String::new(),
            queue_blockers_only: true,
            focused_slot: None,
            focused_brief: None,
            trims: Vec::new(),
            destinations: Vec::new(),
            waveform_views: Vec::new(),
            take_samples: None,
            take_rate: 48_000,
            peak_hold: Vec::new(),
            live_rms: Vec::new(),
            session_type: SessionType::Sfx,
            preview,
            selected_pool_entry: None,
            take_preview: None,
            log,
            log_tail: VecDeque::with_capacity(LOG_TAIL_CAP),
            status: None,
            last_frame: Instant::now(),
            session_start: Instant::now(),
            takes_saved: 0,
            pending_trash: None,
            show_help: false,
            theme_applied: false,
        };

        // Pre-focus the first queue entry so brief + ch0 default land
        // immediately on launch.
        if let Some(first) = app.queue.first_recommendation().cloned() {
            app.focus_entry(&first);
        }
        // Auto-arm the input stream on startup so the per-channel
        // meters come alive immediately — the user needs to *see*
        // input is flowing before committing to a take, especially
        // on Linux where ALSA device naming is a coin flip.
        if app.selected_device.is_some() {
            app.ensure_stream();
        }
        app.push_log("session opened");
        app
    }

    // ----- theme --------------------------------------------------------

    fn apply_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(FG_TEXT);
        visuals.window_fill = BG_CARD;
        visuals.panel_fill = BG_BASE;
        visuals.extreme_bg_color = BG_BASE;
        visuals.faint_bg_color = BG_CARD;
        visuals.widgets.noninteractive.bg_fill = BG_CARD;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, FG_TEXT);
        visuals.widgets.inactive.bg_fill = BG_CARD;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, FG_TEXT);
        visuals.widgets.hovered.bg_fill = BG_CARD_HOT;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_MAGENTA);
        visuals.widgets.active.bg_fill = BG_CARD_HOT;
        visuals.widgets.active.bg_stroke = Stroke::new(1.5, ACCENT_CYAN);
        visuals.widgets.open.bg_fill = BG_CARD_HOT;
        visuals.selection.bg_fill = Color32::from_rgb(70, 40, 100);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT_CYAN);
        visuals.hyperlink_color = ACCENT_CYAN;
        ctx.set_visuals(visuals);

        // Spacing — slightly tighter than default so more info fits
        // comfortably.
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.window_margin = Margin::same(8.0);
        ctx.set_style(style);
    }

    // ----- focus + brief ------------------------------------------------

    fn focus_entry(&mut self, entry: &QueueEntry) {
        let key = SlotKey {
            category: Category::Unit, // queue today is units-only
            unit_id: entry.unit_id.clone(),
            event: entry.event.clone(),
            variant: entry.variant.clone(),
        };
        self.focused_slot = Some(key.clone());
        self.focused_brief = brief::load(
            &self.pack_root,
            key.category,
            &key.unit_id,
            &key.event,
            &key.variant,
        );
        // Auto-update ch0 destination to match — user can still
        // override via the destination picker.
        if !self.destinations.is_empty() {
            self.destinations[0] = Destination::Slot {
                unit_id: key.unit_id.clone(),
                event: key.event.clone(),
                variant: key.variant.clone(),
                audition: true,
            };
        }
    }

    fn refresh_brief(&mut self) {
        if let Some(key) = self.focused_slot.clone() {
            self.focused_brief = brief::load(
                &self.pack_root,
                key.category,
                &key.unit_id,
                &key.event,
                &key.variant,
            );
        } else {
            self.focused_brief = None;
        }
    }

    fn rebuild_queue(&mut self) {
        match Queue::rebuild(&self.pack_root) {
            Ok(q) => {
                self.queue = q;
                self.refresh_brief();
            }
            Err(err) => self.set_status(format!("queue rebuild failed: {err}"), Severity::Err),
        }
    }

    fn filtered_queue(&self) -> Vec<&QueueEntry> {
        let needle = self.queue_search.trim().to_ascii_lowercase();
        self.queue
            .entries
            .iter()
            .filter(|e| {
                if self.queue_blockers_only && e.priority != queue::Priority::Blocker {
                    return false;
                }
                if !needle.is_empty() {
                    let hay = format!(
                        "{}.{}.{} {}",
                        e.unit_id,
                        e.event,
                        e.variant,
                        e.motif.as_deref().unwrap_or(""),
                    )
                    .to_ascii_lowercase();
                    if !hay.contains(&needle) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    // ----- capture ------------------------------------------------------

    fn ensure_stream(&mut self) {
        if self.stream.is_some() {
            return;
        }
        let Some(name) = self.selected_device.clone() else {
            self.set_status("no input device selected", Severity::Err);
            return;
        };
        match capture::start_stream(&name, self.capture.clone()) {
            Ok(s) => {
                self.stream = Some(s);
                let (channels, rate) = {
                    let st = self.capture.lock().unwrap();
                    (st.channels as usize, st.rate)
                };
                self.resize_channel_state(channels);
                self.take_rate = rate;
                self.set_status(
                    format!("stream armed: {name} @ {rate} Hz · {channels} ch"),
                    Severity::Info,
                );
                self.push_log(format!("stream armed: {name} {rate}Hz {channels}ch"));
            }
            Err(err) => self.set_status(format!("failed to open {name}: {err}"), Severity::Err),
        }
    }

    fn stop_stream(&mut self) {
        self.stream = None;
        if let Ok(mut st) = self.capture.lock() {
            st.recording = false;
        }
    }

    fn resize_channel_state(&mut self, channels: usize) {
        self.trims.resize(channels, (0, 0));
        self.destinations.resize(channels, Destination::Discard);
        self.peak_hold.resize(channels, (0.0, Instant::now()));
        self.waveform_views.clear();
        for i in 0..channels {
            let mut v = WaveformView::new(i, format!("ch{}", i + 1));
            v.rate = self.take_rate;
            self.waveform_views.push(v);
        }
        // Re-apply session-type defaults after a device change.
        self.apply_session_defaults(channels);
    }

    fn start_recording(&mut self) {
        self.ensure_stream();
        if self.stream.is_none() {
            return;
        }
        {
            let mut st = self.capture.lock().unwrap();
            st.samples = (0..st.channels).map(|_| Vec::new()).collect();
            st.recording = true;
            st.clear_clip_flags();
        }
        self.take_samples = None;
        self.take_preview = None;
        self.recording_start = Some(Instant::now());
        self.set_status("recording…", Severity::Info);
        self.push_log("record start");
    }

    fn stop_recording(&mut self) {
        let drained = {
            let mut st = self.capture.lock().unwrap();
            st.recording = false;
            st.take_samples()
        };
        self.recording_start = None;
        if drained.iter().all(|v| v.is_empty()) {
            self.set_status("stop: no samples captured", Severity::Warn);
            return;
        }
        let channels = drained.len();
        self.trims = drained.iter().map(|v| (0, v.len())).collect();
        self.apply_session_defaults(channels);
        let longest = drained.iter().map(|v| v.len()).max().unwrap_or(0);
        let rate = self.take_rate.max(1);
        let dur = longest as f32 / rate as f32;
        self.take_samples = Some(drained);
        self.set_status(
            format!("take captured: {channels} ch × {longest} samples (~{dur:.2}s)"),
            Severity::Info,
        );
        self.push_log(format!("record stop {channels}ch {dur:.2}s"));
    }

    fn apply_session_defaults(&mut self, channels: usize) {
        self.destinations = (0..channels).map(|_| Destination::Discard).collect();
        let rec = self.queue.first_recommendation().cloned();
        match self.session_type {
            SessionType::Sfx => {
                if let Some(key) = self.focused_slot.clone() {
                    if channels >= 1 {
                        self.destinations[0] = Destination::Slot {
                            unit_id: key.unit_id,
                            event: key.event,
                            variant: key.variant,
                            audition: true,
                        };
                    }
                } else if let Some(r) = rec {
                    if channels >= 1 {
                        self.destinations[0] = Destination::Slot {
                            unit_id: r.unit_id,
                            event: r.event,
                            variant: r.variant,
                            audition: true,
                        };
                    }
                }
            }
            SessionType::MusicCv => {
                if channels >= 2 {
                    self.destinations[0] = Destination::StereoLeft {
                        unit_id: "music".into(),
                        event: "base_loop".into(),
                        variant: "baseline".into(),
                        audition: true,
                        paired_right: 1,
                    };
                    self.destinations[1] = Destination::StereoRight { paired_left: 0 };
                }
            }
            SessionType::CvOnly => {
                for (i, name) in audio_params::known_names().take(channels).enumerate() {
                    self.destinations[i] = Destination::CvParam {
                        param: name.into(),
                        stem: "base_loop".into(),
                        audition: true,
                    };
                }
            }
        }
    }

    fn save_current_take(&mut self) {
        let Some(samples) = self.take_samples.as_ref() else {
            self.set_status("no take to save", Severity::Warn);
            return;
        };
        if samples.iter().all(|v| v.is_empty()) {
            self.set_status("take is empty", Severity::Warn);
            return;
        }
        let result: Result<SaveResult> = io::save_take(
            &self.pack_root,
            samples,
            self.take_rate,
            &self.trims,
            &self.destinations,
        );
        match result {
            Ok(res) => {
                let written = res.written.len();
                let skipped = res.skipped.len();
                for line in &res.log_lines {
                    self.push_log(line.clone());
                }
                self.set_status(
                    format!("saved {written} file(s), skipped {skipped}"),
                    if written > 0 { Severity::Info } else { Severity::Warn },
                );
                self.queue_dirty_at = Some(Instant::now());
                self.take_samples = None;
                self.take_preview = None;
                if written > 0 {
                    self.takes_saved += 1;
                }
            }
            Err(err) => {
                self.set_status(format!("save failed: {err}"), Severity::Err);
                self.push_log(format!("save failed: {err}"));
            }
        }
    }

    fn discard_take(&mut self) {
        if self.take_samples.is_some() {
            self.take_samples = None;
            self.set_status("take discarded", Severity::Info);
            self.push_log("take discarded");
        } else {
            self.set_status("nothing to discard", Severity::Warn);
        }
    }

    fn play_path(&mut self, path: PathBuf) {
        if let Some(preview) = &self.preview {
            if let Err(err) = preview.play(&path) {
                self.set_status(format!("preview failed: {err}"), Severity::Err);
            } else {
                self.set_status(format!("preview: {}", path.display()), Severity::Info);
            }
        } else {
            self.set_status("preview device not open", Severity::Err);
        }
    }

    fn play_take_channel(&mut self, channel: usize) {
        let Some(samples_all) = self.take_samples.as_ref() else {
            self.set_status("no take to preview", Severity::Warn);
            return;
        };
        let Some(ch_samples) = samples_all.get(channel) else { return };
        let (t_in, t_out) = self.trims.get(channel).copied().unwrap_or((0, ch_samples.len()));
        let slice = &ch_samples[t_in.min(ch_samples.len())..t_out.min(ch_samples.len())];
        if slice.is_empty() {
            self.set_status("channel take is empty", Severity::Warn);
            return;
        }
        let Some(preview) = &self.preview else {
            self.set_status("preview device not open", Severity::Err);
            return;
        };
        let rate = self.take_rate.max(1);
        if let Err(err) = preview.play_samples(slice, rate) {
            self.set_status(format!("take preview failed: {err}"), Severity::Err);
            return;
        }
        self.take_preview = Some(TakePreview {
            channel,
            started: Instant::now(),
            total_samples: slice.len(),
            rate,
        });
        let secs = slice.len() as f32 / rate as f32;
        self.set_status(
            format!("preview ch{} · {:.2}s", channel + 1, secs),
            Severity::Info,
        );
    }

    /// Two-click trash: first click arms the pending slot; a second
    /// click on the same path within `TRASH_CONFIRM_WINDOW` commits
    /// the delete. Re-clicking a different path resets the timer.
    fn request_trash(&mut self, path: &Path) -> Option<PathBuf> {
        let now = Instant::now();
        if let Some((armed, at)) = &self.pending_trash {
            if armed == path && now.duration_since(*at) < TRASH_CONFIRM_WINDOW {
                self.pending_trash = None;
                return Some(path.to_path_buf());
            }
        }
        self.pending_trash = Some((path.to_path_buf(), now));
        self.set_status(
            format!(
                "click 🗑 again within {}s to delete {}",
                TRASH_CONFIRM_WINDOW.as_secs(),
                path.display()
            ),
            Severity::Warn,
        );
        None
    }

    // ----- keyboard -----------------------------------------------------

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return; // let text fields type freely
        }
        let recording = self
            .capture
            .lock()
            .map(|s| s.recording)
            .unwrap_or(false);
        let pressed_space = ctx.input(|i| i.key_pressed(Key::Space));
        let pressed_enter = ctx.input(|i| i.key_pressed(Key::Enter));
        let pressed_delete = ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace));
        let pressed_s = ctx.input(|i| i.key_pressed(Key::S));
        let pressed_p = ctx.input(|i| i.key_pressed(Key::P));
        let pressed_f1 = ctx.input(|i| i.key_pressed(Key::F1));
        let pressed_escape = ctx.input(|i| i.key_pressed(Key::Escape));

        if pressed_f1 {
            self.show_help = !self.show_help;
        }
        if pressed_escape && self.show_help {
            self.show_help = false;
        }
        if pressed_space {
            if recording {
                self.stop_recording();
            } else {
                self.start_recording();
            }
        }
        if pressed_s && recording {
            self.stop_recording();
        }
        if pressed_enter && self.take_samples.is_some() {
            self.save_current_take();
        }
        if pressed_delete && self.take_samples.is_some() {
            self.discard_take();
        }
        if pressed_p {
            if let Some(path) = self.selected_pool_entry.clone() {
                self.play_path(path);
            }
        }
    }

    // ----- status + log -------------------------------------------------

    fn set_status(&mut self, text: impl Into<String>, sev: Severity) {
        let t = text.into();
        match sev {
            Severity::Err => tracing::warn!("{t}"),
            Severity::Warn => tracing::info!("{t}"),
            Severity::Info => tracing::info!("{t}"),
        }
        self.status = Some((t, Instant::now(), sev));
    }

    fn push_log(&mut self, line: impl Into<String>) {
        let line = line.into();
        if let Some(log) = &self.log {
            log.log(&line);
        }
        if self.log_tail.len() >= LOG_TAIL_CAP {
            self.log_tail.pop_front();
        }
        self.log_tail.push_back(line);
    }
}

// ---------------------------------------------------------------------------
// eframe integration

impl eframe::App for RecorderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            Self::apply_theme(ctx);
            self.theme_applied = true;
        }
        // Active repaint so meters + recording timer tick smoothly.
        ctx.request_repaint_after(Duration::from_millis(33));

        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.0, 0.5);
        self.last_frame = Instant::now();

        // Decay live peak meters in shared state. Peak-hold ticks live
        // on App.
        let decay = PEAK_DECAY_PER_SECOND.powf(dt);
        let fresh_rms = {
            let mut st = self.capture.lock().unwrap();
            st.decay_peaks(decay);
            let peaks = st.peak.clone();
            let rms = st.drain_rms();
            drop(st);
            for (i, p) in peaks.iter().enumerate() {
                if i >= self.peak_hold.len() {
                    self.peak_hold.push((*p, Instant::now()));
                    continue;
                }
                if *p >= self.peak_hold[i].0
                    || self.peak_hold[i].1.elapsed().as_secs_f32() > PEAK_HOLD_SECS
                {
                    self.peak_hold[i] = (*p, Instant::now());
                }
            }
            rms
        };
        // Smooth RMS with an exponential follower so the bar slides
        // rather than twitches. Attack is snappier than release so
        // signal onset still reads as "live".
        if self.live_rms.len() != fresh_rms.len() {
            self.live_rms = fresh_rms.clone();
        } else {
            for (slot, &target) in self.live_rms.iter_mut().zip(fresh_rms.iter()) {
                let alpha = if target > *slot { 0.6 } else { 0.15 };
                *slot = *slot * (1.0 - alpha) + target * alpha;
            }
        }

        // Debounced queue rebuild after a save / promote / trash.
        if let Some(at) = self.queue_dirty_at {
            if at.elapsed() > Duration::from_millis(250) {
                self.rebuild_queue();
                self.queue_dirty_at = None;
            }
        }

        // Auto-clear finished take preview so the cursor animation
        // stops when the sample runs out.
        let drop_preview = self
            .take_preview
            .as_ref()
            .map(|p| p.cursor_sample().is_none())
            .unwrap_or(false);
        if drop_preview {
            self.take_preview = None;
        }

        // Expire pending-trash arming so the warm state doesn't
        // linger forever if the user wanders off.
        if let Some((_, at)) = &self.pending_trash {
            if at.elapsed() > TRASH_CONFIRM_WINDOW {
                self.pending_trash = None;
            }
        }

        self.handle_keyboard(ctx);

        egui::TopBottomPanel::top("top_bar")
            .frame(Frame::default().fill(BG_BASE).inner_margin(Margin::symmetric(10.0, 8.0)))
            .show(ctx, |ui| self.show_top_bar(ui));
        // Device-level meter strip lives just below the top bar so it's
        // always visible — the #1 question the user asks when picking a
        // device is "is this one actually getting signal?"
        egui::TopBottomPanel::top("device_meters")
            .frame(Frame::default().fill(BG_BASE).inner_margin(Margin::symmetric(10.0, 4.0)))
            .show(ctx, |ui| self.show_device_meters(ui));
        egui::SidePanel::right("queue_panel")
            .default_width(280.0)
            .resizable(true)
            .frame(Frame::default().fill(BG_BASE).inner_margin(Margin::same(8.0)))
            .show(ctx, |ui| self.show_queue_panel(ui));
        egui::SidePanel::left("brief_panel")
            .default_width(320.0)
            .resizable(true)
            .frame(Frame::default().fill(BG_BASE).inner_margin(Margin::same(8.0)))
            .show(ctx, |ui| self.show_brief_panel(ui));
        egui::TopBottomPanel::bottom("status_bar")
            .frame(Frame::default().fill(BG_BASE).inner_margin(Margin::symmetric(10.0, 6.0)))
            .show(ctx, |ui| self.show_status_bar(ui));
        egui::TopBottomPanel::bottom("log_panel")
            .default_height(120.0)
            .resizable(true)
            .frame(Frame::default().fill(BG_CARD).inner_margin(Margin::same(8.0)))
            .show(ctx, |ui| self.show_log_panel(ui));
        egui::CentralPanel::default()
            .frame(Frame::default().fill(BG_BASE).inner_margin(Margin::same(8.0)))
            .show(ctx, |ui| self.show_central(ui));

        // Help overlay draws last so it sits on top of everything.
        if self.show_help {
            self.show_help_window(ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// top bar

impl RecorderApp {
    fn show_top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("th_record").color(ACCENT_MAGENTA).strong());
            ui.label(RichText::new("·").color(FG_MUTE));
            // Device picker
            let current = self
                .selected_device
                .clone()
                .unwrap_or_else(|| "(no device)".into());
            let mut chosen: Option<String> = None;
            egui::ComboBox::from_id_source("device_pick")
                .selected_text(truncate(&current, 32))
                .width(240.0)
                .show_ui(ui, |ui| {
                    for d in &self.devices {
                        let lbl = format!(
                            "{}  ({} ch · {} Hz · {})",
                            d.name, d.channels, d.default_rate, d.sample_format
                        );
                        if ui
                            .selectable_label(
                                self.selected_device.as_deref() == Some(d.name.as_str()),
                                lbl,
                            )
                            .clicked()
                        {
                            chosen = Some(d.name.clone());
                        }
                    }
                });
            if let Some(name) = chosen {
                if self.selected_device.as_deref() != Some(&name) {
                    self.stop_stream();
                    self.selected_device = Some(name);
                    // Auto-arm so meters go live the instant the user
                    // picks a device — no need to hit REC to find out
                    // whether this ALSA name is actually hooked up.
                    self.ensure_stream();
                }
            }
            if ui.small_button("↻").on_hover_text("Refresh device list").clicked() {
                self.devices = capture::list_input_devices();
            }

            ui.label(RichText::new("·").color(FG_MUTE));

            // Session type
            let mut st = self.session_type;
            egui::ComboBox::from_id_source("session_pick")
                .selected_text(st.label())
                .show_ui(ui, |ui| {
                    for t in [SessionType::Sfx, SessionType::MusicCv, SessionType::CvOnly] {
                        ui.selectable_value(&mut st, t, t.label());
                    }
                });
            if st != self.session_type {
                self.session_type = st;
                let channels = self.waveform_views.len();
                if channels > 0 {
                    self.apply_session_defaults(channels);
                }
            }

            // Right-aligned state card + transport
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.show_transport(ui);
                ui.add_space(12.0);
                self.show_state_card(ui);
            });
        });
    }

    fn show_state_card(&self, ui: &mut Ui) {
        let recording = self
            .capture
            .lock()
            .map(|s| s.recording)
            .unwrap_or(false);
        let ch = self.waveform_views.len();
        let rate = self.take_rate;

        let (tag, tag_color) = if recording {
            ("● REC", ACCENT_RED)
        } else if self.stream.is_some() {
            ("◉ ARMED", ACCENT_CYAN)
        } else {
            ("○ IDLE", FG_MUTE)
        };
        let dur_text = if let Some(start) = self.recording_start {
            let s = start.elapsed().as_secs_f32();
            format!("{s:.2}s")
        } else if let Some(samples) = &self.take_samples {
            let longest = samples.iter().map(|v| v.len()).max().unwrap_or(0);
            let s = longest as f32 / rate.max(1) as f32;
            format!("take · {s:.2}s")
        } else {
            "—".to_string()
        };

        Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .rounding(Rounding::same(4.0))
            .inner_margin(Margin::symmetric(10.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tag).color(tag_color).strong().monospace());
                    ui.label(RichText::new("·").color(FG_MUTE));
                    ui.label(RichText::new(dur_text).color(FG_TEXT).monospace());
                    ui.label(RichText::new("·").color(FG_MUTE));
                    ui.label(
                        RichText::new(format!("{ch} ch @ {rate} Hz"))
                            .color(FG_DIM)
                            .monospace(),
                    );
                });
            });
    }

    fn show_transport(&mut self, ui: &mut Ui) {
        let recording = self
            .capture
            .lock()
            .map(|s| s.recording)
            .unwrap_or(false);

        // Save all
        let save_enabled = self.take_samples.is_some();
        if ui
            .add_enabled(
                save_enabled,
                egui::Button::new(RichText::new("💾 SAVE ALL").monospace())
                    .fill(if save_enabled { Color32::from_rgb(50, 80, 60) } else { BG_CARD })
                    .stroke(Stroke::new(1.0, if save_enabled { ACCENT_GREEN } else { BORDER })),
            )
            .on_hover_text("Write WAVs to disk  (Enter)")
            .clicked()
        {
            self.save_current_take();
        }

        // Discard
        let discard_enabled = self.take_samples.is_some();
        if ui
            .add_enabled(
                discard_enabled,
                egui::Button::new(RichText::new("🗑 DISCARD").monospace())
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .on_hover_text("Throw away the take  (Del)")
            .clicked()
        {
            self.discard_take();
        }

        // Stop
        if ui
            .add_enabled(
                recording,
                egui::Button::new(RichText::new("■ STOP").monospace())
                    .fill(if recording { Color32::from_rgb(80, 40, 40) } else { BG_CARD })
                    .stroke(Stroke::new(1.0, if recording { ACCENT_RED } else { BORDER })),
            )
            .on_hover_text("Stop recording  (S)")
            .clicked()
        {
            self.stop_recording();
        }

        // Record (big, prominent, pulsing while hot)
        let (fill, stroke_c, label) = if recording {
            let t = ui.ctx().input(|i| i.time) as f32;
            let pulse = 0.5 + 0.5 * (t * 6.0).sin();
            let pulse_red = Color32::from_rgb(
                (200.0 + 40.0 * pulse) as u8,
                60,
                60,
            );
            (pulse_red, ACCENT_RED, "● RECORDING")
        } else {
            (Color32::from_rgb(140, 40, 40), ACCENT_RED, "● REC")
        };
        if ui
            .add(
                egui::Button::new(
                    RichText::new(label)
                        .strong()
                        .monospace()
                        .color(Color32::WHITE),
                )
                .fill(fill)
                .stroke(Stroke::new(1.5, stroke_c))
                .min_size(Vec2::new(130.0, 30.0)),
            )
            .on_hover_text("Start / stop recording  (Space)")
            .clicked()
        {
            if recording {
                self.stop_recording();
            } else {
                self.start_recording();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// queue panel

impl RecorderApp {
    fn show_queue_panel(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("audit queue").color(ACCENT_CYAN));
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{:.1}%  {}/{}",
                    self.queue.coverage_percent,
                    self.queue.filled_slots,
                    self.queue.total_slots,
                ))
                .color(FG_DIM)
                .monospace(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("↻").on_hover_text("Refresh audit").clicked() {
                    self.rebuild_queue();
                }
            });
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(RichText::new("find").color(FG_DIM).small());
            ui.text_edit_singleline(&mut self.queue_search);
        });
        ui.checkbox(&mut self.queue_blockers_only, "blockers only");
        ui.add_space(4.0);

        let entries: Vec<QueueEntry> = self.filtered_queue().into_iter().cloned().collect();
        let focused_key = self.focused_slot.clone();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(
                        RichText::new("(no matches)")
                            .italics()
                            .color(FG_MUTE),
                    );
                    return;
                }
                for entry in &entries {
                    let is_focused = focused_key.as_ref().map(|k| {
                        k.unit_id == entry.unit_id
                            && k.event == entry.event
                            && k.variant == entry.variant
                    }).unwrap_or(false);
                    let clicked = queue_row(ui, entry, is_focused);
                    if clicked {
                        self.focus_entry(entry);
                        self.set_status(
                            format!(
                                "focus: {}.{}.{}",
                                entry.unit_id, entry.event, entry.variant
                            ),
                            Severity::Info,
                        );
                    }
                }
            });
    }
}

fn queue_row(ui: &mut Ui, entry: &QueueEntry, focused: bool) -> bool {
    let (tag, tag_color) = match entry.priority {
        queue::Priority::Blocker => ("BLK", ACCENT_RED),
        queue::Priority::Thin => ("THN", ACCENT_AMBER),
        queue::Priority::VariantMissing => ("VAR", FG_DIM),
        queue::Priority::Healthy => ("OK ", ACCENT_GREEN),
    };
    let bg = if focused { BG_CARD_HOT } else { BG_CARD };
    let border = if focused { ACCENT_CYAN } else { BORDER };
    let resp = Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .rounding(Rounding::same(3.0))
        .inner_margin(Margin::symmetric(6.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tag).color(tag_color).strong().small().monospace());
                ui.label(
                    RichText::new(format!(
                        "{}.{}.{}",
                        entry.unit_id, entry.event, entry.variant
                    ))
                    .color(if focused { ACCENT_CYAN } else { FG_TEXT })
                    .monospace(),
                );
            });
            ui.label(
                RichText::new(format!(
                    "  locked {} · aud {}{}",
                    entry.locked_count,
                    entry.audition_count,
                    entry
                        .motif
                        .as_deref()
                        .map(|m| format!("  ·  motif: {m}"))
                        .unwrap_or_default()
                ))
                .small()
                .color(FG_MUTE),
            );
        })
        .response;
    let full_rect = resp.rect;
    let click = ui.interact(
        full_rect,
        egui::Id::new(("qrow", &entry.unit_id, &entry.event, entry.variant.as_str())),
        egui::Sense::click(),
    );
    click.clicked()
}

// ---------------------------------------------------------------------------
// brief panel

impl RecorderApp {
    fn show_brief_panel(&self, ui: &mut Ui) {
        ui.heading(RichText::new("brief").color(ACCENT_CYAN));
        ui.separator();
        let Some(brief) = &self.focused_brief else {
            ui.label(
                RichText::new("(no slot focused — click a queue entry to see its brief)")
                    .italics()
                    .color(FG_MUTE),
            );
            return;
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{}.{}.{}",
                        brief.unit_id, brief.event, brief.variant
                    ))
                    .color(ACCENT_MAGENTA)
                    .strong()
                    .monospace(),
                );
                if let Some(m) = &brief.motif_id {
                    ui.label(
                        RichText::new(format!("motif: {m}"))
                            .color(FG_DIM)
                            .small()
                            .monospace(),
                    );
                }
                ui.add_space(6.0);

                section_heading(ui, "unit");
                wrap_text(ui, &brief.unit_description, FG_TEXT);
                section_heading(ui, "sonic palette");
                wrap_text(ui, &brief.sonic_palette, FG_TEXT);
                section_heading(ui, &format!("event · {}", brief.event));
                wrap_text(ui, &brief.event_description, FG_TEXT);
                if let Some(h) = &brief.filter_hint {
                    section_heading(ui, "filter hint");
                    wrap_text(ui, h, ACCENT_AMBER);
                }

                if brief.motif_description.is_some()
                    || !brief.motif_shared_layers.is_empty()
                {
                    ui.add_space(6.0);
                    ui.separator();
                    egui::CollapsingHeader::new(
                        RichText::new("motif").color(ACCENT_MAGENTA).strong(),
                    )
                    .id_source(("motif_section", &brief.unit_id))
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(d) = &brief.motif_description {
                            wrap_text(ui, d, FG_TEXT);
                        }
                        if let Some(e) = &brief.motif_emotion {
                            ui.label(
                                RichText::new(format!("feel: {e}"))
                                    .italics()
                                    .color(ACCENT_CYAN),
                            );
                        }
                        if let Some(a) = &brief.motif_antipattern {
                            ui.label(
                                RichText::new(format!("antipattern: {a}"))
                                    .italics()
                                    .color(ACCENT_RED),
                            );
                        }
                        if !brief.motif_shared_layers.is_empty() {
                            section_heading(ui, "shared layers");
                            for layer in &brief.motif_shared_layers {
                                ui.label(
                                    RichText::new(format!("• {layer}"))
                                        .color(FG_DIM)
                                        .small(),
                                );
                            }
                        }
                    });
                }
            });
    }
}

// ---------------------------------------------------------------------------
// central (meters + channels + pool)

impl RecorderApp {
    fn show_central(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.show_meters(ui);
                ui.add_space(6.0);
                if self.take_samples.is_some() {
                    self.show_take_assign(ui);
                } else {
                    self.show_live_preview(ui);
                }
                ui.add_space(8.0);
                self.show_pool_panel(ui);
            });
    }

    /// Prominent always-on per-channel level meter strip that sits
    /// directly under the device picker. Shows the user, at a glance,
    /// whether the selected input device is actually delivering audio
    /// — so they don't have to commit to a take just to find out that
    /// `hw:CARD=ES8` was silent.
    ///
    /// Renders:
    /// - Channel index label
    /// - Horizontal RMS bar with peak-hold tick
    /// - dBFS readout
    /// - Green < -12 dBFS, yellow -12..-3 dBFS, red >= -3 dBFS
    ///
    /// Uses the live capture stream's RMS + peak (auto-armed on device
    /// selection) so it updates every frame without the user hitting
    /// REC first.
    fn show_device_meters(&self, ui: &mut Ui) {
        let stream_live = self.stream.is_some();
        let rms = self.live_rms.clone();
        let peaks = self
            .capture
            .lock()
            .map(|s| s.peak.clone())
            .unwrap_or_default();

        Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .rounding(Rounding::same(4.0))
            .inner_margin(Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("INPUT")
                            .color(ACCENT_MAGENTA)
                            .small()
                            .strong()
                            .monospace(),
                    );
                    if !stream_live {
                        ui.label(
                            RichText::new("stream not armed — pick a device above")
                                .color(FG_MUTE)
                                .italics(),
                        );
                        return;
                    }
                    if peaks.is_empty() {
                        ui.label(
                            RichText::new("waiting for first sample…")
                                .color(FG_MUTE)
                                .italics(),
                        );
                        return;
                    }
                });
                if !stream_live || peaks.is_empty() {
                    return;
                }
                // One compact row per channel. `horizontal_wrapped`
                // handles ES-8 (8 ch) gracefully when the window is
                // narrow — meters flow to a new line instead of
                // squishing off-screen.
                ui.horizontal_wrapped(|ui| {
                    for (i, peak) in peaks.iter().enumerate() {
                        let r = rms.get(i).copied().unwrap_or(0.0);
                        let hold = self
                            .peak_hold
                            .get(i)
                            .map(|(p, _)| *p)
                            .unwrap_or(0.0);
                        device_channel_meter(ui, i, r, *peak, hold);
                    }
                });
            });
    }

    fn show_meters(&self, ui: &mut Ui) {
        if self.waveform_views.is_empty() {
            ui.label(
                RichText::new("pick an input device to arm meters")
                    .italics()
                    .color(FG_MUTE),
            );
            return;
        }
        let peaks = self
            .capture
            .lock()
            .map(|s| s.peak.clone())
            .unwrap_or_default();
        Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .rounding(Rounding::same(4.0))
            .inner_margin(Margin::same(8.0))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, peak) in peaks.iter().enumerate() {
                        let hold = self
                            .peak_hold
                            .get(i)
                            .map(|(p, _)| *p)
                            .unwrap_or(0.0);
                        meter(ui, i, *peak, hold);
                    }
                });
            });
    }

    fn show_live_preview(&mut self, ui: &mut Ui) {
        let live = self
            .capture
            .lock()
            .map(|s| s.samples.clone())
            .unwrap_or_default();
        if live.is_empty() {
            return;
        }
        let recording = self
            .capture
            .lock()
            .map(|s| s.recording)
            .unwrap_or(false);
        ui.label(
            RichText::new("live (no take — hit REC to capture, STOP to freeze)")
                .color(FG_MUTE)
                .italics(),
        );
        for (i, ch) in live.iter().enumerate() {
            let mut view = self
                .waveform_views
                .get(i)
                .cloned()
                .unwrap_or_else(|| WaveformView::new(i, format!("ch{}", i + 1)));
            view.rate = self.take_rate;
            view.cursor_sample = None;
            view.show_ruler = false; // live view has no anchor; skip ruler
            view.hot = recording;
            let mut t_in = 0usize;
            let mut t_out = ch.len();
            let meter_peak = self
                .capture
                .lock()
                .ok()
                .and_then(|s| s.peak.get(i).copied())
                .unwrap_or(0.0);
            ui.push_id(("live", i), |ui| {
                view.show(ui, ch, &mut t_in, &mut t_out, meter_peak);
            });
        }
    }

    fn show_take_assign(&mut self, ui: &mut Ui) {
        let samples = self
            .take_samples
            .as_ref()
            .cloned()
            .unwrap_or_default();
        let queue = std::mem::take(&mut self.queue);
        let clipped = self
            .capture
            .lock()
            .map(|s| s.clipped.clone())
            .unwrap_or_default();

        // Deferred actions so we don't borrow self mutably while
        // iterating through its fields.
        let mut play_requests: Vec<usize> = Vec::new();

        for (i, ch_samples) in samples.iter().enumerate() {
            let mut view = self
                .waveform_views
                .get(i)
                .cloned()
                .unwrap_or_else(|| WaveformView::new(i, format!("ch{}", i + 1)));
            view.rate = self.take_rate;
            view.show_ruler = true;
            view.hot = false;
            view.cursor_sample = self
                .take_preview
                .as_ref()
                .filter(|p| p.channel == i)
                .and_then(|p| p.cursor_sample());

            let (mut t_in, mut t_out) = self.trims.get(i).copied().unwrap_or((0, ch_samples.len()));
            if t_out == 0 {
                t_out = ch_samples.len();
            }
            let meter_peak = self
                .capture
                .lock()
                .ok()
                .and_then(|s| s.peak.get(i).copied())
                .unwrap_or(0.0);

            let ch_clipped = clipped.get(i).copied().unwrap_or(false);
            let border = if ch_clipped { ACCENT_RED } else { BORDER };
            Frame::none()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, border))
                .rounding(Rounding::same(4.0))
                .inner_margin(Margin::same(8.0))
                .show(ui, |ui| {
                    ui.push_id(("take", i), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("ch{}", i + 1))
                                    .color(ACCENT_CYAN)
                                    .strong()
                                    .monospace(),
                            );
                            ui.label(
                                RichText::new(format!("{} samples", ch_samples.len()))
                                    .color(FG_DIM)
                                    .small()
                                    .monospace(),
                            );
                            if ch_clipped {
                                ui.label(
                                    RichText::new("⚠ CLIPPED")
                                        .color(ACCENT_RED)
                                        .strong()
                                        .small()
                                        .monospace(),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            !ch_samples.is_empty(),
                                            egui::Button::new("▶ preview ch"),
                                        )
                                        .on_hover_text("Play this channel's trimmed take")
                                        .clicked()
                                    {
                                        play_requests.push(i);
                                    }
                                },
                            );
                        });
                        view.show(ui, ch_samples, &mut t_in, &mut t_out, meter_peak);

                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("→").color(FG_DIM));
                            let mut dest = self.destinations.get(i).cloned().unwrap_or_default();
                            if destination_picker(ui, i, samples.len(), &queue, &mut dest) {
                                self.destinations[i] = dest;
                            }
                        });
                    });
                });

            if i < self.trims.len() {
                self.trims[i] = (t_in, t_out);
            }
        }
        self.queue = queue;
        for ch in play_requests {
            self.play_take_channel(ch);
        }
    }

    fn show_pool_panel(&mut self, ui: &mut Ui) {
        // Identify a slot to show the pool for — prefer focused_slot,
        // else derive from ch0 destination.
        let (unit_id, event, variant) = match self.focused_slot.as_ref() {
            Some(k) => (k.unit_id.clone(), k.event.clone(), k.variant.clone()),
            None => match self.destinations.first() {
                Some(Destination::Slot { unit_id, event, variant, .. })
                | Some(Destination::StereoLeft { unit_id, event, variant, .. }) => {
                    (unit_id.clone(), event.clone(), variant.clone())
                }
                _ => return,
            },
        };

        let pool = io::list_slot_pool(&self.pack_root, &unit_id, &event, &variant);
        let mut do_play: Option<PathBuf> = None;
        let mut do_promote: Option<PathBuf> = None;
        let mut do_trash: Option<PathBuf> = None;
        let selected = self.selected_pool_entry.clone();

        Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .rounding(Rounding::same(4.0))
            .inner_margin(Margin::same(8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("current pool")
                            .color(ACCENT_CYAN)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("{unit_id}.{event}.{variant}"))
                            .color(FG_DIM)
                            .monospace(),
                    );
                });
                ui.separator();
                if pool.is_empty() {
                    ui.label(
                        RichText::new("(empty — record + save to populate)")
                            .italics()
                            .color(FG_MUTE),
                    );
                    return;
                }
                let pending = self
                    .pending_trash
                    .as_ref()
                    .map(|(p, _)| p.as_path());
                for entry in &pool {
                    let is_sel = selected.as_ref() == Some(&entry.path);
                    let clicked = pool_row(
                        ui,
                        entry,
                        is_sel,
                        pending,
                        &mut do_play,
                        &mut do_promote,
                        &mut do_trash,
                    );
                    if clicked {
                        self.selected_pool_entry = Some(entry.path.clone());
                    }
                }
            });

        if let Some(p) = do_play {
            self.play_path(p);
        }
        if let Some(p) = do_promote {
            match io::promote(&self.pack_root, &unit_id, &event, &variant, &p) {
                Ok(new_path) => {
                    self.push_log(format!(
                        "promote {} → {}",
                        p.display(),
                        new_path.display()
                    ));
                    self.set_status(
                        format!("promoted → {}", new_path.display()),
                        Severity::Info,
                    );
                    self.queue_dirty_at = Some(Instant::now());
                }
                Err(err) => self.set_status(format!("promote failed: {err}"), Severity::Err),
            }
        }
        if let Some(p) = do_trash {
            // Two-click confirmation — the first click arms the
            // pending slot; a second click on the same path commits.
            if let Some(confirmed) = self.request_trash(&p) {
                if let Err(err) = io::trash(&confirmed) {
                    self.set_status(format!("trash failed: {err}"), Severity::Err);
                } else {
                    self.push_log(format!("trash {}", confirmed.display()));
                    self.set_status(format!("deleted {}", confirmed.display()), Severity::Info);
                    self.queue_dirty_at = Some(Instant::now());
                    if self.selected_pool_entry.as_ref() == Some(&confirmed) {
                        self.selected_pool_entry = None;
                    }
                }
            }
        }
    }
}

fn pool_row(
    ui: &mut Ui,
    entry: &PoolEntry,
    selected: bool,
    pending_trash: Option<&Path>,
    do_play: &mut Option<PathBuf>,
    do_promote: &mut Option<PathBuf>,
    do_trash: &mut Option<PathBuf>,
) -> bool {
    let (tag, tag_color) = if entry.audition {
        ("AUD", ACCENT_AMBER)
    } else {
        ("LCK", ACCENT_CYAN)
    };
    let bg = if selected { BG_CARD_HOT } else { BG_CARD };
    let border = if selected { ACCENT_CYAN } else { BORDER };
    let armed_for_trash = pending_trash == Some(entry.path.as_path());
    let mut clicked = false;
    Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .rounding(Rounding::same(3.0))
        .inner_margin(Margin::symmetric(6.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let resp = ui.selectable_label(
                    selected,
                    RichText::new(format!(" {} {}", tag, entry.display))
                        .color(if selected { ACCENT_CYAN } else { FG_TEXT })
                        .monospace(),
                );
                if resp.clicked() {
                    clicked = true;
                }
                ui.label(RichText::new(tag).color(tag_color).small().monospace());
                if ui
                    .small_button("▶")
                    .on_hover_text("Preview  (P when selected)")
                    .clicked()
                {
                    *do_play = Some(entry.path.clone());
                }
                if entry.audition {
                    if ui
                        .small_button("⇧")
                        .on_hover_text("Promote to locked pool")
                        .clicked()
                    {
                        *do_promote = Some(entry.path.clone());
                    }
                } else {
                    ui.add_space(18.0);
                }
                let trash_label = if armed_for_trash { "🗑 confirm" } else { "🗑" };
                let trash_color = if armed_for_trash { ACCENT_RED } else { FG_TEXT };
                if ui
                    .add(
                        egui::Button::new(RichText::new(trash_label).color(trash_color))
                            .small()
                            .fill(if armed_for_trash { Color32::from_rgb(60, 20, 20) } else { BG_CARD }),
                    )
                    .on_hover_text(if armed_for_trash {
                        "Click again within 3s to delete"
                    } else {
                        "Delete (requires confirmation)"
                    })
                    .clicked()
                {
                    *do_trash = Some(entry.path.clone());
                }
            });
        });
    clicked
}

// ---------------------------------------------------------------------------
// log + status

impl RecorderApp {
    fn show_log_panel(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("session log").color(ACCENT_CYAN).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(log) = &self.log {
                    ui.label(
                        RichText::new(log.path().display().to_string())
                            .color(FG_MUTE)
                            .small()
                            .monospace(),
                    );
                }
            });
        });
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in self.log_tail.iter() {
                    ui.label(RichText::new(line).color(FG_DIM).small().monospace());
                }
            });
    }

    fn show_status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let shortcuts = "[F1] help  ·  [Space] rec/stop  ·  [Enter] save  ·  [Del] discard  ·  [P] preview";
            ui.label(RichText::new(shortcuts).color(FG_MUTE).small().monospace());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Stats on the far right — session-age + saves count.
                let session_secs = self.session_start.elapsed().as_secs();
                let h = session_secs / 3600;
                let m = (session_secs % 3600) / 60;
                let s = session_secs % 60;
                ui.label(
                    RichText::new(format!(
                        "session {h:02}:{m:02}:{s:02}  ·  {} saved",
                        self.takes_saved
                    ))
                    .color(FG_DIM)
                    .small()
                    .monospace(),
                );
                ui.separator();
                if let Some((msg, t, sev)) = &self.status {
                    if t.elapsed() < STATUS_TTL {
                        let c = match sev {
                            Severity::Info => ACCENT_GREEN,
                            Severity::Warn => ACCENT_AMBER,
                            Severity::Err => ACCENT_RED,
                        };
                        ui.label(RichText::new(msg).color(c).strong());
                    }
                }
            });
        });
    }

    fn show_help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new(RichText::new("th_record · help").color(ACCENT_CYAN).strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(520.0)
            .frame(
                Frame::window(&ctx.style())
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, ACCENT_MAGENTA))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                section_heading(ui, "workflow");
                wrap_text(
                    ui,
                    "1. Click a queue entry to focus the slot — the brief panel on \
                     the left shows the recording intent.\n\
                     2. Press Space to record. Watch meters + clip indicators.\n\
                     3. Press Space (or S) to stop. Trim each channel via the \
                     yellow drag handles on the waveform.\n\
                     4. Assign per-channel destination (defaults are sane for \
                     each session type). Press Enter to SAVE ALL.\n\
                     5. Pool panel lists locked + audition takes for the focused \
                     slot. ▶ previews, ⇧ promotes audition → locked, 🗑 deletes \
                     (requires a second click to confirm).",
                    FG_TEXT,
                );
                ui.add_space(8.0);
                section_heading(ui, "keyboard");
                let rows: &[(&str, &str)] = &[
                    ("Space", "toggle record / stop"),
                    ("S", "stop (while recording)"),
                    ("Enter", "save all (when a take is loaded)"),
                    ("Del / Backspace", "discard the current take"),
                    ("P", "preview selected pool entry"),
                    ("F1", "toggle this help"),
                    ("Esc", "close help"),
                ];
                for (k, desc) in rows {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{k:<16}"))
                                .color(ACCENT_AMBER)
                                .monospace(),
                        );
                        ui.label(RichText::new(*desc).color(FG_TEXT));
                    });
                }
                ui.add_space(8.0);
                section_heading(ui, "session types");
                wrap_text(
                    ui,
                    "SFX        — each channel → one unit/event slot\n\
                     Music + CV — ch0+ch1 stereo music; remaining channels → CV params\n\
                     CV only    — every channel → CV param",
                    FG_TEXT,
                );
                ui.add_space(8.0);
                section_heading(ui, "visual language");
                wrap_text(
                    ui,
                    "• amber waveform handles drag-to-trim\n\
                     • cyan line = play cursor during channel preview\n\
                     • red card border = channel clipped during capture\n\
                     • red pulsing REC button = actively recording\n\
                     • cyan border on queue / pool row = focused / selected",
                    FG_TEXT,
                );
            });
        self.show_help = open;
    }
}

// ---------------------------------------------------------------------------
// destination picker (unchanged functionality; cleaner visual framing)

fn destination_picker(
    ui: &mut Ui,
    channel: usize,
    total_channels: usize,
    queue: &Queue,
    dest: &mut Destination,
) -> bool {
    let mut changed = false;

    let kind = match dest {
        Destination::Discard => 0,
        Destination::Slot { .. } => 1,
        Destination::StereoLeft { .. } => 2,
        Destination::StereoRight { .. } => 3,
        Destination::CvParam { .. } => 4,
    };
    let mut new_kind = kind;
    egui::ComboBox::from_id_source(("dest_kind", channel))
        .selected_text(match kind {
            0 => "discard",
            1 => "sample slot (mono)",
            2 => "stereo L",
            3 => "stereo R",
            4 => "CV param",
            _ => "?",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut new_kind, 0, "discard");
            ui.selectable_value(&mut new_kind, 1, "sample slot (mono)");
            ui.selectable_value(&mut new_kind, 2, "stereo L");
            ui.selectable_value(&mut new_kind, 3, "stereo R");
            ui.selectable_value(&mut new_kind, 4, "CV param");
        });
    if new_kind != kind {
        *dest = match new_kind {
            0 => Destination::Discard,
            1 => Destination::Slot {
                unit_id: queue
                    .first_recommendation()
                    .map(|q| q.unit_id.clone())
                    .unwrap_or_else(|| "rusher".into()),
                event: queue
                    .first_recommendation()
                    .map(|q| q.event.clone())
                    .unwrap_or_else(|| "spawn".into()),
                variant: "baseline".into(),
                audition: true,
            },
            2 => Destination::StereoLeft {
                unit_id: "music".into(),
                event: "base_loop".into(),
                variant: "baseline".into(),
                audition: true,
                paired_right: (channel + 1).min(total_channels.saturating_sub(1)),
            },
            3 => Destination::StereoRight {
                paired_left: channel.saturating_sub(1),
            },
            4 => Destination::CvParam {
                param: audio_params::known_names()
                    .next()
                    .map(String::from)
                    .unwrap_or_else(|| "particle.density_scale".into()),
                stem: "base_loop".into(),
                audition: true,
            },
            _ => Destination::Discard,
        };
        changed = true;
    }

    match dest {
        Destination::Discard => {
            ui.label(RichText::new("(no file written)").color(FG_MUTE).italics());
        }
        Destination::Slot { unit_id, event, variant, audition }
        | Destination::StereoLeft {
            unit_id, event, variant, audition, ..
        } => {
            let units = queue::known_unit_ids(queue);
            egui::ComboBox::from_id_source(("unit", channel))
                .selected_text(unit_id.clone())
                .show_ui(ui, |ui| {
                    for u in &units {
                        if ui.selectable_label(unit_id == u, u).clicked() {
                            *unit_id = u.clone();
                            changed = true;
                        }
                    }
                });
            let events = queue::events_for_unit(queue, unit_id);
            egui::ComboBox::from_id_source(("event", channel))
                .selected_text(event.clone())
                .show_ui(ui, |ui| {
                    for e in &events {
                        if ui.selectable_label(event == e, e).clicked() {
                            *event = e.clone();
                            changed = true;
                        }
                    }
                });
            egui::ComboBox::from_id_source(("variant", channel))
                .selected_text(variant.clone())
                .show_ui(ui, |ui| {
                    for v in VARIANTS {
                        if ui.selectable_label(variant == v, *v).clicked() {
                            *variant = (*v).to_string();
                            changed = true;
                        }
                    }
                });
            let mut aud = *audition;
            if ui.checkbox(&mut aud, "audition").changed() {
                *audition = aud;
                changed = true;
            }
            if let Destination::StereoLeft { paired_right, .. } = dest {
                let mut pr = *paired_right;
                egui::ComboBox::from_id_source(("pair", channel))
                    .selected_text(format!("R=ch{}", pr + 1))
                    .show_ui(ui, |ui| {
                        for i in 0..total_channels {
                            if i == channel {
                                continue;
                            }
                            if ui.selectable_label(pr == i, format!("ch{}", i + 1)).clicked() {
                                pr = i;
                                changed = true;
                            }
                        }
                    });
                *paired_right = pr;
            }
        }
        Destination::StereoRight { paired_left } => {
            let mut pl = *paired_left;
            egui::ComboBox::from_id_source(("pair_l", channel))
                .selected_text(format!("L=ch{}", pl + 1))
                .show_ui(ui, |ui| {
                    for i in 0..total_channels {
                        if i == channel {
                            continue;
                        }
                        if ui.selectable_label(pl == i, format!("ch{}", i + 1)).clicked() {
                            pl = i;
                            changed = true;
                        }
                    }
                });
            *paired_left = pl;
        }
        Destination::CvParam { param, stem, audition } => {
            egui::ComboBox::from_id_source(("cv_param", channel))
                .selected_text(param.clone())
                .show_ui(ui, |ui| {
                    for n in audio_params::known_names() {
                        if ui.selectable_label(param == n, n).clicked() {
                            *param = n.into();
                            changed = true;
                        }
                    }
                });
            let mut stem_s = stem.clone();
            if ui.text_edit_singleline(&mut stem_s).changed() {
                *stem = stem_s;
                changed = true;
            }
            let mut aud = *audition;
            if ui.checkbox(&mut aud, "audition").changed() {
                *audition = aud;
                changed = true;
            }
        }
    }

    changed
}

// ---------------------------------------------------------------------------
// leaf widgets

fn meter(ui: &mut Ui, index: usize, peak: f32, hold: f32) {
    let width = 170.0;
    let height = 22.0;
    let (rect, _resp) =
        ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_rgb(20, 16, 28));
    p.rect_stroke(rect, 3.0, Stroke::new(1.0, BORDER));
    let filled_w = (peak.clamp(0.0, 1.0)) * rect.width();
    let fill_color = meter_color(peak);
    if filled_w > 0.0 {
        p.rect_filled(
            Rect::from_min_max(
                rect.left_top(),
                egui::Pos2::new(rect.left() + filled_w, rect.bottom()),
            ),
            3.0,
            fill_color,
        );
    }
    // Peak-hold tick.
    if hold > 0.0 {
        let x = rect.left() + hold.clamp(0.0, 1.0) * rect.width();
        p.line_segment(
            [egui::Pos2::new(x, rect.top() + 2.0), egui::Pos2::new(x, rect.bottom() - 2.0)],
            Stroke::new(2.0, meter_color(hold)),
        );
    }
    p.text(
        egui::Pos2::new(rect.left() + 5.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("ch{}", index + 1),
        egui::FontId::monospace(10.0),
        Color32::WHITE,
    );
    let db = if peak <= 0.0 {
        "-∞".into()
    } else {
        format!("{:+.0} dB", 20.0 * peak.log10())
    };
    p.text(
        egui::Pos2::new(rect.right() - 5.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        db,
        egui::FontId::monospace(10.0),
        FG_TEXT,
    );
}

fn meter_color(v: f32) -> Color32 {
    if v >= 0.98 {
        ACCENT_RED
    } else if v >= 0.7 {
        ACCENT_AMBER
    } else if v > 0.02 {
        ACCENT_GREEN
    } else {
        FG_MUTE
    }
}

/// Clamp a linear amplitude to the displayable dBFS window used by
/// the device meter (-60 .. 0 dBFS), returning a 0..1 fraction of the
/// bar. Samples quieter than -60 dBFS read as zero so the meter isn't
/// permanently waggling on noise floor.
const METER_FLOOR_DB: f32 = -60.0;

fn amp_to_bar(amp: f32) -> f32 {
    if amp <= 1e-6 {
        return 0.0;
    }
    let db = 20.0 * amp.log10();
    ((db - METER_FLOOR_DB) / (0.0 - METER_FLOOR_DB)).clamp(0.0, 1.0)
}

/// Colour the bar based on dBFS zones rather than raw amplitude so
/// the green/yellow/red thresholds stay perceptually meaningful.
/// - green      below -12 dBFS
/// - yellow     -12 .. -3 dBFS
/// - red        -3 dBFS and above (clipping risk)
fn dbfs_color(amp: f32) -> Color32 {
    if amp <= 1e-6 {
        return FG_MUTE;
    }
    let db = 20.0 * amp.log10();
    if db >= -3.0 {
        ACCENT_RED
    } else if db >= -12.0 {
        ACCENT_AMBER
    } else {
        ACCENT_GREEN
    }
}

/// Device-strip meter: one row per channel, dBFS-scaled, green/yellow/
/// red zones baked in. `rms` drives the solid fill, `peak` drives a
/// lighter overlay, and `hold` draws a short tick that lingers after
/// a transient so the user can see recent headroom. All in 0..1
/// linear amplitude.
fn device_channel_meter(ui: &mut Ui, index: usize, rms: f32, peak: f32, hold: f32) {
    let width = 220.0;
    let height = 20.0;
    let (rect, _resp) =
        ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let p = ui.painter();

    // Channel label sits on a small strip at the left so the bar has
    // a predictable width regardless of ch index.
    let label_w = 32.0;
    let label_rect = Rect::from_min_max(
        rect.left_top(),
        egui::Pos2::new(rect.left() + label_w, rect.bottom()),
    );
    p.text(
        label_rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("ch{}", index + 1),
        egui::FontId::monospace(11.0),
        ACCENT_CYAN,
    );

    let bar = Rect::from_min_max(
        egui::Pos2::new(rect.left() + label_w + 2.0, rect.top() + 2.0),
        egui::Pos2::new(rect.right() - 58.0, rect.bottom() - 2.0),
    );
    p.rect_filled(bar, 3.0, Color32::from_rgb(20, 16, 28));
    p.rect_stroke(bar, 3.0, Stroke::new(1.0, BORDER));

    // Zone guides (-12 and -3 dBFS) so the user can read the bar
    // without squinting at the dB number.
    for db in [-12.0f32, -3.0] {
        let f = ((db - METER_FLOOR_DB) / (0.0 - METER_FLOOR_DB)).clamp(0.0, 1.0);
        let x = bar.left() + f * bar.width();
        p.line_segment(
            [
                egui::Pos2::new(x, bar.top()),
                egui::Pos2::new(x, bar.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(120, 120, 140, 40)),
        );
    }

    // Peak — ghosted background layer behind the solid RMS fill so
    // transients are still visible on quiet material.
    let peak_w = amp_to_bar(peak) * bar.width();
    if peak_w > 0.0 {
        let peak_col = dbfs_color(peak);
        let ghost = Color32::from_rgba_unmultiplied(
            peak_col.r(),
            peak_col.g(),
            peak_col.b(),
            90,
        );
        p.rect_filled(
            Rect::from_min_max(
                bar.left_top(),
                egui::Pos2::new(bar.left() + peak_w, bar.bottom()),
            ),
            3.0,
            ghost,
        );
    }

    // RMS — solid fill, reads as "how loud is this actually".
    let rms_w = amp_to_bar(rms) * bar.width();
    if rms_w > 0.0 {
        p.rect_filled(
            Rect::from_min_max(
                bar.left_top(),
                egui::Pos2::new(bar.left() + rms_w, bar.bottom()),
            ),
            3.0,
            dbfs_color(rms),
        );
    }

    // Peak-hold tick.
    if hold > 1e-6 {
        let x = bar.left() + amp_to_bar(hold) * bar.width();
        p.line_segment(
            [
                egui::Pos2::new(x, bar.top() + 1.0),
                egui::Pos2::new(x, bar.bottom() - 1.0),
            ],
            Stroke::new(2.0, dbfs_color(hold)),
        );
    }

    // dBFS readout — use the louder of rms/peak so a silent channel
    // shows -∞ but an active one shows a useful number.
    let level = peak.max(rms);
    let db_text = if level <= 1e-6 {
        "-∞ dB".to_string()
    } else {
        format!("{:+.0} dB", 20.0 * level.log10())
    };
    p.text(
        egui::Pos2::new(rect.right() - 4.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        db_text,
        egui::FontId::monospace(10.0),
        FG_TEXT,
    );
}

fn section_heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text.to_ascii_uppercase())
            .color(ACCENT_MAGENTA)
            .small()
            .strong(),
    );
}

fn wrap_text(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text.trim()).color(color));
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n - 1).collect();
        format!("{head}…")
    }
}
