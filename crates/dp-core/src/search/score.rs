//! Locked joint scoring of modelled Doppler curves against a whitened log-frequency spectrogram.
//!
//! For one observer hypothesis each event gets a factor series F_e(t) from the real trajectory.
//! On a log axis bin(h·f0·F) = bin(f0) + bpo·log2(h·F), so the f0 sweep is a 1-D slide of a fixed
//! per-frame offset pattern. All events share one clock offset (audio s → UNIX s); only event 1's
//! measurement error is absorbed by the clock sweep, the others ride on it ("locked" scoring).

use super::job::prepare_job;
use super::sweep;
use crate::audio::LogSpectrogram;
use crate::geo::Vec3;
use crate::physics::Atmosphere;
use crate::track::Track;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ScoreParams {
    /// Harmonic numbers scored incoherently, with weights.
    pub harmonics: Vec<(u32, f32)>,
    /// ± bins of frequency tolerance (max-filter half width).
    pub tol_bins: usize,
    /// z subtracted per bin of distance inside the tolerance, so exact hits beat near misses.
    pub tol_penalty: f32,
    /// Clock sweep half-width (s) and step (frames) around the nominal offset.
    pub clock_sweep_s: f64,
    /// Per-frame z clipping before summing (limits loud transients).
    pub z_clip: f32,
    /// f0 sweep step in log bins; sub-bin with linear interpolation, else quantisation biases position.
    pub f0_step_bins: f64,
}

impl Default for ScoreParams {
    fn default() -> Self {
        Self {
            harmonics: vec![(1, 1.0), (2, 1.0), (3, 0.8), (4, 0.6), (5, 0.5), (6, 0.4)],
            tol_bins: 1,
            tol_penalty: 1.0,
            clock_sweep_s: 5.0,
            z_clip: 8.0,
            f0_step_bins: 0.25,
        }
    }
}

/// One audio event to score against one flight.
pub struct EventTarget<'a> {
    pub track: &'a Track,
    /// Audio-relative window (s) the curve must explain.
    pub window: (f64, f64),
    /// Fundamental search range (Hz).
    pub f0_range: (f64, f64),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventScore {
    pub z: f64,
    pub f0: f64,
    /// Fraction of window frames the track covered.
    pub coverage: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JointScore {
    pub total: f64,
    /// UNIX = audio + clock_offset.
    pub clock_offset: f64,
    pub events: Vec<EventScore>,
}

/// Spectrogram with each row max-filtered over ±tol bins and clipped, built once per recording.
pub struct ScoringPlane {
    pub spec: LogSpectrogram,
}

impl ScoringPlane {
    pub fn new(spec: &LogSpectrogram, p: &ScoreParams) -> Self {
        let mut out = spec.clone();
        let (n_f, tol) = (spec.n_f, p.tol_bins);
        for (src, dst) in spec.data.chunks_exact(n_f).zip(out.data.chunks_exact_mut(n_f)) {
            for (b, d) in dst.iter_mut().enumerate() {
                let (lo, hi) = (b.saturating_sub(tol), (b + tol + 1).min(n_f));
                let best = (lo..hi).map(|j| src[j] - p.tol_penalty * j.abs_diff(b) as f32).fold(f32::MIN, f32::max);
                *d = best.clamp(-p.z_clip, p.z_clip);
            }
        }
        Self { spec: out }
    }
}

/// Best locked joint score over the clock sweep for one observer position.
pub fn score_joint(
    plane: &ScoringPlane,
    targets: &[EventTarget<'_>],
    obs: Vec3,
    atm: &Atmosphere,
    clock_nominal: f64,
    p: &ScoreParams,
) -> Option<JointScore> {
    let job = prepare_job(&plane.spec, targets, obs, atm, clock_nominal, p);
    sweep::score_jobs(&plane.spec, std::slice::from_ref(&job), p).pop().flatten()
}
