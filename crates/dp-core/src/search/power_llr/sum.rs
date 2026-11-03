//! One event's LLR over the f0 slide. Sums of x and pixel counts are kept per (segment, harmonic, f0 step);
//! the two variants differ only in how those sums combine:
//! - profiled (`segment_s > 0`, default): ŝ = m − 1 fitted per (segment, harmonic, step), m = mean x, so the
//!   term n(m − 1 − ln m) is the Exp LLR at its own ŝ (weight ŝ/(1+ŝ)); 0 when m ≤ 1, so an absent harmonic
//!   or a faded segment costs nothing; minus its null mean so coverage does not bias.
//! - fixed prior (`segment_s = 0`): Σ (w_h·x − ln(1 + ŝ_h)) with ŝ_h from `snr`, w_h = ŝ_h/(1 + ŝ_h).
//!
//! Both are divided by n_eff (frame overlap), and f0 is marginalised by log-mean-exp over the slide.

use super::LlrParams;
use crate::audio::LogSpectrogram;

/// Null mean of one profiled term: n(m − 1 − ln m)₊ ≈ ½χ²₁ half the time.
const PROFILE_NULL_MEAN: f64 = 0.25;

#[derive(Default)]
pub struct Sums {
    acc: Vec<f64>,
    cnt: Vec<u32>,
    out: Vec<f64>,
}

/// One event at one clock: `offsets[i]` is bpo·log2(F) for frame `frames.start + i` (NaN = uncovered).
/// Returns (ln mean over f0 steps of exp(LLR), argmax step).
#[allow(clippy::too_many_arguments)]
pub fn event_llr(
    spec: &LogSpectrogram,
    frames: std::ops::Range<usize>,
    offsets: &[f64],
    b0_start: f64,
    n_steps: usize,
    step_bins: f64,
    shifts: &[f64],
    lp: &LlrParams,
    sc: &mut Sums,
) -> (f64, usize) {
    let n_h = shifts.len();
    let seg = if lp.segment_s > 0.0 { ((lp.segment_s / spec.dt).round() as usize).max(1) } else { frames.len().max(1) };
    let n_seg = frames.len().div_ceil(seg).max(1);
    sc.acc.clear();
    sc.acc.resize(n_seg * n_h * n_steps, 0.0);
    sc.cnt.clear();
    sc.cnt.resize(n_seg * n_h * n_steps, 0);
    let top = spec.n_f.saturating_sub(1) as f64;
    for (i, frame) in frames.enumerate() {
        let off = offsets[i];
        if !off.is_finite() {
            continue;
        }
        let row = spec.row(frame);
        for (h, &hs) in shifts.iter().enumerate() {
            let base = ((i / seg) * n_h + h) * n_steps;
            let first = b0_start + off + hs;
            let lo = (((-first) / step_bins).ceil().max(0.0) as usize).min(n_steps);
            let hi = (((top - first) / step_bins).ceil().max(0.0) as usize).clamp(lo, n_steps);
            for s in lo..hi {
                let pos = first + s as f64 * step_bins;
                if (0.0..top).contains(&pos) {
                    sc.acc[base + s] += lerp(row, pos) as f64;
                    sc.cnt[base + s] += 1;
                }
            }
        }
    }
    combine(lp, n_h, n_steps, sc);
    log_mean_exp(&sc.out)
}

fn combine(lp: &LlrParams, n_h: usize, n_steps: usize, sc: &mut Sums) {
    sc.out.clear();
    sc.out.resize(n_steps, 0.0);
    let fixed = harmonic_terms(&lp.snr, n_h);
    for (blk, (a, n)) in sc.acc.chunks_exact(n_steps).zip(sc.cnt.chunks_exact(n_steps)).enumerate() {
        let (w, c) = fixed[blk % n_h];
        for ((o, &a), &n) in sc.out.iter_mut().zip(a).zip(n) {
            if n == 0 {
                continue;
            }
            let n = n as f64;
            *o += if lp.segment_s > 0.0 {
                let m = a / n;
                let t = if m > 1.0 { n * (m - 1.0 - m.ln()) } else { 0.0 };
                t / lp.n_eff - PROFILE_NULL_MEAN
            } else {
                (w * a - c * n) / lp.n_eff
            };
        }
    }
}

/// (w_h, ln(1 + ŝ_h)) per harmonic; harmonics past the end of `snr` reuse its last entry.
fn harmonic_terms(snr: &[f64], n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let s = snr.get(i).or(snr.last()).copied().unwrap_or(1.0).max(0.0);
            (s / (1.0 + s), s.ln_1p())
        })
        .collect()
}

/// (ln mean exp(v), argmax index): a uniform prior over the slide.
pub fn log_mean_exp(v: &[f64]) -> (f64, usize) {
    let (arg, &mx) = v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap_or((0, &f64::NEG_INFINITY));
    if !mx.is_finite() {
        return (mx, arg);
    }
    let s: f64 = v.iter().map(|&a| (a - mx).exp()).sum();
    (mx + s.ln() - (v.len() as f64).ln(), arg)
}

fn lerp(row: &[f32], bin: f64) -> f32 {
    let i = bin as usize;
    let fr = (bin - i as f64) as f32;
    row[i] * (1.0 - fr) + row[i + 1] * fr
}
