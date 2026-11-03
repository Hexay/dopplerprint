//! The discovery front end: multi-window whitened spectrograms max-combined on one grid.

use super::hum::{HumParams, analyse_hum};
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
    /// When `mains_hz` is unset, run the hum detector and notch its measured (apparent) fundamental.
    pub auto_mains: bool,
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
            auto_mains: false,
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

    /// `mains_hz`, else (with `auto_mains`) the detected hum fundamental.
    pub fn resolved_mains_hz(&self, audio: &Audio) -> Option<f64> {
        self.mains_hz.or_else(|| {
            let p = HumParams { f_max: self.f_max, ..Default::default() };
            self.auto_mains.then(|| analyse_hum(audio, &p).notch_fundamental()).flatten()
        })
    }

    pub fn spectrogram(&self, audio: &Audio) -> LogSpectrogram {
        let w = WhitenParams {
            mains_hz: self.resolved_mains_hz(audio),
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
