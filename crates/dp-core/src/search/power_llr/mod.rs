//! Power-domain LLR scoring (stage 2 of docs/research/matching_mathematical_methods.md): a CPU-only scorer
//! over the same `Job`s as the z-sum sweep, selected by `[search.score] mode = "power_llr"`. It bypasses the
//! fixed-point sweep and CUDA entirely. Scores are nats, not z. Plane: `plane.rs`; per-event sum: `sum.rs`;
//! measurements: docs/research/power_llr_2026-11-03.md.

mod plane;
mod sum;

pub use plane::{overlap_factor, plane};

use rayon::prelude::*;

use super::fixed;
use super::job::{Job, Prepared};
use super::score::{EventScore, JointScore, ScoreParams};
use crate::audio::LogSpectrogram;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LlrParams {
    /// ŝ_h is profiled per segment of this length (s); 0 = the fixed prior `snr` instead.
    pub segment_s: f64,
    /// Fixed-prior per-pixel SNR ŝ_h (linear) per harmonic.
    pub snr: Vec<f64>,
    /// Cap on P/N̂ (limits impulsive bins without clipping the input).
    pub cap: f32,
    /// Frame-overlap variance factor (1 s Hann, 0.25 s hop; measured, see the doc).
    pub n_eff: f64,
    /// Bisection levels refining the best clock step below one frame (each halves it; 0 = off).
    pub clock_refine_levels: u32,
}

impl Default for LlrParams {
    fn default() -> Self {
        Self { segment_s: 5.0, snr: vec![1.0, 1.0, 0.6, 0.4, 0.3, 0.2], cap: 30.0, n_eff: 2.2, clock_refine_levels: 2 }
    }
}

/// One `JointScore` per job, best over the frame clock sweep then refined below one frame.
pub fn score_jobs(spec: &LogSpectrogram, jobs: &[Job], p: &ScoreParams) -> Vec<Option<JointScore>> {
    let bpo = spec.bins_per_octave as f64;
    let shifts: Vec<f64> = p.harmonics.iter().map(|&(h, _)| bpo * (h as f64).log2()).collect();
    jobs.par_iter()
        .map_init(Scratch::default, |sc, job| score_job(spec, job, &shifts, p, sc))
        .collect()
}

#[derive(Default)]
struct Scratch {
    sums: sum::Sums,
    offs: Vec<f64>,
}

fn score_job(spec: &LogSpectrogram, job: &Job, shifts: &[f64], p: &ScoreParams, sc: &mut Scratch) -> Option<JointScore> {
    let mut best: Option<(f64, JointScore)> = None;
    for k in 0..job.n_k() {
        let js = score_at(spec, job, k as f64, shifts, p, sc);
        if best.as_ref().is_none_or(|b| js.total > b.1.total) {
            best = Some((k as f64, js));
        }
    }
    let (mut centre, mut best) = best?;
    let mut half = 1.0;
    let last = (job.n_k() - 1) as f64;
    for _ in 0..p.llr.clock_refine_levels {
        half *= 0.5;
        let mid = centre;
        for u in [mid - half, mid + half].into_iter().filter(|u| (0.0..=last).contains(u)) {
            let js = score_at(spec, job, u, shifts, p, sc);
            if js.total > best.total {
                (centre, best) = (u, js);
            }
        }
    }
    Some(best)
}

/// All events at fractional clock index `u` (offsets interpolated linearly in time between frames).
fn score_at(spec: &LogSpectrogram, job: &Job, u: f64, shifts: &[f64], p: &ScoreParams, sc: &mut Scratch) -> JointScore {
    let events: Vec<EventScore> = job
        .events
        .iter()
        .map(|pr| {
            offsets_at(pr, u, &mut sc.offs);
            let n = pr.frames.len();
            let covered = sc.offs.iter().filter(|o| o.is_finite()).count();
            let (z, step) = sum::event_llr(
                spec,
                pr.frames.clone(),
                &sc.offs,
                pr.b0_start,
                pr.n_steps,
                p.f0_step_bins,
                shifts,
                &p.llr,
                &mut sc.sums,
            );
            EventScore { z, f0: spec.freq_of(pr.b0(step, p)), coverage: covered as f64 / n.max(1) as f64 }
        })
        .collect();
    let total = events.iter().map(|e| e.z).sum();
    JointScore { total, clock_offset: job.clock_nominal + (u - job.k_max as f64) * spec.dt, events }
}

/// bpo·log2(F) per event frame at fractional clock index `u` (NaN = uncovered).
pub fn offsets_at(pr: &Prepared, u: f64, out: &mut Vec<f64>) {
    let (k, fr) = (u.floor() as usize, u.fract());
    let at = |j: usize| pr.offsets.get(j).filter(|&&o| o != fixed::NONE).map_or(f64::NAN, |&o| unfix(o));
    out.clear();
    out.extend((0..pr.frames.len()).map(|i| if fr == 0.0 { at(i + k) } else { at(i + k) * (1.0 - fr) + at(i + k + 1) * fr }));
}

fn unfix(o: i64) -> f64 {
    o as f64 / (1u64 << fixed::POS_SHIFT) as f64
}
