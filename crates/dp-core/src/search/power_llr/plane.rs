//! The power-LLR plane: x = P/N̂ per pixel, linear single-window power over a noise-mean estimate
//! N̂ = q₂₅·κ, where κ is the bin's null mean/q₂₅ ratio (1/ln(4/3) for Exp(1); measured on white noise on the
//! same grid, because max-pooled log bins are not Exp(1)).

use rayon::prelude::*;

use crate::audio::Audio;
use crate::audio::spectrogram::{LogSpectrogram, SpecParams, power_on_log_axis};
use crate::audio::whiten::{WhitenParams, median, running_quantile};
use crate::sim::noise::Pcg;

const CAL_S: f64 = 60.0;
/// Calibration pools ± this many neighbouring bins (similar pool width) per bin.
const CAL_HALF_BINS: usize = 8;
/// Zero-padded edge frames excluded from calibration.
const CAL_EDGE_FRAMES: usize = 4;

/// x = P/N̂ capped at `cap`, frames re-centred like `whiten` (median over bins of x/null median).
pub fn plane(audio: &Audio, sp: &SpecParams, cap: f32) -> LogSpectrogram {
    let (mut spec, power) = power_on_log_axis(audio, sp);
    let null = NullShape::measure(audio.sample_rate, sp);
    spec.data = normalise(&power, spec.n_t, spec.n_f, sp.hop_s, &null, &WhitenParams::default(), cap);
    spec
}

/// Per-bin noise-only shape: κ = mean/q, and the median of P/mean (for frame centring).
struct NullShape {
    kappa: Vec<f32>,
    median: Vec<f32>,
}

impl NullShape {
    fn measure(sample_rate: u32, sp: &SpecParams) -> Self {
        let q = WhitenParams::default().floor_quantile;
        let mut rng = Pcg::new(0x5EED_11A2);
        let n = (CAL_S * sample_rate as f64) as usize;
        let noise = Audio { sample_rate, samples: (0..n).map(|_| rng.gauss() as f32).collect() };
        let (spec, power) = power_on_log_axis(&noise, sp);
        let (n_t, n_f) = (spec.n_t, spec.n_f);
        let edge = CAL_EDGE_FRAMES.min(n_t / 4);
        let mut scratch = Vec::new();
        let ratios: Vec<Vec<f32>> = (0..n_f)
            .map(|b| {
                let col: Vec<f32> = (edge..n_t - edge).map(|t| power[t * n_f + b]).collect();
                let qb = quantile(&col, q, &mut scratch).max(f32::MIN_POSITIVE);
                col.iter().map(|p| p / qb).collect()
            })
            .collect();
        let (mut kappa, mut med) = (vec![0f32; n_f], vec![0f32; n_f]);
        for b in 0..n_f {
            let pool: Vec<f32> = ratios[b.saturating_sub(CAL_HALF_BINS)..(b + CAL_HALF_BINS + 1).min(n_f)].concat();
            let mean = pool.iter().map(|&r| r as f64).sum::<f64>() / pool.len().max(1) as f64;
            kappa[b] = mean as f32;
            med[b] = median(&pool, &mut scratch) / mean.max(1e-30) as f32;
        }
        Self { kappa, median: med }
    }
}

fn quantile(x: &[f32], q: f64, scratch: &mut Vec<f32>) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    scratch.clear();
    scratch.extend_from_slice(x);
    let k = ((scratch.len() - 1) as f64 * q).round() as usize;
    *scratch.select_nth_unstable_by(k, f32::total_cmp).1
}

fn normalise(power: &[f32], n_t: usize, n_f: usize, dt: f64, null: &NullShape, w: &WhitenParams, cap: f32) -> Vec<f32> {
    let mean = power.iter().map(|&x| x as f64).sum::<f64>() / power.len().max(1) as f64;
    let floor = (1e-10 * mean).max(1e-30) as f32;
    let half_w = ((0.5 * w.median_s / dt).round() as usize).max(1);
    let stride = ((w.stride_s / dt).round() as usize).max(1);
    // The running floor is a quantile of ln P; a quantile commutes with exp, so exp(base) is q₂₅ of P.
    let cols: Vec<Vec<f32>> = (0..n_f)
        .into_par_iter()
        .map(|b| {
            let l: Vec<f32> = (0..n_t).map(|t| (power[t * n_f + b] + floor).ln()).collect();
            let base = running_quantile(&l, half_w, stride, w.floor_quantile);
            (0..n_t).map(|t| power[t * n_f + b] / (base[t].exp() * null.kappa[b])).collect()
        })
        .collect();
    let mut out = vec![0f32; n_t * n_f];
    out.par_chunks_mut(n_f).enumerate().for_each_init(Vec::new, |scratch, (t, row)| {
        for (b, x) in row.iter_mut().enumerate() {
            *x = cols[b][t] / null.median[b];
        }
        let g = if w.frame_center { median(row, scratch) } else { 1.0 };
        let g = if g > 0.0 { g } else { 1.0 };
        for (b, x) in row.iter_mut().enumerate() {
            *x = (cols[b][t] / g).min(cap);
        }
    });
    out
}

/// Var(Σ x over `len` frames) / (len · Var x) along constant-bin curves: the n_eff overlap factor.
/// Also returns Var x itself (1 for Exp(1)). Every 6th bin and disjoint frame blocks give the samples.
pub fn overlap_factor(x: &LogSpectrogram, len: usize) -> (f64, f64) {
    let (mut sums, mut pix) = (Vec::new(), Vec::new());
    let edge = CAL_EDGE_FRAMES + 2;
    for b in (CAL_HALF_BINS..x.n_f.saturating_sub(CAL_HALF_BINS)).step_by(6) {
        let mut t = edge;
        while t + len + edge <= x.n_t {
            let block: Vec<f64> = (t..t + len).map(|k| x.data[k * x.n_f + b] as f64).collect();
            sums.push(block.iter().sum::<f64>());
            pix.extend(block);
            t += len;
        }
    }
    let var = |v: &[f64]| {
        let m = v.iter().sum::<f64>() / v.len().max(1) as f64;
        v.iter().map(|a| (a - m).powi(2)).sum::<f64>() / (v.len().max(2) - 1) as f64
    };
    let vp = var(&pix);
    (var(&sums) / (len as f64 * vp), vp)
}
