//! Automatic discovery of Doppler events (ridges) and their constant-velocity fit.

use crate::audio::LogSpectrogram;
use crate::physics::cv::CvParams;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Ridge {
    /// Audio-relative seconds.
    pub t: Vec<f64>,
    pub f_hz: Vec<f64>,
    pub snr: Vec<f32>,
}

/// Fitted CV parameters with generous bounds used to prune the search (never as hard truth).
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
    /// Ridge of the strongest harmonic, in Hz.
    pub ridge: Ridge,
    /// Harmonic numbers relative to the fundamental that were detected.
    pub harmonics: Vec<u32>,
    pub fit: CvFit,
}

pub fn discover(_spec: &LogSpectrogram) -> Vec<Event> {
    todo!("audio agent: Viterbi ridge tracking + harmonic grouping")
}

pub fn fit_cv(_ridge: &Ridge, _c: f64) -> CvFit {
    todo!("audio agent: robust LM fit of physics::cv::frequency with bound estimation")
}
