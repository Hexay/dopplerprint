//! No `--near`: find the area with `search::global::locate_global` over the cached ADS-B store, then the
//! regional locate runs there as if `--near` had been given. Policy: docs/research/global_search_2026-11-03.md
//! §Recommended locate (a point only for ≥ 2 agreeing passes, an unambiguous margin and a passed nationwide
//! decoy check; else candidates).

use anyhow::Result;

use super::NeedsInput;
use super::cache::Cache;
use super::decoy;
use super::place::Area;
use super::when::StartPrior;
use crate::audio::{Audio, log_spectrogram};
use crate::config::Config;
use crate::dem::Dem;
use crate::events::Event;
use crate::search::global::{AMBIGUITY_MARGIN, GlobalInput, GlobalResult, StoreSource, locate_global};
use crate::search::hypothesis::UNEXPLAINED_ID;
use crate::search::score::ScoringPlane;
use crate::search::{EventSpec, Scorer};

/// Regional radius around the nationwide answer (km): at least this, else the proposal's own radius.
const MIN_RADIUS_KM: f64 = 5.0;
/// Candidates listed when the answer is ambiguous.
const SHOWN: usize = 5;
/// Start of the `NeedsInput` message asking for `--near` (`inputs::missing_fields` keys on it).
pub const ASK_NEAR: &str = "Searched the whole country:";

/// `window` = the ADS-B time window of the real search; decoys run only on days already cached.
pub fn find_area(cache: &Cache, audio: &Audio, events: &[Event], prior: &StartPrior, window: (f64, f64), cfg: &Config) -> Result<Area> {
    let specs: Vec<EventSpec> = events.iter().map(EventSpec::from_event).collect();
    let plane = ScoringPlane::new(&log_spectrogram(audio, &cfg.spec), &cfg.search.score);
    let scorer = Scorer::new(&plane, cfg.search.backend)?;
    let source = StoreSource { store: cache.store(), extra: Vec::new() };
    let national_path = cache.dem_dir().join("5070/conus_1000m.u16");
    let national = national_path.exists().then(|| Dem::open(&national_path)).transpose()?;
    let mut sp = cfg.search.clone();
    sp.assoc.silence.measure_recording(audio, &cfg.spec);
    let run = |shift: f64| {
        let inp = GlobalInput {
            scorer: &scorer,
            events: &specs,
            source: &source,
            prior: (prior.lo + shift, prior.hi + shift),
            duration_s: audio.duration_s(),
            dem: None,
            national: national.as_ref(),
            flat_h_ell_m: None,
        };
        locate_global(&inp, &sp, &cfg.global)
    };
    let res = run(0.0)?;
    let Some(b) = res.best.as_ref().filter(|_| !res.ambiguous) else {
        return Err(NeedsInput(ambiguous_message(&res, &why_ambiguous(&res, events.len()))).into());
    };
    // A nationwide match is the best of thousands of flights; the regional decoy only replays one area.
    let shifts = decoy::shifts(prior, audio.duration_s(), window, cache);
    let check = decoy::check(b.score, &shifts, &mut |d| Ok(run(d)?.best.map(|c| c.score)))?;
    // A decoy is one more distinct rival, so it must be beaten by the same margin as a rival region.
    if let Some(d) = check.filter(|d| !d.passed_by(AMBIGUITY_MARGIN)) {
        let why = format!(
            "flights logged {:.0} min from the given start fit as well as the best match (margin {:.1}), so it may be chance",
            d.shifts_s.iter().map(|s| s.abs()).fold(0.0, f64::max) / 60.0,
            d.margin.unwrap_or(0.0)
        );
        return Err(NeedsInput(ambiguous_message(&res, &why)).into());
    }
    let r = res.regions.first().map_or(0.0, |o| res.proposals[o.proposal].region.radius_m / 1000.0);
    let label = format!("the nationwide match ({:.4}, {:.4})", b.lat_deg, b.lon_deg);
    Ok(Area { lat: b.lat_deg, lon: b.lon_deg, radius_km: r.max(MIN_RADIUS_KM), label })
}

fn why_ambiguous(res: &GlobalResult, n_events: usize) -> String {
    if res.stats.n_max < 2 {
        format!("only one of the {n_events} passes heard can be matched at a time, so flights in several places fit")
    } else {
        format!("no spot beats the next distinct one by a clear margin ({:.1}), so flights in several places fit", res.margin.unwrap_or(0.0))
    }
}

fn ambiguous_message(res: &GlobalResult, why: &str) -> String {
    let mut s = format!("{ASK_NEAR} {why}. Add --near (a city or state), narrow --start, or record longer to catch a second aircraft. Candidates:");
    let flights = |ids: &[String]| ids.iter().filter(|id| id.as_str() != UNEXPLAINED_ID).cloned().collect::<Vec<_>>().join(" + ");
    for c in res.regions.iter().filter_map(|r| r.best.as_ref()).take(SHOWN) {
        s.push_str(&format!("\n  {:.4}, {:.4}  flights {}  score {:.1}", c.lat_deg, c.lon_deg, flights(&c.track_ids), c.score));
    }
    if res.regions.iter().all(|r| r.best.is_none()) {
        for p in res.proposals.iter().take(SHOWN) {
            let ids: Vec<String> = p.flights.iter().filter_map(|f| f.first().cloned()).collect();
            s.push_str(&format!("\n  {:.4}, {:.4}  flights {}", p.region.lat_deg, p.region.lon_deg, flights(&ids)));
        }
    }
    s
}
