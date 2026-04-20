//! Brief viewer — loads a unit TOML + its motif TOML off disk and
//! collapses the bits the UI needs into a single struct. The recorder
//! reads these whenever the focused slot changes so the author always
//! has the design intent next to the waveform they're recording into.

use std::fs;
use std::path::Path;

use terminal_hell::audio::paths::Category;
use terminal_hell::audio::schema::{MotifFile, UnitFile};

#[derive(Debug, Clone, Default)]
pub struct BriefView {
    pub unit_id: String,
    pub event: String,
    pub variant: String,
    pub unit_description: String,
    pub sonic_palette: String,
    pub motif_id: Option<String>,
    pub event_description: String,
    pub filter_hint: Option<String>,
    pub motif_description: Option<String>,
    pub motif_emotion: Option<String>,
    pub motif_antipattern: Option<String>,
    pub motif_shared_layers: Vec<String>,
}

/// Load + flatten the brief for a (category, unit, event, variant).
/// Returns None if the unit TOML is missing or unparseable. Motif
/// resolution is best-effort — a missing motif leaves those fields
/// empty but does not fail the load.
pub fn load(
    pack_root: &Path,
    category: Category,
    unit_id: &str,
    event: &str,
    variant: &str,
) -> Option<BriefView> {
    let unit_path = pack_root
        .join("audio")
        .join(category.dir_name())
        .join(format!("{unit_id}.toml"));
    let text = fs::read_to_string(&unit_path).ok()?;
    let parsed: UnitFile = toml::from_str(&text).ok()?;

    let event_def = parsed.events.get(event);
    let event_description = event_def
        .map(|e| e.description.clone())
        .unwrap_or_default();
    let filter_hint = event_def.and_then(|e| e.filter_hint.clone());
    let motif_id = parsed.unit.motif.clone();

    let (m_desc, m_emotion, m_anti, m_layers) = motif_id
        .as_ref()
        .and_then(|m| load_motif(pack_root, m))
        .unwrap_or_default();

    Some(BriefView {
        unit_id: unit_id.into(),
        event: event.into(),
        variant: variant.into(),
        unit_description: parsed.unit.description,
        sonic_palette: parsed.unit.sonic_palette,
        motif_id,
        event_description,
        filter_hint,
        motif_description: m_desc,
        motif_emotion: m_emotion,
        motif_antipattern: m_anti,
        motif_shared_layers: m_layers,
    })
}

/// Motif file stems use underscores; references in unit TOMLs use
/// dots (e.g. `carcosa.glyph_tear` → `carcosa_glyph_tear.toml`). Hide
/// the convention here so callers never have to think about it.
fn load_motif(
    pack_root: &Path,
    motif_ref: &str,
) -> Option<(
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<String>,
)> {
    let file_stem = motif_ref.replace('.', "_");
    let path = pack_root
        .join("audio")
        .join("motifs")
        .join(format!("{file_stem}.toml"));
    let text = fs::read_to_string(&path).ok()?;
    let parsed: MotifFile = toml::from_str(&text).ok()?;
    Some((
        Some(parsed.description),
        parsed.emotion,
        parsed.antipattern,
        parsed.shared_layers,
    ))
}
