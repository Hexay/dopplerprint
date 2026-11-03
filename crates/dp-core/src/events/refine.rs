//! Event ridge re-measured on the complex STFT before the CV fit: each detected harmonic is refined from its
//! folded seed and the results are folded again with the detection z² weights (as in `candidate::analyze`).

use super::Ridge;
use super::candidate::{Cand, peak_near};
use super::merge::ridge_of;
use crate::audio::{IfRefiner, LogSpectrogram};

fn slope_hz_s(f: &[f64], i: usize, dt: f64) -> f64 {
    let (a, b) = (i.saturating_sub(2), (i + 2).min(f.len() - 1));
    if b > a { (f[b] - f[a]) / ((b - a) as f64 * dt) } else { 0.0 }
}

pub(super) fn refined_ridge(spec: &LogSpectrogram, cd: &Cand, r: &IfRefiner) -> Ridge {
    let mut ridge = ridge_of(spec, cd, cd.strongest as f64);
    for (i, f) in ridge.f_hz.iter_mut().enumerate() {
        let (k, fund) = (cd.start + i, cd.fund_hz[i]);
        let slope = slope_hz_s(&cd.fund_hz, i, spec.dt);
        let (mut num, mut den) = (0.0, 0.0);
        for &h in &cd.harmonics {
            let seed = fund * h as f64;
            let Some((_, z)) = peak_near(spec.row(k), spec.bin_of(seed)) else { continue };
            let Some(fh) = r.refine(spec.time_of(k), seed, slope * h as f64) else { continue };
            let w = (z.max(0.0) as f64).powi(2);
            num += w * fh / h as f64;
            den += w;
        }
        if den > 0.0 {
            *f = num / den * cd.strongest as f64;
        }
    }
    ridge
}
