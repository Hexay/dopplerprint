//! Split candidates that one CV curve cannot explain: the tracker can hop between crossing lines of two
//! aircraft. Each half is re-analysed; genuine fragments are re-joined later by `stitch::stitch`.

use rayon::prelude::*;

use super::DiscoverParams;
use super::candidate::{Cand, analyze};
use super::fit::fit_cv_with;
use super::merge::ridge_of;
use crate::audio::LogSpectrogram;

const SPLIT_TRIALS: usize = 16;
const MAX_DEPTH: u32 = 3;

/// Relative inlier rmse of the CV fit to a candidate's folded fundamental.
pub(super) fn fit_rel_rmse(spec: &LogSpectrogram, c: &Cand, p: &DiscoverParams) -> f64 {
    let fit = fit_cv_with(&ridge_of(spec, c, 1.0), p.c, &p.fit);
    fit.rmse_hz / fit.best.f0.max(1e-9)
}

fn sub(c: &Cand, a: usize, b: usize) -> (usize, Vec<f32>) {
    (c.start + a, c.base[a..b].to_vec())
}

fn split_one(spec: &LogSpectrogram, c: Cand, p: &DiscoverParams, depth: u32, out: &mut Vec<Cand>) {
    let n = c.base.len();
    let min = (p.min_fragment_s / spec.dt).ceil() as usize;
    let whole = fit_rel_rmse(spec, &c, p);
    if depth >= MAX_DEPTH || whole <= p.split_rmse_rel || n < 2 * min {
        out.push(c);
        return;
    }
    let step = ((n - 2 * min) / SPLIT_TRIALS).max(1);
    let best = (min..=n - min)
        .step_by(step)
        .filter_map(|k| {
            let (sa, ba) = sub(&c, 0, k);
            let (sb, bb) = sub(&c, k, n);
            let (ca, cb) = (analyze(spec, sa, &ba, p)?, analyze(spec, sb, &bb, p)?);
            let (ra, rb) = (fit_rel_rmse(spec, &ca, p), fit_rel_rmse(spec, &cb, p));
            Some(((ra * ra * k as f64 + rb * rb * (n - k) as f64) / n as f64, ca, cb))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0));
    match best {
        Some((ms, ca, cb)) if ms.sqrt() < 0.5 * whole => {
            split_one(spec, ca, p, depth + 1, out);
            split_one(spec, cb, p, depth + 1, out);
        }
        _ => out.push(c),
    }
}

pub(super) fn split_inconsistent(cands: Vec<Cand>, spec: &LogSpectrogram, p: &DiscoverParams) -> Vec<Cand> {
    cands
        .into_par_iter()
        .flat_map_iter(|c| {
            let mut out = Vec::new();
            split_one(spec, c, p, 0, &mut out);
            out
        })
        .collect()
}
