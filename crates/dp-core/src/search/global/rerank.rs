//! Cheap spectrogram rerank of proposals: one joint score of each proposal's supporting flights from a
//! 3×3 grid around its centre. Geometry alone cannot separate thousands of chance two-event coincidences.

use rayon::prelude::*;

use super::{GlobalParams, RegionProposal};
use crate::geo::{Geodetic, east_north, geodetic_to_ecef};
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
    let targets: Vec<EventTarget<'_>> = events
        .iter()
        .zip(&p.tracks)
        .filter_map(|(e, ts)| ts.first().map(|&t| EventTarget { track: &tracks[t as usize], window: e.window, f0_range: e.f0_range }))
        .collect();
    if targets.is_empty() {
        return None;
    }
    let (lat, lon) = (p.region.lat_deg, p.region.lon_deg);
    let (east, north) = east_north(lat, lon);
    let centre = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: ground(lat, lon) + sp.mic_height_m });
    let clock = 0.5 * (p.clock_range.0 + p.clock_range.1);
    let mut score = sp.score.clone();
    score.clock_sweep_s = score.clock_sweep_s.max(0.5 * (p.clock_range.1 - p.clock_range.0) + gp.clock_tol_s);
    let o = gp.rerank_offset_m;
    let mut best: Option<f64> = None;
    for dx in [-o, 0.0, o] {
        for dy in [-o, 0.0, o] {
            let obs = centre + east * dx + north * dy;
            if let Some(js) = score_joint(plane, &targets, obs, &sp.atmosphere, clock, &score) {
                best = Some(best.map_or(js.total, |b: f64| b.max(js.total)));
            }
        }
    }
    best
}
