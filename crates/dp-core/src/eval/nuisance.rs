//! Shared-nuisance profile over the model atmosphere: one search per (c, horizontal wind) grid point,
//! merged into one ranking where each cell keeps its best value. The value is shared by every event
//! of the recording, so on multi-event cases the grid point is constrained jointly (roadmap #4).
//! Eval-level prototype: K× the search cost; the search-internal version belongs in search::score.

use std::collections::HashSet;

use super::case::Posed;
use crate::config::EvalParams;
use crate::geo::east_north;
use crate::physics::Atmosphere;
use crate::search::{SearchParams, SearchResult};

/// One `SearchParams` per grid point; just `base` when no grid is configured.
pub fn variants(base: &SearchParams, c: &Posed, e: &EvalParams) -> Vec<SearchParams> {
    let cs: Vec<f64> = if e.nuisance_c.is_empty() { vec![base.atmosphere.c] } else { e.nuisance_c.clone() };
    let ws: Vec<[f64; 2]> = if e.nuisance_wind_en.is_empty() { vec![[0.0, 0.0]] } else { e.nuisance_wind_en.clone() };
    let (east, north) = east_north(c.truth_lat, c.truth_lon);
    let mut out = Vec::with_capacity(cs.len() * ws.len());
    for &sound in &cs {
        for &[we, wn] in &ws {
            let mut p = base.clone();
            p.atmosphere = Atmosphere { c: sound, wind: base.atmosphere.wind + east * we + north * wn };
            out.push(p);
        }
    }
    out
}

/// Profile merge: all candidates, best first, one entry per cell. Returns the index of the variant
/// that produced the overall best candidate.
pub fn merge(runs: Vec<SearchResult>) -> (SearchResult, usize) {
    let best_run = runs
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.ranked.first().map(|c| (i, c.score)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |x| x.0);
    let mut out = SearchResult::default();
    for r in runs {
        out.cells_scored += r.cells_scored;
        out.hypotheses_scored += r.hypotheses_scored;
        out.ranked.extend(r.ranked);
    }
    out.ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen = HashSet::new();
    out.ranked.retain(|c| seen.insert(c.cell));
    (out, best_run)
}
