//! `[eval] region_mode = "global"`: pose a case with no region and score `search::global::locate_global`.
//! Sim tracks are injected into the last store day at the same UTC time of day (explicit day shift), so
//! the national traffic is real and the truth flights are the sim's.

use std::path::Path;

use super::case::Posed;
use super::{CaseResult, distance_m, same_flights};
use crate::adsb::manifest::list_days;
use crate::audio::spectrogram::log_spectrogram;
use crate::config::Config;
use crate::dem::Dem;
use crate::events::{DiscoverConfig, discover_audio};
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::search::EventSpec;
use crate::search::global::{GlobalInput, GlobalResult, RegionProposal, StoreSource, locate_global};
use crate::search::Scorer;
use crate::search::score::ScoringPlane;
use crate::track::Track;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct GlobalCaseStats {
    /// 1-based rank of the first proposal whose region contains the truth.
    pub truth_proposal_rank: Option<usize>,
    /// 1-based rank of the first proposal whose supporting flights include every truth flight.
    pub truth_flights_proposal_rank: Option<usize>,
    /// 1-based rank, after regional scoring, of the region whose proposal contains the truth.
    pub truth_region_rank: Option<usize>,
    pub n_proposals: usize,
    pub n_max: usize,
    pub votes: usize,
    pub window_tracks: usize,
    pub vote_cell_m: f64,
    pub load_s: f64,
    pub propose_s: f64,
    pub rerank_s: f64,
    pub locate_s: f64,
    pub ambiguous: bool,
    pub day_shift_s: f64,
    /// Event bounds actually used (after the optional CV refit).
    pub events: Vec<EventSpec>,
    pub top: Vec<RegionProposal>,
    /// First proposal containing the truth.
    pub truth: Option<RegionProposal>,
}

pub(super) fn run_case(cfg: &Config, c: &Posed) -> CaseResult {
    match try_run(cfg, c) {
        Ok(r) => r,
        Err(e) => CaseResult { id: c.id.clone(), status: format!("global failed: {e:#}"), ..Default::default() },
    }
}

fn try_run(cfg: &Config, c: &Posed) -> anyhow::Result<CaseResult> {
    let e = &cfg.eval;
    let sim = c.id.starts_with("sim_");
    let shift = if sim { day_shift(&e.store_dir, c.t_start_true)? } else { 0.0 };
    let extra: Vec<Track> = if sim { c.tracks.iter().map(|t| shifted(t, shift)).collect() } else { Vec::new() };
    let t0 = c.t_start_true + shift;
    let events = match (sim, e.global_truth_bounds, e.global_fit_events) {
        (false, true, _) => truth_bounds(c),
        (false, false, true) => refit(c),
        _ => c.events.clone(),
    };
    let plane = ScoringPlane::new(&log_spectrogram(&c.audio, &cfg.spec), &cfg.search.score);
    let scorer = Scorer::new(&plane, cfg.search.backend)?;
    let source = StoreSource { store: e.store_dir.clone(), extra };
    // Sims are rendered over flat truth ground; real cases get the national DEM when it has been built.
    let national_path = e.data_dir.join("dem/5070/conus_1000m.u16");
    let national = (!sim && national_path.exists()).then(|| Dem::open(&national_path)).transpose()?;
    let inp = GlobalInput {
        scorer: &scorer,
        events: &events,
        source: &source,
        prior: (t0 - e.time_prior_s, t0 + e.time_prior_s),
        duration_s: c.audio.duration_s(),
        dem: c.dem.as_ref(),
        national: national.as_ref(),
        flat_h_ell_m: c.truth_h_ell,
    };
    let mut sp = cfg.search.clone();
    sp.assoc.silence.measure_recording(&c.audio, &cfg.spec);
    let res = locate_global(&inp, &sp, &cfg.global)?;
    Ok(result(c, t0, shift, events, res))
}

fn result(c: &Posed, t0: f64, shift: f64, events: Vec<EventSpec>, res: GlobalResult) -> CaseResult {
    let truth: Option<Vec<String>> = c.truth_flights.iter().cloned().collect();
    // Per-event ids follow the events the global search kept.
    let truth = truth.map(|t| res.events_used.iter().filter_map(|&i| t.get(i).cloned()).collect::<Vec<_>>());
    let events: Vec<EventSpec> = res.events_used.iter().map(|&i| events[i].clone()).collect();
    let inside = |p: &RegionProposal| distance_m(p.region.lat_deg, p.region.lon_deg, c.truth_lat, c.truth_lon) <= p.region.radius_m;
    let has_flights = |p: &RegionProposal| {
        truth.as_ref().is_some_and(|t| {
            p.flights.len() == t.len() && p.flights.iter().zip(t).all(|(ids, f)| ids.iter().any(|id| same_flights(std::slice::from_ref(id), std::slice::from_ref(f))))
        })
    };
    let best = res.best.clone();
    let stats = GlobalCaseStats {
        truth_proposal_rank: res.proposals.iter().position(inside).map(|i| i + 1),
        truth_flights_proposal_rank: res.proposals.iter().position(has_flights).map(|i| i + 1),
        truth_region_rank: res.regions.iter().position(|r| inside(&res.proposals[r.proposal])).map(|i| i + 1),
        n_proposals: res.stats.n_proposals,
        n_max: res.stats.n_max,
        votes: res.stats.votes,
        window_tracks: res.stats.tracks,
        vote_cell_m: res.stats.vote_cell_m,
        load_s: res.timings.load_s,
        propose_s: res.timings.propose_s,
        rerank_s: res.timings.rerank_s,
        locate_s: res.timings.locate_s,
        ambiguous: res.ambiguous,
        day_shift_s: shift,
        events,
        top: res.proposals.iter().take(10).cloned().collect(),
        truth: res.proposals.iter().find(|p| inside(p)).cloned(),
    };
    eprintln!(
        "{:<34} global: truth proposal #{:?} (flights #{:?}) of {}, region #{:?}, n_max {}, ambiguous {}",
        c.id, stats.truth_proposal_rank, stats.truth_flights_proposal_rank, stats.n_proposals, stats.truth_region_rank, stats.n_max, stats.ambiguous
    );
    CaseResult {
        id: c.id.clone(),
        status: "ok".into(),
        error_m: best.as_ref().map(|b| distance_m(b.lat_deg, b.lon_deg, c.truth_lat, c.truth_lon)),
        flights_correct: truth.as_ref().and(best.as_ref()).map(|b| truth.as_ref().is_some_and(|t| same_flights(&b.track_ids, t))),
        truth_tuple_rank: stats.truth_region_rank,
        margin: res.margin,
        clock_err_s: best.as_ref().map(|b| b.clock - t0),
        n_tracks: res.stats.tracks,
        n_events: c.events.len(),
        cells: res.regions.iter().map(|r| r.cells).sum(),
        hypotheses: res.regions.iter().map(|r| r.hypotheses).sum(),
        runtime_s: 0.0,
        best,
        tuples: res.regions.iter().filter_map(|r| r.best.clone()).take(10).collect(),
        global: Some(stats),
        truth_lat_lon: Some((c.truth_lat, c.truth_lon)),
        ..Default::default()
    }
}

/// Whole days that move `t` onto the last store day at the same UTC time of day.
fn day_shift(store: &Path, t: f64) -> anyhow::Result<f64> {
    let days = list_days(store)?;
    let (_, mf) = days.last().ok_or_else(|| anyhow::anyhow!("empty store {}", store.display()))?;
    Ok(mf.day0 - (t / 86_400.0).floor() * 86_400.0)
}

fn shifted(t: &Track, dt: f64) -> Track {
    let mut t = t.clone();
    t.points.iter_mut().for_each(|p| p.t += dt);
    t
}

/// Oracle events with `pose_sim`'s bound widths around the truth flight's slant range and speed at its
/// closest approach (±120 s of the oracle CPA). Events without a truth flight keep their oracle bounds.
fn truth_bounds(c: &Posed) -> Vec<EventSpec> {
    let h = c.truth_h_ell.or_else(|| c.dem.as_ref()?.height_ellipsoid_latlon(c.truth_lat, c.truth_lon)).unwrap_or(0.0);
    let obs = geodetic_to_ecef(Geodetic { lat_deg: c.truth_lat, lon_deg: c.truth_lon, h_ell_m: h });
    let t_cpa = |o: &EventSpec| c.t_start_true + o.t_cpa_audio;
    c.events
        .iter()
        .zip(&c.truth_flights)
        .map(|(o, f)| {
            let cpa = f.as_ref().and_then(|f| {
                c.tracks
                    .iter()
                    .filter(|t| format!("{:06x}", t.icao24) == *f)
                    .flat_map(|t| &t.points)
                    .filter(|p| (p.t - t_cpa(o)).abs() <= 120.0)
                    .map(|p| ((p.pos - obs).norm(), p.vel.norm()))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            });
            eprintln!("{} truth bounds: flight {f:?} cpa (d m, v m/s) {cpa:?}", c.id);
            match cpa {
                Some((d, v)) => EventSpec { d_range: (d * 0.6, d * 1.6), v_range: (v * 0.75, v * 1.3), ..o.clone() },
                None => o.clone(),
            }
        })
        .collect()
}

/// Oracle events with d/v bounds from the discovered event whose CPA is nearest (within 15 s).
fn refit(c: &Posed) -> Vec<EventSpec> {
    let found = discover_audio(&c.audio, &DiscoverConfig::default());
    c.events
        .iter()
        .map(|o| {
            let near = found
                .iter()
                .filter(|f| (f.fit.best.t_cpa_arrive - o.t_cpa_audio).abs() <= 15.0)
                .min_by(|a, b| {
                    let d = |f: &&crate::events::Event| (f.fit.best.t_cpa_arrive - o.t_cpa_audio).abs();
                    d(a).total_cmp(&d(b))
                });
            match near {
                Some(f) => EventSpec { d_range: f.fit.d_range, v_range: f.fit.v_range, kappa_range: f.fit.kappa_range, ..o.clone() },
                None => o.clone(),
            }
        })
        .collect()
}
