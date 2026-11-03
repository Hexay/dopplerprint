//! Truth-referenced measurement of one search: stage-wise recall of the truth and continuous scores
//! (score gap, truth-cell rank, log score, 90% region coverage) from a tempered posterior over coarse cells.
//! Posterior: p(cell) = (1−ε)·softmax(β·s) + ε/N, s = best score of the cell or any descendant;
//! see docs/research/roadmap_experiments_2026-11-03.md for why β and ε are fixed, not fitted.

use std::collections::HashMap;

use crate::geo::albers::{self, Cell};
use crate::geo::{Geodetic, Vec3, geodetic_to_ecef};
use crate::search::hypothesis::{associate, clock_window, cpa_reach_m};
use crate::search::pipeline::{Candidate, SearchInput, SearchParams, SearchResult, coarse_cells, score_cell};
use crate::track::Track;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Stage {
    pub stage: String,
    pub ok: bool,
    pub rank: Option<usize>,
    pub of: Option<usize>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    pub stages: Vec<Stage>,
    /// First stage the truth did not survive.
    pub lost_at: Option<String>,
    /// Best hypothesis score in the finest cell containing the truth.
    pub truth_score: Option<f64>,
    /// Best score minus truth score: > 0 the model prefers elsewhere, < 0 the search missed the truth.
    pub score_gap: Option<f64>,
    pub truth_cell_rank: Option<usize>,
    /// ln posterior mass of the coarse cell containing the truth (nats).
    pub log_score: Option<f64>,
    pub hpd90_contains: Option<bool>,
    pub hpd90_km2: Option<f64>,
}

pub struct Truth<'a> {
    pub lat: f64,
    pub lon: f64,
    /// Ids per event when every event's flight is known.
    pub flights: Option<&'a [String]>,
}

/// Posterior temperature β and uniform floor ε.
#[derive(Clone, Copy, Debug)]
pub struct PosteriorParams {
    pub beta: f64,
    pub floor: f64,
}

/// `ps` are the searched parameter variants (one unless a nuisance grid ran); they differ only in
/// atmosphere, so the lattice comes from `ps[0]` and truth-side stages pass if any variant passes.
pub fn measure(inp: &SearchInput<'_>, ps: &[SearchParams], res: &SearchResult, t: &Truth<'_>, pp: PosteriorParams) -> Metrics {
    let p = &ps[0];
    let mut stages = flight_stages(inp, ps, t);
    stages.extend(level_stages(p, res, t));
    let lost_at = stages.iter().find(|s| !s.ok).map(|s| s.stage.clone());
    let truth_cell = Cell::of(albers::forward(t.lat, t.lon), finest_step(p));
    let truth_score =
        ps.iter().filter_map(|q| score_cell(inp, q, truth_cell).0.map(|c| c.score)).max_by(f64::total_cmp);
    let best = res.ranked.first().map(|c| c.score);
    let mut m = Metrics {
        stages,
        lost_at,
        truth_score,
        score_gap: best.zip(truth_score).map(|(b, s)| b - s),
        ..Default::default()
    };
    posterior(inp, p, &res.ranked, t, pp, res.abstains(), &mut m);
    m
}

pub fn same_flight(id: &str, truth: &str) -> bool {
    let icao = id.rsplit('/').next().unwrap_or(id).split('-').next().unwrap_or(id);
    id == truth || icao.eq_ignore_ascii_case(truth)
}

pub fn finest_step(p: &SearchParams) -> u32 {
    let mut s = p.coarse_step_m;
    while s / 2 >= p.min_step_m && s > 1 {
        s /= 2;
    }
    s
}

fn truth_observer(inp: &SearchInput<'_>, p: &SearchParams, t: &Truth<'_>) -> Vec3 {
    let ground = inp.dem.and_then(|d| d.height_ellipsoid_latlon(t.lat, t.lon)).unwrap_or(p.flat_h_ell_m);
    geodetic_to_ecef(Geodetic { lat_deg: t.lat, lon_deg: t.lon, h_ell_m: ground + p.mic_height_m })
}

fn stage(name: &str, ok: bool, rank: Option<usize>, of: Option<usize>) -> Stage {
    Stage { stage: name.into(), ok, rank, of }
}

/// catalog → near → assoc, evaluated at the exact truth position.
fn flight_stages(inp: &SearchInput<'_>, ps: &[SearchParams], t: &Truth<'_>) -> Vec<Stage> {
    let Some(truth) = t.flights else { return Vec::new() };
    let p = &ps[0];
    let has_all = |tracks: &[&Track]| truth.iter().all(|f| tracks.iter().any(|tr| same_flight(&tr.id, f)));
    let all: Vec<&Track> = inp.catalog.all().iter().collect();
    let obs = truth_observer(inp, p, t);
    let (t0, t1) = clock_window(inp.events, &p.assoc, 0.0);
    let nearby = inp.catalog.near(obs, p.track_radius_m, t0, t1 + inp.duration_s);
    let near: Vec<&Track> = nearby.iter().map(|n| n.track).collect();
    let (mut assoc_ok, mut n_hyps) = (false, 0);
    for q in ps {
        let cpas: Vec<_> = nearby.iter().map(|n| n.cpa(obs, &q.atmosphere, cpa_reach_m(inp.events, &q.assoc, 0.0))).collect();
        let hyps = associate(inp.events, &cpas, inp.duration_s, &q.assoc, 0.0);
        assoc_ok |= hyps.iter().any(|h| h.tracks.iter().zip(truth).all(|(i, f)| i.is_some_and(|i| same_flight(&near[i].id, f))));
        n_hyps = n_hyps.max(hyps.len());
    }
    vec![
        stage("catalog", has_all(&all), None, Some(all.len())),
        stage("near", has_all(&near), None, Some(near.len())),
        stage("assoc", assoc_ok, None, Some(n_hyps)),
    ]
}

/// One stage per lattice level: was the cell containing the truth scored, and kept for refinement.
fn level_stages(p: &SearchParams, res: &SearchResult, t: &Truth<'_>) -> Vec<Stage> {
    let xy = albers::forward(t.lat, t.lon);
    let finest = finest_step(p);
    let mut out = Vec::new();
    let mut step = p.coarse_step_m;
    loop {
        let level: Vec<&Candidate> = res.ranked.iter().filter(|c| c.cell.step_m == step).collect();
        let target = Cell::of(xy, step);
        let rank = level.iter().position(|c| c.cell == target).map(|i| i + 1);
        // Kept = refined, i.e. a child was scored next level; exact for merged nuisance runs too.
        let kids = target.children();
        let refined = || res.ranked.iter().any(|c| kids.contains(&c.cell));
        let ok = rank.is_some() && (step == finest || refined());
        out.push(stage(&format!("L{step}"), ok, rank, Some(level.len())));
        if step == finest {
            return out;
        }
        step /= 2;
    }
}

/// An abstained search gets the uniform posterior's log score and no region.
fn posterior(inp: &SearchInput<'_>, p: &SearchParams, ranked: &[Candidate], t: &Truth<'_>, pp: PosteriorParams, abstained: bool, m: &mut Metrics) {
    let step = p.coarse_step_m;
    let cells = coarse_cells(&inp.region, step);
    let truth = Cell::of(albers::forward(t.lat, t.lon), step);
    if cells.is_empty() || !cells.contains(&truth) {
        return;
    }
    if abstained {
        m.log_score = Some(-(cells.len() as f64).ln());
        return;
    }
    let mut best: HashMap<Cell, f64> = HashMap::new();
    for c in ranked {
        let a = Cell::of(c.cell.center(), step);
        let e = best.entry(a).or_insert(f64::NEG_INFINITY);
        *e = e.max(c.score);
    }
    let mass = cell_mass(&cells, &best, pp);
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by(|&a, &b| mass[b].total_cmp(&mass[a]));
    let ti = cells.iter().position(|c| *c == truth).unwrap();
    let rank = order.iter().position(|&i| i == ti).unwrap() + 1;
    let (mut acc, mut n90) = (0.0, 0usize);
    for &i in &order {
        if acc >= 0.9 {
            break;
        }
        acc += mass[i];
        n90 += 1;
    }
    m.truth_cell_rank = Some(rank);
    m.log_score = Some(mass[ti].ln());
    m.hpd90_contains = Some(rank <= n90);
    m.hpd90_km2 = Some(n90 as f64 * (step as f64 / 1000.0).powi(2));
}

fn cell_mass(cells: &[Cell], best: &HashMap<Cell, f64>, pp: PosteriorParams) -> Vec<f64> {
    let logits: Vec<f64> = cells.iter().map(|c| best.get(c).map_or(f64::NEG_INFINITY, |s| pp.beta * s)).collect();
    let mx = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let n = cells.len() as f64;
    if !mx.is_finite() {
        return vec![1.0 / n; cells.len()];
    }
    let w: Vec<f64> = logits.iter().map(|l| (l - mx).exp()).collect();
    let z: f64 = w.iter().sum();
    w.iter().map(|x| (1.0 - pp.floor) * x / z + pp.floor / n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_is_normalised_floored_and_finest_step_follows_halving() {
        let cells: Vec<Cell> = (0..4).map(|i| Cell { ix: i, iy: 0, step_m: 200 }).collect();
        let best: HashMap<Cell, f64> = [(cells[0], 2.0), (cells[1], 1.0)].into();
        let m = cell_mass(&cells, &best, PosteriorParams { beta: 1.0, floor: 0.04 });
        assert!((m.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(m[0] > m[1] && m[1] > m[2] && (m[2] - 0.01).abs() < 1e-12 && m[2] == m[3]);
        let p = |c, m| SearchParams { coarse_step_m: c, min_step_m: m, ..Default::default() };
        assert_eq!((finest_step(&p(200, 25)), finest_step(&p(200, 30)), finest_step(&p(200, 200))), (25, 50, 200));
    }
}
