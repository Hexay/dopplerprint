//! Coarse-to-fine search over an EPSG:5070 lattice. Each cell keeps its best hypothesis
//! (flights per event + shared clock) under locked joint scoring minus the silence penalty.
//! Per level: plan cells in parallel on the CPU → score every hypothesis in one batch (CPU or CUDA) → reduce.

use std::time::Instant;

use rayon::prelude::*;

use super::backend::{Backend, Scorer};
use super::catalog::{Catalog, Nearby};
use super::hypothesis::{AssocParams, EventSpec, Hypothesis, associate, cpa_reach_m};
use super::job::{Job, prepare_job};
use super::score::{EventScore, EventTarget, JointScore, ScoreParams};
use crate::dem::Dem;
use crate::geo::albers::{self, Cell};
use crate::geo::{Geodetic, Vec3, geodetic_to_ecef};
use crate::physics::Atmosphere;

/// Cells per scoring batch (one GPU launch); bounds the prepared Doppler offsets held at once.
const BATCH_CELLS: usize = 4096;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SearchParams {
    pub coarse_step_m: u32,
    pub min_step_m: u32,
    /// Cells carried into each refinement level.
    pub keep_top: usize,
    /// Catalogue radius around a cell (m); beyond this a flight is assumed inaudible.
    pub track_radius_m: f64,
    pub mic_height_m: f64,
    /// Ellipsoidal ground height used when no DEM is supplied.
    pub flat_h_ell_m: f64,
    pub atmosphere: Atmosphere,
    pub assoc: AssocParams,
    pub score: ScoreParams,
    pub backend: Backend,
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            coarse_step_m: 200,
            min_step_m: 25,
            keep_top: 200,
            track_radius_m: 10_000.0,
            mic_height_m: 1.5,
            flat_h_ell_m: 0.0,
            atmosphere: Atmosphere::default(),
            assoc: AssocParams::default(),
            score: ScoreParams::default(),
            backend: Backend::default(),
        }
    }
}

/// Circular search region.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub radius_m: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    pub cell: Cell,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub score: f64,
    pub raw: f64,
    pub clock: f64,
    pub track_ids: Vec<String>,
    pub events: Vec<EventScore>,
    pub unexplained: usize,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    /// Every scored cell at every level, best first.
    pub ranked: Vec<Candidate>,
    pub cells_scored: usize,
    pub hypotheses_scored: usize,
    /// Wall time per stage; not serialised, so ranking dumps stay comparable across runs.
    #[serde(skip)]
    pub timing: SearchTiming,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SearchTiming {
    /// near + CPA + association + Doppler offsets (CPU, parallel over cells).
    pub plan_s: f64,
    /// f0 sweep + assembly (CPU or CUDA).
    pub score_s: f64,
}

impl std::ops::AddAssign for SearchTiming {
    fn add_assign(&mut self, o: Self) {
        self.plan_s += o.plan_s;
        self.score_s += o.score_s;
    }
}

pub struct SearchInput<'a> {
    pub scorer: &'a Scorer<'a>,
    pub events: &'a [EventSpec],
    pub catalog: &'a dyn Catalog,
    pub region: Region,
    pub duration_s: f64,
    pub dem: Option<&'a Dem>,
}

pub fn search(inp: &SearchInput<'_>, p: &SearchParams) -> SearchResult {
    let mut level = coarse_cells(&inp.region, p.coarse_step_m);
    let mut all: Vec<Candidate> = Vec::new();
    let (mut hyps, mut cells) = (0usize, 0usize);
    let mut timing = SearchTiming::default();
    loop {
        cells += level.len();
        let scored = score_level(inp, p, &level, &mut timing);
        hyps += scored.iter().map(|s| s.1).sum::<usize>();
        let mut found: Vec<Candidate> = scored.into_iter().filter_map(|s| s.0).collect();
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        let step = level.first().map_or(0, |c| c.step_m);
        let next: Vec<Cell> = found.iter().take(p.keep_top).flat_map(|c| c.cell.children()).collect();
        all.extend(found);
        if step / 2 < p.min_step_m || next.is_empty() {
            all.sort_by(|a, b| b.score.total_cmp(&a.score));
            return SearchResult { ranked: all, cells_scored: cells, hypotheses_scored: hyps, timing };
        }
        level = next;
    }
}

pub fn coarse_cells(r: &Region, step: u32) -> Vec<Cell> {
    let c = albers::forward(r.lat_deg, r.lon_deg);
    let n = (r.radius_m / step as f64).ceil() as i32;
    let center = Cell::of(c, step);
    let mut out = Vec::new();
    for dy in -n..=n {
        for dx in -n..=n {
            let cell = Cell { ix: center.ix + dx, iy: center.iy + dy, step_m: step };
            let xy = cell.center();
            if (xy.x - c.x).hypot(xy.y - c.y) <= r.radius_m {
                out.push(cell);
            }
        }
    }
    out
}

pub fn cell_observer(cell: Cell, dem: Option<&Dem>, p: &SearchParams) -> (f64, f64, Vec3) {
    let (lat, lon) = albers::inverse(cell.center());
    let ground = dem.and_then(|d| d.height_ellipsoid_latlon(lat, lon)).unwrap_or(p.flat_h_ell_m);
    (lat, lon, geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: ground + p.mic_height_m }))
}

pub fn score_cell(inp: &SearchInput<'_>, p: &SearchParams, cell: Cell) -> (Option<Candidate>, usize) {
    score_cells(inp, p, &[cell]).pop().expect("one result per cell")
}

/// Best candidate per cell and the number of hypotheses scored there; all cells' hypotheses form one batch.
pub fn score_cells(inp: &SearchInput<'_>, p: &SearchParams, cells: &[Cell]) -> Vec<(Option<Candidate>, usize)> {
    let batch = plan_batch(inp, p, cells);
    score_planned(inp, p, batch)
}

/// `score_cells` over a level in BATCH_CELLS batches, planning batch i+1 on the CPU while batch i scores.
/// Timing fields are per-stage busy time, so plan_s + score_s can exceed the wall time.
fn score_level(
    inp: &SearchInput<'_>,
    p: &SearchParams,
    level: &[Cell],
    timing: &mut SearchTiming,
) -> Vec<(Option<Candidate>, usize)> {
    let mut out = Vec::with_capacity(level.len());
    let mut pending: Option<Batch<'_>> = None;
    for chunk in level.chunks(BATCH_CELLS).map(Some).chain([None]) {
        let ((next, plan_s), scored) = rayon::join(
            || {
                let t = Instant::now();
                (chunk.map(|c| plan_batch(inp, p, c)), t.elapsed().as_secs_f64())
            },
            || {
                pending.take().map(|b| {
                    let t = Instant::now();
                    (score_planned(inp, p, b), t.elapsed().as_secs_f64())
                })
            },
        );
        timing.plan_s += plan_s;
        if let Some((s, score_s)) = scored {
            timing.score_s += score_s;
            out.extend(s);
        }
        pending = next;
    }
    out
}

struct Batch<'c> {
    plans: Vec<CellPlan<'c>>,
    jobs: Vec<Job>,
}

fn plan_batch<'c>(inp: &SearchInput<'c>, p: &SearchParams, cells: &[Cell]) -> Batch<'c> {
    let mut plans: Vec<CellPlan<'_>> = cells.par_iter().map(|&c| plan_cell(inp, p, c)).collect();
    let jobs: Vec<Job> = plans.iter_mut().flat_map(|pl| std::mem::take(&mut pl.jobs)).collect();
    Batch { plans, jobs }
}

fn score_planned(inp: &SearchInput<'_>, p: &SearchParams, b: Batch<'_>) -> Vec<(Option<Candidate>, usize)> {
    let mut scores = inp.scorer.score(&b.jobs, &p.score).into_iter();
    b.plans
        .iter()
        .map(|pl| (best_candidate(pl, scores.by_ref().take(pl.hyps.len()), p), pl.hyps.len()))
        .collect()
}

struct CellPlan<'c> {
    cell: Cell,
    lat: f64,
    lon: f64,
    nearby: Vec<Nearby<'c>>,
    hyps: Vec<Hypothesis>,
    jobs: Vec<Job>,
}

fn plan_cell<'c>(inp: &SearchInput<'c>, p: &SearchParams, cell: Cell) -> CellPlan<'c> {
    let (lat, lon, obs) = cell_observer(cell, inp.dem, p);
    let (t0, t1) = p.assoc.t_start_prior;
    let reach = cpa_reach_m(inp.events, &p.assoc);
    // Tracks beyond `reach` get no CPA, so they cannot enter a hypothesis or the silence count.
    let nearby = inp.catalog.near(obs, p.track_radius_m.min(reach), t0, t1 + inp.duration_s);
    let cpas: Vec<_> = nearby.iter().map(|n| n.cpa(obs, &p.atmosphere, reach)).collect();
    let hyps = associate(inp.events, &cpas, inp.duration_s, &p.assoc);
    let spec = &inp.scorer.plane().spec;
    let jobs = hyps
        .iter()
        .map(|h| {
            let targets: Vec<EventTarget<'_>> = inp
                .events
                .iter()
                .zip(&h.tracks)
                .map(|(e, &ti)| EventTarget { track: nearby[ti].track, window: e.window, f0_range: e.f0_range })
                .collect();
            prepare_job(spec, &targets, obs, &p.atmosphere, h.clock, &p.score)
        })
        .collect();
    CellPlan { cell, lat, lon, nearby, hyps, jobs }
}

fn best_candidate(
    pl: &CellPlan<'_>,
    scores: impl Iterator<Item = Option<JointScore>>,
    p: &SearchParams,
) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for (h, js) in pl.hyps.iter().zip(scores) {
        let Some(js) = js else { continue };
        let score = js.total - p.assoc.silence_penalty * h.unexplained as f64;
        if best.as_ref().is_none_or(|b| score > b.score) {
            best = Some(Candidate {
                cell: pl.cell,
                lat_deg: pl.lat,
                lon_deg: pl.lon,
                score,
                raw: js.total,
                clock: js.clock_offset,
                track_ids: h.tracks.iter().map(|&i| pl.nearby[i].track.id.clone()).collect(),
                events: js.events,
                unexplained: h.unexplained,
            });
        }
    }
    best
}
