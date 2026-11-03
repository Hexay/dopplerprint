//! Fit the posterior temperature and conformal HPD levels from cached eval surfaces (no re-search), then
//! score them on held-out runs. T maximises the mean log score at truth over calibration cases; q̂ per
//! stratum is the split-conformal quantile of HPD-split scores at that T. See docs/research/calibration_2026-11-03.md.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;

use super::CaseResult;
use super::conformal::q_hat;
use super::coverage::{Coverage, NOMINAL, Shape, measure};
use crate::search::calibration::{Calibration, Stratum};
use crate::search::surface::Surface;

/// (name, min events, max events).
pub const STRATA: [(&str, usize, usize); 3] = [("1 pass", 1, 1), ("2 passes", 2, 2), ("≥3 passes", 3, 99)];
/// Fewest real cases for a stratum's q̂ to come from real data alone (enough for the 90% level).
const MIN_REAL: usize = 9;

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
}

/// Every ok case with a cached surface in the run directories (`<id>.json`).
pub fn load(dirs: &[PathBuf]) -> Result<Vec<Case>> {
    let mut out = Vec::new();
    for d in dirs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(d)
            .with_context(|| format!("{}", d.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json") && p.file_stem().is_some_and(|s| s != "summary"))
            .collect();
        files.sort();
        for f in files {
            let r: CaseResult = serde_json::from_slice(&std::fs::read(&f)?).with_context(|| format!("{}", f.display()))?;
            if let (true, Some(surface), Some(truth)) = (r.status == "ok", r.surface, r.truth_lat_lon) {
                out.push(Case { id: r.id, n_events: r.n_events, surface, truth });
            }
        }
    }
    Ok(out)
}

pub fn stratum_of(n_events: usize) -> usize {
    STRATA.iter().position(|s| (s.1..=s.2).contains(&n_events)).unwrap_or(0)
}

pub fn mean_log_score(cases: &[&Case], sh: Shape) -> f64 {
    let s: f64 = cases.par_iter().map(|c| c.measure(sh, &[]).log_score).sum();
    s / cases.len().max(1) as f64
}

/// Golden-section maximum of the mean log score over ln T ∈ [ln 0.2, ln 50] (unimodal in practice; the
/// coarse curve in the report shows it).
pub fn fit_temperature(cases: &[&Case], base: Shape) -> f64 {
    let f = |lt: f64| mean_log_score(cases, Shape { temperature: lt.exp(), ..base });
    let (mut a, mut b) = (0.2f64.ln(), 50f64.ln());
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
    (0.5 * (a + b)).exp()
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
    /// (T, mean log score) on the T-fit cases, coarse grid.
    pub curve: Vec<(f64, f64)>,
    pub n_t_fit: usize,
}

/// `t_cases`: id substrings selecting the cases T is fitted on (all calibration cases when empty).
pub fn fit(calib: &[Case], t_cases: &[String], base: Shape, provenance: &str) -> Fitted {
    let all: Vec<&Case> = calib.iter().collect();
    let t_fit: Vec<&Case> =
        calib.iter().filter(|c| t_cases.is_empty() || t_cases.iter().any(|s| c.id.contains(s.as_str()))).collect();
    let t = fit_temperature(&t_fit, base);
    let sh = Shape { temperature: t, ..base };
    let curve = [0.5, 1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0]
        .iter()
        .map(|&x| (x, mean_log_score(&t_fit, Shape { temperature: x, ..base })))
        .collect();
    let calibration = Calibration {
        temperature: (t * 100.0).round() / 100.0,
        floor: base.floor,
        persistence_nats: base.persistence_nats,
        provenance: provenance.into(),
        stratum: strata(&all, sh),
    };
    Fitted { calibration, curve, n_t_fit: t_fit.len() }
}

pub fn write_toml(path: &Path, c: &Calibration) -> Result<()> {
    let head = "# Written by `dopplerprint calibrate`; see docs/research/calibration_2026-11-03.md.\n";
    std::fs::write(path, format!("{head}{}", toml::to_string(c)?))?;
    Ok(())
}
