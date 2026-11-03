//! Automatic discovery of Doppler events (ridges) and their constant-velocity fit. See docs/AUDIO.md.

mod candidate;
mod comb;
mod discover;
mod fit;
mod linalg;
mod merge;
mod split;
mod stats;
mod track;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod testsynth;

use crate::audio::{Audio, FrontEnd, LogSpectrogram};
use crate::physics::cv::CvParams;

pub use fit::{FitParams, fit_cv_with};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Ridge {
    /// Audio-relative seconds.
    pub t: Vec<f64>,
    pub f_hz: Vec<f64>,
    pub snr: Vec<f32>,
}

/// Fitted CV parameters with generous bounds used to prune the search (never as hard truth).
/// `rmse_hz` is over Huber inliers (|r| ≤ 3σ).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CvFit {
    pub best: CvParams,
    pub v_range: (f64, f64),
    pub d_range: (f64, f64),
    pub f0_range: (f64, f64),
    pub t_cpa_range: (f64, f64),
    pub rmse_hz: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub id: usize,
    /// Ridge of the strongest harmonic, in Hz (folded from all detected harmonics, times `ridge_harmonic`).
    pub ridge: Ridge,
    /// Harmonic numbers relative to the fundamental that were detected.
    pub harmonics: Vec<u32>,
    /// Fit of `ridge`, so `fit.best.f0` is the rest frequency of harmonic `ridge_harmonic`.
    pub fit: CvFit,
    #[serde(default)]
    pub ridge_harmonic: u32,
    /// Detection z: Stouffer combination of per-harmonic on-ridge vs off-ridge whitened power.
    #[serde(default)]
    pub snr: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DiscoverParams {
    /// Comb size: harmonics 1..=H of a fundamental.
    pub harmonics: u32,
    pub min_duration_s: f64,
    /// Tracks shorter than this are discarded before stitching.
    pub min_fragment_s: f64,
    /// Minimum fitted fall over the ridge, relative to f0 (≈ 2v/c for a full pass).
    pub min_drop: f64,
    /// Steepest relative fall |ḟ|/f (1/s) the tracker can follow; ≈ v²/(c·d) at CPA.
    pub max_rel_slope: f64,
    /// Tracker: per-frame cost subtracted from the comb z (noise paths must drift negative).
    pub path_bias: f32,
    /// Tracker step costs, in comb-z units: one-bin fall, each extra bin of fall, one-bin rise.
    pub lam_down: f32,
    pub lam_steep: f32,
    pub lam_up: f32,
    pub min_path_score: f32,
    /// Presence threshold (z) for a harmonic on a track.
    pub harmonic_z: f64,
    pub min_snr: f64,
    /// Reject fits whose inlier rmse exceeds this fraction of f0.
    pub max_rmse_rel: f64,
    /// Physically implausible fits (partial ridges extrapolated to the clamp) are rejected.
    pub max_speed_mps: f64,
    pub min_cpa_m: f64,
    /// Try splitting a candidate whose CV fit rmse exceeds this fraction of f0 (line hops between aircraft).
    pub split_rmse_rel: f64,
    /// Frames per statistically independent sample (≈ win/hop).
    pub frames_per_indep: f64,
    pub max_gap_s: f64,
    pub max_tracks: usize,
    pub c: f64,
    pub fit: FitParams,
}

impl Default for DiscoverParams {
    fn default() -> Self {
        Self {
            harmonics: 6,
            min_duration_s: 10.0,
            min_fragment_s: 3.0,
            min_drop: 0.02,
            max_rel_slope: 0.25,
            path_bias: 1.2,
            lam_down: 0.2,
            lam_steep: 0.5,
            lam_up: 1.0,
            min_path_score: 12.0,
            harmonic_z: 3.0,
            min_snr: 6.0,
            max_rmse_rel: 0.03,
            max_speed_mps: 260.0,
            min_cpa_m: 10.0,
            split_rmse_rel: 0.008,
            frames_per_indep: 4.0,
            max_gap_s: 20.0,
            max_tracks: 400,
            c: 343.0,
            fit: FitParams::default(),
        }
    }
}

pub fn discover(spec: &LogSpectrogram) -> Vec<Event> {
    discover_with(spec, &DiscoverParams::default())
}

/// Front end + discovery settings as one flat JSON object (CLI `--params`, Python `**kwargs`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DiscoverConfig {
    #[serde(flatten)]
    pub front: FrontEnd,
    #[serde(flatten)]
    pub params: DiscoverParams,
}

impl DiscoverConfig {
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

pub fn discover_audio(audio: &Audio, cfg: &DiscoverConfig) -> Vec<Event> {
    discover_with(&cfg.front.spectrogram(audio), &cfg.params)
}

pub fn events_to_json(events: &[Event]) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(events)?)
}

pub fn events_from_json(s: &str) -> anyhow::Result<Vec<Event>> {
    Ok(serde_json::from_str(s)?)
}

pub fn fit_to_json(fit: &CvFit) -> anyhow::Result<String> {
    Ok(serde_json::to_string(fit)?)
}

pub use discover::discover_with;

pub fn fit_cv(ridge: &Ridge, c: f64) -> CvFit {
    fit_cv_with(ridge, c, &FitParams::default())
}
