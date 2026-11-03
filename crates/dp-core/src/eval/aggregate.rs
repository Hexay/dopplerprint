//! Run-level metrics with 95% bootstrap CIs, and stage-wise recall counts.

use std::fmt::Write as _;

use super::CaseResult;
use super::stats::{Ci, bootstrap, mean, median, rate};

/// Success tiers (m), after Sattler et al.: never read a tier finer than the case's truth uncertainty.
pub const TIERS_M: [f64; 4] = [100.0, 300.0, 1000.0, 3000.0];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Aggregate {
    pub n_ok: usize,
    pub median_error_m: Ci,
    pub hit: Vec<(f64, Ci)>,
    /// Error of the reported answer (`modes::answer`); `median_error_m` is the peak cell.
    #[serde(default)]
    pub answer_error_m: Option<Ci>,
    #[serde(default)]
    pub answer_hit300: Option<Ci>,
    pub flights_correct: Ci,
    pub mean_log_score: Ci,
    pub median_truth_cell_rank: Ci,
    pub median_score_gap: Ci,
    pub hpd90_coverage: Ci,
    pub median_hpd90_km2: Ci,
    /// Area-weighted posterior over refined cells at the shipped calibration (`eval::coverage`).
    #[serde(default)]
    pub calib_log_score: Option<Ci>,
    /// Coverage and median area (km²) of the shipped regions for each nominal level.
    #[serde(default)]
    pub calib_regions: Vec<(f64, Ci, Ci)>,
    /// (stage, survived, evaluated) in pipeline order.
    pub recall: Vec<(String, usize, usize)>,
}

pub fn aggregate(results: &[CaseResult]) -> Aggregate {
    let ok: Vec<&CaseResult> = results.iter().filter(|r| r.status == "ok").collect();
    let col = |f: &dyn Fn(&CaseResult) -> Option<f64>| -> Vec<f64> { ok.iter().filter_map(|r| f(r)).collect() };
    let errs = col(&|r| r.error_m);
    let answer = col(&|r| r.coverage.as_ref().and_then(|c| c.answer_err_m));
    Aggregate {
        n_ok: ok.len(),
        median_error_m: bootstrap(&errs, median),
        hit: TIERS_M.iter().map(|&t| (t, rate(errs.iter().map(|&e| e <= t)))).collect(),
        answer_error_m: Some(bootstrap(&answer, median)),
        answer_hit300: Some(rate(answer.iter().map(|&e| e <= 300.0))),
        flights_correct: rate(ok.iter().filter_map(|r| r.flights_correct)),
        mean_log_score: bootstrap(&col(&|r| r.metrics.log_score), mean),
        median_truth_cell_rank: bootstrap(&col(&|r| r.metrics.truth_cell_rank.map(|x| x as f64)), median),
        median_score_gap: bootstrap(&col(&|r| r.metrics.score_gap), median),
        hpd90_coverage: rate(ok.iter().filter_map(|r| r.metrics.hpd90_contains)),
        median_hpd90_km2: bootstrap(&col(&|r| r.metrics.hpd90_km2), median),
        calib_log_score: Some(bootstrap(&col(&|r| r.coverage.as_ref().map(|c| c.log_score)), mean)),
        calib_regions: calib_regions(&ok),
        recall: recall(&ok),
    }
}

fn calib_regions(ok: &[&CaseResult]) -> Vec<(f64, Ci, Ci)> {
    let cov: Vec<&super::coverage::Coverage> = ok.iter().filter_map(|r| r.coverage.as_ref()).collect();
    super::coverage::NOMINAL
        .iter()
        .enumerate()
        .map(|(j, &nom)| {
            let hit = rate(cov.iter().filter_map(|c| c.covered().get(j).copied()));
            let area: Vec<f64> = cov.iter().filter_map(|c| c.area_km2.get(j).copied()).collect();
            (nom, hit, bootstrap(&area, median))
        })
        .collect()
}

fn recall(ok: &[&CaseResult]) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for s in ok.iter().flat_map(|r| &r.metrics.stages) {
        let i = out.iter().position(|o| o.0 == s.stage).unwrap_or_else(|| {
            out.push((s.stage.clone(), 0, 0));
            out.len() - 1
        });
        out[i].1 += s.ok as usize;
        out[i].2 += 1;
    }
    out
}

pub fn markdown(a: &Aggregate) -> String {
    let mut md = format!("## Metrics (n = {}, estimate [95% bootstrap CI over cases])\n\n", a.n_ok);
    md.push_str("| metric | value |\n|---|---|\n");
    let mut row = |k: &str, v: String| {
        let _ = writeln!(md, "| {k} | {v} |");
    };
    row("median error m", a.median_error_m.fmt(1));
    for (t, ci) in &a.hit {
        row(&format!("error ≤ {t} m"), ci.fmt(2));
    }
    if let (Some(e), Some(h)) = (&a.answer_error_m, &a.answer_hit300) {
        row("median error m, reported answer (peak's core centroid) / ≤ 300 m",format!("{} / {}", e.fmt(1), h.fmt(2)));
    }
    row("flights correct", a.flights_correct.fmt(2));
    row("mean log score (nats, coarse cell)", a.mean_log_score.fmt(2));
    row("median truth-cell rank", a.median_truth_cell_rank.fmt(0));
    row("median score gap (best − truth)", a.median_score_gap.fmt(2));
    row("90% region contains truth", a.hpd90_coverage.fmt(2));
    row("median 90% region km²", a.median_hpd90_km2.fmt(2));
    if let Some(ci) = &a.calib_log_score {
        row("calibrated log score (ln km⁻², refined cells)", ci.fmt(2));
    }
    for (nom, hit, area) in &a.calib_regions {
        row(&format!("calibrated {:.0}% region: contains truth / median km²", nom * 100.0), format!("{} / {}", hit.fmt(2), area.fmt(2)));
    }
    md.push_str("\n## Stage recall (truth survived / evaluated)\n\n");
    let stages: Vec<String> = a.recall.iter().map(|(s, k, n)| format!("{s} {k}/{n}")).collect();
    let _ = writeln!(md, "{}", stages.join(" → "));
    md
}
