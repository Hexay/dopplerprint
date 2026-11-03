//! Corpus + sim evaluation: pose each case, search, and measure against truth.
//! Output: runs/<stamp>/summary.{md,json} and one <case>.json per case.

pub mod aggregate;
pub mod atmosphere;
pub mod case;
pub mod calib_report;
pub mod calibrate;
pub mod compare;
pub mod conformal;
pub mod coverage;
pub mod global;
pub mod livermore;
pub mod metrics;
pub mod nuisance;
pub mod report;
pub mod split;
pub mod stats;

use std::path::Path;
use std::time::Instant;

use anyhow::Result;

use crate::config::Config;
use crate::dem::geoid_undulation;
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::physics::Atmosphere;
use crate::search::pipeline::Candidate;
use crate::audio::spectrogram::log_spectrogram;
use crate::search::score::ScoringPlane;
use crate::search::surface::Surface;
use coverage::Coverage;
use crate::search::{MemCatalog, Region, Scorer, SearchInput, search};
use metrics::{Metrics, PosteriorParams, Truth, same_flight};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CaseResult {
    pub id: String,
    pub status: String,
    pub error_m: Option<f64>,
    pub flights_correct: Option<bool>,
    /// 1-based rank of the truth flight tuple among per-tuple best cells.
    pub truth_tuple_rank: Option<usize>,
    /// Best score minus the best score of any other flight tuple (z units).
    pub margin: Option<f64>,
    pub clock_err_s: Option<f64>,
    pub n_tracks: usize,
    pub n_events: usize,
    pub cells: usize,
    pub hypotheses: usize,
    pub runtime_s: f64,
    /// Where the f0 sweep ran ("cpu" / "cuda (<device>)"); scores are identical, runtimes are not.
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub timing: CaseTiming,
    #[serde(default)]
    pub metrics: Metrics,
    /// Model atmosphere of the best candidate (the chosen grid point when a nuisance grid ran).
    #[serde(default)]
    pub atmosphere: Option<Atmosphere>,
    #[serde(default)]
    pub truth_lat_lon: Option<(f64, f64)>,
    /// At the shipped calibration (`search::calibration`).
    #[serde(default)]
    pub coverage: Option<Coverage>,
    /// Cached refined surface, so `calibrate` re-tempers without re-searching.
    #[serde(default)]
    pub surface: Option<Surface>,
    /// Where the atmosphere came from when `eval.weather` is set (see `eval::atmosphere::sourced`).
    #[serde(default)]
    pub weather: Option<String>,
    pub best: Option<Candidate>,
    pub tuples: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<global::GlobalCaseStats>,
}

/// Wall seconds per stage of one case (search stages summed over nuisance variants).
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CaseTiming {
    /// Decode, ADS-B load, DEM.
    pub pose_s: f64,
    /// Spectrogram, scoring plane, device upload, catalogue index.
    pub plane_s: f64,
    pub plan_s: f64,
    pub score_s: f64,
    pub metrics_s: f64,
}

pub fn run(cfg: &Config, sim_dir: &Path, out_dir: &Path) -> Result<Vec<CaseResult>> {
    std::fs::create_dir_all(out_dir)?;
    let mut results = Vec::new();
    for src in case::list(&cfg.eval, sim_dir)? {
        let t = Instant::now();
        let mut r = match case::pose(&src, &cfg.eval) {
            Ok(posed) => {
                let pose_s = t.elapsed().as_secs_f64();
                let mut r = run_case(cfg, &posed);
                r.timing.pose_s = pose_s;
                r
            }
            Err(e) => CaseResult { id: src.id(), status: format!("skipped: {e:#}"), ..Default::default() },
        };
        r.runtime_s = t.elapsed().as_secs_f64();
        eprintln!("{:<34} {}", r.id, report::one_line(&r));
        std::fs::write(out_dir.join(format!("{}.json", r.id)), serde_json::to_vec_pretty(&r)?)?;
        results.push(r);
    }
    report::write(out_dir, cfg, &results)?;
    Ok(results)
}

fn run_case(cfg: &Config, c: &case::Posed) -> CaseResult {
    if cfg.eval.region_mode == crate::config::RegionMode::Global {
        return global::run_case(cfg, c);
    }
    let mut p = cfg.search.clone();
    let e = &cfg.eval;
    p.assoc.t_start_prior = (c.t_start_true - e.time_prior_s, c.t_start_true + e.time_prior_s);
    p.flat_h_ell_m = c.truth_h_ell.unwrap_or_else(|| geoid_undulation(c.truth_lat, c.truth_lon));
    let region = displaced_region(c, e.region_offset_m, e.region_radius_m);
    let (base, weather) = atmosphere::sourced(p.atmosphere, c, e, &region);
    p.atmosphere = atmosphere::case_atmosphere(base, c, e);
    let variants = nuisance::variants(&p, c, e);
    // Not search::locate: the truth metrics need the plane and catalogue the search used.
    let t_plane = Instant::now();
    let plane = ScoringPlane::new(&log_spectrogram(&c.audio, &cfg.spec), &p.score);
    let scorer = match Scorer::new(&plane, p.backend) {
        Ok(s) => s,
        Err(e) => return CaseResult { id: c.id.clone(), status: format!("skipped: {e:#}"), ..Default::default() },
    };
    let catalog = MemCatalog::new(c.tracks.clone());
    let inp = SearchInput {
        scorer: &scorer,
        events: &c.events,
        catalog: &catalog,
        region,
        duration_s: c.audio.duration_s(),
        dem: c.dem.as_ref(),
    };
    let plane_s = t_plane.elapsed().as_secs_f64();
    let (res, chosen) = nuisance::merge(variants.iter().map(|q| search(&inp, q)).collect());
    let surface = Surface::new(&res.ranked, &inp.region, p.coarse_step_m);
    let tuples = best_per_tuple(&res.ranked);
    let best = tuples.first().cloned();
    let truth_ids: Option<Vec<String>> = c.truth_flights.iter().cloned().collect();
    let is_truth = |cand: &Candidate| truth_ids.as_ref().is_some_and(|t| same_flights(&cand.track_ids, t));
    let truth = Truth { lat: c.truth_lat, lon: c.truth_lon, flights: truth_ids.as_deref() };
    let pp = PosteriorParams { beta: e.posterior_beta, floor: e.posterior_floor };
    let t_metrics = Instant::now();
    let metrics = metrics::measure(&inp, &variants, &res, &truth, pp);
    let timing = CaseTiming {
        plane_s,
        plan_s: res.timing.plan_s,
        score_s: res.timing.score_s,
        metrics_s: t_metrics.elapsed().as_secs_f64(),
        ..Default::default()
    };
    CaseResult {
        id: c.id.clone(),
        status: "ok".into(),
        error_m: best.as_ref().map(|b| distance_m(b.lat_deg, b.lon_deg, c.truth_lat, c.truth_lon)),
        flights_correct: truth_ids.as_ref().and(best.as_ref()).map(is_truth),
        truth_tuple_rank: tuples.iter().position(&is_truth).map(|i| i + 1),
        margin: match tuples.as_slice() {
            [a, b, ..] => Some(a.score - b.score),
            _ => None,
        },
        clock_err_s: best.as_ref().map(|b| b.clock - c.t_start_true),
        n_tracks: c.tracks.len(),
        n_events: c.events.len(),
        cells: res.cells_scored,
        hypotheses: res.hypotheses_scored,
        runtime_s: 0.0,
        device: scorer.device(),
        timing,
        metrics,
        atmosphere: Some(variants[chosen].atmosphere),
        truth_lat_lon: Some((c.truth_lat, c.truth_lon)),
        coverage: Some(coverage::measure(&surface, c.truth_lat, c.truth_lon, coverage::Shape::shipped(), &coverage::shipped_masses(c.events.len()))),
        surface: Some(surface),
        weather,
        best,
        tuples: tuples.into_iter().take(10).collect(),
        global: None,
    }
}

/// Deterministic per-case displacement so truth never sits at the region centre.
fn displaced_region(c: &case::Posed, offset_m: f64, radius_m: f64) -> Region {
    let h = c.id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |a, b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let bearing = (h % 3600) as f64 / 10.0;
    let (dn, de) = (offset_m * bearing.to_radians().cos(), offset_m * bearing.to_radians().sin());
    Region {
        lat_deg: c.truth_lat + dn / 111_320.0,
        lon_deg: c.truth_lon + de / (111_320.0 * c.truth_lat.to_radians().cos()),
        radius_m,
    }
}

fn best_per_tuple(ranked: &[Candidate]) -> Vec<Candidate> {
    let mut seen = std::collections::HashSet::new();
    ranked.iter().filter(|c| seen.insert(c.track_ids.clone())).cloned().collect()
}

/// Store ids look like "2026-09-08/a1dfeb-3"; sim ids are compared verbatim.
fn same_flights(ids: &[String], truth: &[String]) -> bool {
    ids.len() == truth.len() && ids.iter().zip(truth).all(|(id, t)| same_flight(id, t))
}

pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let a = geodetic_to_ecef(Geodetic { lat_deg: lat1, lon_deg: lon1, h_ell_m: 0.0 });
    let b = geodetic_to_ecef(Geodetic { lat_deg: lat2, lon_deg: lon2, h_ell_m: 0.0 });
    (a - b).norm()
}
