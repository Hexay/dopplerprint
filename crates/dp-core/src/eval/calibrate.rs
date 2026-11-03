//! Fit the posterior temperature T, uniform floor ε and conformal HPD levels from cached eval surfaces (no
//! re-search), then score them on held-out runs. T and ε maximise the mean log score at truth over the
//! T-fit cases; q̂ per stratum is the split-conformal quantile of HPD-split scores at that shape.
//! See docs/research/calibration_discovered_2026-11-03.md (and calibration_2026-11-03.md for the method).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;

use super::CaseResult;
use super::conformal::q_hat;
use super::coverage::{Coverage, NOMINAL, Shape, measure};
use crate::search::calibration::{Calibration, Stratum};
use crate::search::surface::Surface;

/// (name, min events, max events); events = passes searched (discovered events in the app path).
pub const STRATA: [(&str, usize, usize); 3] = [("1 pass", 1, 1), ("2 passes", 2, 2), ("≥3 passes", 3, 99)];
/// Fewest real cases for a stratum's q̂ to come from real data alone (enough for the 90% level).
const MIN_REAL: usize = 9;

/// Which searched events a run's results come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Events {
    /// The front end's events (`eval.oracle_events = false`): what `locate` searches.
    Discovered,
    /// Truth-label events.
    Oracle,
}

impl Events {
    pub fn name(self) -> &'static str {
        match self {
            Events::Discovered => "discovered",
            Events::Oracle => "oracle",
        }
    }
}

pub struct Case {
    pub id: String,
    pub n_events: usize,
    pub surface: Surface,
    pub truth: (f64, f64),
}

impl Case {
    pub fn is_sim(&self) -> bool {
        self.id.starts_with("sim_")
    }
    pub fn measure(&self, sh: Shape, masses: &[f64]) -> Coverage {
        measure(&self.surface, self.truth.0, self.truth.1, sh, masses)
    }
    fn log_score(&self, sh: Shape) -> f64 {
        let leaf = self.surface.leaf_at(self.truth.0, self.truth.1);
        self.surface.posterior(sh.temperature, sh.floor).density_at(leaf).ln()
    }
}

/// One run directory's cases in one event mode. `unanswered`: searched nothing or found no candidate, so
/// `locate` reports no region; they are not calibration cases but count as misses unconditionally.
pub struct Set {
    pub run: PathBuf,
    pub events: Events,
    pub cases: Vec<Case>,
    pub unanswered: Vec<String>,
}

/// A discovered run keeps its results at the top and the oracle baseline in `oracle/`; an oracle-only run
/// keeps oracle results at the top.
pub fn results_dir(run: &Path, ev: Events) -> Result<PathBuf> {
    let discovered = run.join("discovery").is_dir();
    Ok(match (ev, discovered) {
        (Events::Discovered, true) | (Events::Oracle, false) => run.to_path_buf(),
        (Events::Oracle, true) => run.join("oracle"),
        (Events::Discovered, false) => bail!("{} is not a discovered-events run (eval.oracle_events = false)", run.display()),
    })
}

pub fn is_discovered_run(run: &Path) -> bool {
    run.join("discovery").is_dir()
}

/// Every ok case of `run` in mode `ev`, from the per-case `<id>.json` files.
pub fn load(run: &Path, ev: Events) -> Result<Set> {
    let d = results_dir(run, ev)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
        .with_context(|| format!("{}", d.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.file_stem().is_some_and(|s| s != "summary"))
        .collect();
    files.sort();
    let mut set = Set { run: run.to_path_buf(), events: ev, cases: Vec::new(), unanswered: Vec::new() };
    for f in files {
        let r: CaseResult = serde_json::from_slice(&std::fs::read(&f)?).with_context(|| format!("{}", f.display()))?;
        match (r.status == "ok", r.surface, r.truth_lat_lon) {
            (true, Some(surface), Some(truth)) => set.cases.push(Case { id: r.id, n_events: r.n_events, surface, truth }),
            (true, None, Some(_)) => set.unanswered.push(r.id),
            _ => {}
        }
    }
    Ok(set)
}

pub fn stratum_of(n_events: usize) -> usize {
    STRATA.iter().position(|s| (s.1..=s.2).contains(&n_events)).unwrap_or(0)
}

pub fn mean_log_score(cases: &[&Case], sh: Shape) -> f64 {
    let s: f64 = cases.par_iter().map(|c| c.log_score(sh)).sum();
    s / cases.len().max(1) as f64
}

/// Golden-section argmax of `f` on [a, b] (unimodal in practice; the report's coarse curves show it).
fn golden(f: impl Fn(f64) -> f64, mut a: f64, mut b: f64) -> f64 {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let (mut x1, mut x2) = (b - g * (b - a), a + g * (b - a));
    let (mut f1, mut f2) = (f(x1), f(x2));
    while b - a > 0.01 {
        if f1 < f2 {
            (a, x1, f1) = (x1, x2, f2);
            x2 = a + g * (b - a);
            f2 = f(x2);
        } else {
            (b, x2, f2) = (x2, x1, f1);
            x1 = b - g * (b - a);
            f1 = f(x1);
        }
    }
    0.5 * (a + b)
}

/// T maximising the mean log score over ln T ∈ [ln 0.2, ln 50] at `base.floor`.
pub fn fit_temperature(cases: &[&Case], base: Shape) -> f64 {
    golden(|lt| mean_log_score(cases, Shape { temperature: lt.exp(), ..base }), 0.2f64.ln(), 50f64.ln()).exp()
}

/// ε maximising the mean log score over ln ε ∈ [ln 0.001, ln 0.5] at `base.temperature`.
pub fn fit_floor(cases: &[&Case], base: Shape) -> f64 {
    golden(|le| mean_log_score(cases, Shape { floor: le.exp(), ..base }), 0.001f64.ln(), 0.5f64.ln()).exp()
}

/// Per-stratum q̂ at `sh`: real cases alone when there are enough, else real + sim.
pub fn strata(cases: &[&Case], sh: Shape) -> Vec<Stratum> {
    let scores: Vec<(usize, bool, f64)> =
        cases.par_iter().map(|c| (stratum_of(c.n_events), c.is_sim(), c.measure(sh, &[]).hpd_score)).collect();
    STRATA
        .iter()
        .enumerate()
        .map(|(k, &(name, lo, hi))| {
            let real: Vec<f64> = scores.iter().filter(|s| s.0 == k && !s.1).map(|s| s.2).collect();
            let (v, source) = if real.len() >= MIN_REAL {
                (real, "real")
            } else {
                let all: Vec<f64> = scores.iter().filter(|s| s.0 == k).map(|s| s.2).collect();
                (all, if real.is_empty() { "sim" } else { "real+sim" })
            };
            Stratum {
                name: name.into(),
                min_events: lo,
                max_events: hi,
                n: v.len(),
                source: source.into(),
                nominal: NOMINAL.to_vec(),
                q_hat: NOMINAL.iter().map(|&l| q_hat(&v, l).unwrap_or(1.0)).collect(),
            }
        })
        .collect()
}

pub struct Fitted {
    pub calibration: Calibration,
    /// (T, mean log score) on the T-fit cases at the fitted ε, coarse grid.
    pub curve: Vec<(f64, f64)>,
    /// (ε, mean log score) at the fitted T; empty when ε was not fitted.
    pub floor_curve: Vec<(f64, f64)>,
    pub n_t_fit: usize,
}

/// `t_cases`: id substrings selecting the cases T (and ε) are fitted on (all calibration cases when empty).
/// With `fit_floor`, T and ε are fitted by coordinate ascent; otherwise ε stays `base.floor`.
pub fn fit(all: &[&Case], t_cases: &[String], base: Shape, fit_eps: bool, provenance: &str) -> Fitted {
    let t_fit: Vec<&Case> =
        all.iter().copied().filter(|c| t_cases.is_empty() || t_cases.iter().any(|s| c.id.contains(s.as_str()))).collect();
    let mut sh = base;
    for _ in 0..if fit_eps { 4 } else { 1 } {
        sh.temperature = fit_temperature(&t_fit, sh);
        if fit_eps {
            sh.floor = round_sig(fit_floor(&t_fit, sh), 2);
        }
    }
    sh.temperature = (sh.temperature * 100.0).round() / 100.0;
    let curve = [0.5, 1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0]
        .iter()
        .map(|&x| (x, mean_log_score(&t_fit, Shape { temperature: x, ..sh })))
        .collect();
    let floor_curve = if fit_eps {
        [0.001, 0.003, 0.01, 0.03, 0.1, 0.2, 0.3].iter().map(|&x| (x, mean_log_score(&t_fit, Shape { floor: x, ..sh }))).collect()
    } else {
        Vec::new()
    };
    let calibration = Calibration {
        temperature: sh.temperature,
        floor: sh.floor,
        persistence_nats: sh.persistence_nats,
        provenance: provenance.into(),
        stratum: strata(all, sh),
    };
    Fitted { calibration, curve, floor_curve, n_t_fit: t_fit.len() }
}

fn round_sig(x: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits - 1 - x.abs().log10().floor() as i32);
    (x * p).round() / p
}

pub fn write_toml(path: &Path, c: &Calibration) -> Result<()> {
    let head = "# Written by `dopplerprint calibrate`; see docs/research/calibration_discovered_2026-11-03.md.\n";
    std::fs::write(path, format!("{head}{}", toml::to_string(c)?))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_dir_follows_the_event_mode() {
        let d = tempfile::tempdir().unwrap();
        let run = d.path();
        assert_eq!(results_dir(run, Events::Oracle).unwrap(), run);
        assert!(results_dir(run, Events::Discovered).is_err(), "an oracle-only run has no discovered results");
        std::fs::create_dir(run.join("discovery")).unwrap();
        assert_eq!(results_dir(run, Events::Discovered).unwrap(), run);
        assert_eq!(results_dir(run, Events::Oracle).unwrap(), run.join("oracle"));
    }

    #[test]
    fn golden_and_rounding() {
        assert!((golden(|x| -(x - 1.3).powi(2), 0.0, 4.0) - 1.3).abs() < 0.01);
        assert_eq!(round_sig(0.07349, 2), 0.073);
        assert_eq!(round_sig(0.25, 2), 0.25);
    }
}
