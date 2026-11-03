//! Paired comparison of two eval runs (variant B against baseline A), per recording.

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

/// (name, accessor, lower is better).
const METRICS: [Metric; 5] = [
    ("error m", |r| r.error_m, true),
    ("log score", |r| r.metrics.log_score, false),
    ("truth-cell rank", |r| r.metrics.truth_cell_rank.map(|x| x as f64), true),
    ("score gap", |r| r.metrics.score_gap, true),
    ("|clock err| s", |r| r.clock_err_s.map(f64::abs), true),
];

/// Markdown table of paired deltas B − A with bootstrap CIs and win/loss counts, over cases whose id
/// contains one of `only` (all when empty).
pub fn compare(a_dir: &Path, b_dir: &Path, only: &[String]) -> Result<String> {
    let (a, b) = (load(a_dir)?, load(b_dir)?);
    let pairs: Vec<(&CaseResult, &CaseResult)> = a
        .iter()
        .filter(|r| r.status == "ok" && (only.is_empty() || only.iter().any(|f| r.id.contains(f.as_str()))))
        .filter_map(|ra| b.iter().find(|rb| rb.id == ra.id && rb.status == "ok").map(|rb| (ra, rb)))
        .collect();
    let mut md = format!("# {} vs {}\n\n{} paired cases. Δ = B − A.\n\n", b_dir.display(), a_dir.display(), pairs.len());
    md.push_str("| metric | n | mean Δ [95% CI] | median Δ [95% CI] | B better / worse / tie |\n|---|---|---|---|---|\n");
    for (name, get, lower_better) in METRICS {
        let d: Vec<f64> = pairs.iter().filter_map(|(ra, rb)| Some(get(rb)? - get(ra)?)).filter(|x| x.is_finite()).collect();
        let better = d.iter().filter(|&&x| if lower_better { x < -1e-9 } else { x > 1e-9 }).count();
        let tie = d.iter().filter(|x| x.abs() <= 1e-9).count();
        let _ = writeln!(
            md,
            "| {name} | {} | {} | {} | {better} / {} / {tie} |",
            d.len(),
            bootstrap(&d, mean).fmt(2),
            bootstrap(&d, median).fmt(2),
            d.len() - better - tie
        );
    }
    let flip = |f: fn(&CaseResult) -> Option<bool>| {
        let gained = pairs.iter().filter(|(ra, rb)| f(ra) == Some(false) && f(rb) == Some(true)).count();
        let lost = pairs.iter().filter(|(ra, rb)| f(ra) == Some(true) && f(rb) == Some(false)).count();
        format!("+{gained} / −{lost}")
    };
    let _ = writeln!(md, "\nFlights correct flips {}; in-90%-region flips {}.", flip(|r| r.flights_correct), flip(|r| r.metrics.hpd90_contains));
    Ok(md)
}
