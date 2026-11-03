//! Discovery pipeline: comb image → peel best local Viterbi paths (suppressing each path's harmonic family)
//! → harmonic analysis → merge/stitch → CV fit and validation.

use rayon::prelude::*;

use super::candidate::{Cand, analyze};
use super::comb::{Comb, harmonic_shift};
use super::fit::{cv_freq, fit_cv_with};
use super::merge::{merge_related, ridge_of, same_source_keep, stitch};
use super::split::split_inconsistent;
use super::track::{Tracker, TrackerParams};
use super::{DiscoverParams, Event, Ridge};
use crate::audio::LogSpectrogram;

const SUPPRESS_HALF_BINS: i64 = 3;
const SUPPRESS_MARGIN_FRAMES: usize = 4;
const CUT_WINDOW_S: f64 = 3.0;
const MAX_RISE_BINS: f32 = 4.0;

/// Repeatedly take the best-scoring path, cut it into Doppler-plausible segments, analyse them, and floor the
/// whole harmonic family in the emission image so the next pass finds something else.
fn peel_tracks(spec: &LogSpectrogram, p: &DiscoverParams) -> Vec<Cand> {
    let (n_t, n_f) = (spec.n_t, spec.n_f);
    let mut e: Vec<f32> = spec.data.iter().map(|x| x.clamp(-2.0, 6.0)).collect();
    let mut comb = Comb::new(spec.bins_per_octave, p.harmonics, n_f);
    let mut c = comb.image(&e, n_t);
    let dmax = (p.max_rel_slope / std::f64::consts::LN_2 * spec.bins_per_octave as f64 * spec.dt).ceil();
    let tp = TrackerParams {
        dmax: (dmax as usize).clamp(1, 100),
        lam_down: p.lam_down,
        lam_steep: p.lam_steep,
        lam_up: p.lam_up,
        bias: p.path_bias,
    };
    let mut tracker = Tracker::new(n_t, n_f, tp);
    tracker.run_from(&c, 0, n_t);

    let min_frag = (p.min_fragment_s / spec.dt).ceil() as usize;
    let gap_frames = (CUT_WINDOW_S / spec.dt).ceil() as usize;
    let mut cands: Vec<Cand> = Vec::new();
    for _ in 0..p.max_tracks {
        let Some((t, u, score)) = tracker.best() else { break };
        if score < p.min_path_score {
            break;
        }
        let path = tracker.backtrack(t, u);
        let found: Vec<Cand> = path
            .segments(&c, n_f, p.path_bias, gap_frames, MAX_RISE_BINS)
            .into_iter()
            .filter(|(_, seg)| seg.len() >= min_frag)
            .filter_map(|(s, seg)| analyze(spec, s, &seg, p))
            .collect();
        let (mut a, mut b) = suppress(&mut e, path.start, &path.u, spec, p.harmonics);
        for cd in &found {
            let (a2, b2) = suppress(&mut e, cd.start, &cd.base, spec, p.harmonics);
            (a, b) = (a.min(a2), b.max(b2));
        }
        for t in a..=b {
            comb.recompute_row(&e[t * n_f..(t + 1) * n_f], &mut c[t * n_f..(t + 1) * n_f]);
        }
        tracker.run_from(&c, a, b);
        cands.extend(found);
    }
    cands
}

pub fn discover_with(spec: &LogSpectrogram, p: &DiscoverParams) -> Vec<Event> {
    if spec.n_t < 8 || spec.n_f < 8 {
        return Vec::new();
    }
    let cands = peel_tracks(spec, p);
    let cands = split_inconsistent(cands, spec, p);
    let cands = merge_related(cands, spec, p);
    let cands = stitch(cands, spec, p);
    let cands = merge_related(cands, spec, p);
    let valid: Vec<(Cand, Event)> = cands
        .into_par_iter()
        .filter_map(|cd| finalize(spec, &cd, p).map(|e| (cd, e)))
        .collect();
    let (kept, events): (Vec<Cand>, Vec<Event>) = valid.into_iter().unzip();
    let keep = same_source_keep(&kept, spec);
    let mut events: Vec<Event> = events
        .into_iter()
        .zip(keep)
        .filter_map(|(e, k)| k.then_some(e))
        .collect();
    events.sort_by(|a, b| a.fit.best.t_cpa_arrive.total_cmp(&b.fit.best.t_cpa_arrive));
    events.iter_mut().enumerate().for_each(|(i, ev)| ev.id = i);
    events
}

/// Floor every multiple of `base` (and a few frames beyond its ends) at 0 in the emission image.
/// Returns the touched frame range.
fn suppress(e: &mut [f32], start: usize, base: &[f32], spec: &LogSpectrogram, harmonics: u32) -> (usize, usize) {
    let n_f = spec.n_f;
    let a = start.saturating_sub(SUPPRESS_MARGIN_FRAMES);
    let b = (start + base.len() - 1 + SUPPRESS_MARGIN_FRAMES).min(spec.n_t - 1);
    for t in a..=b {
        let u = base[t.saturating_sub(start).min(base.len() - 1)] as f64;
        for m in 1..=2 * harmonics {
            let pos = (u + harmonic_shift(spec.bins_per_octave, m as f64)).round() as i64;
            for k in pos - SUPPRESS_HALF_BINS..=pos + SUPPRESS_HALF_BINS {
                if (0..n_f as i64).contains(&k) {
                    let x = &mut e[t * n_f + k as usize];
                    *x = x.min(0.0);
                }
            }
        }
    }
    (a, b)
}

/// Frames where the strongest harmonic is clearly above the floor, if there are enough of them.
fn confident(r: &Ridge) -> Ridge {
    let keep: Vec<usize> = (0..r.t.len()).filter(|&i| r.snr[i] >= 1.0).collect();
    if keep.len() < 8 || keep.len() * 5 < r.t.len() * 2 {
        return r.clone();
    }
    Ridge {
        t: keep.iter().map(|&i| r.t[i]).collect(),
        f_hz: keep.iter().map(|&i| r.f_hz[i]).collect(),
        snr: keep.iter().map(|&i| r.snr[i]).collect(),
    }
}

fn finalize(spec: &LogSpectrogram, cd: &Cand, p: &DiscoverParams) -> Option<Event> {
    if (cd.base.len() as f64) * spec.dt < p.min_duration_s || cd.score < p.min_snr {
        return None;
    }
    let ridge = ridge_of(spec, cd, cd.strongest as f64);
    let fit = fit_cv_with(&confident(&ridge), p.c, &p.fit);
    let b = &fit.best;
    if fit.rmse_hz.is_nan() || fit.rmse_hz > p.max_rmse_rel * b.f0 || b.v > p.max_speed_mps || b.d_cpa < p.min_cpa_m {
        return None;
    }
    let f_at = |t: f64| cv_freq(b.f0, b.v, b.d_cpa, b.t_cpa_arrive, b.c, t);
    let drop = (f_at(ridge.t[0]) - f_at(*ridge.t.last()?)) / b.f0;
    if drop < p.min_drop {
        return None;
    }
    Some(Event {
        id: 0,
        ridge,
        harmonics: cd.harmonics.clone(),
        fit,
        ridge_harmonic: cd.strongest,
        snr: cd.score,
    })
}
