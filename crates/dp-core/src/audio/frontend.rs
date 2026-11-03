//! The discovery front end: multi-window whitened spectrograms max-combined on one grid.

use super::spectrogram::{LogSpectrogram, SpecParams, combine_max, log_spectrogram_multi};
use super::{Audio, WhitenParams};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FrontEnd {
    /// Window lengths (s): short ones resolve steep CPAs, long ones integrate faint asymptotes.
    pub wins_s: Vec<f64>,
    pub hop_s: f64,
    pub f_min: f64,
    pub f_max: f64,
    pub bins_per_octave: u32,
    pub mains_hz: Option<f64>,
}

impl Default for FrontEnd {
    fn default() -> Self {
        let s = SpecParams::default();
        Self {
            wins_s: vec![0.5, 1.0, 2.0],
            hop_s: s.hop_s,
            f_min: s.f_min,
            f_max: s.f_max,
            bins_per_octave: s.bins_per_octave,
            mains_hz: None,
        }
    }
}

impl FrontEnd {
    pub fn spec_params(&self) -> SpecParams {
        SpecParams {
            win_s: self.wins_s.first().copied().unwrap_or(1.0),
            hop_s: self.hop_s,
            f_min: self.f_min,
            f_max: self.f_max,
            bins_per_octave: self.bins_per_octave,
        }
    }

    pub fn spectrogram(&self, audio: &Audio) -> LogSpectrogram {
        let w = WhitenParams {
            mains_hz: self.mains_hz,
            ..Default::default()
        };
        let wins = if self.wins_s.is_empty() {
            vec![1.0]
        } else {
            self.wins_s.clone()
        };
        let specs = log_spectrogram_multi(audio, &self.spec_params(), &w, &wins);
        combine_max(&specs).expect("at least one window")
    }
}
