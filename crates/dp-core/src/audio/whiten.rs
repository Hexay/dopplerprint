//! Robust per-bin whitening of log power: running low-quantile floor, per-bin MAD z-score, per-frame
//! centring, clipping. See docs/AUDIO.md for why the floor is a low quantile and the scale is global.

use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WhitenParams {
    /// Running-floor window (s).
    pub median_s: f64,
    /// Floor quantile within the window: a tone may occupy a bin for up to (1 − q) of the window and survive.
    pub floor_quantile: f64,
    /// Running statistics are evaluated on this time grid and interpolated between.
    pub stride_s: f64,
    pub clip_lo: f32,
    pub clip_hi: f32,
    /// Subtract each frame's median across bins (neutralises AGC and broadband bursts).
    pub frame_center: bool,
    /// Mains fundamental (50 or 60 Hz); its harmonics are zeroed within `notch_hz`.
    pub mains_hz: Option<f64>,
    pub notch_hz: f64,
}

impl Default for WhitenParams {
    fn default() -> Self {
        Self {
            median_s: 60.0,
            floor_quantile: 0.25,
            stride_s: 2.0,
            clip_lo: -3.0,
            clip_hi: 8.0,
            frame_center: true,
            mains_hz: None,
            notch_hz: 0.6,
        }
    }
}

const MAD_TO_SIGMA: f32 = 1.4826;

/// `power` is row-major `[frame][bin]`; returns z-scores in the same layout.
pub fn whiten(power: &[f32], n_t: usize, n_f: usize, dt: f64, freqs: &[f64], p: &WhitenParams) -> Vec<f32> {
    assert_eq!(power.len(), n_t * n_f);
    let mean = power.iter().map(|&x| x as f64).sum::<f64>() / power.len().max(1) as f64;
    // Relative floor keeps whitening exactly gain-invariant and digital silence finite.
    let floor = (1e-10 * mean).max(1e-30) as f32;
    let half_w = ((0.5 * p.median_s / dt).round() as usize).max(1);
    let stride = ((p.stride_s / dt).round() as usize).max(1);
    let cols: Vec<Vec<f32>> = (0..n_f)
        .into_par_iter()
        .map(|b| {
            let l: Vec<f32> = (0..n_t).map(|t| (power[t * n_f + b] + floor).ln()).collect();
            let base = running_quantile(&l, half_w, stride, p.floor_quantile);
            let mut r: Vec<f32> = l.iter().zip(&base).map(|(a, m)| a - m).collect();
            let mut scratch = Vec::new();
            let centre = median(&r, &mut scratch);
            let dev: Vec<f32> = r.iter().map(|x| (x - centre).abs()).collect();
            let sigma = (MAD_TO_SIGMA * median(&dev, &mut scratch)).max(1e-6);
            r.iter_mut().for_each(|x| *x = (*x - centre) / sigma);
            r
        })
        .collect();
    let notch: Vec<bool> = freqs
        .iter()
        .map(|&f| p.mains_hz.is_some_and(|m| near_harmonic(f, m, p.notch_hz)))
        .collect();
    let mut out = vec![0f32; n_t * n_f];
    out.par_chunks_mut(n_f)
        .enumerate()
        .for_each_init(Vec::new, |scratch, (t, row)| {
            for (b, z) in row.iter_mut().enumerate() {
                *z = cols[b][t];
            }
            let c = if p.frame_center { median(row, scratch) } else { 0.0 };
            for (z, &n) in row.iter_mut().zip(&notch) {
                *z = if n { 0.0 } else { (*z - c).clamp(p.clip_lo, p.clip_hi) };
            }
        });
    out
}

fn near_harmonic(f: f64, mains: f64, tol: f64) -> bool {
    let k = (f / mains).round();
    k >= 1.0 && (f - k * mains).abs() <= tol
}

pub(crate) fn median(x: &[f32], scratch: &mut Vec<f32>) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    scratch.clear();
    scratch.extend_from_slice(x);
    let mid = scratch.len() / 2;
    *scratch.select_nth_unstable_by(mid, f32::total_cmp).1
}

fn quantile(x: &[f32], q: f64, scratch: &mut Vec<f32>) -> f32 {
    scratch.clear();
    scratch.extend_from_slice(x);
    let k = ((scratch.len() - 1) as f64 * q).round() as usize;
    *scratch.select_nth_unstable_by(k, f32::total_cmp).1
}

/// q-quantile over `[t - half_w, t + half_w]`, evaluated every `stride` samples and linearly interpolated.
pub(crate) fn running_quantile(x: &[f32], half_w: usize, stride: usize, q: f64) -> Vec<f32> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    let mut knots: Vec<usize> = (0..n).step_by(stride).collect();
    if *knots.last().expect("n > 0") != n - 1 {
        knots.push(n - 1);
    }
    let mut scratch = Vec::with_capacity(2 * half_w + 1);
    let vals: Vec<f32> = knots
        .iter()
        .map(|&c| quantile(&x[c.saturating_sub(half_w)..(c + half_w + 1).min(n)], q, &mut scratch))
        .collect();
    let mut out = vec![0f32; n];
    for (k, w) in knots.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        for (i, o) in out[a..=b].iter_mut().enumerate() {
            let f = i as f32 / (b - a) as f32;
            *o = vals[k] * (1.0 - f) + vals[k + 1] * f;
        }
    }
    if knots.len() == 1 {
        out[0] = vals[0];
    }
    out
}
