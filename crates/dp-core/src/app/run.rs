//! The locate steps: inputs → decode → discover → ADS-B (cached/fetched) → search → report → files.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, ensure};

use super::cache::Cache;
use super::fetch::{FetchOptions, dates_between, ensure_days};
use super::inputs::{given_start, registrations, resolve_area, start_prior, terrain};
use super::place::Area;
use super::posterior;
use super::report::{Clock, MIN_EVENT_Z, Report, Status, alternatives, location, matched};
use super::results::{self, ResultInput};
use super::ui::Ui;
use super::weather::{describe_used, model_atmosphere};
use super::when::{fmt_utc, StartPrior};
use super::{LocateRequest, NeedsInput};
use crate::adsb::{self, TrackQuery};
use crate::audio::{self, Audio};
use crate::config::Config;
use crate::dem::{Dem, geoid_undulation};
use crate::events::{DiscoverConfig, Event, discover_with};
use crate::search::{self, EventSpec, Region, SearchParams, SearchResult};
use crate::track::Track;

/// ADS-B loaded beyond the start prior and the recording, so passes at the edges have whole tracks.
const TRACK_MARGIN_S: f64 = 900.0;
/// Tracks loaded this far beyond the search radius (audibility plus margin).
const TRACK_REACH_M: f64 = 15_000.0;
/// Cruising jets never matter acoustically; lower jets are kept and flagged.
const MAX_ALT_M: f64 = 9_000.0;

pub fn run(req: &LocateRequest, ui: &dyn Ui) -> Result<Report> {
    let started = Instant::now();
    let mut rep = Report::new(&req.audio.display().to_string());
    if let Err(e) = steps(req, ui, &mut rep, started) {
        let n = e.downcast::<NeedsInput>()?;
        rep.status = Status::NeedsInput;
        rep.message = Some(n.0);
    }
    rep.runtime_s = started.elapsed().as_secs_f64();
    Ok(rep)
}

fn steps(req: &LocateRequest, ui: &dyn Ui, rep: &mut Report, started: Instant) -> Result<()> {
    ensure!(req.audio.is_file(), "no such file: {}", req.audio.display());
    let cache = Cache::resolve(req.cache.as_deref())?;
    let cfg = match &req.config {
        Some(path) => Config::from_toml(&std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?)?,
        None => Config::default(),
    };
    let given = given_start(req)?;
    let area = resolve_area(req, &cache, ui)?;
    rep.area = Some(area.clone());

    let mut dcfg = DiscoverConfig::default();
    dcfg.front.mains_hz = req.mains_hz.or(dcfg.front.mains_hz);
    ui.step("audio", 0.05, &format!("Reading {}...", req.audio.display()));
    let (audio, _) = audio::decode_band(&req.audio, dcfg.front.f_max)?;
    rep.duration_s = audio.duration_s();
    let prior = start_prior(&given, req.start_tolerance.as_deref(), rep.duration_s)?;
    rep.start = Some(prior.clone());

    ui.step("passes", 0.15, "Listening for aircraft passes...");
    let spec = dcfg.front.spectrogram(&audio);
    let mut events = discover_with(&spec, &dcfg.params);
    rep.events_found = events.len();
    if events.is_empty() {
        return stop(rep, "no aircraft pass found in the audio; dopplerprint needs at least one propeller or helicopter tone that drops in pitch as it goes by.");
    }
    events.sort_by(|a, b| b.snr.total_cmp(&a.snr));
    events.truncate(req.max_events.max(1));
    events.sort_by(|a, b| a.fit.best.t_cpa_arrive.total_cmp(&b.fit.best.t_cpa_arrive));

    let window = (prior.lo - TRACK_MARGIN_S, prior.hi + rep.duration_s + TRACK_MARGIN_S);
    let fo = FetchOptions { offline: req.offline, yes: req.yes, keep_raw: req.keep_raw, max_cache_gb: req.max_cache_gb };
    ensure_days(&cache, &dates_between(window.0, window.1), &fo, ui)?;
    let tracks = load_tracks(&cache, &area, window)?;
    rep.tracks_considered = tracks.len();
    if tracks.is_empty() {
        let msg = format!("no aircraft in the ADS-B data near {} between {} and {}.", area.label, fmt_utc(window.0), fmt_utc(window.1));
        return stop(rep, &msg);
    }

    let (dem, terrain_note) = terrain(req, &cache, &area)?;
    rep.terrain = terrain_note;
    let mut p = search_params(&cfg, &prior, &area);
    let air = model_atmosphere(req, &cache, &area, dem.as_ref(), prior.nominal + 0.5 * rep.duration_s, p.atmosphere, ui);
    p.atmosphere = air.atmosphere;
    rep.air = air.source.clone();
    ui.step("search", 0.5, &format!(
        "Matching {} pass{} against {} flights within {:.0} km of {}...",
        events.len(),
        if events.len() == 1 { "" } else { "es" },
        tracks.len(),
        area.radius_km,
        area.label
    ));
    let (res, used) = search_dropping_weakest(&audio, events, &tracks, &area, dem.as_ref(), &cfg, &p)?;
    rep.cells_scored = res.cells_scored;
    rep.events_used = used.len();
    let Some(best) = res.ranked.first() else {
        let msg = format!(
            "none of the {} passes heard matches a logged flight within {:.0} km of {} for a start between {} and {}; check --start (and its UTC offset) and --near.",
            rep.events_found,
            area.radius_km,
            area.label,
            fmt_utc(prior.lo),
            fmt_utc(prior.hi)
        );
        return stop(rep, &msg);
    };

    rep.location = Some(location(best));
    let calibrated = posterior::posterior(&res.ranked, &search_region(&area), p.coarse_step_m, used.len());
    rep.posterior = calibrated.as_ref().map(|c| c.0.clone());
    rep.air = describe_used(&air, best, &tracks, dem.as_ref());
    let (alts, conf) = alternatives(&res.ranked);
    (rep.alternatives, rep.confidence) = (alts, Some(conf));
    let icaos: Vec<u32> = tracks.iter().filter(|t| best.track_ids.contains(&t.id)).map(|t| t.icao24).collect();
    let regs = registrations(&cache.typedb(), &icaos);
    let t_cpa: Vec<f64> = used.iter().map(|e| e.fit.best.t_cpa_arrive).collect();
    rep.aircraft = matched(best, &t_cpa, &tracks, &|icao| regs.get(&icao).cloned());
    rep.clock = Some(Clock { start_unix: best.clock, start_utc: fmt_utc(best.clock), offset_from_given_s: best.clock - prior.nominal });
    let weakest = best.events.iter().enumerate().min_by(|a, b| a.1.z.total_cmp(&b.1.z));
    match weakest {
        Some((i, w)) if w.z < MIN_EVENT_Z => {
            rep.status = Status::Inconclusive;
            rep.message = Some(format!(
                "the best spot explains pass {} only weakly (match z {:.1} < {MIN_EVENT_Z}); treat it as a guess.",
                i + 1,
                w.z
            ));
        }
        _ => rep.status = Status::Found,
    }

    let dir = req.out.clone().unwrap_or_else(|| default_out(req));
    rep.files = results::paths(&dir).iter().map(|p| p.display().to_string()).collect();
    rep.runtime_s = started.elapsed().as_secs_f64();
    let clock_window = (best.clock - 600.0, best.clock + rep.duration_s + 600.0);
    let inp = ResultInput {
        report: rep,
        ranked: &res.ranked,
        coarse_step_m: p.coarse_step_m,
        events: &used,
        tracks: &tracks,
        window: clock_window,
        spec: Some(&spec),
        region: calibrated.as_ref().map(|c| (&c.1, c.2.as_slice())),
    };
    results::write(&dir, &inp)
}

fn stop(rep: &mut Report, msg: &str) -> Result<()> {
    rep.status = Status::Inconclusive;
    rep.message = Some(msg.to_string());
    Ok(())
}

fn default_out(req: &LocateRequest) -> PathBuf {
    let stem = req.audio.file_stem().map_or("recording".into(), |s| s.to_string_lossy().into_owned());
    PathBuf::from("dopplerprint-results").join(stem)
}

fn load_tracks(cache: &Cache, a: &Area, window: (f64, f64)) -> Result<Vec<Track>> {
    let reach = a.radius_km * 1000.0 + TRACK_REACH_M;
    let (dlat, dlon) = (reach / 111_000.0, reach / (111_000.0 * a.lat.to_radians().cos()));
    let q = TrackQuery {
        t_start: window.0,
        t_end: window.1,
        bbox: Some((a.lat - dlat, a.lon - dlon, a.lat + dlat, a.lon + dlon)),
        max_alt_m: Some(MAX_ALT_M),
        exclude_engines: Vec::new(),
    };
    adsb::load_tracks(&cache.store(), &q)
}

fn search_params(cfg: &Config, prior: &StartPrior, a: &Area) -> SearchParams {
    let mut p = cfg.search.clone();
    p.assoc.t_start_prior = (prior.lo, prior.hi);
    p.flat_h_ell_m = geoid_undulation(a.lat, a.lon);
    p
}

fn search_region(a: &Area) -> Region {
    Region { lat_deg: a.lat, lon_deg: a.lon, radius_m: a.radius_km * 1000.0 }
}

/// A hypothesis must explain every event, so one spurious event blocks the search: drop the weakest and retry.
fn search_dropping_weakest(
    audio: &Audio,
    mut events: Vec<Event>,
    tracks: &[Track],
    a: &Area,
    dem: Option<&Dem>,
    cfg: &Config,
    p: &SearchParams,
) -> anyhow::Result<(SearchResult, Vec<Event>)> {
    let region = search_region(a);
    loop {
        let specs: Vec<EventSpec> = events.iter().map(EventSpec::from_event).collect();
        let res = search::locate(audio, &specs, tracks.to_vec(), region, dem, &cfg.spec, p)?;
        if !res.ranked.is_empty() || events.len() <= 1 {
            return Ok((res, events));
        }
        let weakest = events.iter().enumerate().min_by(|x, y| x.1.snr.total_cmp(&y.1.snr)).map_or(0, |w| w.0);
        events.remove(weakest);
    }
}
