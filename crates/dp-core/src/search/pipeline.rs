//! Coarse-to-fine search over an EPSG:5070 lattice. Each cell keeps its best hypothesis
//! (flights per event + shared clock) under locked joint scoring minus the silence penalty.

use rayon::prelude::*;

use super::catalog::Catalog;
use super::hypothesis::{AssocParams, EventSpec, associate};
use super::score::{EventScore, EventTarget, ScoreParams, ScoringPlane, score_joint};
use crate::dem::Dem;
use crate::geo::albers::{self, Cell};
use crate::geo::{Geodetic, Vec3, geodetic_to_ecef};
use crate::physics::Atmosphere;

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
}

pub struct SearchInput<'a> {
    pub plane: &'a ScoringPlane,
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
    loop {
        cells += level.len();
        let scored: Vec<(Option<Candidate>, usize)> = level.par_iter().map(|c| score_cell(inp, p, *c)).collect();
        hyps += scored.iter().map(|s| s.1).sum::<usize>();
        let mut found: Vec<Candidate> = scored.into_iter().filter_map(|s| s.0).collect();
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        let step = level.first().map_or(0, |c| c.step_m);
        let next: Vec<Cell> = found.iter().take(p.keep_top).flat_map(|c| c.cell.children()).collect();
        all.extend(found);
        if step / 2 < p.min_step_m || next.is_empty() {
            all.sort_by(|a, b| b.score.total_cmp(&a.score));
            return SearchResult { ranked: all, cells_scored: cells, hypotheses_scored: hyps };
        }
        level = next;
    }
}

fn coarse_cells(r: &Region, step: u32) -> Vec<Cell> {
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

fn score_cell(inp: &SearchInput<'_>, p: &SearchParams, cell: Cell) -> (Option<Candidate>, usize) {
    let (lat, lon, obs) = cell_observer(cell, inp.dem, p);
    let (t0, t1) = p.assoc.t_start_prior;
    let nearby = inp.catalog.near(obs, p.track_radius_m, t0, t1 + inp.duration_s);
    let hyps = associate(inp.events, &nearby, obs, &p.atmosphere, inp.duration_s, &p.assoc);
    let mut best: Option<Candidate> = None;
    for h in &hyps {
        let targets: Vec<EventTarget<'_>> = inp
            .events
            .iter()
            .zip(&h.tracks)
            .map(|(e, &ti)| EventTarget { track: nearby[ti], window: e.window, f0_range: e.f0_range })
            .collect();
        let Some(js) = score_joint(inp.plane, &targets, obs, &p.atmosphere, h.clock, &p.score) else { continue };
        let score = js.total - p.assoc.silence_penalty * h.unexplained as f64;
        if best.as_ref().is_none_or(|b| score > b.score) {
            best = Some(Candidate {
                cell,
                lat_deg: lat,
                lon_deg: lon,
                score,
                raw: js.total,
                clock: js.clock_offset,
                track_ids: h.tracks.iter().map(|&i| nearby[i].id.clone()).collect(),
                events: js.events,
                unexplained: h.unexplained,
            });
        }
    }
    (best, hyps.len())
}
