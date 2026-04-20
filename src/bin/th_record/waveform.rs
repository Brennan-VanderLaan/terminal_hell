//! Waveform visualization — min/max downsampled, draggable trim
//! handles, optional time ruler at the bottom and a play cursor line
//! when preview playback is active.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

const HEIGHT: f32 = 72.0;
const RULER_HEIGHT: f32 = 14.0;
const HANDLE_HITBOX_PX: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    None,
    In,
    Out,
}

#[derive(Clone)]
pub struct WaveformView {
    /// Held for future uses (e.g. per-channel context menus).
    #[allow(dead_code)]
    pub channel_index: usize,
    pub label: String,
    pub armed: bool,
    /// Sample rate the waveform was captured at. Drives the time
    /// ruler tick spacing + duration label.
    pub rate: u32,
    /// Sample index of the playback cursor, if previewing. When
    /// `Some`, draws a vertical line + filled triangle at the top
    /// of the waveform.
    pub cursor_sample: Option<usize>,
    /// Draws the second-stamped ruler underneath when true.
    pub show_ruler: bool,
    /// Red-outline the waveform card while recording this channel.
    pub hot: bool,
}

impl WaveformView {
    pub fn new(channel_index: usize, label: String) -> Self {
        Self {
            channel_index,
            label,
            armed: true,
            rate: 48_000,
            cursor_sample: None,
            show_ruler: true,
            hot: false,
        }
    }

    pub fn show(
        &self,
        ui: &mut Ui,
        samples: &[f32],
        trim_in: &mut usize,
        trim_out: &mut usize,
        meter_peak: f32,
    ) {
        let available = ui.available_width();
        let height = if self.show_ruler { HEIGHT + RULER_HEIGHT } else { HEIGHT };
        let (response, painter) =
            ui.allocate_painter(Vec2::new(available, height), Sense::click_and_drag());

        let full_rect = response.rect;
        let wave_rect = Rect::from_min_max(
            full_rect.left_top(),
            Pos2::new(full_rect.right(), full_rect.top() + HEIGHT),
        );
        let ruler_rect = Rect::from_min_max(
            Pos2::new(full_rect.left(), full_rect.top() + HEIGHT),
            full_rect.right_bottom(),
        );

        paint_background(&painter, wave_rect, self.armed, self.hot);
        paint_label(&painter, wave_rect, &self.label, meter_peak, self.hot);

        if samples.is_empty() {
            painter.text(
                wave_rect.center(),
                egui::Align2::CENTER_CENTER,
                "no data — arm and hit REC",
                egui::FontId::default(),
                Color32::from_gray(140),
            );
            if self.show_ruler {
                paint_ruler(&painter, ruler_rect, 0, self.rate);
            }
            return;
        }

        paint_waveform(&painter, wave_rect, samples, self.armed);
        paint_trims(&painter, wave_rect, samples.len(), *trim_in, *trim_out);
        if let Some(cursor) = self.cursor_sample {
            paint_cursor(&painter, wave_rect, samples.len(), cursor);
        }
        paint_duration(&painter, wave_rect, *trim_in, *trim_out, samples.len(), self.rate);
        if self.show_ruler {
            paint_ruler(&painter, ruler_rect, samples.len(), self.rate);
        }

        // Drag → move nearest trim handle. Only pay attention if the
        // interaction started inside the waveform rect (not the
        // ruler strip).
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                if wave_rect.contains(pos) {
                    let target = nearest_handle(pos, wave_rect, samples.len(), *trim_in, *trim_out);
                    ui.data_mut(|d| {
                        d.insert_temp::<DragTarget>(response.id, target);
                    });
                }
            }
        }
        if response.dragged() {
            let target = ui
                .data(|d| d.get_temp::<DragTarget>(response.id))
                .unwrap_or(DragTarget::None);
            if let Some(pos) = response.interact_pointer_pos() {
                let sample_pos = pos_to_sample(pos.x, wave_rect, samples.len());
                match target {
                    DragTarget::In => {
                        *trim_in = sample_pos.min(trim_out.saturating_sub(1));
                    }
                    DragTarget::Out => {
                        *trim_out = sample_pos.max(trim_in.saturating_add(1));
                    }
                    DragTarget::None => {}
                }
            }
        }
    }
}

fn paint_background(p: &egui::Painter, rect: Rect, armed: bool, hot: bool) {
    let bg = if hot {
        Color32::from_rgb(36, 18, 22)
    } else if armed {
        Color32::from_rgb(20, 26, 34)
    } else {
        Color32::from_rgb(24, 24, 24)
    };
    p.rect_filled(rect, 4.0, bg);
    let border = if hot {
        Stroke::new(1.5, Color32::from_rgb(230, 80, 80))
    } else {
        Stroke::new(1.0, Color32::from_gray(60))
    };
    p.rect_stroke(rect, 4.0, border);
    p.line_segment(
        [
            Pos2::new(rect.left(), rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0, Color32::from_gray(60)),
    );
}

fn paint_label(p: &egui::Painter, rect: Rect, label: &str, peak: f32, hot: bool) {
    let db = if peak <= 0.0 { -120.0 } else { 20.0 * peak.log10() };
    let peak_color = if peak >= 0.98 {
        Color32::from_rgb(230, 80, 80)
    } else if peak >= 0.7 {
        Color32::from_rgb(230, 190, 60)
    } else {
        Color32::from_rgb(90, 210, 140)
    };
    let label_color = if hot {
        Color32::from_rgb(240, 170, 170)
    } else {
        Color32::from_gray(220)
    };
    p.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::monospace(11.0),
        label_color,
    );
    p.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 22.0),
        egui::Align2::LEFT_TOP,
        format!("peak {:+.1} dB", db.clamp(-120.0, 6.0)),
        egui::FontId::monospace(10.0),
        peak_color,
    );
}

fn paint_waveform(p: &egui::Painter, rect: Rect, samples: &[f32], armed: bool) {
    let color = if armed {
        Color32::from_rgb(120, 210, 200)
    } else {
        Color32::from_rgb(110, 160, 150)
    };
    let w = rect.width().max(1.0) as usize;
    let len = samples.len();
    if len == 0 {
        return;
    }
    for px in 0..w {
        let lo_i = (px * len) / w;
        let hi_i = (((px + 1) * len) / w).clamp(lo_i + 1, len);
        let mut lo = 0.0f32;
        let mut hi = 0.0f32;
        for &s in &samples[lo_i..hi_i] {
            if s < lo {
                lo = s;
            }
            if s > hi {
                hi = s;
            }
        }
        let x = rect.left() + px as f32;
        let y_lo = rect.center().y - lo * (rect.height() * 0.45);
        let y_hi = rect.center().y - hi * (rect.height() * 0.45);
        p.line_segment(
            [Pos2::new(x, y_lo), Pos2::new(x, y_hi)],
            Stroke::new(1.0, color),
        );
    }
}

fn paint_trims(p: &egui::Painter, rect: Rect, total: usize, t_in: usize, t_out: usize) {
    if total == 0 {
        return;
    }
    let x_in = sample_to_x(t_in, rect, total);
    let x_out = sample_to_x(t_out, rect, total);
    let dim = Color32::from_black_alpha(130);
    if x_in > rect.left() {
        p.rect_filled(
            Rect::from_min_max(rect.left_top(), Pos2::new(x_in, rect.bottom())),
            0.0,
            dim,
        );
    }
    if x_out < rect.right() {
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x_out, rect.top()), rect.right_bottom()),
            0.0,
            dim,
        );
    }
    let handle_color = Color32::from_rgb(240, 200, 60);
    p.line_segment(
        [
            Pos2::new(x_in, rect.top()),
            Pos2::new(x_in, rect.bottom()),
        ],
        Stroke::new(2.0, handle_color),
    );
    p.line_segment(
        [
            Pos2::new(x_out, rect.top()),
            Pos2::new(x_out, rect.bottom()),
        ],
        Stroke::new(2.0, handle_color),
    );
    for x in [x_in, x_out] {
        p.rect_filled(
            Rect::from_center_size(Pos2::new(x, rect.top() + 4.0), Vec2::new(8.0, 8.0)),
            2.0,
            handle_color,
        );
        p.rect_filled(
            Rect::from_center_size(Pos2::new(x, rect.bottom() - 4.0), Vec2::new(8.0, 8.0)),
            2.0,
            handle_color,
        );
    }
}

fn paint_cursor(p: &egui::Painter, rect: Rect, total: usize, cursor: usize) {
    if total == 0 {
        return;
    }
    let x = sample_to_x(cursor, rect, total);
    let line_color = Color32::from_rgb(120, 220, 220);
    p.line_segment(
        [
            Pos2::new(x, rect.top()),
            Pos2::new(x, rect.bottom()),
        ],
        Stroke::new(1.5, line_color),
    );
    // Triangular marker at top so the cursor reads at a glance
    // even against a dense waveform.
    let tri = [
        Pos2::new(x, rect.top() + 0.0),
        Pos2::new(x - 4.0, rect.top() - 6.0),
        Pos2::new(x + 4.0, rect.top() - 6.0),
    ];
    p.add(egui::epaint::Shape::convex_polygon(
        tri.to_vec(),
        line_color,
        Stroke::NONE,
    ));
}

fn paint_duration(
    p: &egui::Painter,
    rect: Rect,
    t_in: usize,
    t_out: usize,
    total: usize,
    rate: u32,
) {
    if total == 0 || rate == 0 {
        return;
    }
    let kept = t_out.saturating_sub(t_in);
    let total_s = total as f32 / rate as f32;
    let kept_s = kept as f32 / rate as f32;
    let text = format!("kept {:.2}s  /  full {:.2}s", kept_s, total_s);
    p.text(
        Pos2::new(rect.right() - 6.0, rect.top() + 6.0),
        egui::Align2::RIGHT_TOP,
        text,
        egui::FontId::monospace(10.0),
        Color32::from_gray(200),
    );
}

fn paint_ruler(p: &egui::Painter, rect: Rect, total: usize, rate: u32) {
    p.rect_filled(rect, 0.0, Color32::from_rgb(14, 14, 20));
    let label_color = Color32::from_gray(170);
    let tick_color = Color32::from_gray(90);

    if total == 0 || rate == 0 {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "—",
            egui::FontId::monospace(9.0),
            label_color,
        );
        return;
    }

    let total_s = total as f32 / rate as f32;
    // Pick a tick interval that yields 4–12 ticks across the rect.
    let interval = pick_tick_interval(total_s);
    let mut t = 0.0f32;
    while t <= total_s + 1e-4 {
        let frac = (t / total_s).clamp(0.0, 1.0);
        let x = rect.left() + frac * rect.width();
        p.line_segment(
            [
                Pos2::new(x, rect.top()),
                Pos2::new(x, rect.top() + 4.0),
            ],
            Stroke::new(1.0, tick_color),
        );
        p.text(
            Pos2::new(x + 2.0, rect.top() + 5.0),
            egui::Align2::LEFT_TOP,
            format!("{t:.2}s"),
            egui::FontId::monospace(9.0),
            label_color,
        );
        t += interval;
    }
}

fn pick_tick_interval(total_s: f32) -> f32 {
    let candidates = [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
    for c in candidates {
        if total_s / c <= 12.0 {
            return c;
        }
    }
    // Fallback for very long takes.
    (total_s / 8.0).max(0.01)
}

fn sample_to_x(sample: usize, rect: Rect, total: usize) -> f32 {
    if total == 0 {
        return rect.left();
    }
    rect.left() + (sample as f32 / total as f32) * rect.width()
}

fn pos_to_sample(x: f32, rect: Rect, total: usize) -> usize {
    let r = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    (r * total as f32) as usize
}

fn nearest_handle(pos: Pos2, rect: Rect, total: usize, t_in: usize, t_out: usize) -> DragTarget {
    let x_in = sample_to_x(t_in, rect, total);
    let x_out = sample_to_x(t_out, rect, total);
    let d_in = (pos.x - x_in).abs();
    let d_out = (pos.x - x_out).abs();
    if d_in <= d_out && d_in <= HANDLE_HITBOX_PX * 3.0 {
        DragTarget::In
    } else if d_out <= d_in && d_out <= HANDLE_HITBOX_PX * 3.0 {
        DragTarget::Out
    } else if pos.x < (x_in + x_out) * 0.5 {
        DragTarget::In
    } else {
        DragTarget::Out
    }
}
