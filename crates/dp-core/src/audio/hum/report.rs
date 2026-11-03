//! Serialisable hum report. All times are audio-relative seconds; all frequencies are on the audio's
//! (labelled) clock, i.e. what the spectrogram shows.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HumReport {
    /// A stable mains-like line set was found (see docs/AUDIO.md, "Mains hum").
    pub present: bool,
    /// 50 or 60 when present.
    pub nominal_hz: Option<f64>,
    /// Long-run mean apparent fundamental (mean of valid trace frames). 0 if no candidate at all.
    pub fundamental_hz: f64,
    pub harmonics: Vec<HarmonicLine>,
    /// Long-term line-to-floor ratio of the strongest harmonic (dB, Welch resolution).
    pub snr_db: f64,
    /// Fraction of trace frames with a valid estimate.
    pub detect_frac: f64,
    /// Std of the valid trace (Hz at the fundamental).
    pub wander_hz: f64,
    /// Trace frame length actually used (longer than requested when the hum is weak).
    pub frame_s: f64,
    pub ambiguous: bool,
    pub rate: RateCheck,
    pub discontinuities: Vec<Discontinuity>,
    /// Bands event discovery should mask; empty unless present.
    pub bands: Vec<HumBand>,
    pub trace: Vec<HumFrame>,
    /// Human-readable reasons for `present = false` or caveats.
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarmonicLine {
    pub k: u32,
    pub f_hz: f64,
    pub snr_db: f64,
    /// Fraction of trace frames in which this harmonic alone clears `frame_snr_db`.
    pub detect_frac: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumFrame {
    pub t_s: f64,
    /// Harmonic-combined fundamental estimate (Hz).
    pub f_hz: f64,
    /// Best single-harmonic line SNR in this frame (dB).
    pub snr_db: f64,
    /// Weighted spread of the per-harmonic estimates of the fundamental (0 with one harmonic).
    pub spread_hz: f64,
    pub n_harmonics: u32,
    pub valid: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateStatus {
    /// No hum, so no check.
    #[default]
    Unknown,
    Ok,
    /// Off nominal by more than grid + clock tolerance but not by a common-rate ratio.
    ClockOffset,
    /// Off nominal by a ratio of two common sample rates (e.g. 44100/48000).
    Mislabelled,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RateCheck {
    pub status: RateStatus,
    pub labelled_sr: u32,
    /// (apparent fundamental / nominal − 1) in ppm.
    pub deviation_ppm: f64,
    /// labelled_sr · nominal / apparent: the rate the recording was really sampled at.
    pub implied_sr: f64,
    pub implied_sr_sigma: f64,
    /// Nearest common rate when status is Mislabelled and labelled_sr is a container rate.
    pub likely_sr: Option<u32>,
    /// Linear-fit change of the trace over the file (Hz at the fundamental).
    pub drift_hz: f64,
    pub drift_flag: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscontinuityKind {
    /// Hum (or all signal) vanishes for a short interval: inserted silence or a dropped buffer zero-filled.
    Dropout,
    /// Hum phase steps: samples removed or inserted (an edit or buffer drop) without a level change.
    PhaseJump,
    /// Hum frequency steps between adjacent frames: a splice of material recorded at different times.
    FreqJump,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Discontinuity {
    pub t_s: f64,
    pub t_end_s: f64,
    pub kind: DiscontinuityKind,
    /// Phase step in cycles of the fundamental, known only modulo 1/k of the tracked harmonic.
    pub jump_cycles: Option<f64>,
    pub jump_hz: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumBand {
    pub k: u32,
    pub lo_hz: f64,
    pub hi_hz: f64,
}

impl HumReport {
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Fundamental to pass as `FrontEnd::mains_hz` / `WhitenParams::mains_hz` (apparent, so it also
    /// notches the right lines when the sample rate is mislabelled).
    pub fn notch_fundamental(&self) -> Option<f64> {
        self.present.then_some(self.fundamental_hz)
    }
}

/// Mask bands k·f ± (k·max(3·wander, 0.05) + 0.25) Hz up to `f_max`.
pub(super) fn bands(f: f64, wander_hz: f64, f_max: f64) -> Vec<HumBand> {
    let per_k = (3.0 * wander_hz).max(0.05);
    (1u32..)
        .map(|k| (k, k as f64 * f, k as f64 * per_k + 0.25))
        .take_while(|&(_, c, _)| c <= f_max)
        .map(|(k, c, hw)| HumBand { k, lo_hz: c - hw, hi_hz: c + hw })
        .collect()
}
