//! Pass-likeness of a discovered event and the selection of events to search.
//! Logistic on fit evidence (fall over the ridge, both sides of CPA observed, CV residual, d-range width),
//! fitted on the Livermore dev split + sim_chain/sim_atm, plus fixed (not fitted) physical-implausibility
//! penalties: the training set has too few near/jet negatives to learn them. Jets are down-ranked, not dropped.
//! Method, split and held-out numbers: research_notes/.../fix_event_select_fitbias.md.

use super::Event;
use super::fit::cv_freq;

/// Events below this are searched only when nothing else is.
pub const MIN_PASS_P: f64 = 0.5;

const W0: f64 = -4.631;
const W_LN_SWING: f64 = 2.845;
const W_SIDES: f64 = 0.773;
const W_LN_RMSE: f64 = -2.154;
const W_LN_DWIDTH: f64 = -0.148;
const W_IMPLAUSIBLE: f64 = -3.0;
/// Below this CPA distance an aircraft fit is implausible (road traffic: v ≈ 20 m/s at d ≈ 30 m).
const NEAR_M: f64 = 150.0;
const JET_V_MPS: f64 = 150.0;
const JET_D_M: f64 = 8000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassFeatures {
    /// Fitted fall over the ridge, relative to f0.
    pub swing: f64,
    /// min(observed before CPA, after CPA) / ridge span: 0 = one-sided.
    pub sides: f64,
    pub rmse_rel: f64,
    pub d_width: f64,
    /// 0 for a plausible prop/heli pass; grows with jet-like speed/range and with implausibly small CPA distance.
    pub implausible: f64,
}

pub fn features(e: &Event) -> PassFeatures {
    let (b, t) = (&e.fit.best, &e.ridge.t);
    let (t0, t1) = (t.first().copied().unwrap_or(0.0), t.last().copied().unwrap_or(0.0));
    let span = (t1 - t0).max(1e-9);
    let tc = b.t_cpa_arrive.clamp(t0, t1);
    let f_at = |x: f64| cv_freq(b.f0, b.v, b.d_cpa, b.t_cpa_arrive, b.c, x);
    let d = b.d_cpa.max(1.0);
    let jet = ((b.v - JET_V_MPS) / 50.0).max(0.0) + (d / JET_D_M).ln().max(0.0);
    PassFeatures {
        swing: (f_at(t0) - f_at(t1)) / b.f0.max(1e-9),
        sides: (tc - t0).min(t1 - tc) / span,
        rmse_rel: e.fit.rmse_hz / b.f0.max(1e-9),
        d_width: e.fit.d_range.1 / e.fit.d_range.0.max(1e-9),
        implausible: jet + (NEAR_M / d).ln().max(0.0),
    }
}

/// Probability-like score in (0, 1).
pub fn pass_likeness(x: &PassFeatures) -> f64 {
    let z = W0
        + W_LN_SWING * x.swing.max(1e-3).ln()
        + W_SIDES * x.sides
        // Clamped to the training support (real ridges never fit below ~1e-3 of f0).
        + W_LN_RMSE * x.rmse_rel.max(1e-3).ln()
        + W_LN_DWIDTH * x.d_width.max(1.0).ln()
        + W_IMPLAUSIBLE * x.implausible;
    1.0 / (1.0 + (-z).exp())
}

/// Events whose CPAs arrive this close are one pass (unmerged tones or harmonics of one aircraft: all 4 such
/// pairs on Livermore dev, docs/research/clock_along_track_2026-11-04.md); only the strongest is searched.
pub const SAME_PASS_S: f64 = 10.0;

/// An event whose d range spans more than this factor places nothing, yet hires a flight and drags the shared
/// clock (Oyster Bay's d [1, 13159] m ground source at 3005 s: clock 45 s, answer 6 km off at ±120 s). Pass-like
/// labelled events reach 66; the junk ones 5818 and 10201. `GlobalParams::max_event_d_ratio` is the stricter
/// nationwide form.
pub const MAX_EVENT_D_RATIO: f64 = 1000.0;

/// Indices (CPA order) of the events to search: the `max_events` strongest by SNR among those with
/// `pass_p ≥ MIN_PASS_P`, a d range within `MAX_EVENT_D_RATIO` and no stronger one within `SAME_PASS_S`, or the
/// single most pass-like event when none qualifies.
pub fn select(events: &[Event], max_events: usize) -> Vec<usize> {
    let qualifies = |e: &Event| e.pass_p >= MIN_PASS_P && e.fit.d_range.1 <= MAX_EVENT_D_RATIO * e.fit.d_range.0.max(1e-9);
    let mut idx: Vec<usize> = (0..events.len()).filter(|&i| qualifies(&events[i])).collect();
    if idx.is_empty() {
        idx.extend((0..events.len()).max_by(|&a, &b| events[a].pass_p.total_cmp(&events[b].pass_p)));
    }
    idx.sort_by(|&a, &b| events[b].snr.total_cmp(&events[a].snr));
    let t = |i: usize| events[i].fit.best.t_cpa_arrive;
    let mut kept: Vec<usize> = Vec::new();
    for i in idx {
        if kept.iter().all(|&k| (t(k) - t(i)).abs() >= SAME_PASS_S) {
            kept.push(i);
        }
    }
    let mut idx = kept;
    idx.truncate(max_events.max(1));
    idx.sort_by(|&a, &b| events[a].fit.best.t_cpa_arrive.total_cmp(&events[b].fit.best.t_cpa_arrive));
    idx
}
