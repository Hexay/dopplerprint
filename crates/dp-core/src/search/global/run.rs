//! Region-free locate: CONUS window tracks → proposals → regional search on the top-K → merged ranking.

use std::time::Instant;

use super::source::{TrackSource, window_tracks};
use super::rerank::rerank;
use super::{GlobalParams, ProposalStats, RegionProposal, informative_events, propose_regions};
use crate::adsb::TrackQuery;
use crate::dem::{Dem, geoid_undulation};
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::search::hypothesis::EventSpec;
use crate::search::pipeline::{Candidate, SearchInput, SearchParams, search};
use crate::search::Scorer;
use crate::search::silence::SilenceMode;
use crate::search::{Catalog, MemCatalog};

pub struct GlobalInput<'a> {
    pub scorer: &'a Scorer<'a>,
    pub events: &'a [EventSpec],
    pub source: &'a dyn TrackSource,
    /// Recording-start prior (UNIX s).
    pub prior: (f64, f64),
    pub duration_s: f64,
    /// Fine DEM, used where it covers a cell.
    pub dem: Option<&'a Dem>,
    /// Coarse national DEM (`data/dem/5070/conus_1000m`): ground under the votes, and wherever `dem` does not
    /// cover.
    pub national: Option<&'a Dem>,
    /// Ground ellipsoidal height where no DEM covers; `None` = geoid + mid `ground_msl_m`.
    pub flat_h_ell_m: Option<f64>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RegionOutcome {
    /// Index into `GlobalResult::proposals`.
    pub proposal: usize,
    pub best: Option<Candidate>,
    pub n_tracks: usize,
    pub cells: usize,
    pub hypotheses: usize,
    pub runtime_s: f64,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct GlobalTimings {
    pub load_s: f64,
    pub propose_s: f64,
    pub rerank_s: f64,
    pub locate_s: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct GlobalResult {
    /// Indices into `GlobalInput::events` that were searched (`informative_events`); candidates' per-event
    /// ids follow this order.
    pub events_used: Vec<usize>,
    pub proposals: Vec<RegionProposal>,
    pub stats: ProposalStats,
    /// Scored regions, best first.
    pub regions: Vec<RegionOutcome>,
    pub best: Option<Candidate>,
    /// Best score minus the best of any region whose best cell lies > `ambiguity_m` away (z units).
    pub margin: Option<f64>,
    /// Fewer than two events agree anywhere, or the runner-up region is within `ambiguity_margin`.
    pub ambiguous: bool,
    pub timings: GlobalTimings,
}

/// Separation (m) beyond which two regions' best cells are distinct answers.
const AMBIGUITY_M: f64 = 2_000.0;
/// Score margin (z) below which a distinct runner-up makes the answer ambiguous.
pub const AMBIGUITY_MARGIN: f64 = 3.0;

pub fn locate_global(inp: &GlobalInput<'_>, sp: &SearchParams, gp: &GlobalParams) -> anyhow::Result<GlobalResult> {
    let mut timings = GlobalTimings::default();
    let events_used = informative_events(inp.events, gp);
    let events: Vec<EventSpec> = events_used.iter().map(|&i| inp.events[i].clone()).collect();
    let t = Instant::now();
    let tracks = window_tracks(inp.source, &events, inp.prior, gp)?;
    timings.load_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let (mut proposals, stats) = propose_regions(&events, &tracks, inp.prior, inp.national, gp);
    timings.propose_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let ground = |lat: f64, lon: f64| {
        let dem = inp.dem.and_then(|d| d.height_ellipsoid_latlon(lat, lon));
        let dem = dem.or_else(|| inp.national.and_then(|d| d.height_ellipsoid_latlon(lat, lon)));
        dem.unwrap_or_else(|| flat_ground(inp, gp, lat, lon))
    };
    rerank(&mut proposals, &tracks, inp.scorer.plane(), &events, &ground, sp, gp);
    drop(tracks);
    timings.rerank_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut regions = Vec::new();
    for (i, prop) in proposals.iter().enumerate().take(gp.top_k) {
        regions.push(score_region(inp, &events, i, prop, sp, gp)?);
    }
    timings.locate_s = t.elapsed().as_secs_f64();
    let score = |r: &RegionOutcome| r.best.as_ref().map_or(f64::NEG_INFINITY, |b| b.score);
    regions.sort_by(|a, b| score(b).total_cmp(&score(a)));
    let best = regions.first().and_then(|r| r.best.clone());
    let margin = best.as_ref().and_then(|b| {
        let rival = regions.iter().filter_map(|r| r.best.as_ref()).find(|c| distance_m(c, b) > AMBIGUITY_M);
        rival.map(|c| b.score - c.score)
    });
    let ambiguous = stats.n_max < 2 || margin.is_some_and(|m| m < AMBIGUITY_MARGIN);
    Ok(GlobalResult { events_used, proposals, stats, regions, best, margin, ambiguous, timings })
}

fn score_region(
    inp: &GlobalInput<'_>,
    events: &[EventSpec],
    idx: usize,
    prop: &RegionProposal,
    sp: &SearchParams,
    gp: &GlobalParams,
) -> anyhow::Result<RegionOutcome> {
    let t = Instant::now();
    let r = prop.region;
    let mut p = sp.clone();
    if !gp.region_silence {
        p.assoc.silence.mode = SilenceMode::Flat;
        p.assoc.silence_penalty = 0.0;
    }
    p.assoc.t_start_prior = (
        (prop.clock_range.0 - gp.clock_tol_s).max(inp.prior.0),
        (prop.clock_range.1 + gp.clock_tol_s).min(inp.prior.1),
    );
    // The fine DEM when it covers this region, else the national one; flat ground where neither does.
    let covers = |d: &&Dem| d.height_msl_latlon(r.lat_deg, r.lon_deg).is_some();
    let dem = inp.dem.filter(covers).or(inp.national.filter(covers));
    p.flat_h_ell_m = flat_ground(inp, gp, r.lat_deg, r.lon_deg);
    let reach = r.radius_m + p.track_radius_m + 2_000.0;
    let (dlat, dlon) = (reach / 111_000.0, reach / (111_000.0 * r.lat_deg.to_radians().cos()));
    let q = TrackQuery {
        t_start: p.assoc.t_start_prior.0 - 120.0,
        t_end: p.assoc.t_start_prior.1 + inp.duration_s + 120.0,
        bbox: Some((r.lat_deg - dlat, r.lon_deg - dlon, r.lat_deg + dlat, r.lon_deg + dlon)),
        max_alt_m: Some(9_000.0),
        exclude_engines: Vec::new(),
    };
    let catalog = MemCatalog::new(inp.source.load(&q)?);
    let n_tracks = catalog.all().len();
    let si = SearchInput { scorer: inp.scorer, events, catalog: &catalog, region: r, duration_s: inp.duration_s, dem };
    let res = search(&si, &p);
    Ok(RegionOutcome {
        proposal: idx,
        best: res.ranked.into_iter().next(),
        n_tracks,
        cells: res.cells_scored,
        hypotheses: res.hypotheses_scored,
        runtime_s: t.elapsed().as_secs_f64(),
    })
}

fn flat_ground(inp: &GlobalInput<'_>, gp: &GlobalParams, lat: f64, lon: f64) -> f64 {
    inp.flat_h_ell_m.unwrap_or_else(|| geoid_undulation(lat, lon) + 0.5 * (gp.ground_msl_m.0 + gp.ground_msl_m.1))
}

fn distance_m(a: &Candidate, b: &Candidate) -> f64 {
    let ecef = |c: &Candidate| geodetic_to_ecef(Geodetic { lat_deg: c.lat_deg, lon_deg: c.lon_deg, h_ell_m: 0.0 });
    (ecef(a) - ecef(b)).norm()
}
