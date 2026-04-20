//! Audit — walks the audio content tree and reports coverage. Drives
//! both `terminal_hell audio audit` and (eventually) the recorder's
//! priority queue.
//!
//! For each unit TOML and each event declared inside it, the audit
//! walks `samples/units/<id>/` to count matching files per variant
//! (`baseline`, `p25`, …, `phantom`) and checks the `_audition/`
//! subfolder for candidate takes. Motifs are reported separately —
//! they're shared layers recorded once and inherited across sibling
//! units.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::audio::paths::{self, AUDITION_DIR, Category, VARIANTS};
use crate::audio::schema::{MotifFile, UnitFile};

/// One cell in the unit × event × variant matrix.
#[derive(Debug, Clone)]
pub struct SlotStatus {
    pub unit_id: String,
    pub event: String,
    pub variant: &'static str,
    pub locked_count: usize,
    pub audition_count: usize,
}

impl SlotStatus {
    pub fn is_baseline(&self) -> bool {
        self.variant == "baseline"
    }
    pub fn is_filled(&self) -> bool {
        self.locked_count > 0
    }
    pub fn is_blocker(&self) -> bool {
        self.is_baseline() && !self.is_filled()
    }
}

#[derive(Debug, Clone)]
pub struct MotifStatus {
    pub id: String,
    pub description: String,
    pub shared_layer_count: usize,
    pub recorded_layer_count: usize,
}

#[derive(Debug, Clone)]
pub struct UnitSummary {
    pub id: String,
    pub motif: Option<String>,
    pub slots: Vec<SlotStatus>,
    /// Which content category this entity belongs to. Used by the
    /// renderer to group units / weapons / primitives / UI / carcosa
    /// into separate sections of the report.
    pub category: Category,
}

impl UnitSummary {
    pub fn filled(&self) -> usize {
        self.slots.iter().filter(|s| s.is_filled()).count()
    }
    pub fn total(&self) -> usize {
        self.slots.len()
    }
    pub fn blockers(&self) -> usize {
        self.slots.iter().filter(|s| s.is_blocker()).count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Coverage {
    pub units: Vec<UnitSummary>,
    pub motifs: Vec<MotifStatus>,
}

impl Coverage {
    pub fn total_slots(&self) -> usize {
        self.units.iter().map(|u| u.total()).sum()
    }
    pub fn filled_slots(&self) -> usize {
        self.units.iter().map(|u| u.filled()).sum()
    }
    pub fn blocker_count(&self) -> usize {
        self.units.iter().map(|u| u.blockers()).sum()
    }
    pub fn percent(&self) -> f32 {
        let t = self.total_slots();
        if t == 0 {
            0.0
        } else {
            100.0 * self.filled_slots() as f32 / t as f32
        }
    }
}

/// Categories walked as entities (every emitter with `[unit]` + `[events.*]`
/// tables). Motifs are walked separately because they carry no slot matrix.
/// Music is deferred until the music pipeline lands — its schema is
/// stem-based rather than event-based.
const ENTITY_CATEGORIES: &[Category] = &[
    Category::Unit,
    Category::Weapon,
    Category::Primitive,
    Category::Ui,
    Category::Carcosa,
];

/// Walk `<pack_root>/audio/` and produce a Coverage report spanning
/// every entity category (units / weapons / primitives / UI / carcosa)
/// plus the motif catalogue.
pub fn walk(pack_root: &Path) -> Result<Coverage> {
    let audio_root = paths::audio_root(pack_root);
    if !audio_root.is_dir() {
        return Ok(Coverage::default());
    }

    let mut units = Vec::new();
    for category in ENTITY_CATEGORIES {
        let mut cat = walk_category(pack_root, &audio_root, *category)?;
        units.append(&mut cat);
    }
    let motifs = walk_motifs(&audio_root)?;

    Ok(Coverage { units, motifs })
}

/// Walk one entity category's TOML dir and build per-file summaries.
fn walk_category(
    pack_root: &Path,
    audio_root: &Path,
    category: Category,
) -> Result<Vec<UnitSummary>> {
    let dir = audio_root.join(category.dir_name());
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let parsed: UnitFile = match toml::from_str(&text) {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    category = category.dir_name(),
                    ?err,
                    "audio audit: entity TOML parse failed"
                );
                continue;
            }
        };
        let id = parsed.unit.id.clone();
        let motif = parsed.unit.motif.clone();

        let sample_dir = paths::sample_dir(pack_root, category, &id);
        let aud_dir = sample_dir.join(AUDITION_DIR);

        let mut slots = Vec::with_capacity(parsed.events.len() * VARIANTS.len());
        let mut event_names: Vec<&String> = parsed.events.keys().collect();
        event_names.sort();
        for event in event_names {
            for &variant in VARIANTS {
                let locked = count_matching(&sample_dir, event, variant);
                let audition = count_matching(&aud_dir, event, variant);
                slots.push(SlotStatus {
                    unit_id: id.clone(),
                    event: event.clone(),
                    variant,
                    locked_count: locked,
                    audition_count: audition,
                });
            }
        }

        out.push(UnitSummary {
            id: stem.to_string(),
            motif,
            slots,
            category,
        });
    }
    Ok(out)
}

fn walk_motifs(audio_root: &Path) -> Result<Vec<MotifStatus>> {
    let dir = audio_root.join(Category::Motif.dir_name());
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());
    let samples_motif_dir = audio_root.join("samples").join(Category::Motif.dir_name());

    let mut out = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let text = fs::read_to_string(&path)?;
        let parsed: MotifFile = match toml::from_str(&text) {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(path = %path.display(), ?err, "audio audit: motif TOML parse failed");
                continue;
            }
        };
        let shared_layer_count = parsed.shared_layers.len();
        let recorded_layer_count = count_motif_recordings(&samples_motif_dir, stem);
        out.push(MotifStatus {
            id: stem.to_string(),
            description: first_line(&parsed.description),
            shared_layer_count,
            recorded_layer_count,
        });
    }
    Ok(out)
}

fn count_matching(dir: &Path, event: &str, variant: &str) -> usize {
    if !dir.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
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
        if paths::file_matches(stem, event, variant) {
            count += 1;
        }
    }
    count
}

fn count_motif_recordings(motif_samples_dir: &Path, motif_id: &str) -> usize {
    let dir = motif_samples_dir.join(motif_id);
    if !dir.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(&dir) else { return 0 };
    entries
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            s.ends_with(".wav") || s.ends_with(".ogg")
        })
        .count()
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// Pretty-printed report for humans. Mirrors the example in
/// `audio-spec.md §7`, grouped by category so you can see at a glance
/// how deep the hole is for each emitter family.
pub fn format_pretty(cov: &Coverage) -> String {
    let mut out = String::new();

    // Group + emit by category in a fixed order so the report layout
    // stays stable across runs.
    let mut last_cat: Option<Category> = None;
    for unit in &cov.units {
        if last_cat != Some(unit.category) {
            last_cat = Some(unit.category);
            out.push_str(&format!(
                "═══ {} ═══════════════════════════════════════\n",
                unit.category.dir_name()
            ));
        }
        out.push_str(&format!(
            "{:<18}[{}/{} slots filled]",
            unit.id,
            unit.filled(),
            unit.total()
        ));
        if let Some(m) = &unit.motif {
            out.push_str(&format!("  motif: {m}"));
        }
        out.push('\n');

        // Group slots by event.
        let mut last_event: Option<&str> = None;
        for slot in &unit.slots {
            let event_col = if last_event != Some(&slot.event) {
                last_event = Some(&slot.event);
                slot.event.clone()
            } else {
                String::new()
            };
            let status = if slot.locked_count > 0 {
                format!("locked {} · audition {}", slot.locked_count, slot.audition_count)
            } else if slot.audition_count > 0 {
                format!("— missing (audition: {} candidate)", slot.audition_count)
            } else {
                "— missing".to_string()
            };
            let flag = if slot.is_blocker() { "  [BLOCKS m9]" } else { "" };
            out.push_str(&format!(
                "  {:<18}{:<10}{}{}\n",
                event_col, slot.variant, status, flag
            ));
        }
        out.push('\n');
    }

    if !cov.motifs.is_empty() {
        out.push_str("motifs:\n");
        for m in &cov.motifs {
            out.push_str(&format!(
                "  {:<24}  {} / {} shared layers recorded — {}\n",
                m.id, m.recorded_layer_count, m.shared_layer_count, m.description
            ));
        }
        out.push('\n');
    }

    // Session suggestion — motif-grouped blockers first.
    let blockers: Vec<&SlotStatus> = cov
        .units
        .iter()
        .flat_map(|u| u.slots.iter())
        .filter(|s| s.is_blocker())
        .collect();
    if !blockers.is_empty() {
        out.push_str("suggested next session (m9 blockers, motif-grouped):\n");
        // Group blockers by unit then motif.
        let mut seen_motifs: Vec<String> = Vec::new();
        for unit in &cov.units {
            if let Some(m) = &unit.motif {
                if !seen_motifs.contains(m) && unit.blockers() > 0 {
                    seen_motifs.push(m.clone());
                    out.push_str(&format!("  - motif.{m}  (record shared layers first)\n"));
                }
            }
        }
        for b in blockers {
            out.push_str(&format!("  - {}.{}.{}\n", b.unit_id, b.event, b.variant));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "coverage: {:.1}% ({} of {} slots filled) · {} baseline blockers\n",
        cov.percent(),
        cov.filled_slots(),
        cov.total_slots(),
        cov.blocker_count(),
    ));

    out
}

/// CLI entry for `terminal_hell audio audit`. Prints the pretty
/// report to stdout and optionally writes the same report to a
/// markdown file for commit.
pub fn run_cli(write: Option<PathBuf>) -> Result<()> {
    let root = crate::audio::default_content_root();
    let cov = walk(&root)?;
    let rendered = format_pretty(&cov);
    println!("{rendered}");
    if let Some(path) = write {
        let md = render_markdown(&cov, &root);
        let mut f = fs::File::create(&path)?;
        f.write_all(md.as_bytes())?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}

fn render_markdown(cov: &Coverage, root: &Path) -> String {
    let mut out = String::new();
    out.push_str("# Audio coverage\n\n");
    out.push_str(&format!(
        "Pack: `{}`\n\nCoverage: **{:.1}%** ({} / {} slots filled) · {} baseline blockers\n\n",
        root.display(),
        cov.percent(),
        cov.filled_slots(),
        cov.total_slots(),
        cov.blocker_count(),
    ));
    out.push_str("## Entities\n\n");
    let mut last_cat: Option<Category> = None;
    for unit in &cov.units {
        if last_cat != Some(unit.category) {
            last_cat = Some(unit.category);
            out.push_str(&format!("### `{}`\n\n", unit.category.dir_name()));
        }
        out.push_str(&format!("#### `{}`\n\n", unit.id));
        if let Some(m) = &unit.motif {
            out.push_str(&format!("Motif: `{m}`\n\n"));
        }
        out.push_str("| event | variant | locked | audition | status |\n");
        out.push_str("|---|---|---|---|---|\n");
        for slot in &unit.slots {
            let status = if slot.is_blocker() {
                "⛔ BLOCKS m9"
            } else if slot.locked_count > 0 {
                "✓"
            } else if slot.audition_count > 0 {
                "auditioning"
            } else {
                "—"
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                slot.event, slot.variant, slot.locked_count, slot.audition_count, status
            ));
        }
        out.push('\n');
    }
    if !cov.motifs.is_empty() {
        out.push_str("## Motifs\n\n");
        out.push_str("| motif | shared layers | recorded | description |\n");
        out.push_str("|---|---|---|---|\n");
        for m in &cov.motifs {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                m.id, m.shared_layer_count, m.recorded_layer_count, m.description
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[allow(unused_imports)]
    use std::io::Write;

    #[test]
    fn walks_empty_tree_cleanly() {
        let tmp = std::env::temp_dir().join("th_audit_empty_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let cov = walk(&tmp).unwrap();
        assert_eq!(cov.total_slots(), 0);
        assert_eq!(cov.percent(), 0.0);
    }

    #[test]
    fn counts_locked_and_audition_files() {
        let tmp = std::env::temp_dir().join("th_audit_matrix_test");
        let _ = fs::remove_dir_all(&tmp);

        // Minimal unit TOML.
        let units = tmp.join("audio").join("units");
        fs::create_dir_all(&units).unwrap();
        let mut f = fs::File::create(units.join("rusher.toml")).unwrap();
        f.write_all(
            br#"
            [unit]
            id = "rusher"
            description = "test"
            sonic_palette = "test"

            [events.spawn]
            description = "test"
            spatial = true
            "#,
        )
        .unwrap();

        // Two locked baseline takes + one audition candidate.
        let samples = tmp.join("audio").join("samples").join("units").join("rusher");
        fs::create_dir_all(samples.join("_audition")).unwrap();
        fs::File::create(samples.join("spawn_baseline_01.wav")).unwrap();
        fs::File::create(samples.join("spawn_baseline_02.wav")).unwrap();
        fs::File::create(samples.join("_audition").join("spawn_baseline_xx.wav")).unwrap();

        let cov = walk(&tmp).unwrap();
        assert_eq!(cov.units.len(), 1);
        assert_eq!(cov.units[0].category, Category::Unit);
        let baseline = cov.units[0]
            .slots
            .iter()
            .find(|s| s.event == "spawn" && s.variant == "baseline")
            .unwrap();
        assert_eq!(baseline.locked_count, 2);
        assert_eq!(baseline.audition_count, 1);
        assert!(!baseline.is_blocker());
    }
}
