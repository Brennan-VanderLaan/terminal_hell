//! Audit coverage → UI-friendly recording queue.
//!
//! Wraps `terminal_hell::audio::audit::walk()` and serves a
//! flat list of slots ordered for studio sessions: motif-grouped
//! baseline blockers first, then remaining baseline slots, then
//! variant slots last. Every entry carries enough display text to
//! populate the GUI's "next up" panel without another walk.

use anyhow::Result;
use std::path::Path;

use terminal_hell::audio::audit::{self, SlotStatus};

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub unit_id: String,
    pub event: String,
    pub variant: String,
    pub motif: Option<String>,
    pub locked_count: usize,
    pub audition_count: usize,
    pub priority: Priority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Baseline slot is empty — blocks shipping the unit.
    Blocker = 0,
    /// Baseline slot has fewer than 3 takes (variation threshold).
    Thin = 1,
    /// Variant slot is empty.
    VariantMissing = 2,
    /// Slot has ≥ 3 takes.
    Healthy = 3,
}

#[derive(Default)]
pub struct Queue {
    pub entries: Vec<QueueEntry>,
    pub coverage_percent: f32,
    pub total_slots: usize,
    pub filled_slots: usize,
}

impl Queue {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn rebuild(pack_root: &Path) -> Result<Self> {
        let cov = audit::walk(pack_root)?;
        let mut entries = Vec::new();
        for unit in &cov.units {
            for slot in &unit.slots {
                entries.push(QueueEntry {
                    unit_id: slot.unit_id.clone(),
                    event: slot.event.clone(),
                    variant: slot.variant.to_string(),
                    motif: unit.motif.clone(),
                    locked_count: slot.locked_count,
                    audition_count: slot.audition_count,
                    priority: priority_for(slot),
                });
            }
        }
        entries.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.motif.cmp(&b.motif))
                .then_with(|| a.unit_id.cmp(&b.unit_id))
                .then_with(|| a.event.cmp(&b.event))
                .then_with(|| a.variant.cmp(&b.variant))
        });
        Ok(Self {
            entries,
            coverage_percent: cov.percent(),
            total_slots: cov.total_slots(),
            filled_slots: cov.filled_slots(),
        })
    }

    pub fn first_recommendation(&self) -> Option<&QueueEntry> {
        self.entries.iter().find(|e| e.priority < Priority::Healthy)
    }
}

fn priority_for(slot: &SlotStatus) -> Priority {
    if slot.is_blocker() {
        Priority::Blocker
    } else if slot.variant == "baseline" && slot.locked_count < 3 {
        Priority::Thin
    } else if slot.locked_count == 0 {
        Priority::VariantMissing
    } else {
        Priority::Healthy
    }
}

/// Flat list of unit ids pulled from the queue, for populating the
/// destination-picker dropdown. Preserves audit-tree order.
pub fn known_unit_ids(queue: &Queue) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in &queue.entries {
        if !out.contains(&entry.unit_id) {
            out.push(entry.unit_id.clone());
        }
    }
    out
}

/// Flat list of events per unit id.
pub fn events_for_unit(queue: &Queue, unit_id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in &queue.entries {
        if entry.unit_id == unit_id && !out.contains(&entry.event) {
            out.push(entry.event.clone());
        }
    }
    out
}
