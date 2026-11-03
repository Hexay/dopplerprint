use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;

use super::CaseResult;
use super::aggregate::{aggregate, markdown};
use crate::config::Config;

fn opt(v: Option<f64>, prec: usize) -> String {
    v.map_or("–".into(), |x| format!("{x:.prec$}"))
}

fn opt_u(v: Option<usize>) -> String {
    v.map_or("–".into(), |x| x.to_string())
}

fn lost(r: &CaseResult) -> String {
    r.metrics.lost_at.clone().unwrap_or_else(|| "–".into())
}

pub fn one_line(r: &CaseResult) -> String {
    if r.status != "ok" {
        return r.status.clone();
    }
    let t = &r.timing;
    let stages = format!(
        "[pose {:.1} plane {:.1} plan {:.1} score {:.1} metrics {:.1}]",
        t.pose_s, t.plane_s, t.plan_s, t.score_s, t.metrics_s
    );
    format!(
        "err {:>8} m  flights {:<5}  rank {:>3}  margin {:>6}  clock {:>6} s  cells {:>6}  {:.1} s {stages}  lost {:<7} logS {:>6}  gap {:>6}",
        opt(r.error_m, 1),
        r.flights_correct.map_or("–".into(), |b| b.to_string()),
        opt_u(r.truth_tuple_rank),
        opt(r.margin, 2),
        opt(r.clock_err_s, 2),
        r.cells,
        r.runtime_s,
        lost(r),
        opt(r.metrics.log_score, 2),
        opt(r.metrics.score_gap, 2),
    )
}

pub fn write(dir: &Path, cfg: &Config, results: &[CaseResult]) -> Result<()> {
    let ok: Vec<&CaseResult> = results.iter().filter(|r| r.status == "ok").collect();
    let mut errs: Vec<f64> = ok.iter().filter_map(|r| r.error_m).collect();
    errs.sort_by(f64::total_cmp);
    let median = errs.get(errs.len() / 2).copied();
    let correct = ok.iter().filter(|r| r.flights_correct == Some(true)).count();
    let within = |m: f64| errs.iter().filter(|&&e| e <= m).count();

    let mut md = String::from("# doppler_print eval\n\n");
    let _ = writeln!(
        md,
        "{} cases, {} ran, {} skipped. Correct flights {}/{}. Median error {} m; ≤100 m {}, ≤500 m {}.\n",
        results.len(),
        ok.len(),
        results.len() - ok.len(),
        correct,
        ok.len(),
        opt(median, 1),
        within(100.0),
        within(500.0)
    );
    md.push_str("| case | status | error m | flights | rank | margin | clock s | tracks | cells | s | lost at | gap | cell rank | log score | in 90% |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in results {
        let status = if r.status == "ok" { "ok" } else { r.status.split(':').next().unwrap_or("") };
        let m = &r.metrics;
        let _ = writeln!(
            md,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} | {} | {} | {} | {} | {} |",
            r.id,
            status,
            opt(r.error_m, 1),
            r.flights_correct.map_or("–".into(), |b| b.to_string()),
            opt_u(r.truth_tuple_rank),
            opt(r.margin, 2),
            opt(r.clock_err_s, 2),
            r.n_tracks,
            r.cells,
            r.runtime_s,
            lost(r),
            opt(m.score_gap, 2),
            opt_u(m.truth_cell_rank),
            opt(m.log_score, 2),
            m.hpd90_contains.map_or("–".into(), |b| b.to_string()),
        );
    }
    let agg = aggregate(results);
    let _ = write!(md, "\n{}", markdown(&agg));
    let skipped: Vec<String> = results.iter().filter(|r| r.status != "ok").map(|r| format!("- {}: {}", r.id, r.status)).collect();
    if !skipped.is_empty() {
        let _ = writeln!(md, "\n## Skipped\n\n{}", skipped.join("\n"));
    }
    std::fs::write(dir.join("summary.md"), md)?;
    let slim: Vec<_> = results.iter().map(|r| CaseResult { tuples: Vec::new(), surface: None, ..r.clone() }).collect();
    let json = serde_json::json!({ "config": cfg, "aggregate": agg, "results": slim });
    std::fs::write(dir.join("summary.json"), serde_json::to_vec_pretty(&json)?)?;
    Ok(())
}
