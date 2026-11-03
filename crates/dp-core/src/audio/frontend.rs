//! The discovery front end: multi-window whitened spectrograms max-combined on one grid, with persistent
//! narrowband lines masked.

use super::lines::{MIN_DURATION_S, persistent_lines};
use super::resample::decimate_for;
use super::spectrogram::{LogSpectrogram, SpecParams, combine_max, log_spectrogram_multi, power_on_log_axis};
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
    /// Zero bins holding a persistent narrowband line (hum, motor slip, local tones); see docs/AUDIO.md.
    pub mask_lines: bool,
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
            mask_lines: true,
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

    fn wins(&self) -> Vec<f64> {
        if self.wins_s.is_empty() { vec![1.0] } else { self.wins_s.clone() }
    }

    pub fn spectrogram(&self, audio: &Audio) -> LogSpectrogram {
        let w = WhitenParams {
            mains_hz: self.mains_hz,
            ..Default::default()
        };
        let a = decimate_for(audio, self.f_max);
        let specs = log_spectrogram_multi(&a, &self.spec_params(), &w, &self.wins());
        let mut out = combine_max(&specs).expect("at least one window");
        if self.mask_lines && a.duration_s() >= MIN_DURATION_S {
            let mask = self.line_mask(&a);
            for row in out.data.chunks_mut(out.n_f) {
                row.iter_mut().zip(&mask).filter(|(_, m)| **m).for_each(|(z, _)| *z = 0.0);
            }
        }
        out
    }

    /// Lines are found on the longest window (finest resolution) and widened to the shortest window's lobe.
    pub fn line_mask(&self, audio: &Audio) -> Vec<bool> {
        let wins = self.wins();
        let (short, long) = wins.iter().fold((f64::MAX, 0.0f64), |(s, l), &w| (s.min(w), l.max(w)));
        let (grid, power) = power_on_log_axis(audio, &SpecParams { win_s: long, ..self.spec_params() });
        persistent_lines(&power, grid.n_t, &grid.freqs(), 1.0 / short)
    }
}
