//! Lightweight per-loop frame profiler. Goal: find out which
//! subsection of the sim/render pipeline is eating frame time when
//! the game starts grinding. Overhead is <1µs per begin/end pair so
//! we can sprinkle scopes liberally without biasing the measurement.
//!
//! Usage pattern:
//!
//! ```ignore
//! self.perf.begin("enemy_loop");
//! // ... work ...
//! self.perf.end("enemy_loop");
//! ```
//!
//! Counts (entity populations, queue depths) flow through
//! `set_count`. The report merges timings + counts into a single
//! sorted-by-time table dumped to the log console once per second.
//!
//! Labels must be `&'static str` so they're pointer-comparable +
//! allocation-free. New labels slot in wherever needed.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How often `end_frame` rolls the current-frame timings into the
/// rolling window that feeds the log report.
pub const REPORT_PERIOD: Duration = Duration::from_millis(1000);

#[derive(Default)]
pub struct FrameProfile {
    /// Start timestamps for currently-open scopes. A label may be
    /// opened and closed multiple times per frame; each begin stomps
    /// the previous start, so scopes MUST NOT overlap for the same
    /// label.
    starts: HashMap<&'static str, Instant>,
    /// Sum of elapsed time this frame, per label.
    this_frame: HashMap<&'static str, Duration>,
    /// Rolling window totals — summed across frames, divided by
    /// `frames_in_window` at report time for the avg.
    rolling_total: HashMap<&'static str, Duration>,
    /// Rolling invocation counts so we can report per-scope call
    /// counts (useful for "N projectiles / frame" style queries).
    rolling_calls: HashMap<&'static str, u32>,
    /// Latest counter values (set via `set_count`). Not rolling —
    /// we just report whatever was last set.
    counts: HashMap<&'static str, u64>,
    frames_in_window: u32,
    /// When we last emitted a report. Reports fire ~REPORT_PERIOD
    /// apart regardless of frame rate.
    last_report: Option<Instant>,
}

impl FrameProfile {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn begin(&mut self, label: &'static str) {
        self.starts.insert(label, Instant::now());
    }

    #[inline]
    pub fn end(&mut self, label: &'static str) {
        if let Some(start) = self.starts.remove(label) {
            let dur = start.elapsed();
            *self.this_frame.entry(label).or_default() += dur;
            *self.rolling_calls.entry(label).or_default() += 1;
        }
    }

    /// Record an entity / queue population. Overwrites any previous
    /// value for this label; the report shows the most recent.
    pub fn set_count(&mut self, label: &'static str, n: usize) {
        self.counts.insert(label, n as u64);
    }

    /// Call once at the end of every frame. Folds the current frame's
    /// timings into the rolling window and clears the per-frame map.
    pub fn end_frame(&mut self) {
        for (label, dur) in self.this_frame.drain() {
            *self.rolling_total.entry(label).or_default() += dur;
        }
        self.frames_in_window += 1;
    }

    /// If it's been >= REPORT_PERIOD since the last report, produce a
    /// snapshot and reset the window. Otherwise returns None. The
    /// caller logs the lines somewhere visible (we pipe to tracing
    /// so they land in the backtick log console).
    pub fn maybe_report(&mut self) -> Option<Vec<ReportLine>> {
        let now = Instant::now();
        let due = match self.last_report {
            Some(t) => now.duration_since(t) >= REPORT_PERIOD,
            None => true,
        };
        if !due {
            return None;
        }
        self.last_report = Some(now);
        let frames = self.frames_in_window.max(1);
        let mut timings: Vec<ReportLine> = self
            .rolling_total
            .iter()
            .map(|(k, total)| {
                let calls = self.rolling_calls.get(k).copied().unwrap_or(0);
                ReportLine {
                    label: k,
                    avg_per_frame: *total / frames,
                    total_this_window: *total,
                    calls_per_frame: calls / frames,
                }
            })
            .collect();
        timings.sort_by(|a, b| b.avg_per_frame.cmp(&a.avg_per_frame));
        self.rolling_total.clear();
        self.rolling_calls.clear();
        self.frames_in_window = 0;
        Some(timings)
    }

    /// Drain the rolling window + any un-rolled current frame into
    /// `(label, total, calls)` tuples and reset everything. Unlike
    /// `maybe_report`, this ignores the 1-second report cadence — it
    /// produces a single whole-run aggregate for bench runners.
    ///
    /// Folds `this_frame` into the result too, so callers that never
    /// invoke `end_frame` (e.g. headless bench paths that skip the
    /// render step where `end_frame` normally fires) still get every
    /// scope's accumulated time. Sorted descending by total.
    pub fn take_totals(&mut self) -> Vec<(&'static str, Duration, u32)> {
        // Fold any open-but-not-rolled frame into rolling_total first
        // so headless bench runs (no render → no end_frame call)
        // don't lose their last frames' data.
        for (label, dur) in self.this_frame.drain() {
            *self.rolling_total.entry(label).or_default() += dur;
        }
        let mut out: Vec<(&'static str, Duration, u32)> = self
            .rolling_total
            .iter()
            .map(|(k, total)| {
                let calls = self.rolling_calls.get(k).copied().unwrap_or(0);
                (*k, *total, calls)
            })
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        self.rolling_total.clear();
        self.rolling_calls.clear();
        self.frames_in_window = 0;
        out
    }

    /// Current counter values as a sorted Vec of (label, value).
    pub fn counts_snapshot(&self) -> Vec<(&'static str, u64)> {
        let mut out: Vec<_> = self.counts.iter().map(|(k, v)| (*k, *v)).collect();
        out.sort_by(|a, b| a.0.cmp(b.0));
        out
    }
}

#[derive(Clone, Debug)]
pub struct ReportLine {
    pub label: &'static str,
    pub avg_per_frame: Duration,
    pub total_this_window: Duration,
    pub calls_per_frame: u32,
}

impl ReportLine {
    pub fn format(&self) -> String {
        let avg_us = self.avg_per_frame.as_micros();
        format!(
            "{:<22} avg {:>5}µs  calls/f {:>4}  window {}ms",
            self.label,
            avg_us,
            self.calls_per_frame,
            self.total_this_window.as_millis()
        )
    }
}
