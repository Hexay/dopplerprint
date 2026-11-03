//! Fragment stitching: join mutually-best fragment pairs that one CV curve explains on both sides, then re-run
//! `analyze` on the combined base so harmonics are re-derived.

use std::collections::HashSet;

use rayon::prelude::*;

use super::candidate::{Cand, analyze};
use super::comb::harmonic_shift;
use super::fit::{cv_freq, fit_cv_with};
use super::merge::{accept_union, ridge_of, union_base};
use super::{CvFit, DiscoverParams, Ridge};
use crate::audio::LogSpectrogram;

/// Each fragment's own fit must predict the other's near end to within this fraction.
const STITCH_PREDICT_REL: f64 = 0.03;

/// A candidate's ridge and its own CV fit, computed once per candidate rather than once per pair.
struct Frag {
    ridge: Ridge,
    fit: CvFit,
}

fn frag(spec: &LogSpectrogram, c: &Cand, p: &DiscoverParams) -> Frag {
    let ridge = ridge_of(spec, c, 1.0);
    let fit = fit_cv_with(&ridge, p.c, &p.fit);
    Frag { ridge, fit }
}

fn concat_ridge(a: &Ridge, b: &Ridge, b_scale: f64) -> Ridge {
    let mut r = a.clone();
    for i in 0..b.t.len() {
        if b.t[i] > a.t.last().copied().unwrap_or(f64::MIN) {
            r.t.push(b.t[i]);
            r.f_hz.push(b.f_hz[i] * b_scale);
            r.snr.push(b.snr[i]);
        }
    }
    r
}

/// Best stitch of `b` after `a`: (joint rmse, ratio b→a) if a single CV curve explains both fragments.
fn stitch_fit(a: (&Cand, &Frag), b: (&Cand, &Frag), spec: &LogSpectrogram, p: &DiscoverParams) -> Option<(f64, f64)> {
    let ((a, Frag { ridge: ra, fit: fa }), (b, Frag { ridge: rb, fit: fb })) = (a, b);
    let gap = b.start as f64 - a.end() as f64;
    if gap * spec.dt > p.max_gap_s || gap < -8.0 || b.end() <= a.end() {
        return None;
    }
    let ragged = |f: &CvFit| f.rmse_hz > 2.0 * p.split_rmse_rel * f.best.f0;
    if ragged(fa) || ragged(fb) {
        return None;
    }
    // Tolerance from the stronger fragment: a weak, ragged fragment must not loosen it.
    let strong = if a.score >= b.score { fa } else { fb };
    let tol = (1.5 * strong.rmse_hz).max(0.5 * p.split_rmse_rel * strong.best.f0);
    let (ta_end, tb_start) = (*ra.t.last()?, *rb.t.first()?);
    let (fa_end, fb_start) = (*ra.f_hz.last()?, *rb.f_hz.first()?);
    let pa = cv_freq(fa.best.f0, fa.best.v, fa.best.d_cpa, fa.best.t_cpa_arrive, p.c, tb_start);
    let pb = cv_freq(fb.best.f0, fb.best.v, fb.best.d_cpa, fb.best.t_cpa_arrive, p.c, ta_end);
    [1.0, 2.0, 0.5, 1.5, 2.0 / 3.0, 3.0, 1.0 / 3.0]
        .into_iter()
        .filter(|&r| {
            let continuous = fb_start * r <= fa_end * 1.01;
            continuous
                && (pa / (fb_start * r) - 1.0).abs() < STITCH_PREDICT_REL
                && (pb * r / fa_end - 1.0).abs() < STITCH_PREDICT_REL
        })
        .filter_map(|r| {
            let j = fit_cv_with(&concat_ridge(ra, rb, r), p.c, &p.fit);
            (j.rmse_hz <= tol).then_some((j.rmse_hz, r))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
}

type PairFit = Option<(f64, f64)>;

/// Pair results only depend on the two candidates, so they are cached and only the merged candidate's row and
/// column are recomputed after a union.
struct Pairs<'a> {
    spec: &'a LogSpectrogram,
    p: &'a DiscoverParams,
    frags: Vec<Frag>,
    /// `fit[i][j]`: `stitch_fit` of j after i.
    fit: Vec<Vec<PairFit>>,
}

impl<'a> Pairs<'a> {
    fn new(cands: &[Cand], spec: &'a LogSpectrogram, p: &'a DiscoverParams) -> Self {
        let frags: Vec<Frag> = cands.par_iter().map(|c| frag(spec, c, p)).collect();
        let mut s = Self { spec, p, frags, fit: Vec::new() };
        s.fit = (0..cands.len()).into_par_iter().map(|i| s.row(cands, i)).collect();
        s
    }

    fn pair(&self, cands: &[Cand], i: usize, j: usize) -> PairFit {
        let (a, b) = ((&cands[i], &self.frags[i]), (&cands[j], &self.frags[j]));
        (i != j).then(|| stitch_fit(a, b, self.spec, self.p)).flatten()
    }

    fn row(&self, cands: &[Cand], i: usize) -> Vec<PairFit> {
        (0..cands.len()).map(|j| self.pair(cands, i, j)).collect()
    }

    /// `cands[i]` was replaced and `cands[j]` removed (already applied to `cands`).
    fn merged(&mut self, cands: &[Cand], i: usize, j: usize) {
        self.frags.remove(j);
        self.fit.remove(j);
        self.fit.iter_mut().for_each(|r| {
            r.remove(j);
        });
        let i = if j < i { i - 1 } else { i };
        self.frags[i] = frag(self.spec, &cands[i], self.p);
        let col: Vec<PairFit> = (0..cands.len()).into_par_iter().map(|k| self.pair(cands, k, i)).collect();
        self.fit[i] = self.row(cands, i);
        for (k, v) in col.into_iter().enumerate() {
            self.fit[k][i] = v;
        }
    }
}

/// Join mutually-best fragment pairs until none is left; a rejected union is never retried.
pub(super) fn stitch(mut cands: Vec<Cand>, spec: &LogSpectrogram, p: &DiscoverParams) -> Vec<Cand> {
    let key = |c: &Cand| (c.start, c.end());
    let mut rejected = HashSet::new();
    let mut pairs = Pairs::new(&cands, spec, p);
    loop {
        let n = cands.len();
        let fits: Vec<(usize, usize, f64, f64)> = (0..n)
            .flat_map(|i| (0..n).map(move |j| (i, j)))
            .filter_map(|(i, j)| pairs.fit[i][j].map(|(e, r)| (i, j, e, r)))
            .filter(|&(i, j, ..)| !rejected.contains(&(key(&cands[i]), key(&cands[j]))))
            .collect();
        let best_next = |i: usize| fits.iter().filter(|f| f.0 == i).min_by(|x, y| x.2.total_cmp(&y.2));
        let best_prev = |j: usize| fits.iter().filter(|f| f.1 == j).min_by(|x, y| x.2.total_cmp(&y.2));
        let Some(&(i, j, _, r)) = (0..n)
            .filter_map(best_next)
            .find(|f| best_prev(f.1).is_some_and(|g| g.0 == f.0))
        else {
            return cands;
        };
        let (s, base) = union_base(&cands[i], &cands[j], harmonic_shift(spec.bins_per_octave, r) as f32);
        match accept_union(analyze(spec, s, &base, p), &cands[i], &cands[j]) {
            Some(m) => {
                cands[i] = m;
                cands.remove(j);
                pairs.merged(&cands, i, j);
            }
            None => {
                rejected.insert((key(&cands[i]), key(&cands[j])));
            }
        }
    }
}
