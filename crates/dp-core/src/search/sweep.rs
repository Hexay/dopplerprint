//! CPU f0 sweep. Arithmetic: see fixed.rs; the CUDA kernel (gpu/sweep.cu) computes the same integers.

use rayon::prelude::*;

use super::fixed;
use super::job::{Job, Prepared, StepBest, assemble, harmonic_shifts};
use super::score::{JointScore, ScoreParams};
use crate::audio::LogSpectrogram;

pub fn score_jobs(spec: &LogSpectrogram, jobs: &[Job], p: &ScoreParams) -> Vec<Option<JointScore>> {
    let shifts = harmonic_shifts(spec, p);
    jobs.par_iter()
        .map_init(Vec::new, |acc, job| {
            assemble(spec, job, p, |e, k| best_f0(spec, &job.events[e], k, &shifts, acc, p))
        })
        .collect()
}

/// Frames outer so each row is read once per sweep; steps inner over the contiguous in-row run.
fn best_f0(
    spec: &LogSpectrogram,
    pr: &Prepared,
    k: usize,
    shifts: &[(i64, f32)],
    acc: &mut Vec<i64>,
    p: &ScoreParams,
) -> StepBest {
    acc.clear();
    acc.resize(pr.n_steps, 0);
    let (step, top) = (fixed::pos(p.f0_step_bins), fixed::top(spec.n_f));
    let mut covered = 0;
    for (i, frame) in pr.frames.clone().enumerate() {
        let off = pr.offsets[i + k];
        if off == fixed::NONE {
            continue;
        }
        covered += 1;
        let row = spec.row(frame);
        for &(hs, w) in shifts {
            let base = pr.b0_start_fx + off + hs;
            let run = fixed::in_row(base, step, top, pr.n_steps);
            let mut pos = base + run.start as i64 * step;
            for a in &mut acc[run] {
                *a += fixed::term(w * fixed::lerp(row, pos));
                pos += step;
            }
        }
    }
    let mut best = StepBest { z: f64::NEG_INFINITY, step: 0, covered };
    for (s, &a) in acc.iter().enumerate() {
        let z = fixed::total(a) / pr.norm;
        if z > best.z {
            best = StepBest { z, step: s, covered };
        }
    }
    best
}
