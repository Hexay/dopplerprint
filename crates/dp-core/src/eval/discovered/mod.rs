//! Eval with discovered events (`eval.oracle_events = false`): each case is searched twice on the same
//! posing, with oracle events (baseline) and with the events the real front end finds in its audio.
//! Output: the usual summary/per-case JSON for the discovered run, `oracle/` for the baseline,
//! `discovery/<case>.json`, `cases.csv`, and a comparison section appended to summary.md.

mod csv;
mod front;
mod summary;
#[cfg(test)]
mod tests;
pub mod truth;

use std::path::Path;
use std::time::Instant;

use anyhow::Result;

use super::case::{self, Posed};
use super::{CaseResult, report, run_case};
use crate::config::Config;
use crate::dem::geoid_undulation;
use crate::events::DiscoverConfig;
use crate::geo::{Geodetic, geodetic_to_ecef};
use truth::{Found, Labelled, Truthed};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DiscoveryParams {
    /// Front end and discovery settings, as `dopplerprint locate` uses them.
    pub discover: DiscoverConfig,
    /// Strongest passes searched (`app::LocateRequest::max_events`).
    pub max_events: usize,
    /// Truth matching: CPA arrival tolerance (s), harmonic f0 tolerance (fraction), audible slant range (m).
    pub match_dt_s: f64,
    pub match_f0_frac: f64,
    pub max_slant_m: f64,
}

impl Default for DiscoveryParams {
    fn default() -> Self {
        Self { discover: DiscoverConfig::default(), max_events: 4, match_dt_s: 8.0, match_f0_frac: 0.06, max_slant_m: 10_000.0 }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct EventRow {
    pub found: Found,
    pub truth: Truthed,
    pub used: bool,
}

/// Front-end outcome of one case.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FrontRecord {
    pub id: String,
    pub duration_s: f64,
    pub n_truth: usize,
    pub recalled: usize,
    pub events: Vec<EventRow>,
    /// Searched events whose best-candidate flight is right / with a known flight.
    pub assoc_correct: usize,
    pub assoc_of: usize,
    pub discover_s: f64,
}

pub struct Paired {
    pub oracle: CaseResult,
    pub discovered: CaseResult,
    pub front: FrontRecord,
}

pub fn run(cfg: &Config, sim_dir: &Path, out_dir: &Path) -> Result<Vec<CaseResult>> {
    let (oracle_dir, front_dir) = (out_dir.join("oracle"), out_dir.join("discovery"));
    std::fs::create_dir_all(&oracle_dir)?;
    std::fs::create_dir_all(&front_dir)?;
    let mut pairs = Vec::new();
    for src in case::list(&cfg.eval, sim_dir)? {
        let t = Instant::now();
        let pair = match case::pose(&src, &cfg.eval) {
            Ok(mut posed) => run_pair(cfg, &mut posed, t.elapsed().as_secs_f64()),
            Err(e) => {
                let r = CaseResult { id: src.id(), status: format!("skipped: {e:#}"), ..Default::default() };
                Paired { oracle: r.clone(), discovered: r, front: FrontRecord { id: src.id(), ..Default::default() } }
            }
        };
        eprintln!("{:<34} oracle     {}", pair.oracle.id, report::one_line(&pair.oracle));
        eprintln!("{:<34} discovered {}", "", report::one_line(&pair.discovered));
        std::fs::write(oracle_dir.join(format!("{}.json", pair.oracle.id)), serde_json::to_vec_pretty(&pair.oracle)?)?;
        std::fs::write(out_dir.join(format!("{}.json", pair.discovered.id)), serde_json::to_vec_pretty(&pair.discovered)?)?;
        std::fs::write(front_dir.join(format!("{}.json", pair.front.id)), serde_json::to_vec_pretty(&pair.front)?)?;
        pairs.push(pair);
    }
    let oracle: Vec<CaseResult> = pairs.iter().map(|p| p.oracle.clone()).collect();
    let discovered: Vec<CaseResult> = pairs.iter().map(|p| p.discovered.clone()).collect();
    report::write(&oracle_dir, cfg, &oracle)?;
    report::write(out_dir, cfg, &discovered)?;
    csv::write(&out_dir.join("cases.csv"), &pairs)?;
    let mut md = std::fs::read_to_string(out_dir.join("summary.md"))?;
    md.insert_str(0, &summary::markdown(&pairs));
    std::fs::write(out_dir.join("summary.md"), md)?;
    Ok(discovered)
}

fn run_pair(cfg: &Config, c: &mut Posed, pose_s: f64) -> Paired {
    let p = &cfg.eval.discovery;
    let t = Instant::now();
    let mut oracle = run_case(cfg, c);
    oracle.timing.pose_s = pose_s;
    oracle.runtime_s = pose_s + t.elapsed().as_secs_f64();

    let t = Instant::now();
    let truth: Vec<Labelled> = c
        .events
        .iter()
        .zip(&c.truth_flights)
        .map(|(e, f)| Labelled { t_cpa: e.t_cpa_audio, f0: 0.5 * (e.f0_range.0 + e.f0_range.1), flight: f.clone() })
        .collect();
    let events = front::discover(&c.audio, p);
    let discover_s = t.elapsed().as_secs_f64();
    let found: Vec<Found> = events.iter().map(Found::from_event).collect();
    let ground = c.truth_h_ell.unwrap_or_else(|| geoid_undulation(c.truth_lat, c.truth_lon));
    let obs = geodetic_to_ecef(Geodetic { lat_deg: c.truth_lat, lon_deg: c.truth_lon, h_ell_m: ground + cfg.search.mic_height_m });
    let atm = c.sim_atmosphere.unwrap_or(cfg.search.atmosphere);
    let passes = truth::passes(&c.tracks, obs, &atm, c.t_start_true, c.audio.duration_s(), p);
    let labels = truth::label(&found, &truth, &passes, p);

    let (mut discovered, used) = front::locate(cfg, c, &events, &labels, front::strongest(&events, p.max_events));
    front::add_front_stages(&mut discovered, &labels, &used);
    let (assoc_correct, assoc_of) = front::assoc_correct(&discovered, &labels, &used);
    discovered.timing.pose_s = pose_s + discover_s;
    discovered.runtime_s = pose_s + t.elapsed().as_secs_f64();
    let recalled = labels.iter().filter(|l| l.truth_index.is_some()).count();
    let events = found
        .into_iter()
        .zip(labels)
        .enumerate()
        .map(|(i, (found, truth))| EventRow { found, truth, used: used.contains(&i) })
        .collect();
    let front = FrontRecord {
        id: c.id.clone(),
        duration_s: c.audio.duration_s(),
        n_truth: truth.len(),
        recalled,
        events,
        assoc_correct,
        assoc_of,
        discover_s,
    };
    Paired { oracle, discovered, front }
}
