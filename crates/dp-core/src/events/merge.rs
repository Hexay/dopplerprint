//! Duplicate/harmonic merging (constant log-shift over the overlap) and fragment stitching (one CV curve
//! must explain both sides). Both re-run `analyze` on the combined base so harmonics are re-derived.

use std::collections::HashSet;

use super::candidate::{Cand, analyze, gcd};
use super::comb::harmonic_shift;
use super::fit::{cv_freq, fit_cv_with};
use super::stats::quantile;
use super::{DiscoverParams, Ridge};
use crate::audio::LogSpectrogram;

/// Each fragment's own fit must predict the other's near end to within this fraction.
const STITCH_PREDICT_REL: f64 = 0.03;
/// A constant log-ratio only proves a common source if the lines actually fell this much over the overlap.
const SAME_SOURCE_MIN_FALL_BINS: f64 = 6.0;

fn ratios(h: u32) -> Vec<f64> {
    let mut r = Vec::new();
    for a in 1..=h {
        for b in 1..=h {
            if gcd(a, b) == 1 {
                r.push(a as f64 / b as f64);
            }
        }
    }
    r
}

/// Median log-shift `a − b` over the overlap if it is constant (same source); `None` otherwise.
fn related_shift(a: &Cand, b: &Cand, spec: &LogSpectrogram, p: &DiscoverParams) -> Option<f32> {
    let (lo, hi) = (a.start.max(b.start), a.end().min(b.end()));
    if hi < lo {
        return None;
    }
    let n = hi - lo + 1;
    if n < 8 || (n as f64) < 0.3 * a.base.len().min(b.base.len()) as f64 {
        return None;
    }
    let bpo = spec.bins_per_octave;
    let ratio_bins = |t: usize| (bpo as f64 * (a.fund_hz[t - a.start] / b.fund_hz[t - b.start]).log2()) as f32;
    let mut d: Vec<f32> = (lo..=hi).map(ratio_bins).collect();
    let mut s = Vec::new();
    let m = crate::audio::whiten::median(&d, &mut s);
    d.iter_mut().for_each(|x| *x = (*x - m).abs());
    if 1.4826 * crate::audio::whiten::median(&d, &mut s) > 1.5 {
        return None;
    }
    let dup = harmonic_shift(bpo, 1.03) as f32;
    let ok = m.abs() <= dup
        || ratios(p.harmonics)
            .iter()
            .any(|&r| (m - harmonic_shift(bpo, r) as f32).abs() <= 1.0);
    ok.then_some(m)
}

/// `into`'s base, extended by `from` (moved onto `into`'s axis by `shift`); gaps linearly interpolated.
fn union_base(into: &Cand, from: &Cand, shift: f32) -> (usize, Vec<f32>) {
    let (s, e) = (into.start.min(from.start), into.end().max(from.end()));
    let raw: Vec<Option<f32>> = (s..=e)
        .map(|t| into.base_at(t).or_else(|| from.base_at(t).map(|u| u + shift)))
        .collect();
    let mut out = Vec::with_capacity(raw.len());
    let mut prev: Option<(usize, f32)> = None;
    for (i, v) in raw.iter().enumerate() {
        if let Some(x) = *v {
            if let Some((pi, px)) = prev {
                for k in pi + 1..i {
                    out.push(px + (x - px) * (k - pi) as f32 / (i - pi) as f32);
                }
            }
            out.push(x);
            prev = Some((i, x));
        }
    }
    (s, out)
}

/// Over-merge guard: the union must stay comparably convincing (a line present in only one part dilutes by ~√2).
fn accept_union(m: Option<Cand>, a: &Cand, b: &Cand) -> Option<Cand> {
    m.filter(|m| m.score >= 0.6 * a.score.max(b.score))
}

pub(super) fn merge_related(mut cands: Vec<Cand>, spec: &LogSpectrogram, p: &DiscoverParams) -> Vec<Cand> {
    for _ in 0..4 {
        cands.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut out: Vec<Cand> = Vec::new();
        let mut changed = false;
        for c in cands {
            let merged = out.iter().enumerate().find_map(|(i, g)| {
                let shift = related_shift(g, &c, spec, p)?;
                let (s, base) = union_base(g, &c, shift);
                accept_union(analyze(spec, s, &base, p), g, &c).map(|m| (i, m))
            });
            match merged {
                Some((i, m)) => {
                    out[i] = m;
                    changed = true;
                }
                None => out.push(c),
            }
        }
        cands = out;
        if !changed {
            break;
        }
    }
    cands
}

/// Lines of one source share the Doppler factor, so their log-ratio is constant through the fall even when the
/// ratio is no small-integer harmonic (piston shaft orders, exhaust vs prop). Flags the stronger such line.
pub(super) fn same_source_keep(cands: &[Cand], spec: &LogSpectrogram) -> Vec<bool> {
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| cands[b].score.total_cmp(&cands[a].score));
    let bpo = spec.bins_per_octave as f64;
    let mut flags = vec![false; cands.len()];
    let mut keep: Vec<&Cand> = Vec::new();
    for i in order {
        let c = &cands[i];
        let dup = keep.iter().any(|k| {
            let (lo, hi) = (k.start.max(c.start), k.end().min(c.end()));
            if hi < lo || ((hi - lo + 1) as f64) < 0.5 * k.base.len().min(c.base.len()) as f64 {
                return false;
            }
            let lk: Vec<f64> = (lo..=hi).map(|t| bpo * k.fund_hz[t - k.start].log2()).collect();
            let lc: Vec<f64> = (lo..=hi).map(|t| bpo * c.fund_hz[t - c.start].log2()).collect();
            let fall = quantile(&lk, 0.9) - quantile(&lk, 0.1);
            let d: Vec<f64> = lk.iter().zip(&lc).map(|(a, b)| a - b).collect();
            let m = quantile(&d, 0.5);
            let spread = 1.4826 * quantile(&d.iter().map(|x| (x - m).abs()).collect::<Vec<_>>(), 0.5);
            fall >= SAME_SOURCE_MIN_FALL_BINS && spread <= 1.0
        });
        if !dup {
            keep.push(c);
            flags[i] = true;
        }
    }
    flags
}

pub(super) fn ridge_of(spec: &LogSpectrogram, c: &Cand, scale: f64) -> Ridge {
    Ridge {
        t: (c.start..=c.end()).map(|k| spec.time_of(k)).collect(),
        f_hz: c.fund_hz.iter().map(|f| f * scale).collect(),
        snr: c.peak_z.clone(),
    }
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
fn stitch_fit(a: &Cand, b: &Cand, spec: &LogSpectrogram, p: &DiscoverParams) -> Option<(f64, f64)> {
    let gap = b.start as f64 - a.end() as f64;
    if gap * spec.dt > p.max_gap_s || gap < -8.0 || b.end() <= a.end() {
        return None;
    }
    let (ra, rb) = (ridge_of(spec, a, 1.0), ridge_of(spec, b, 1.0));
    let fa = fit_cv_with(&ra, p.c, &p.fit);
    let fb = fit_cv_with(&rb, p.c, &p.fit);
    let ragged = |f: &super::CvFit| f.rmse_hz > 2.0 * p.split_rmse_rel * f.best.f0;
    if ragged(&fa) || ragged(&fb) {
        return None;
    }
    // Tolerance from the stronger fragment: a weak, ragged fragment must not loosen it.
    let strong = if a.score >= b.score { &fa } else { &fb };
    let tol = (1.5 * strong.rmse_hz).max(0.5 * p.split_rmse_rel * strong.best.f0);
    let (ta_end, tb_start) = (*ra.t.last()?, *rb.t.first()?);
    let (fa_end, fb_start) = (*ra.f_hz.last()?, *rb.f_hz.first()?);
    let pa = cv_freq(
        fa.best.f0,
        fa.best.v,
        fa.best.d_cpa,
        fa.best.t_cpa_arrive,
        p.c,
        tb_start,
    );
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
            let j = fit_cv_with(&concat_ridge(&ra, &rb, r), p.c, &p.fit);
            (j.rmse_hz <= tol).then_some((j.rmse_hz, r))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
}

/// Join mutually-best fragment pairs until none is left; a rejected union is never retried.
pub(super) fn stitch(mut cands: Vec<Cand>, spec: &LogSpectrogram, p: &DiscoverParams) -> Vec<Cand> {
    let key = |c: &Cand| (c.start, c.end());
    let mut rejected = HashSet::new();
    loop {
        let n = cands.len();
        let fits: Vec<(usize, usize, f64, f64)> = (0..n)
            .flat_map(|i| (0..n).map(move |j| (i, j)))
            .filter(|&(i, j)| i != j && !rejected.contains(&(key(&cands[i]), key(&cands[j]))))
            .filter_map(|(i, j)| stitch_fit(&cands[i], &cands[j], spec, p).map(|(e, r)| (i, j, e, r)))
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
            }
            None => {
                rejected.insert((key(&cands[i]), key(&cands[j])));
            }
        }
    }
}
