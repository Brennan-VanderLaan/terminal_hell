//! Waveform visualization + trim handles.
//!
//! Draws a per-channel waveform with min/max downsampling (one
//! vertical line per pixel) and overlays two draggable trim handles.
//! Drag closer to the in-handle moves in; drag closer to the
//! out-handle moves out. Outputs the trim indices back into the
//! caller's state.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

const HEIGHT: f32 = 64.0;
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
}

impl WaveformView {
    /// Render a waveform. `samples` is the captured channel data;
    /// `trim_in` / `trim_out` are sample indices into it and may be
    /// mutated by drag interaction. `meter_peak` drives a small peak
    /// LED on the left edge.
    pub fn show(
        &self,
        ui: &mut Ui,
        samples: &[f32],
        trim_in: &mut usize,
        trim_out: &mut usize,
        meter_peak: f32,
    ) {
        let available = ui.available_width();
        let (response, painter) =
            ui.allocate_painter(Vec2::new(available, HEIGHT), Sense::click_and_drag());

        let rect = response.rect;
        paint_background(&painter, rect, self.armed);
        paint_label(&painter, rect, &self.label, meter_peak);

        if samples.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "no data — arm and hit REC",
                egui::FontId::default(),
                Color32::from_gray(140),
            );
            return;
        }

        paint_waveform(&painter, rect, samples, self.armed);
        paint_trims(&painter, rect, samples.len(), *trim_in, *trim_out);

        // Drag handling. Decide which handle based on proximity at
        // the drag-start point, then move that handle for the rest
        // of the drag.
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let target = nearest_handle(pos, rect, samples.len(), *trim_in, *trim_out);
                ui.data_mut(|d| {
                    d.insert_temp::<DragTarget>(response.id, target);
                });
            }
        }
        if response.dragged() {
            let target = ui
                .data(|d| d.get_temp::<DragTarget>(response.id))
                .unwrap_or(DragTarget::None);
            if let Some(pos) = response.interact_pointer_pos() {
                let sample_pos = pos_to_sample(pos.x, rect, samples.len());
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

fn paint_background(p: &egui::Painter, rect: Rect, armed: bool) {
    let bg = if armed {
        Color32::from_rgb(20, 26, 34)
    } else {
        Color32::from_rgb(24, 24, 24)
    };
    p.rect_filled(rect, 4.0, bg);
    // Center line.
    p.line_segment(
        [
            Pos2::new(rect.left(), rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0, Color32::from_gray(60)),
    );
}

fn paint_label(p: &egui::Painter, rect: Rect, label: &str, peak: f32) {
    let db = if peak <= 0.0 { -120.0 } else { 20.0 * peak.log10() };
    let peak_color = if peak >= 0.98 {
        Color32::from_rgb(230, 80, 80)
    } else if peak >= 0.7 {
        Color32::from_rgb(230, 190, 60)
    } else {
        Color32::from_rgb(90, 210, 140)
    };
    p.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::monospace(11.0),
        Color32::from_gray(220),
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
    // Shaded regions outside the trim window.
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
    // Handle lines.
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
    // Top/bottom grips on each handle.
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
