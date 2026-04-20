//! Serde types mirroring the TOML schemas documented in `audio-spec.md`.
//!
//! These are parsing types only; the engine converts them into its
//! runtime representation. Keeping a narrow parse layer means a bad
//! TOML file fails at load with a useful error instead of panicking
//! deep inside the mixer.

use serde::Deserialize;
use std::collections::HashMap;

// ----- §4 unit TOML ---------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct UnitFile {
    pub unit: UnitDef,
    #[serde(default)]
    pub events: HashMap<String, EventDef>,
    #[serde(default)]
    pub runtime: Option<UnitRuntime>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitDef {
    pub id: String,
    pub description: String,
    pub sonic_palette: String,
    #[serde(default)]
    pub motif: Option<String>,
    #[serde(default)]
    pub bus: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventDef {
    pub description: String,
    #[serde(default)]
    pub filter_hint: Option<String>,
    #[serde(default)]
    pub spatial: bool,
    /// Per-event bus override. If absent, the unit's bus applies.
    #[serde(default)]
    pub bus: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UnitRuntime {
    #[serde(default)]
    pub pitch_jitter_st: Option<[f32; 2]>,
    #[serde(default)]
    pub gain_jitter_db: Option<[f32; 2]>,
    #[serde(default)]
    pub max_concurrent: Option<u32>,
}

// ----- §5 motif file --------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct MotifFile {
    pub description: String,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub antipattern: Option<String>,
    #[serde(default)]
    pub shared_layers: Vec<String>,
}

// ----- §8 buses.toml --------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct BusesFile {
    pub bus: HashMap<String, BusDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BusDef {
    /// `gain_db` is either an f32 or a named curve string such as
    /// `"corruption_curve"`. We keep it as `toml::Value` and let the
    /// engine resolve at bind time so the schema layer can't reject a
    /// valid spec.
    #[serde(default)]
    pub gain_db: Option<toml::Value>,
    #[serde(default)]
    pub chain: Vec<EffectNode>,
    #[serde(default)]
    pub ducks: Option<String>,

    // Aux bus fields (e.g. `bus.reverb_small`) — the chain stays empty
    // and the effect is the bus itself.
    #[serde(rename = "type", default)]
    pub bus_type: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub decay_s: Option<f32>,
    #[serde(default)]
    pub damping: Option<f32>,
    #[serde(default)]
    pub pre_delay_ms: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EffectNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    /// All additional parameter keys — `cutoff_hz`, `resonance`,
    /// `threshold_db`, `ratio`, etc. Resolved per node_type at bind
    /// time.
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}

// ----- §9 reactions.toml ----------------------------------------------------

/// Top-level reactions file is a flat map of reaction-id → definition.
pub type ReactionsFile = HashMap<String, ReactionDef>;

#[derive(Debug, Clone, Deserialize)]
pub struct ReactionDef {
    pub trigger: String,
    #[serde(default)]
    pub mode: Option<String>, // None | "continuous"
    #[serde(default)]
    pub input: Option<ReactionInput>,
    #[serde(default)]
    pub actions: Vec<ReactionAction>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReactionInput {
    pub name: String,
    pub clamp: [f32; 2],
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReactionAction {
    pub bus: String,
    pub effect: String,
    pub param: String,
    #[serde(default)]
    pub to: Option<toml::Value>,
    #[serde(default)]
    pub hold_ms: Option<u32>,
    #[serde(default)]
    pub release_ms: Option<u32>,
    #[serde(default)]
    pub curve: Option<String>,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub combine: Option<String>,
}

// ----- §11 music stems ------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct MusicFile {
    #[serde(flatten)]
    pub stems: HashMap<String, MusicStem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MusicStem {
    pub bpm: f32,
    pub bars: u32,
    #[serde(default)]
    pub swing: f32,
    /// Historical name — stereo music take globs. Either `chunks`
    /// (original Spec wording) or `audio` (the recorder's
    /// capture-unified name) is accepted.
    #[serde(default)]
    pub chunks: Vec<String>,
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default)]
    pub sequence: Option<String>,
    pub bus: String,
    /// CV envelope routes attached to this stem. Each points at a
    /// mono-WAV glob and drives a named gameplay parameter. Takes
    /// pair by basename with the stem's audio takes (name-matched,
    /// random fallback). See `audio::cv`.
    #[serde(default)]
    pub cv: Vec<super::cv::CvRoute>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_unit() {
        let src = r#"
            [unit]
            id = "rusher"
            description = "Light melee chaser."
            sonic_palette = "thin, papery, wet"
            motif = "carcosa.glyph_tear"
            bus = "sfx"

            [events.spawn]
            description = "Fires once when a Rusher enters the arena."
            filter_hint = "body 140-900Hz"
            spatial = true

            [runtime]
            pitch_jitter_st = [-1.5, 1.5]
            max_concurrent = 3
        "#;
        let file: UnitFile = toml::from_str(src).expect("parses");
        assert_eq!(file.unit.id, "rusher");
        assert_eq!(file.events.len(), 1);
        assert!(file.events["spawn"].spatial);
        assert_eq!(file.runtime.unwrap().max_concurrent, Some(3));
    }

    #[test]
    fn parses_motif() {
        let src = r#"
            description = "Something stepping through a glyph-tear."
            emotion = "gut-drop"
            antipattern = "no screams."
            shared_layers = ["pitched-down exhale", "sub-flutter"]
        "#;
        let file: MotifFile = toml::from_str(src).expect("parses");
        assert_eq!(file.shared_layers.len(), 2);
    }

    #[test]
    fn parses_buses_with_chain_and_aux() {
        let src = r#"
            [bus.sfx]
            gain_db = -3
            chain = [
              { id = "lpf", type = "lowpass", cutoff_hz = 20000, resonance = 0.15 },
              { id = "comp", type = "compressor", threshold_db = -12, ratio = 3 },
            ]

            [bus.reverb_small]
            type = "reverb"
            size = 0.25
            decay_s = 0.6
        "#;
        let file: BusesFile = toml::from_str(src).expect("parses");
        assert_eq!(file.bus["sfx"].chain.len(), 2);
        assert_eq!(file.bus["reverb_small"].bus_type.as_deref(), Some("reverb"));
    }

    #[test]
    fn parses_reactions_flat_map() {
        let src = r#"
            [explosion_nearby]
            trigger = "event:explosion"
            actions = [
              { bus = "sfx", effect = "lpf", param = "cutoff_hz", to = 800, hold_ms = 120, release_ms = 600, curve = "exp" }
            ]

            [near_daemon]
            trigger = "proximity:hastur_daemon"
            mode = "continuous"
            input = { name = "distance", clamp = [2.0, 20.0] }
            actions = []
        "#;
        let file: ReactionsFile = toml::from_str(src).expect("parses");
        assert_eq!(file.len(), 2);
        assert_eq!(file["near_daemon"].mode.as_deref(), Some("continuous"));
    }
}
