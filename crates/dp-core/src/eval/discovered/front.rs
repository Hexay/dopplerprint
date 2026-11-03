//! The locate front end on a posed case, as `app::run` does it: band-limit, `discover_refined`,
//! `events::select`, search with `EventSpec::from_event`, and drop the weakest event while nothing matches
//! (select can still keep a plausible pass with no ADS-B track). The search itself is `eval::run_case`, so metrics are those of oracle mode.

use super::DiscoveryParams;
use super::truth::Truthed;
use crate::audio::Audio;
use crate::audio::resample::decimate_for;
use crate::config::Config;
use crate::eval::case::Posed;
use crate::eval::metrics::{Metrics, Stage};
use crate::eval::{CaseResult, run_case};
use crate::events::{Event, discover_refined, select};
use crate::search::pipeline::coarse_cells;
use crate::search::{EventSpec, Region};

pub fn discover(audio: &Audio, p: &DiscoveryParams) -> Vec<Event> {
    let a = decimate_for(audio, p.discover.front.f_max);
    let spec = p.discover.front.spectrogram(&a);
    discover_refined(&a, &spec, &p.discover)
}

/// The events `app::run` searches (`events::select`), in CPA order.
pub fn strongest(events: &[Event], max_events: usize) -> Vec<usize> {
    select(events, max_events)
}

/// Overwrites `c.events` / `c.truth_flights` with the searched subset. Returns the result and that subset.
pub fn locate(cfg: &Config, c: &mut Posed, events: &[Event], labels: &[Truthed], mut used: Vec<usize>) -> (CaseResult, Vec<usize>) {
    if used.is_empty() {
        return (no_answer(cfg, c), used);
    }
    loop {
        c.events = used.iter().map(|&i| EventSpec::from_event(&events[i])).collect();
        c.oracle_events = false;
        c.truth_flights = used.iter().map(|&i| labels[i].flight.clone()).collect();
        let r = run_case(cfg, c);
        if r.status != "ok" || r.best.is_some() || used.len() <= 1 {
            return (r, used);
        }
        let weakest = (0..used.len()).min_by(|&a, &b| events[used[a]].snr.total_cmp(&events[used[b]].snr)).unwrap_or(0);
        used.remove(weakest);
    }
}

/// Prepend `discover` (some event is a real pass) and `select` (every searched event is) to the stages.
pub fn add_front_stages(r: &mut CaseResult, labels: &[Truthed], used: &[usize]) {
    let real = labels.iter().filter(|t| t.is_real()).count();
    let select_ok = !used.is_empty() && used.iter().all(|&i| labels[i].is_real());
    let front = [
        Stage { stage: "discover".into(), ok: real > 0, rank: None, of: Some(labels.len()) },
        Stage { stage: "select".into(), ok: select_ok, rank: None, of: Some(used.len()) },
    ];
    let m = &mut r.metrics;
    m.stages.splice(0..0, front);
    m.lost_at = m.stages.iter().find(|s| !s.ok).map(|s| s.stage.clone());
}

/// Searched events whose best-candidate track is their true flight, of those with a known flight.
pub fn assoc_correct(r: &CaseResult, labels: &[Truthed], used: &[usize]) -> (usize, usize) {
    let Some(best) = &r.best else { return (0, used.iter().filter(|&&i| labels[i].flight.is_some()).count()) };
    let mut out = (0, 0);
    for (k, &i) in used.iter().enumerate() {
        let Some(f) = &labels[i].flight else { continue };
        out.1 += 1;
        out.0 += best.track_ids.get(k).is_some_and(|id| crate::eval::metrics::same_flight(id, f)) as usize;
    }
    out
}

/// Nothing to search: a miss with the uniform posterior's log score (region taken at the truth).
fn no_answer(cfg: &Config, c: &Posed) -> CaseResult {
    let region = Region { lat_deg: c.truth_lat, lon_deg: c.truth_lon, radius_m: cfg.eval.region_radius_m };
    let n = coarse_cells(&region, cfg.search.coarse_step_m).len().max(1);
    CaseResult {
        id: c.id.clone(),
        status: "ok".into(),
        flights_correct: Some(false),
        n_tracks: c.tracks.len(),
        metrics: Metrics { log_score: Some(-(n as f64).ln()), ..Default::default() },
        truth_lat_lon: Some((c.truth_lat, c.truth_lon)),
        ..Default::default()
    }
}
