//! The recorder's egui `App` — owns device state, the shared capture
//! buffer, per-channel UI state (trim, destination), the audit queue,
//! and the preview player. Update loop follows the layout sketch in
//! `audio-spec.md §14.7`.

use anyhow::Result;
use egui::{Color32, RichText, Ui};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::capture::{self, CaptureState, DeviceInfo};
use crate::io::{self, Destination, PoolEntry, Preview, SaveResult, SessionLog, SessionType};
use crate::queue::{self, Queue, QueueEntry};
use crate::waveform::WaveformView;

use terminal_hell::audio::params as audio_params;

const PEAK_DECAY_PER_SECOND: f32 = 0.75; // -2.5dB/s visual falloff
const STATUS_TTL: Duration = Duration::from_secs(6);
const VARIANTS: &[&str] = &["baseline", "p25", "p50", "p75", "p100", "phantom"];

pub struct RecorderApp {
    // ---- device + capture ----------------------------------------
    devices: Vec<DeviceInfo>,
    selected_device: Option<String>,
    capture: Arc<Mutex<CaptureState>>,
    stream: Option<cpal::Stream>,

    // ---- content root + audit queue ------------------------------
    pack_root: PathBuf,
    queue: Queue,
    queue_dirty_at: Option<Instant>,

    // ---- per-channel UI state ------------------------------------
    trims: Vec<(usize, usize)>,
    destinations: Vec<Destination>,
    waveform_views: Vec<WaveformView>,
    /// Most recent STOP'd take. When Some, displays the waveforms +
    /// assign panel. When None, the waveforms show live capture.
    take_samples: Option<Vec<Vec<f32>>>,
    take_rate: u32,

    // ---- session config ------------------------------------------
    session_type: SessionType,

    // ---- playback ------------------------------------------------
    preview: Option<Preview>,

    // ---- session log --------------------------------------------
    log: Option<SessionLog>,

    // ---- transient UI state --------------------------------------
    status: Option<(String, Instant)>,
    last_frame: Instant,
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

        Self {
            devices,
            selected_device,
            capture: Arc::new(Mutex::new(CaptureState::new())),
            stream: None,
            pack_root,
            queue,
            queue_dirty_at: None,
            trims: Vec::new(),
            destinations: Vec::new(),
            waveform_views: Vec::new(),
            take_samples: None,
            take_rate: 48_000,
            session_type: SessionType::Sfx,
            preview,
            log,
            status: None,
            last_frame: Instant::now(),
        }
    }

    fn rebuild_queue(&mut self) {
        match Queue::rebuild(&self.pack_root) {
            Ok(q) => self.queue = q,
            Err(err) => self.set_status(format!("queue rebuild failed: {err}"), true),
        }
    }

    fn set_status(&mut self, text: impl Into<String>, is_err: bool) {
        let t = text.into();
        if is_err {
            tracing::warn!("{t}");
        } else {
            tracing::info!("{t}");
        }
        self.status = Some((t, Instant::now()));
    }

    fn ensure_stream(&mut self) {
        if self.stream.is_some() {
            return;
        }
        let Some(name) = self.selected_device.clone() else {
            self.set_status("no input device selected", true);
            return;
        };
        match capture::start_stream(&name, self.capture.clone()) {
            Ok(s) => {
                self.stream = Some(s);
                // Pre-size per-channel UI state to the actual channel count.
                let (channels, rate) = {
                    let st = self.capture.lock().unwrap();
                    (st.channels as usize, st.rate)
                };
                self.resize_channel_state(channels);
                self.take_rate = rate;
                self.set_status(format!("stream armed: {name}"), false);
            }
            Err(err) => {
                self.set_status(format!("failed to open {name}: {err}"), true);
            }
        }
    }

    fn stop_stream(&mut self) {
        self.stream = None;
        let mut st = self.capture.lock().unwrap();
        st.recording = false;
    }

    fn resize_channel_state(&mut self, channels: usize) {
        self.trims.resize(channels, (0, 0));
        self.destinations.resize(channels, Destination::Discard);
        self.waveform_views.clear();
        for i in 0..channels {
            self.waveform_views.push(WaveformView {
                channel_index: i,
                label: format!("ch{}", i + 1),
                armed: true,
            });
        }
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
        }
        self.take_samples = None;
        self.set_status("recording…", false);
    }

    fn stop_recording(&mut self) {
        let drained = {
            let mut st = self.capture.lock().unwrap();
            st.recording = false;
            st.take_samples()
        };
        if drained.iter().all(|v| v.is_empty()) {
            self.set_status("stop: no samples captured", true);
            return;
        }
        let channels = drained.len();
        // Reset trims to full range per channel.
        self.trims = drained.iter().map(|v| (0, v.len())).collect();
        // Reapply session-type defaults so the destinations match the
        // recorder's current mode.
        self.apply_session_defaults(channels);
        let total = drained.first().map(|v| v.len()).unwrap_or(0);
        let rate = self.take_rate.max(1);
        self.take_samples = Some(drained);
        self.set_status(
            format!(
                "take captured: {channels} ch × {} samples (~{:.1}s)",
                total,
                total as f32 / rate as f32
            ),
            false,
        );
    }

    fn apply_session_defaults(&mut self, channels: usize) {
        self.destinations = (0..channels).map(|_| Destination::Discard).collect();
        if let Some(rec) = self.queue.first_recommendation().cloned() {
            match self.session_type {
                SessionType::Sfx => {
                    // Pre-fill ch0 with the top-priority slot as
                    // audition; user can tweak.
                    self.destinations[0] = Destination::Slot {
                        unit_id: rec.unit_id,
                        event: rec.event,
                        variant: rec.variant,
                        audition: true,
                    };
                }
                SessionType::MusicCv => {
                    if channels >= 2 {
                        // Stereo pair on ch0+ch1 for music, rest
                        // default discard — user picks CV param per
                        // channel.
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
    }

    fn save_current_take(&mut self) {
        let Some(samples) = self.take_samples.as_ref() else {
            self.set_status("no take to save", true);
            return;
        };
        if samples.is_empty() {
            self.set_status("take is empty", true);
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
                if let Some(log) = &self.log {
                    for line in &res.log_lines {
                        log.log(line);
                    }
                }
                self.set_status(
                    format!("saved {written} file(s), skipped {skipped}"),
                    false,
                );
                self.queue_dirty_at = Some(Instant::now());
                self.take_samples = None;
            }
            Err(err) => self.set_status(format!("save failed: {err}"), true),
        }
    }

    fn play_path(&mut self, path: PathBuf) {
        if let Some(preview) = &self.preview {
            if let Err(err) = preview.play(&path) {
                self.set_status(format!("preview failed: {err}"), true);
            } else {
                self.set_status(format!("preview: {}", path.display()), false);
            }
        } else {
            self.set_status("preview device not open", true);
        }
    }
}

impl eframe::App for RecorderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Continuous repaint so meters animate during capture.
        ctx.request_repaint_after(Duration::from_millis(33));

        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.0, 0.5);
        self.last_frame = Instant::now();

        // Decay peaks in the shared state so the UI sees a falloff
        // curve even while the capture thread is idle.
        let decay = PEAK_DECAY_PER_SECOND.powf(dt);
        {
            let mut st = self.capture.lock().unwrap();
            st.decay_peaks(decay);
        }

        // Debounced queue rebuild after a save or a pool promote/trash.
        if let Some(at) = self.queue_dirty_at {
            if at.elapsed() > Duration::from_millis(250) {
                self.rebuild_queue();
                self.queue_dirty_at = None;
            }
        }

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| self.show_top_bar(ui));
        egui::SidePanel::right("queue_panel")
            .default_width(320.0)
            .show(ctx, |ui| self.show_queue_panel(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.show_main(ui));
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| self.show_status_bar(ui));
    }
}

// -----------------------------------------------------------------
// UI sections

impl RecorderApp {
    fn show_top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("th_record");
            ui.separator();

            // Device picker
            let current = self
                .selected_device
                .clone()
                .unwrap_or_else(|| "(no device)".into());
            let mut chosen: Option<String> = None;
            egui::ComboBox::from_label("input")
                .selected_text(current.clone())
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
                }
            }

            if ui.button("🔄 refresh devices").clicked() {
                self.devices = capture::list_input_devices();
            }

            ui.separator();

            // Session type toggle
            let mut st = self.session_type;
            egui::ComboBox::from_label("session")
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

            ui.separator();

            let recording = self
                .capture
                .lock()
                .map(|s| s.recording)
                .unwrap_or(false);
            let rec_text = if recording {
                RichText::new("● RECORDING").color(Color32::from_rgb(230, 80, 80))
            } else {
                RichText::new("● REC").color(Color32::from_gray(220))
            };
            if ui.button(rec_text).clicked() {
                if !recording {
                    self.start_recording();
                }
            }
            if ui.button("■ STOP").clicked() {
                self.stop_recording();
            }
            if ui.button("💾 SAVE ALL").clicked() {
                self.save_current_take();
            }
            if ui.button("🗑 discard take").clicked() {
                self.take_samples = None;
                self.set_status("take discarded", false);
            }
        });
    }

    fn show_queue_panel(&mut self, ui: &mut Ui) {
        ui.heading("audit queue");
        ui.label(format!(
            "{:.1}% · {}/{} slots filled",
            self.queue.coverage_percent,
            self.queue.filled_slots,
            self.queue.total_slots,
        ));
        if ui.button("refresh audit").clicked() {
            self.rebuild_queue();
        }
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in self.queue.entries.iter().take(40) {
                show_queue_entry(ui, entry);
            }
        });
    }

    fn show_main(&mut self, ui: &mut Ui) {
        // Meters row (always visible while stream is armed).
        if !self.waveform_views.is_empty() {
            ui.horizontal(|ui| {
                let peaks = self
                    .capture
                    .lock()
                    .map(|s| s.peak.clone())
                    .unwrap_or_default();
                for (i, peak) in peaks.iter().enumerate() {
                    show_meter(ui, i, *peak);
                }
            });
            ui.separator();
        }

        // Waveforms + trim per channel.
        if let Some(samples) = self.take_samples.clone() {
            self.show_take_assign(ui, &samples);
        } else {
            ui.label(
                RichText::new("no take yet — hit REC to capture, STOP to freeze")
                    .color(Color32::from_gray(170)),
            );
            ui.add_space(8.0);
            // Show live samples as they stream in when recording.
            let live = self
                .capture
                .lock()
                .map(|s| s.samples.clone())
                .unwrap_or_default();
            for (i, ch) in live.iter().enumerate() {
                let view = self
                    .waveform_views
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| WaveformView {
                        channel_index: i,
                        label: format!("ch{}", i + 1),
                        armed: true,
                    });
                let mut t_in = 0usize;
                let mut t_out = ch.len();
                let meter_peak = self
                    .capture
                    .lock()
                    .ok()
                    .and_then(|s| s.peak.get(i).copied())
                    .unwrap_or(0.0);
                view.show(ui, ch, &mut t_in, &mut t_out, meter_peak);
            }
        }
    }

    fn show_take_assign(&mut self, ui: &mut Ui, samples: &[Vec<f32>]) {
        let queue = std::mem::take(&mut self.queue);
        let mut assignment_changed = false;
        for (i, ch_samples) in samples.iter().enumerate() {
            let view = self
                .waveform_views
                .get(i)
                .cloned()
                .unwrap_or_else(|| WaveformView {
                    channel_index: i,
                    label: format!("ch{}", i + 1),
                    armed: true,
                });
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

            ui.push_id(i, |ui| {
                view.show(ui, ch_samples, &mut t_in, &mut t_out, meter_peak);

                ui.horizontal(|ui| {
                    ui.label(format!("ch{} →", i + 1));
                    let mut dest = self.destinations.get(i).cloned().unwrap_or_default();
                    if show_destination_picker(ui, i, samples.len(), &queue, &mut dest) {
                        self.destinations[i] = dest;
                        assignment_changed = true;
                    }
                });
            });

            if i < self.trims.len() {
                self.trims[i] = (t_in, t_out);
            }
        }

        // Show the current pool for the slot on ch0 if it's a Slot
        // destination — quick-access promote/trash + play preview.
        if let Some(Destination::Slot { unit_id, event, variant, .. })
        | Some(Destination::StereoLeft { unit_id, event, variant, .. }) =
            self.destinations.first().cloned()
        {
            ui.separator();
            ui.label(
                RichText::new(format!("current pool: {unit_id}.{event}.{variant}"))
                    .color(Color32::from_gray(220)),
            );
            let pool = io::list_slot_pool(&self.pack_root, &unit_id, &event, &variant);
            let mut do_play: Option<PathBuf> = None;
            let mut do_promote: Option<PathBuf> = None;
            let mut do_trash: Option<PathBuf> = None;
            egui::Grid::new("pool").striped(true).show(ui, |ui| {
                for entry in &pool {
                    render_pool_row(ui, entry, &mut do_play, &mut do_promote, &mut do_trash);
                    ui.end_row();
                }
                if pool.is_empty() {
                    ui.label("(empty)");
                    ui.end_row();
                }
            });
            if let Some(p) = do_play {
                self.play_path(p);
            }
            if let Some(p) = do_promote {
                match io::promote(&self.pack_root, &unit_id, &event, &variant, &p) {
                    Ok(new_path) => {
                        if let Some(log) = &self.log {
                            log.log(format!("promote {} → {}", p.display(), new_path.display()));
                        }
                        self.set_status(
                            format!("promoted → {}", new_path.display()),
                            false,
                        );
                        self.queue_dirty_at = Some(Instant::now());
                    }
                    Err(err) => self.set_status(format!("promote failed: {err}"), true),
                }
            }
            if let Some(p) = do_trash {
                if let Err(err) = io::trash(&p) {
                    self.set_status(format!("trash failed: {err}"), true);
                } else {
                    if let Some(log) = &self.log {
                        log.log(format!("trash {}", p.display()));
                    }
                    self.set_status(format!("deleted {}", p.display()), false);
                    self.queue_dirty_at = Some(Instant::now());
                }
            }
        }

        self.queue = queue;
        let _ = assignment_changed; // reserved for future auto-save behavior
    }

    fn show_status_bar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some((msg, t)) = &self.status {
                if t.elapsed() < STATUS_TTL {
                    ui.label(RichText::new(msg).color(Color32::from_gray(220)));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(log) = &self.log {
                    ui.label(
                        RichText::new(format!("log: {}", log.path().display()))
                            .color(Color32::from_gray(120))
                            .small(),
                    );
                }
            });
        });
    }
}

// -----------------------------------------------------------------
// leaf widgets

fn show_meter(ui: &mut Ui, index: usize, peak: f32) {
    let width = 160.0;
    let height = 14.0;
    let (rect, _resp) =
        ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_rgb(28, 28, 34));
    let filled_w = (peak.clamp(0.0, 1.0)) * rect.width();
    let fill_color = if peak >= 0.98 {
        Color32::from_rgb(230, 80, 80)
    } else if peak >= 0.7 {
        Color32::from_rgb(230, 190, 60)
    } else {
        Color32::from_rgb(90, 210, 140)
    };
    if filled_w > 0.0 {
        p.rect_filled(
            egui::Rect::from_min_max(
                rect.left_top(),
                egui::Pos2::new(rect.left() + filled_w, rect.bottom()),
            ),
            3.0,
            fill_color,
        );
    }
    p.text(
        egui::Pos2::new(rect.left() + 4.0, rect.center().y),
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
        egui::Pos2::new(rect.right() - 4.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        db,
        egui::FontId::monospace(10.0),
        Color32::from_gray(230),
    );
}

fn show_queue_entry(ui: &mut Ui, entry: &QueueEntry) {
    let color = match entry.priority {
        queue::Priority::Blocker => Color32::from_rgb(230, 120, 120),
        queue::Priority::Thin => Color32::from_rgb(230, 200, 120),
        queue::Priority::VariantMissing => Color32::from_gray(200),
        queue::Priority::Healthy => Color32::from_gray(120),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{:?}", entry.priority)).color(color).small());
        ui.label(format!("{}.{}.{}", entry.unit_id, entry.event, entry.variant));
    });
    ui.label(
        RichText::new(format!(
            "    locked {} · audition {}  {}",
            entry.locked_count,
            entry.audition_count,
            entry
                .motif
                .as_deref()
                .map(|m| format!("(motif: {m})"))
                .unwrap_or_default()
        ))
        .small()
        .color(Color32::from_gray(150)),
    );
    ui.separator();
}

/// Returns true when the destination was modified.
fn show_destination_picker(
    ui: &mut Ui,
    channel: usize,
    total_channels: usize,
    queue: &Queue,
    dest: &mut Destination,
) -> bool {
    let mut changed = false;

    // Destination kind radio.
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

    // Second column — kind-specific fields.
    match dest {
        Destination::Discard => {
            ui.label(RichText::new("(file will not be written)").color(Color32::from_gray(140)));
        }
        Destination::Slot { unit_id, event, variant, audition }
        | Destination::StereoLeft {
            unit_id, event, variant, audition, ..
        } => {
            let units = queue::known_unit_ids(queue);
            let mut unit_changed = false;
            egui::ComboBox::from_id_source(("unit", channel))
                .selected_text(unit_id.clone())
                .show_ui(ui, |ui| {
                    for u in &units {
                        if ui
                            .selectable_label(unit_id == u, u)
                            .clicked()
                        {
                            *unit_id = u.clone();
                            unit_changed = true;
                        }
                    }
                });
            if unit_changed {
                changed = true;
                // Reset event to first known event of this unit.
                let events = queue::events_for_unit(queue, unit_id);
                if let Some(first) = events.first() {
                    *event = first.clone();
                }
            }

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
            if ui.checkbox(&mut aud, "audition pool").changed() {
                *audition = aud;
                changed = true;
            }

            if let Destination::StereoLeft { paired_right, .. } = dest {
                let mut pr = *paired_right;
                egui::ComboBox::from_id_source(("pair", channel))
                    .selected_text(format!("R = ch{}", pr + 1))
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
                .selected_text(format!("L = ch{}", pl + 1))
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
            if ui.checkbox(&mut aud, "audition pool").changed() {
                *audition = aud;
                changed = true;
            }
        }
    }

    changed
}

fn render_pool_row(
    ui: &mut Ui,
    entry: &PoolEntry,
    do_play: &mut Option<PathBuf>,
    do_promote: &mut Option<PathBuf>,
    do_trash: &mut Option<PathBuf>,
) {
    let tag = if entry.audition { "AUD" } else { "LCK" };
    let tag_color = if entry.audition {
        Color32::from_rgb(230, 190, 60)
    } else {
        Color32::from_rgb(120, 210, 200)
    };
    ui.label(RichText::new(tag).color(tag_color).monospace());
    ui.label(RichText::new(&entry.display).monospace());
    if ui.button("▶ play").clicked() {
        *do_play = Some(entry.path.clone());
    }
    if entry.audition {
        if ui.button("⇧ promote").clicked() {
            *do_promote = Some(entry.path.clone());
        }
    } else {
        ui.add_space(8.0);
    }
    if ui.button("🗑 trash").clicked() {
        *do_trash = Some(entry.path.clone());
    }
}
