//! Markdown for `dopplerprint calibrate`: the fit, q̂ per stratum, and per-set coverage, log score,
//! region area, peak-vs-mass mode choice and mirror splits, before (a previous calibration) vs after.

use std::fmt::Write as _;

use super::calib_section::section;
use super::calibrate::{Case, Fitted, Set};
use super::conformal::coverage_floor;
use crate::search::calibration::Calibration;

pub fn markdown(f: &Fitted, calib: &[Set], tests: &[Set], before: &Calibration, detail: &[String]) -> String {
    let c = &f.calibration;
    let mut md = String::from("# Calibration\n\n");
    let pooled: Vec<&Case> = calib.iter().flat_map(|s| &s.cases).collect();
    let n_sim = pooled.iter().filter(|x| x.is_sim()).count();
    let _ = writeln!(
        md,
        "{}\n\nCalibration cases {} ({} real, {} sim); T and ε fitted on {}. Fitted T = **{:.2}** z, ε = **{}** \
         (before: T {:.2}, ε {}); persistence {} nats.\n",
        c.provenance,
        pooled.len(),
        pooled.len() - n_sim,
        n_sim,
        f.n_t_fit,
        c.temperature,
        c.floor,
        before.temperature,
        before.floor,
        c.persistence_nats
    );
    md.push_str("| set | events | cases | unanswered (no region) |\n|---|---|---|---|\n");
    for s in calib.iter().chain(tests) {
        let _ = writeln!(md, "| {} | {} | {} | {} |", s.run.display(), s.events.name(), s.cases.len(), s.unanswered.len());
    }
    md.push_str(&curve("Mean log score at truth vs T (T-fit cases, fitted ε, ln km⁻²)", "T", &f.curve));
    if !f.floor_curve.is_empty() {
        md.push_str(&curve("Mean log score at truth vs ε (T-fit cases, fitted T)", "ε", &f.floor_curve));
    }
    md.push_str("\n## q̂ per stratum (HPD mass to report for nominal 50/80/90%)\n\n");
    md.push_str("| stratum | n | source | q̂50 | q̂80 | q̂90 | 5% floor of delivered 90% coverage | before q̂50/80/90 |\n|---|---|---|---|---|---|---|---|\n");
    for s in &c.stratum {
        let floor = coverage_floor(s.n, 0.9).map_or("n too small".into(), |x| format!("{x:.2}"));
        let q: Vec<String> = s.q_hat.iter().map(|x| format!("{x:.3}")).collect();
        let b = before.stratum(s.min_events).map_or("–".into(), |b| b.q_hat.iter().map(|x| format!("{x:.3}")).collect::<Vec<_>>().join(" / "));
        let _ = writeln!(md, "| {} | {} | {} | {} | {floor} | {b} |", s.name, s.n, s.source, q.join(" | "));
    }
    let _ = write!(md, "\n{}", section("calibration (in-sample, pooled)", &pooled, 0, c, before, detail));
    for s in tests {
        let cases: Vec<&Case> = s.cases.iter().collect();
        let name = format!("test: {} ({} events)", s.run.display(), s.events.name());
        let _ = write!(md, "\n{}", section(&name, &cases, s.unanswered.len(), c, before, detail));
    }
    md
}

fn curve(title: &str, x: &str, pts: &[(f64, f64)]) -> String {
    let mut md = format!("\n## {title}\n\n| {x} | ");
    md.push_str(&pts.iter().map(|p| format!("{}", p.0)).collect::<Vec<_>>().join(" | "));
    md.push_str(" |\n|---|");
    md.push_str(&"---|".repeat(pts.len()));
    md.push_str("\n| log score | ");
    md.push_str(&pts.iter().map(|p| format!("{:.2}", p.1)).collect::<Vec<_>>().join(" | "));
    md.push_str(" |\n");
    md
}
