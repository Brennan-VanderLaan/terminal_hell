//! CV envelope loader — stub.
//!
//! Full playback (multichannel WAV demux, smoothing, bar-locked
//! scheduling, name-matched music↔CV pairing) lands with the music
//! pipeline in a later pass. For now this module owns the schema
//! types and a loader that records the declared routes so the audit
//! tool can report CV coverage alongside sample coverage.
//!
//! Shape:
//!
//! ```toml
//! # content/core/audio/music/synthwave_base.toml
//! [base_loop]
//! bpm = 96
//! bars = 8
//! audio = "samples/music/base/base_A_*.wav"
//! bus = "music"
//!
//! [[base_loop.cv]]
//! param     = "enemy.repel_scale_visual"
//! range     = [0.6, 1.8]
//! smooth_ms = 120
//! files     = "cv/enemy.repel_scale_visual/base_A_*.wav"
//! ```

use serde::Deserialize;

/// One CV route attached to a music stem.
#[derive(Debug, Clone, Deserialize)]
pub struct CvRoute {
    /// Name of the registered gameplay parameter this CV writes to
    /// (see `audio::params::PARAMS`).
    pub param: String,
    /// Min/max values the 0..=1 CV input is mapped to. If the stem's
    /// pool starts empty the engine holds the param at the midpoint
    /// of this range so "no CV yet" reads as neutral, not zero.
    pub range: [f32; 2],
    /// One-pole lowpass time constant in milliseconds. Default 100.
    #[serde(default)]
    pub smooth_ms: Option<u32>,
    /// Glob pointing at the mono WAV files holding the CV envelope.
    /// Takes are paired by basename with the music stem's audio
    /// takes (see `audio-spec.md §14`).
    pub files: String,
}

impl CvRoute {
    pub fn effective_smooth_ms(&self) -> u32 {
        self.smooth_ms.unwrap_or(100)
    }
    pub fn midpoint(&self) -> f32 {
        (self.range[0] + self.range[1]) * 0.5
    }
}

/// Apply the "fallback to midpoint" behavior for every route attached
/// to a given music stem. Called at engine init + whenever a stem's
/// CV pool is empty so the param registry starts with a meaningful
/// value rather than the declared `default` (which may disagree with
/// the CV's declared range).
pub fn seed_midpoints(routes: &[CvRoute]) {
    for route in routes {
        crate::audio::params::set(&route.param, route.midpoint());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cv_route() {
        let src = r#"
            param = "enemy.repel_scale_visual"
            range = [0.6, 1.8]
            smooth_ms = 120
            files = "cv/enemy.repel_scale_visual/base_A_*.wav"
        "#;
        let route: CvRoute = toml::from_str(src).expect("parses");
        assert_eq!(route.param, "enemy.repel_scale_visual");
        assert_eq!(route.effective_smooth_ms(), 120);
        assert!((route.midpoint() - 1.2).abs() < 1e-6);
    }

    #[test]
    fn default_smooth_ms_is_100() {
        let src = r#"
            param = "fx.screen_shake_scale"
            range = [0.5, 2.0]
            files = "cv/fx.screen_shake_scale/test_*.wav"
        "#;
        let route: CvRoute = toml::from_str(src).expect("parses");
        assert_eq!(route.effective_smooth_ms(), 100);
    }
}
