//! Cheap spectrogram rerank of proposals. At each of a proposal's best voted cells, the supporting-flight
//! combination whose CPA geometry best fits the events' d/κ/v bounds and CPA times is scored jointly; the
//! proposal keeps its best cell. Geometry alone cannot separate thousands of chance two-event coincidences,
//! and a spectrogram score without the geometry gate lets wrong pairs fit noise (Oyster Bay: truth #19).

use rayon::prelude::*;

use super::{GlobalParams, RegionProposal};
use crate::geo::{Geodetic, Vec3, geodetic_to_ecef};
use crate::physics::cpa;
use crate::search::hypothesis::EventSpec;
use crate::search::pipeline::SearchParams;
use crate::search::score::{EventTarget, ScoringPlane, score_joint};
use crate::track::Track;

/// Ground ellipsoidal height at (lat, lon).
pub type Ground<'a> = &'a (dyn Fn(f64, f64) -> f64 + Sync);

/// Sets `quick` on the first `gp.rerank_max` proposals and re-sorts them by (events explained, quick).
pub(super) fn rerank(
    props: &mut [RegionProposal],
    tracks: &[Track],
    plane: &ScoringPlane,
    events: &[EventSpec],
    ground: Ground<'_>,
    sp: &SearchParams,
    gp: &GlobalParams,
) {
    let n = gp.rerank_max.min(props.len());
    let head = &mut props[..n];
    head.par_iter_mut().for_each(|p| p.quick = quick_score(p, tracks, plane, events, ground, sp, gp));
    let q = |p: &RegionProposal| p.quick.unwrap_or(f64::NEG_INFINITY);
    head.sort_by(|a, b| b.n_events.cmp(&a.n_events).then(q(b).total_cmp(&q(a))));
}

fn quick_score(
    p: &RegionProposal,
    tracks: &[Track],
    plane: &ScoringPlane,
    events: &[EventSpec],
    ground: Ground<'_>,
    sp: &SearchParams,
    gp: &GlobalParams,
) -> Option<f64> {
    let clock = 0.5 * (p.clock_range.0 + p.clock_range.1);
    let mut score = sp.score.clone();
    score.clock_sweep_s = score.clock_sweep_s.max(0.5 * (p.clock_range.1 - p.clock_range.0) + gp.clock_tol_s);
    let centre = [(p.region.lat_deg, p.region.lon_deg)];
    let points = if p.cells.is_empty() { &centre[..] } else { &p.cells[..] };
    points
        .iter()
        .filter_map(|&(lat, lon)| {
            let obs = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: ground(lat, lon) + sp.mic_height_m });
            let targets = best_targets(p, tracks, events, obs, clock, sp, gp)?;
            score_joint(plane, &targets, obs, &sp.atmosphere, clock, &score).map(|js| js.total)
        })
        .reduce(f64::max)
}

/// Per explained event, the supporting flight with the smallest geometry cost from `obs`; None when an
/// explained event has no flight inside the slack-widened bounds.
fn best_targets<'t>(
    p: &RegionProposal,
    tracks: &'t [Track],
    events: &[EventSpec],
    obs: Vec3,
    clock: f64,
    sp: &SearchParams,
    gp: &GlobalParams,
) -> Option<Vec<EventTarget<'t>>> {
    let t_tol = 0.5 * (p.clock_range.1 - p.clock_range.0) + gp.clock_tol_s + gp.rerank_t_slack_s;
    let mut out = Vec::new();
    for (e, ts) in events.iter().zip(&p.tracks).filter(|(_, ts)| !ts.is_empty()) {
        let t_tol = t_tol + e.t_cpa_halfwidth_s;
        let best = ts
            .iter()
            .filter_map(|&ti| {
                let tr = &tracks[ti as usize];
                let c = cpa(tr, obs, &sp.atmosphere)?;
                let (d_lo, d_hi) = e.d_range_at(c.ground_speed);
                let s = gp.rerank_d_slack;
                let dt = c.t_arrive - clock - e.t_cpa_audio;
                let ok = d_lo <= d_hi
                    && c.slant_m >= d_lo / s
                    && c.slant_m <= d_hi * s
                    && c.ground_speed >= e.v_range.0 / s
                    && c.ground_speed <= e.v_range.1 * s
                    && dt.abs() <= t_tol;
                let cost = (c.slant_m / (d_lo * d_hi).sqrt()).ln().powi(2) + (dt / t_tol).powi(2);
                ok.then_some((cost, tr))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        out.push(EventTarget { track: best.1, window: e.window, f0_range: e.f0_range });
    }
    (!out.is_empty()).then_some(out)
}
