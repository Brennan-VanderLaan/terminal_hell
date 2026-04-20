//! Filesystem naming conventions for audio content.
//!
//! All audio lives under `<content-root>/audio/`. Sample files follow
//! the filesystem-as-matrix convention from `audio-spec.md §6`:
//!
//!     samples/<category>/<id>/<event>_<variant>_<take>.<ext>
//!
//! Variants: `baseline`, `p25`, `p50`, `p75`, `p100`, `phantom`.
//!
//! Audition candidates live in a sibling `_audition/` directory at
//! the same level (spec §14.4), produced by the `th_record` tool:
//!
//!     samples/<category>/<id>/_audition/<event>_<variant>_<stamp>.<ext>
//!
//! This module is the single source of truth for those paths so the
//! game engine, audit tool, and recorder can never disagree.

use std::path::{Path, PathBuf};

pub const AUDITION_DIR: &str = "_audition";

/// Variants declared by the variant matrix. `baseline` is mandatory
/// for every event; the rest are optional slots surfaced by the audit
/// tool as needs-recording TODOs.
pub const VARIANTS: &[&str] = &["baseline", "p25", "p50", "p75", "p100", "phantom"];

/// Content categories grouping audio files in the tree. Each maps to
/// a subdirectory name under both `audio/` (for TOML briefs) and
/// `audio/samples/` (for the files themselves).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Motif,
    Unit,
    Weapon,
    Primitive,
    Ui,
    Carcosa,
    Music,
}

impl Category {
    pub fn dir_name(self) -> &'static str {
        match self {
            Category::Motif => "motifs",
            Category::Unit => "units",
            Category::Weapon => "weapons",
            Category::Primitive => "primitives",
            Category::Ui => "ui",
            Category::Carcosa => "carcosa",
            Category::Music => "music",
        }
    }
}

/// Root `audio/` directory inside a content pack.
pub fn audio_root(pack_root: &Path) -> PathBuf {
    pack_root.join("audio")
}

/// Path to a TOML brief for an id inside a category, e.g.
/// `content/core/audio/units/rusher.toml`.
pub fn brief_path(pack_root: &Path, category: Category, id: &str) -> PathBuf {
    audio_root(pack_root)
        .join(category.dir_name())
        .join(format!("{id}.toml"))
}

/// Directory holding sample files for an id, e.g.
/// `content/core/audio/samples/units/rusher/`.
pub fn sample_dir(pack_root: &Path, category: Category, id: &str) -> PathBuf {
    audio_root(pack_root)
        .join("samples")
        .join(category.dir_name())
        .join(id)
}

/// Audition-pool directory for an id.
pub fn audition_dir(pack_root: &Path, category: Category, id: &str) -> PathBuf {
    sample_dir(pack_root, category, id).join(AUDITION_DIR)
}

/// Basename stem for a specific take inside the locked pool, e.g.
/// `spawn_baseline_01`.
pub fn locked_stem(event: &str, variant: &str, take: u32) -> String {
    format!("{event}_{variant}_{take:02}")
}

/// Basename stem for an audition take. Stamp is intended to be a
/// millisecond wall-clock or monotonic counter from the recorder.
pub fn audition_stem(event: &str, variant: &str, stamp: u64) -> String {
    format!("{event}_{variant}_{stamp}")
}

/// True if a filename belongs to the (event, variant) pool. Used by
/// the audit walker and the hot-reload swap to resolve which pool a
/// touched file belongs to. Matches on the `<event>_<variant>_` prefix
/// so future extra take-ids / suffixes are free.
pub fn file_matches(name: &str, event: &str, variant: &str) -> bool {
    let prefix = format!("{event}_{variant}_");
    name.starts_with(&prefix)
}

/// Split `<event>_<variant>_<rest>` back into `(event, variant)` given
/// the known event set. Returns None if nothing matches; takes an
/// iterator of known events to disambiguate (events may contain
/// underscores, e.g. `hit_reaction`).
pub fn split_event_variant<'a, I: IntoIterator<Item = &'a str>>(
    stem: &str,
    known_events: I,
) -> Option<(String, String)> {
    for event in known_events {
        let prefix = format!("{event}_");
        if let Some(rest) = stem.strip_prefix(&prefix) {
            for variant in VARIANTS {
                let var_prefix = format!("{variant}_");
                if rest.starts_with(&var_prefix) || rest == *variant {
                    return Some((event.to_string(), (*variant).to_string()));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_format_as_documented() {
        assert_eq!(locked_stem("spawn", "baseline", 1), "spawn_baseline_01");
        assert_eq!(locked_stem("spawn", "p100", 12), "spawn_p100_12");
        assert_eq!(
            audition_stem("footstep", "baseline", 1714_000_000),
            "footstep_baseline_1714000000"
        );
    }

    #[test]
    fn file_matching_honors_event_and_variant() {
        assert!(file_matches("spawn_baseline_01.wav", "spawn", "baseline"));
        assert!(!file_matches("spawn_baseline_01.wav", "spawn", "p50"));
        assert!(!file_matches("death_baseline_01.wav", "spawn", "baseline"));
    }

    #[test]
    fn split_handles_compound_events() {
        let events = ["spawn", "footstep", "hit_reaction", "death"];
        assert_eq!(
            split_event_variant("hit_reaction_baseline_01", events),
            Some(("hit_reaction".into(), "baseline".into()))
        );
        assert_eq!(
            split_event_variant("spawn_p50_02", events),
            Some(("spawn".into(), "p50".into()))
        );
        assert_eq!(split_event_variant("garbage_ref", events), None);
    }
}
