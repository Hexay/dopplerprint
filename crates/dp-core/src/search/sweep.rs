//! CPU f0 sweep. Arithmetic: see fixed.rs; the CUDA kernel (gpu/sweep.cu) computes the same integers.

use rayon::prelude::*;

use super::{drift, fixed};
use super::job::{Job, Prepared, StepBest, assemble, harmonic_shifts};
use super::score::{JointScore, ScoreParams};
use crate::audio::LogSpectrogram;

#[cfg(test)]
#[path = "sweep_tests.rs"]
mod tests;

pub fn score_jobs(spec: &LogSpectrogram, jobs: &[Job], p: &ScoreParams) -> Vec<Option<JointScore>> {
    let shifts = harmonic_shifts(spec, p);
    let pens = drift::penalties(&p.drift_grid, p.drift_sigma_per_s);
    jobs.par_iter()
        .map_init(Vec::new, |acc, job| {
            assemble(spec, job, p, |e, k| best_f0(spec, &job.events[e], k, &shifts, &pens, acc, p))
        })
        .collect()
}

/// Best over the plain offsets and each `drift` variant less its prior `pens`; ties keep β = 0.
#[allow(clippy::too_many_arguments)]
fn best_f0(
    spec: &LogSpectrogram,
    pr: &Prepared,
    k: usize,
    shifts: &[(i64, f32)],
    pens: &[f64],
    acc: &mut Vec<i64>,
    p: &ScoreParams,
) -> StepBest {
    let mut best = sweep_one(spec, pr, None, k, shifts, acc, p);
    for (d, pen) in pr.drift.iter().zip(pens) {
        let mut b = sweep_one(spec, pr, Some(d), k, shifts, acc, p);
        b.z -= pen;
        if b.z > best.z {
            best = b;
        }
    }
    best
}

/// Accumulator layout: step s lives at `(s mod P)·len + s / P`. When the step divides one bin (P = 2^shift
/// steps per bin, 0.25 → 4) steps of one residue share a lerp fraction and read the row contiguously.
#[derive(Clone, Copy)]
struct Layout {
    shift: u32,
    len: usize,
}

impl Layout {
    fn new(step: i64, n_steps: usize) -> Self {
        let bin = 1i64 << fixed::POS_SHIFT;
        // Divisors of the 2^32 bin are powers of two; anything else keeps plain step order (P = 1).
        let shift = if step > 0 && bin % step == 0 && bin / step <= 64 { (bin / step).trailing_zeros() } else { 0 };
        Self { shift, len: n_steps.div_ceil(1 << shift) }
    }

    fn period(self) -> usize {
        1 << self.shift
    }

    fn index(self, s: usize) -> usize {
        (s & (self.period() - 1)) * self.len + (s >> self.shift)
    }
}

/// One sweep: everything `accumulate` reads.
struct Sweep<'a> {
    spec: &'a LogSpectrogram,
    pr: &'a Prepared,
    delta: Option<&'a [i64]>,
    k: usize,
    shifts: &'a [(i64, f32)],
    step: i64,
    lay: Layout,
}

/// Frames outer so each row is read once per sweep; steps inner over the contiguous in-row run.
#[allow(clippy::too_many_arguments)]
fn sweep_one(
    spec: &LogSpectrogram,
    pr: &Prepared,
    delta: Option<&[i64]>,
    k: usize,
    shifts: &[(i64, f32)],
    acc: &mut Vec<i64>,
    p: &ScoreParams,
) -> StepBest {
    let step = fixed::pos(p.f0_step_bins);
    let lay = Layout::new(step, pr.n_steps);
    acc.clear();
    acc.resize(lay.period() * lay.len, 0);
    let sw = Sweep { spec, pr, delta, k, shifts, step, lay };
    let covered = accumulate_dispatch(&sw, acc);
    let mut best = StepBest { z: f64::NEG_INFINITY, step: 0, covered };
    for s in 0..pr.n_steps {
        let z = fixed::total(acc[lay.index(s)]) / pr.norm;
        if z > best.z {
            best = StepBest { z, step: s, covered };
        }
    }
    best
}

fn accumulate_dispatch(sw: &Sweep, acc: &mut [i64]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: AVX2 is present. Same IEEE f32 operations as the baseline path (no FMA), so same integers.
        return unsafe { accumulate_avx2(sw, acc) };
    }
    accumulate(sw, acc)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn accumulate_avx2(sw: &Sweep, acc: &mut [i64]) -> u32 {
    accumulate(sw, acc)
}

/// Adds every in-row term into `acc` (laid out by `sw.lay`); returns the frames covered.
#[inline(always)]
fn accumulate(sw: &Sweep, acc: &mut [i64]) -> u32 {
    let (pr, step, lay) = (sw.pr, sw.step, sw.lay);
    let top = fixed::top(sw.spec.n_f);
    let mut covered = 0;
    for (i, frame) in pr.frames.clone().enumerate() {
        let off = pr.offsets[i + sw.k];
        if off == fixed::NONE {
            continue;
        }
        let off = off + sw.delta.map_or(0, |d| d[i + sw.k]);
        covered += 1;
        let row = sw.spec.row(frame);
        for &(hs, w) in sw.shifts {
            let base = pr.b0_start_fx + off + hs;
            if lay.shift == 0 {
                let run = fixed::in_row(base, step, top, pr.n_steps);
                let mut pos = base + run.start as i64 * step;
                for a in &mut acc[run] {
                    *a += fixed::term(w * fixed::lerp(row, pos));
                    pos += step;
                }
                continue;
            }
            let run = fixed::in_row_pow2(base, fixed::POS_SHIFT - lay.shift, top, pr.n_steps);
            for s0 in run.start..run.end.min(run.start + lay.period()) {
                // Steps s0 + m·P share one lerp fraction and advance one bin per m.
                let n = (run.end - s0 + lay.period() - 1) >> lay.shift;
                let at = lay.index(s0);
                let (j, fr) = fixed::split(base + s0 as i64 * step);
                let (lo, hi) = (&row[j..j + n], &row[j + 1..j + 1 + n]);
                let a = 1.0 - fr;
                // `fixed::lerp` with the fraction hoisted: the same f32 operations, so the same terms.
                for ((x, &l), &h) in acc[at..at + n].iter_mut().zip(lo).zip(hi) {
                    *x += fixed::term_select(w * (l * a + h * fr));
                }
            }
        }
    }
    covered
}
