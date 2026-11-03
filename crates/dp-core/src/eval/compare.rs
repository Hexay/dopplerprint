//! Paired comparison of two eval runs (variant B against baseline A), per recording, and of the candidate
//! answers (peak cell vs mode centroids) within one run.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use super::CaseResult;
use super::stats::{bootstrap, mean, median};

fn load(run: &Path) -> Result<Vec<CaseResult>> {
    let path = run.join("summary.json");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).with_context(|| format!("{}", path.display()))?)?;
    Ok(serde_json::from_value(v["results"].clone())?)
}

type Metric = (&'static str, fn(&CaseResult) -> Option<f64>, bool);

fn cov(r: &CaseResult, f: fn(&super::coverage::Coverage) -> Option<f64>) -> Option<f64> {
    r.coverage.as_ref().and_then(f)
}

/// (name, accessor, lower is better).
const METRICS: [Metric; 11] = [
    ("error m", |r| r.error_m, true),
    ("error m, heaviest-mode centroid", |r| cov(r, |c| c.mass_centroid_err_m), true),
    ("error m, peak-mode centroid", |r| cov(r, |c| c.peak_centroid_err_m), true),
    ("error m, reported answer", |r| cov(r, |c| c.answer_err_m), true),
    ("log score", |r| r.metrics.log_score, false),
    ("calibrated log score", |r| cov(r, |c| Some(c.log_score)), false),
    ("calibrated 90% area km²", |r| cov(r, |c| c.area_km2.get(2).copied()), true),
    ("truth-cell rank", |r| r.metrics.truth_cell_rank.map(|x| x as f64), true),
    ("score gap", |r| r.metrics.score_gap, true),
    ("|clock err| s", |r| r.clock_err_s.map(f64::abs), true),
    ("runtime s", |r| Some(r.runtime_s), true),
];

/// Candidate answers of one run: (name, error accessor). The first is the reference.
type Answer = (&'static str, fn(&CaseResult) -> Option<f64>);

const ANSWERS: [Answer; 9] = [
    ("peak cell", |r| r.error_m),
    ("peak of heaviest mode", |r| cov(r, |c| c.mass_choice_err_m)),
    ("centroid of peak's mode", |r| cov(r, |c| c.peak_centroid_err_m)),
    ("centroid of heaviest mode", |r| cov(r, |c| c.mass_centroid_err_m)),
    ("peak's core centroid, 0.5 nats", |r| cov(r, |c| c.core_err_m.first().copied())),
    ("peak's core centroid, 1 nat", |r| cov(r, |c| c.core_err_m.get(1).copied())),
    ("peak's core centroid, 2 nats", |r| cov(r, |c| c.core_err_m.get(2).copied())),
    ("peak's core centroid, 4 nats", |r| cov(r, |c| c.core_err_m.get(3).copied())),
    ("reported answer", |r| cov(r, |c| c.answer_err_m)),
];

fn wanted(r: &CaseResult, only: &[String]) -> bool {
    r.status == "ok" && (only.is_empty() || only.iter().any(|f| r.id.contains(f.as_str())))
}

/// Count of B better / worse / tie as "b / w / t".
fn tally(d: &[f64], lower_better: bool) -> String {
    let better = d.iter().filter(|&&x| if lower_better { x < -1e-9 } else { x > 1e-9 }).count();
    let tie = d.iter().filter(|x| x.abs() <= 1e-9).count();
    format!("{better} / {} / {tie}", d.len() - better - tie)
}

/// Markdown table of paired deltas B − A with bootstrap CIs and win/loss counts, over cases whose id
/// contains one of `only` (all when empty), then B's answer table.
pub fn compare(a_dir: &Path, b_dir: &Path, only: &[String]) -> Result<String> {
    let (a, b) = (load(a_dir)?, load(b_dir)?);
    let pairs: Vec<(&CaseResult, &CaseResult)> = a
        .iter()
        .filter(|r| wanted(r, only))
        .filter_map(|ra| b.iter().find(|rb| rb.id == ra.id && rb.status == "ok").map(|rb| (ra, rb)))
        .collect();
    let mut md = format!("# {} vs {}\n\n{} paired cases. Δ = B − A.\n\n", b_dir.display(), a_dir.display(), pairs.len());
    md.push_str("| metric | n | mean Δ [95% CI] | median Δ [95% CI] | B better / worse / tie |\n|---|---|---|---|---|\n");
    for (name, get, lower_better) in METRICS {
        let d: Vec<f64> = pairs.iter().filter_map(|(ra, rb)| Some(get(rb)? - get(ra)?)).filter(|x| x.is_finite()).collect();
        let _ = writeln!(md, "| {name} | {} | {} | {} | {} |", d.len(), bootstrap(&d, mean).fmt(2), bootstrap(&d, median).fmt(2), tally(&d, lower_better));
    }
    let flip = |f: &dyn Fn(&CaseResult) -> Option<bool>| {
        let gained = pairs.iter().filter(|(ra, rb)| f(ra) == Some(false) && f(rb) == Some(true)).count();
        let lost = pairs.iter().filter(|(ra, rb)| f(ra) == Some(true) && f(rb) == Some(false)).count();
        format!("+{gained} / −{lost}")
    };
    let refined = |r: &CaseResult| Some(!r.metrics.lost_at.as_deref().is_some_and(|s| s.starts_with('L')));
    let _ = writeln!(
        md,
        "\nFlights correct flips {}; in-90%-region flips {}; in calibrated 90% region flips {}; truth refined to the finest level flips {}.\n",
        flip(&|r| r.flights_correct),
        flip(&|r| r.metrics.hpd90_contains),
        flip(&|r| r.coverage.as_ref().and_then(|c| c.covered().get(2).copied())),
        flip(&refined)
    );
    md.push_str(&answers(&b.iter().filter(|r| wanted(r, only)).collect::<Vec<_>>()));
    Ok(md)
}

/// Each candidate answer's median error and ≤ 300 m count, paired against the peak cell.
pub fn answers(rs: &[&CaseResult]) -> String {
    let mut md = String::from("| answer (B) | n | median error m [95% CI] | ≤ 300 m | median Δ vs peak cell [95% CI] | better / worse / tie |\n|---|---|---|---|---|---|\n");
    let reference = ANSWERS[0].1;
    for (name, get) in ANSWERS {
        let both: Vec<(f64, f64)> = rs.iter().filter_map(|r| Some((get(r)?, reference(r)?))).collect();
        let errs: Vec<f64> = both.iter().map(|x| x.0).collect();
        let d: Vec<f64> = both.iter().map(|x| x.0 - x.1).collect();
        let _ = writeln!(
            md,
            "| {name} | {} | {} | {} | {} | {} |",
            errs.len(),
            bootstrap(&errs, median).fmt(1),
            errs.iter().filter(|&&e| e <= 300.0).count(),
            bootstrap(&d, median).fmt(1),
            tally(&d, true)
        );
    }
    md
}
