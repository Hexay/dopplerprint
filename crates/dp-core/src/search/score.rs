//! Locked joint scoring of modelled Doppler curves against a whitened log-frequency spectrogram.
//!
//! For one observer hypothesis each event gets a factor series F_e(t) from the real trajectory.
//! On a log axis bin(h·f0·F) = bin(f0) + bpo·log2(h·F), so the f0 sweep is a 1-D slide of a fixed
//! per-frame offset pattern. All events share one clock offset (audio s → UNIX s); only event 1's
//! measurement error is absorbed by the clock sweep, the others ride on it ("locked" scoring).

use crate::audio::LogSpectrogram;
use crate::geo::Vec3;
use crate::physics::{self, Atmosphere};
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
    let spec = &plane.spec;
    let k_max = (p.clock_sweep_s / spec.dt).round() as i64;
    let prepared: Vec<Prepared> = targets.iter().map(|t| prepare(spec, t, obs, atm, clock_nominal, k_max)).collect();
    let bpo = spec.bins_per_octave as f64;
    let shifts: Vec<(f64, f32)> = p.harmonics.iter().map(|&(h, w)| (bpo * (h as f64).log2(), w)).collect();
    let mut acc = Vec::new();
    let mut best: Option<JointScore> = None;
    for k in -k_max..=k_max {
        let events: Vec<EventScore> =
            prepared.iter().map(|pr| best_f0(spec, pr, k + k_max, &shifts, &mut acc, p)).collect();
        let total = events.iter().map(|e| e.z).sum::<f64>();
        if best.as_ref().is_none_or(|b| total > b.total) {
            best = Some(JointScore { total, clock_offset: clock_nominal + k as f64 * spec.dt, events });
        }
    }
    best
}

struct Prepared {
    frames: std::ops::Range<usize>,
    /// bpo·log2(F) for frame (frames.start + i) at sweep index k is `offsets[i + k]`.
    offsets: Vec<f64>,
    b0_range: (f64, f64),
}

fn prepare(spec: &LogSpectrogram, t: &EventTarget<'_>, obs: Vec3, atm: &Atmosphere, clock: f64, k_max: i64) -> Prepared {
    let f_a = spec.frame_of(t.window.0).ceil().max(0.0) as usize;
    let f_b = (spec.frame_of(t.window.1).floor().max(0.0) as usize + 1).min(spec.n_t);
    let first = f_a as i64 - k_max;
    let n = (f_b.saturating_sub(f_a)) as i64 + 2 * k_max;
    let times: Vec<f64> = (0..n.max(0)).map(|j| spec.t0 + (first + j) as f64 * spec.dt + clock).collect();
    let bpo = spec.bins_per_octave as f64;
    let offsets = physics::doppler_factors(t.track, obs, &times, atm).into_iter().map(|f| bpo * f.log2()).collect();
    let b0_range = (spec.bin_of(t.f0_range.0), spec.bin_of(t.f0_range.1));
    Prepared { frames: f_a..f_b.max(f_a), offsets, b0_range }
}

/// `shifts` = (bpo·log2 h, weight) per harmonic; `acc` is scratch. Frames are the outer loop so each row is
/// read once per sweep, but every f0 step's sum still accumulates frame by frame, harmonic by harmonic.
fn best_f0(
    spec: &LogSpectrogram,
    pr: &Prepared,
    k: i64,
    shifts: &[(f64, f32)],
    acc: &mut Vec<f64>,
    p: &ScoreParams,
) -> EventScore {
    let n_frames = pr.frames.len();
    let covered = (0..n_frames).filter(|&i| pr.offsets[i + k as usize].is_finite()).count();
    let norm = ((n_frames.max(1)) as f64 * p.harmonics.iter().map(|(_, w)| (*w as f64).powi(2)).sum::<f64>()).sqrt();
    let mut best = EventScore { z: f64::NEG_INFINITY, f0: f64::NAN, coverage: covered as f64 / n_frames.max(1) as f64 };
    let n_steps = ((pr.b0_range.1 - pr.b0_range.0) / p.f0_step_bins).ceil().max(0.0) as usize;
    let b0_at = |step: usize| pr.b0_range.0 + step as f64 * p.f0_step_bins;
    acc.clear();
    acc.resize(n_steps + 1, 0.0);
    for (i, frame) in pr.frames.clone().enumerate() {
        let off = pr.offsets[i + k as usize];
        if !off.is_finite() {
            continue;
        }
        let row = spec.row(frame);
        for &(hs, w) in shifts {
            for (step, s) in acc.iter_mut().enumerate() {
                *s += (w * lerp(row, b0_at(step) + off + hs)) as f64;
            }
        }
    }
    for (step, &s) in acc.iter().enumerate() {
        let z = s / norm;
        if z > best.z {
            best.z = z;
            best.f0 = spec.freq_of(b0_at(step));
        }
    }
    best
}

fn lerp(row: &[f32], bin: f64) -> f32 {
    // Truncation is floor for bin ≥ 0 and avoids a libm floor call (no SSE4.1 on the baseline target).
    if bin < 0.0 {
        return 0.0;
    }
    let i = bin as usize;
    if i + 1 >= row.len() {
        return 0.0;
    }
    let fr = (bin - i as f64) as f32;
    row[i] * (1.0 - fr) + row[i + 1] * fr
}
