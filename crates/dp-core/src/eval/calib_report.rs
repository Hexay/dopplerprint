//! Markdown for `dopplerprint calibrate`: the fit, q̂ per stratum, and held-out coverage, log score,
//! peak-vs-mass mode choice and mirror splits.

use std::fmt::Write as _;

use rayon::prelude::*;

use super::calibrate::{Case, Fitted, STRATA, stratum_of};
use super::conformal::{clopper_pearson, coverage_floor};
use super::coverage::{Coverage, NOMINAL, Shape};
use super::stats::{bootstrap, mean, median};

/// Per case: coverage at the old shape, and at the fitted shape with areas at that stratum's q̂.
struct Scored<'a> {
    case: &'a Case,
    before: Coverage,
    after: Coverage,
}

fn score<'a>(cases: &'a [Case], f: &Fitted, before: Shape) -> Vec<Scored<'a>> {
    let c = &f.calibration;
    let after = Shape { temperature: c.temperature, floor: c.floor, persistence_nats: c.persistence_nats };
    cases
        .par_iter()
        .map(|case| {
            let q = &c.stratum[stratum_of(case.n_events)].q_hat;
            Scored { case, before: case.measure(before, &NOMINAL), after: case.measure(after, q) }
        })
        .collect()
}

fn frac(k: usize, n: usize) -> String {
    if n == 0 {
        return "–".into();
    }
    let (lo, hi) = clopper_pearson(k, n);
    format!("{k}/{n} = {:.2} [{lo:.2}, {hi:.2}]", k as f64 / n as f64)
}

pub fn markdown(f: &Fitted, calib: &[Case], test: &[Case], before: Shape) -> String {
    let c = &f.calibration;
    let mut md = String::from("# Calibration\n\n");
    let n_sim = calib.iter().filter(|x| x.is_sim()).count();
    let _ = writeln!(
        md,
        "{}\n\nCalibration cases {} ({} real, {} sim); T fitted on {}. Fitted T = **{:.2}** z (before {:.2}); ε = {}, persistence {} nats.\n",
        c.provenance,
        calib.len(),
        calib.len() - n_sim,
        n_sim,
        f.n_t_fit,
        c.temperature,
        before.temperature,
        c.floor,
        c.persistence_nats
    );
    md.push_str("## Mean log score at truth vs T (T-fit cases, ln km⁻²)\n\n| T | ");
    md.push_str(&f.curve.iter().map(|x| format!("{}", x.0)).collect::<Vec<_>>().join(" | "));
    md.push_str(" |\n|---|");
    md.push_str(&"---|".repeat(f.curve.len()));
    md.push_str("\n| log score | ");
    md.push_str(&f.curve.iter().map(|x| format!("{:.2}", x.1)).collect::<Vec<_>>().join(" | "));
    md.push_str(" |\n\n## q̂ per stratum (HPD mass to report for nominal 50/80/90%)\n\n");
    md.push_str("| stratum | n | source | q̂50 | q̂80 | q̂90 | 5% floor of delivered 90% coverage |\n|---|---|---|---|---|---|---|\n");
    for s in &c.stratum {
        let floor = coverage_floor(s.n, 0.9).map_or("n too small".into(), |x| format!("{x:.2}"));
        let q: Vec<String> = s.q_hat.iter().map(|x| format!("{x:.3}")).collect();
        let _ = writeln!(md, "| {} | {} | {} | {} | {floor} |", s.name, s.n, s.source, q.join(" | "));
    }
    for (name, set) in [("calibration (in-sample)", calib), ("test (held out)", test)] {
        let _ = write!(md, "\n{}", section(name, &score(set, f, before), c, before));
    }
    md
}

fn section(name: &str, rows: &[Scored<'_>], c: &crate::search::calibration::Calibration, before: Shape) -> String {
    let mut md = format!("## {name}\n\n");
    if rows.is_empty() {
        md.push_str("no cases\n");
        return md;
    }
    md.push_str("Coverage = truth inside the HPD region (k/n [95% Clopper–Pearson]). Raw = region of the nominal mass;\n");
    let _ = writeln!(md, "conformal = region of mass q̂ at the fitted T. Before = T {:.2}.\n", before.temperature);
    md.push_str("| stratum | nominal | raw, before | raw, fitted T | conformal | median area km² (conformal) |\n|---|---|---|---|---|---|\n");
    for (k, st) in STRATA.iter().enumerate() {
        let r: Vec<&Scored> = rows.iter().filter(|r| stratum_of(r.case.n_events) == k).collect();
        if r.is_empty() {
            continue;
        }
        for (j, &nom) in NOMINAL.iter().enumerate() {
            let q = c.stratum[k].q_hat[j];
            let hits = |f: &dyn Fn(&Scored) -> bool| frac(r.iter().filter(|x| f(x)).count(), r.len());
            let areas: Vec<f64> = r.iter().map(|x| x.after.area_km2[j]).collect();
            let _ = writeln!(
                md,
                "| {} | {nom} | {} | {} | {} | {:.2} |",
                st.0,
                hits(&|x| x.before.hpd_score <= nom),
                hits(&|x| x.after.hpd_score <= nom),
                hits(&|x| x.after.hpd_score <= q),
                median(&areas)
            );
        }
    }
    md.push_str("\n| stratum | n | log score before | log score fitted | paired Δ |\n|---|---|---|---|---|\n");
    for (k, st) in STRATA.iter().enumerate() {
        let r: Vec<&Scored> = rows.iter().filter(|r| stratum_of(r.case.n_events) == k).collect();
        if r.is_empty() {
            continue;
        }
        let b: Vec<f64> = r.iter().map(|x| x.before.log_score).collect();
        let a: Vec<f64> = r.iter().map(|x| x.after.log_score).collect();
        let d: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
        let (b, a, d) = (bootstrap(&b, mean), bootstrap(&a, mean), bootstrap(&d, mean));
        let _ = writeln!(md, "| {} | {} | {} | {} | {} |", st.0, r.len(), b.fmt(2), a.fmt(2), d.fmt(2));
    }
    md.push_str(&modes(rows));
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
    let mut md = format!("\nModes at fitted T: {multi}/{n} cases have > 1 mode (≥ 1% mass); peak and mass pick different modes in {differ}.\n\n");
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
