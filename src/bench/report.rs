//! Per-scenario metrics + pretty/JSON formatting.

use std::time::Duration;

pub struct ScenarioReport {
    pub name: String,
    pub render_mode: &'static str,
    pub arena: (u16, u16),
    pub duration_secs: f32,
    pub frames: u32,
    /// Sim-tick latency stats (tick_authoritative + tick_client when
    /// render mode applies). Microseconds.
    pub tick_p50_us: u64,
    pub tick_p95_us: u64,
    pub tick_p99_us: u64,
    pub tick_max_us: u64,
    /// Frame-to-frame wall time stats (sleep-adjusted, so ≈16.7ms at
    /// 60 fps is the floor). Microseconds.
    pub frame_p50_us: u64,
    pub frame_p95_us: u64,
    pub frame_p99_us: u64,
    pub frame_max_us: u64,
    /// Effective fps (frames / duration).
    pub fps: f32,
    /// Peak entity counts seen during the scenario.
    pub peak_enemies: u32,
    pub peak_particles: u32,
    pub peak_projectiles: u32,
    pub peak_corpses: u32,
    /// Avg per-frame duration in microseconds, per instrumented
    /// subsystem label. Sorted descending by avg. Sourced from
    /// `FrameProfile::take_totals`.
    pub subsystems: Vec<SubsystemSample>,
    /// True if the scenario ended because all enemies died (when
    /// `stop_when_clear`). False if it ran to full duration.
    pub cleared_early: bool,
}

pub struct SubsystemSample {
    pub label: String,
    /// Total time spent in this scope across the whole scenario,
    /// divided by the frame count. Integer microseconds — good
    /// enough to spot hot paths and track regressions across commits.
    pub avg_us: u64,
    /// Total invocations of this scope across the scenario. Unlike
    /// the live perf overlay (which shows calls/frame), the bench
    /// report gives raw totals so sub-frame cadence scopes (called
    /// once every Nth frame) don't round away to zero.
    pub total_calls: u32,
}

impl ScenarioReport {
    /// Stdout-friendly table. One scenario per block, sortable.
    pub fn format_pretty(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "─── {} [{}] arena {}×{} · {:.1}s / {} frames · {:.1} fps{} ───\n",
            self.name,
            self.render_mode,
            self.arena.0,
            self.arena.1,
            self.duration_secs,
            self.frames,
            self.fps,
            if self.cleared_early { " (cleared early)" } else { "" },
        ));
        s.push_str(&format!(
            "  tick   p50 {:>6}µs  p95 {:>6}µs  p99 {:>6}µs  max {:>6}µs\n",
            self.tick_p50_us, self.tick_p95_us, self.tick_p99_us, self.tick_max_us,
        ));
        s.push_str(&format!(
            "  frame  p50 {:>6}µs  p95 {:>6}µs  p99 {:>6}µs  max {:>6}µs\n",
            self.frame_p50_us, self.frame_p95_us, self.frame_p99_us, self.frame_max_us,
        ));
        s.push_str(&format!(
            "  peak  enemies {}  particles {}  projectiles {}  corpses {}\n",
            self.peak_enemies, self.peak_particles, self.peak_projectiles, self.peak_corpses,
        ));
        if !self.subsystems.is_empty() {
            s.push_str("  subsystems (avg µs/frame):\n");
            for sub in self.subsystems.iter().take(12) {
                s.push_str(&format!(
                    "    {:<22} {:>6}µs  total calls {:>6}\n",
                    sub.label, sub.avg_us, sub.total_calls,
                ));
            }
        }
        s
    }
}

/// Hand-rolled JSON for the whole batch. Avoids adding serde_json
/// just for this — the schema is flat and stable enough that a
/// dedicated encoder is one screen of code.
pub fn reports_to_json(reports: &[ScenarioReport]) -> String {
    let mut s = String::from("[\n");
    for (i, r) in reports.iter().enumerate() {
        s.push_str(&format_report_json(r));
        if i + 1 < reports.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("]\n");
    s
}

fn format_report_json(r: &ScenarioReport) -> String {
    let mut s = String::new();
    s.push_str("  {\n");
    push_kv_str(&mut s, "name", &r.name);
    push_kv_str(&mut s, "render_mode", r.render_mode);
    push_kv_num(&mut s, "arena_w", r.arena.0 as u64);
    push_kv_num(&mut s, "arena_h", r.arena.1 as u64);
    push_kv_f32(&mut s, "duration_secs", r.duration_secs);
    push_kv_num(&mut s, "frames", r.frames as u64);
    push_kv_num(&mut s, "tick_p50_us", r.tick_p50_us);
    push_kv_num(&mut s, "tick_p95_us", r.tick_p95_us);
    push_kv_num(&mut s, "tick_p99_us", r.tick_p99_us);
    push_kv_num(&mut s, "tick_max_us", r.tick_max_us);
    push_kv_num(&mut s, "frame_p50_us", r.frame_p50_us);
    push_kv_num(&mut s, "frame_p95_us", r.frame_p95_us);
    push_kv_num(&mut s, "frame_p99_us", r.frame_p99_us);
    push_kv_num(&mut s, "frame_max_us", r.frame_max_us);
    push_kv_f32(&mut s, "fps", r.fps);
    push_kv_num(&mut s, "peak_enemies", r.peak_enemies as u64);
    push_kv_num(&mut s, "peak_particles", r.peak_particles as u64);
    push_kv_num(&mut s, "peak_projectiles", r.peak_projectiles as u64);
    push_kv_num(&mut s, "peak_corpses", r.peak_corpses as u64);
    // subsystems array (last — comma handling simpler at the tail).
    s.push_str("    \"subsystems\": [");
    for (i, sub) in r.subsystems.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "\n      {{\"label\":\"{}\",\"avg_us\":{},\"total_calls\":{}}}",
            json_escape(&sub.label),
            sub.avg_us,
            sub.total_calls,
        ));
    }
    if !r.subsystems.is_empty() {
        s.push_str("\n    ");
    }
    s.push_str("],\n");
    push_kv_bool(&mut s, "cleared_early", r.cleared_early);
    // Trim the trailing ",\n" from the last kv; simpler to cut and rebuild.
    if s.ends_with(",\n") {
        s.truncate(s.len() - 2);
        s.push('\n');
    }
    s.push_str("  }");
    s
}

fn push_kv_str(s: &mut String, k: &str, v: &str) {
    s.push_str(&format!("    \"{}\": \"{}\",\n", k, json_escape(v)));
}
fn push_kv_num(s: &mut String, k: &str, v: u64) {
    s.push_str(&format!("    \"{}\": {},\n", k, v));
}
fn push_kv_f32(s: &mut String, k: &str, v: f32) {
    s.push_str(&format!("    \"{}\": {:.4},\n", k, v));
}
fn push_kv_bool(s: &mut String, k: &str, v: bool) {
    s.push_str(&format!("    \"{}\": {},\n", k, v));
}

fn json_escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Percentile over a sorted slice. `p` in 0.0..=1.0. Empty slice
/// → 0. Used by the runner to collapse frame-time samples.
pub fn percentile_us(sorted: &[Duration], p: f32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)].as_micros() as u64
}
