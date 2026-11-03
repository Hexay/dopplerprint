//! End-to-end discovered-mode eval of event selection, as `eval::discovered::front` runs it (search, drop the
//! weakest while nothing ranks, no answer = miss with the uniform log score), for the SNR policy (main) and
//! `events::select`. MEASURE_EXPERIMENT=<toml> [MEASURE_SIM_DIR] [MEASURE_WINDOW_K] [MEASURE_SPLIT_FILE].

use super::super::{Event, discover_refined, select};
use crate::audio::resample::decimate_for;
use crate::config::Config;
use crate::dem::geoid_undulation;
use crate::eval::case::{self, Posed};
use crate::eval::discovered::truth::{self, Found, Labelled, Truthed};
use crate::eval::{CaseResult, run_case};
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::search::pipeline::coarse_cells;
use crate::search::{EventSpec, Region};

fn by_snr(events: &[Event], k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by(|&a, &b| events[b].snr.total_cmp(&events[a].snr));
    idx.truncate(k.max(1));
    idx
}

fn search(cfg: &Config, c: &mut Posed, events: &[Event], labels: &[Truthed], mut used: Vec<usize>) -> (Option<CaseResult>, Vec<usize>) {
    if used.is_empty() {
        return (None, used);
    }
    loop {
        c.events = used.iter().map(|&i| EventSpec::from_event(&events[i])).collect();
        c.truth_flights = used.iter().map(|&i| labels[i].flight.clone()).collect();
        let r = run_case(cfg, c);
        if r.status != "ok" || r.best.is_some() || used.len() <= 1 {
            return (Some(r), used);
        }
        let weakest = (0..used.len()).min_by(|&a, &b| events[used[a]].snr.total_cmp(&events[used[b]].snr)).unwrap_or(0);
        used.remove(weakest);
    }
}

#[derive(Default)]
struct Tally {
    err: Vec<f64>,
    n: usize,
    le300: usize,
    flights: usize,
    log_score: f64,
    hpd90: usize,
    select_ok: usize,
    spurious_used: usize,
}

impl Tally {
    fn add(&mut self, r: Option<&CaseResult>, uniform: f64, labels: &[Truthed], used: &[usize]) {
        self.n += 1;
        let err = r.and_then(|r| r.error_m).unwrap_or(f64::INFINITY);
        self.err.push(err);
        self.le300 += (err <= 300.0) as usize;
        self.flights += r.and_then(|r| r.flights_correct).unwrap_or(false) as usize;
        self.log_score += r.and_then(|r| r.metrics.log_score).unwrap_or(uniform);
        self.hpd90 += r.and_then(|r| r.metrics.hpd90_contains).unwrap_or(false) as usize;
        self.select_ok += (!used.is_empty() && used.iter().all(|&i| labels[i].is_real())) as usize;
        self.spurious_used += used.iter().filter(|&&i| !labels[i].is_real()).count();
    }
    fn row(&self, name: &str) -> String {
        let mut e = self.err.clone();
        e.sort_by(f64::total_cmp);
        let med = e.get(e.len() / 2).copied().unwrap_or(f64::NAN);
        format!(
            "| {name} | {} | {:.0} | {} | {} | {:.2} | {} | {} | {} |",
            self.n, med, self.le300, self.flights, self.log_score / self.n.max(1) as f64, self.hpd90, self.select_ok, self.spurious_used
        )
    }
}

#[test]
#[ignore]
fn select_eval() {
    let path = std::env::var("MEASURE_EXPERIMENT").expect("MEASURE_EXPERIMENT");
    let sim_dir = std::path::PathBuf::from(std::env::var("MEASURE_SIM_DIR").unwrap_or_default());
    let mut cfg = Config::from_toml(&std::fs::read_to_string(&path).expect("experiment")).expect("toml");
    if let Some(k) = std::env::var("MEASURE_WINDOW_K").ok().and_then(|s| s.parse().ok()) {
        cfg.eval.discovery.discover.params.fit.window_k = k;
    }
    let test_ids: Vec<String> = std::env::var("MEASURE_SPLIT_FILE")
        .map(|p| std::fs::read_to_string(p).expect("split").lines().filter_map(|l| l.split_once(" = ").map(|x| x.0.to_string())).collect())
        .unwrap_or_default();
    let p = cfg.eval.discovery.clone();
    let groups = ["dev", "test"];
    let mut tallies: Vec<[Tally; 2]> = (0..2).map(|_| Default::default()).collect();
    println!("window_k {}", p.discover.params.fit.window_k);
    for src in case::list(&cfg.eval, &sim_dir).expect("cases") {
        let Ok(mut c) = case::pose(&src, &cfg.eval) else { continue };
        let a = decimate_for(&c.audio, p.discover.front.f_max);
        let events = discover_refined(&a, &p.discover.front.spectrogram(&a), &p.discover);
        let labelled: Vec<Labelled> = c
            .events
            .iter()
            .zip(&c.truth_flights)
            .map(|(e, f)| Labelled { t_cpa: e.t_cpa_audio, f0: 0.5 * (e.f0_range.0 + e.f0_range.1), flight: f.clone() })
            .collect();
        let found: Vec<Found> = events.iter().map(Found::from_event).collect();
        let ground = c.truth_h_ell.unwrap_or_else(|| geoid_undulation(c.truth_lat, c.truth_lon));
        let obs = geodetic_to_ecef(Geodetic { lat_deg: c.truth_lat, lon_deg: c.truth_lon, h_ell_m: ground + cfg.search.mic_height_m });
        let atm = c.sim_atmosphere.unwrap_or(cfg.search.atmosphere);
        let passes = truth::passes(&c.tracks, obs, &atm, c.t_start_true, c.audio.duration_s(), &p);
        let mut labels = truth::label(&found, &labelled, &passes, &p);
        truth::gate_clock(&mut labels, cfg.eval.time_prior_s);
        let region = Region { lat_deg: c.truth_lat, lon_deg: c.truth_lon, radius_m: cfg.eval.region_radius_m };
        let uniform = -(coarse_cells(&region, cfg.search.coarse_step_m).len().max(1) as f64).ln();
        let g = test_ids.contains(&c.id) as usize;
        let mut line = format!("| {} | {} |", c.id, groups[g]);
        for (k, pick) in [by_snr(&events, p.max_events), select(&events, p.max_events)].into_iter().enumerate() {
            let (r, used) = search(&cfg, &mut c, &events, &labels, pick);
            tallies[k][g].add(r.as_ref(), uniform, &labels, &used);
            let tag: String = used.iter().map(|&i| if labels[i].is_real() { 'R' } else { '.' }).collect();
            let err = r.as_ref().and_then(|r| r.error_m).map_or("miss".into(), |e| format!("{e:.0}"));
            let hpd = if r.as_ref().and_then(|r| r.metrics.hpd90_contains).unwrap_or(false) { "in90" } else { "out90" };
            let ls = r.as_ref().and_then(|r| r.metrics.log_score).unwrap_or(uniform);
            line += &format!(" {err} {tag} {hpd} {ls:.1} |");
        }
        println!("{line}");
    }
    println!("\n| policy / split | n | median err m | ≤300 m | flights | log score | 90% | all searched real | spurious searched |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (k, name) in ["snr top-k (main)", "pass-likeness select"].iter().enumerate() {
        for (g, gn) in groups.iter().enumerate() {
            if tallies[k][g].n > 0 {
                println!("{}", tallies[k][g].row(&format!("{name} / {gn}")));
            }
        }
    }
}
