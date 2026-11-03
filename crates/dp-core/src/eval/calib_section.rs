//! One section of the `dopplerprint calibrate` report: coverage, region area and log score per stratum
//! (before vs after), mode choice, mirror splits, and rows for watched cases.

use std::fmt::Write as _;

use rayon::prelude::*;

use super::calibrate::{Case, STRATA, stratum_of};
use super::conformal::clopper_pearson;
use super::coverage::{Coverage, NOMINAL, Shape, masses};
use super::stats::{bootstrap, mean, median};
use crate::search::calibration::Calibration;

struct Scored<'a> {
    case: &'a Case,
    before: Coverage,
    after: Coverage,
}

fn frac(k: usize, n: usize) -> String {
    if n == 0 {
        return "–".into();
    }
    let (lo, hi) = clopper_pearson(k, n);
    format!("{k}/{n} = {:.2} [{lo:.2}, {hi:.2}]", k as f64 / n as f64)
}

pub fn section(name: &str, cases: &[&Case], unanswered: usize, c: &Calibration, before: &Calibration, detail: &[String]) -> String {
    let mut md = format!("## {name}\n\n");
    if cases.is_empty() {
        md.push_str("no answered cases\n");
        return md;
    }
    let rows: Vec<Scored> = cases
        .par_iter()
        .map(|&case| Scored {
            case,
            before: case.measure(Shape::of(before), &masses(before, case.n_events)),
            after: case.measure(Shape::of(c), &masses(c, case.n_events)),
        })
        .collect();
    let _ = writeln!(
        md,
        "Coverage = truth inside the HPD region (k/n [95% Clopper–Pearson]). Before = T {:.2}, ε {} with its q̂; \
         raw = nominal mass at the fitted shape; after = fitted q̂. Areas are medians (km²).\n",
        before.temperature, before.floor
    );
    md.push_str("| stratum | nominal | before | raw, fitted | after | area before | area after |\n|---|---|---|---|---|---|---|\n");
    let groups = groups(&rows);
    for (gname, r) in &groups {
        for (j, &nom) in NOMINAL.iter().enumerate() {
            let hits = |f: &dyn Fn(&Scored) -> bool| frac(r.iter().filter(|x| f(x)).count(), r.len());
            let area = |f: &dyn Fn(&Scored) -> f64| median(&r.iter().map(|x| f(x)).collect::<Vec<_>>());
            let _ = writeln!(
                md,
                "| {gname} | {nom} | {} | {} | {} | {:.2} | {:.2} |",
                hits(&|x| x.before.covered()[j]),
                hits(&|x| x.after.hpd_score <= nom),
                hits(&|x| x.after.covered()[j]),
                area(&|x| x.before.area_km2[j]),
                area(&|x| x.after.area_km2[j]),
            );
        }
    }
    if unanswered > 0 {
        let k = rows.iter().filter(|x| x.after.covered()[2]).count();
        let _ = writeln!(md, "\n{unanswered} unanswered (no region). Counting them as misses, 90% after = {}.", frac(k, rows.len() + unanswered));
    }
    md.push_str("\n| stratum | n | log score before | log score after | paired Δ |\n|---|---|---|---|---|\n");
    for (gname, r) in &groups {
        let b: Vec<f64> = r.iter().map(|x| x.before.log_score).collect();
        let a: Vec<f64> = r.iter().map(|x| x.after.log_score).collect();
        let d: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
        let (b, a, d) = (bootstrap(&b, mean), bootstrap(&a, mean), bootstrap(&d, mean));
        let _ = writeln!(md, "| {gname} | {} | {} | {} | {} |", r.len(), b.fmt(2), a.fmt(2), d.fmt(2));
    }
    md.push_str(&modes(&rows));
    md.push_str(&watched(&rows, detail));
    md
}

/// Rows per stratum present, then all of them when more than one stratum is.
fn groups<'a, 'b>(rows: &'b [Scored<'a>]) -> Vec<(String, Vec<&'b Scored<'a>>)> {
    let mut g: Vec<(String, Vec<&Scored>)> = STRATA
        .iter()
        .enumerate()
        .map(|(k, st)| (st.0.to_string(), rows.iter().filter(|r| stratum_of(r.case.n_events) == k).collect::<Vec<_>>()))
        .filter(|(_, r)| !r.is_empty())
        .collect();
    if g.len() > 1 {
        g.push(("all".into(), rows.iter().collect()));
    }
    g
}

fn watched(rows: &[Scored<'_>], detail: &[String]) -> String {
    let mut w: Vec<&Scored> = rows.iter().filter(|r| detail.iter().any(|d| r.case.id.contains(d.as_str()))).collect();
    if w.is_empty() {
        return String::new();
    }
    w.sort_by(|a, b| a.case.id.cmp(&b.case.id));
    let mut md = String::from("\n| case | passes | log score before → after | HPD score before → after | in 90% before → after | 90% area km² before → after |\n|---|---|---|---|---|---|\n");
    for r in w {
        let (b, a) = (&r.before, &r.after);
        let _ = writeln!(
            md,
            "| {} | {} | {:.2} → {:.2} | {:.3} → {:.3} | {} → {} | {:.2} → {:.2} |",
            r.case.id, r.case.n_events, b.log_score, a.log_score, b.hpd_score, a.hpd_score, b.covered()[2], a.covered()[2], b.area_km2[2], a.area_km2[2]
        );
    }
    md
}

fn modes(rows: &[Scored<'_>]) -> String {
    let cov: Vec<&Coverage> = rows.iter().map(|r| &r.after).collect();
    let n = cov.len();
    let in_mode: Vec<&&Coverage> = cov.iter().filter(|c| c.truth_mode.is_some()).collect();
    let by_mass = cov.iter().filter(|c| c.truth_mode == Some(0)).count();
    let by_peak = cov.iter().filter(|c| c.truth_mode == Some(c.peak_mode)).count();
    let differ = cov.iter().filter(|c| c.peak_mode != 0).count();
    let loss = |f: fn(&Coverage) -> Option<f64>| bootstrap(&in_mode.iter().filter_map(|c| f(c)).collect::<Vec<_>>(), mean);
    let err = |f: fn(&Coverage) -> Option<f64>| median(&cov.iter().filter_map(|c| f(c)).collect::<Vec<_>>());
    let multi = cov.iter().filter(|c| c.n_modes > 1).count();
    let mut md = format!("\nModes after: {multi}/{n} cases have > 1 mode (≥ 1% mass); peak and mass pick different modes in {differ}.\n\n");
    md.push_str("| choice | truth in chosen mode | mode log loss (nats) | median error of chosen peak m |\n|---|---|---|---|\n");
    let _ = writeln!(md, "| by mass | {} | {} | {:.0} |", frac(by_mass, n), loss(|c| c.mode_loss_mass).fmt(3), err(|c| c.mass_choice_err_m));
    let _ = writeln!(md, "| by peak | {} | {} | {:.0} |", frac(by_peak, n), loss(|c| c.mode_loss_peak).fmt(3), err(|c| c.peak_choice_err_m));
    let single: Vec<_> = rows.iter().filter(|r| r.case.n_events == 1).filter_map(|r| r.after.mirror).collect();
    if !single.is_empty() {
        let right = single.iter().filter(|m| m.heavier_is_truth).count();
        let sure_wrong = single.iter().filter(|m| !m.heavier_is_truth && m.split > 0.8).count();
        let splits: Vec<f64> = single.iter().map(|m| m.split).collect();
        let _ = writeln!(
            md,
            "\nSingle passes with a same-flight rival mode holding the truth: {}. Heavier side is the true side {}; \
             median split {:.2}; heavier side > 80% and wrong in {}.",
            single.len(),
            frac(right, single.len()),
            median(&splits),
            sure_wrong
        );
    }
    md
}
