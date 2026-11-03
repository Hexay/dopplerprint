//! Duplicate/harmonic merging (constant log-shift over the overlap). Re-runs `analyze` on the combined base so
//! harmonics are re-derived. Fragment stitching lives in `stitch`.

use super::candidate::{Cand, analyze, gcd};
use super::comb::harmonic_shift;
use super::stats::quantile;
use super::{DiscoverParams, Ridge};
use crate::audio::LogSpectrogram;

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
pub(super) fn union_base(into: &Cand, from: &Cand, shift: f32) -> (usize, Vec<f32>) {
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
pub(super) fn accept_union(m: Option<Cand>, a: &Cand, b: &Cand) -> Option<Cand> {
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
