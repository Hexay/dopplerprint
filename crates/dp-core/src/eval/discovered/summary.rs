//! Markdown comparing discovered with oracle events on the same cases. Unlike the standard summary,
//! a case where nothing was searched counts as a miss (infinite error) instead of dropping out.

use std::fmt::Write as _;

use super::truth::Source;
use super::{FrontRecord, Paired};
use crate::eval::CaseResult;
use crate::eval::aggregate::{TIERS_M, aggregate};
use crate::eval::stats::median;

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub found: usize,
    pub labelled: usize,
    pub adsb: usize,
    pub spurious: usize,
    pub used: usize,
    pub used_spurious: usize,
}

pub fn counts(f: &FrontRecord) -> Counts {
    let mut c = Counts { found: f.events.len(), ..Default::default() };
    for e in &f.events {
        match e.truth.source {
            Some(Source::Labelled) => c.labelled += 1,
            Some(Source::Adsb) => c.adsb += 1,
            None => c.spurious += 1,
        }
        c.used += e.used as usize;
        c.used_spurious += (e.used && e.truth.source.is_none()) as usize;
    }
    c
}

pub fn err(r: &CaseResult) -> f64 {
    r.error_m.unwrap_or(f64::INFINITY)
}

fn fmt_m(x: f64) -> String {
    if x.is_finite() { format!("{x:.0}") } else { "none".into() }
}

fn frac(k: usize, n: usize) -> String {
    if n == 0 { "–".into() } else { format!("{k}/{n} ({:.0}%)", 100.0 * k as f64 / n as f64) }
}

pub fn markdown(pairs: &[Paired]) -> String {
    let ok: Vec<&Paired> = pairs.iter().filter(|p| p.oracle.status == "ok" && p.discovered.status == "ok").collect();
    let mut md = format!("# Discovered vs oracle events\n\n{} cases, {} ran in both modes.\n\n", pairs.len(), ok.len());
    headline(&mut md, &ok);
    stages(&mut md, &ok);
    front_end(&mut md, &ok);
    per_case(&mut md, &ok);
    md.push_str("\n---\n\n");
    md
}

fn headline(md: &mut String, ok: &[&Paired]) {
    let n = ok.len();
    md.push_str("| metric (no answer = miss) | oracle | discovered |\n|---|---|---|\n");
    let mut row = |k: &str, o: String, d: String| {
        let _ = writeln!(md, "| {k} | {o} | {d} |");
    };
    let col = |f: &dyn Fn(&Paired) -> &CaseResult| -> Vec<&CaseResult> { ok.iter().map(|p| f(p)).collect() };
    let (o, d) = (col(&|p| &p.oracle), col(&|p| &p.discovered));
    let answered = |v: &[&CaseResult]| frac(v.iter().filter(|r| r.best.is_some()).count(), n);
    row("answered", answered(&o), answered(&d));
    let med = |v: &[&CaseResult]| fmt_m(median(&v.iter().map(|r| err(r)).collect::<Vec<_>>()));
    row("median error m", med(&o), med(&d));
    for t in TIERS_M {
        let hit = |v: &[&CaseResult]| frac(v.iter().filter(|r| err(r) <= t).count(), n);
        row(&format!("error ≤ {t} m"), hit(&o), hit(&d));
    }
    let known: Vec<&&Paired> = ok.iter().filter(|p| p.oracle.flights_correct.is_some()).collect();
    let fc = |f: &dyn Fn(&Paired) -> Option<bool>| frac(known.iter().filter(|p| f(p) == Some(true)).count(), known.len());
    row("flights correct", fc(&|p| p.oracle.flights_correct), fc(&|p| p.discovered.flights_correct));
    let both: Vec<(f64, f64)> = ok.iter().filter_map(|p| p.oracle.metrics.log_score.zip(p.discovered.metrics.log_score)).collect();
    let mean = |f: fn(&(f64, f64)) -> f64| format!("{:.2} (n {})", both.iter().map(f).sum::<f64>() / both.len().max(1) as f64, both.len());
    row("mean log score (paired)", mean(|x| x.0), mean(|x| x.1));
    let hpd = |v: &[&CaseResult]| frac(v.iter().filter(|r| r.metrics.hpd90_contains == Some(true)).count(), n);
    row("90% region contains truth", hpd(&o), hpd(&d));
    let shipped = |v: &[&CaseResult]| {
        let hits = v.iter().filter_map(|r| r.coverage.as_ref()).filter(|c| c.covered().last() == Some(&true)).count();
        frac(hits, n)
    };
    row("shipped calibrated 90% region contains truth", shipped(&o), shipped(&d));
}

fn stages(md: &mut String, ok: &[&Paired]) {
    let line = |v: Vec<CaseResult>| {
        let mut recall = aggregate(&v).recall;
        recall.sort_by_key(|(s, _, _)| pipeline_order(s));
        recall.iter().map(|(s, k, n)| format!("{s} {k}/{n}")).collect::<Vec<_>>().join(" → ")
    };
    let _ = writeln!(md, "\nStage recall, oracle: {}", line(ok.iter().map(|p| p.oracle.clone()).collect()));
    let _ = writeln!(md, "\nStage recall, discovered: {}\n", line(ok.iter().map(|p| p.discovered.clone()).collect()));
    let mut names: Vec<String> = Vec::new();
    let lost = |r: &CaseResult| r.metrics.lost_at.clone().unwrap_or_else(|| "survived".into());
    for p in ok {
        for s in [lost(&p.oracle), lost(&p.discovered)] {
            if !names.contains(&s) {
                names.push(s);
            }
        }
    }
    md.push_str("| lost at | oracle | discovered |\n|---|---|---|\n");
    for s in names {
        let k = |f: fn(&Paired) -> &CaseResult| ok.iter().filter(|p| lost(f(p)) == s).count();
        let _ = writeln!(md, "| {s} | {} | {} |", k(|p| &p.oracle), k(|p| &p.discovered));
    }
}

/// Front stages, flight stages, then lattice levels coarse to fine.
fn pipeline_order(stage: &str) -> (usize, i64) {
    match stage {
        "discover" => (0, 0),
        "select" => (1, 0),
        "catalog" => (2, 0),
        "near" => (3, 0),
        "assoc" => (4, 0),
        s => (5, -s.trim_start_matches('L').parse::<i64>().unwrap_or(0)),
    }
}

fn front_end(md: &mut String, ok: &[&Paired]) {
    let c: Vec<Counts> = ok.iter().map(|p| counts(&p.front)).collect();
    let sum = |f: fn(&Counts) -> usize| c.iter().map(f).sum::<usize>();
    let minutes: f64 = ok.iter().map(|p| p.front.duration_s / 60.0).sum();
    let (recalled, n_truth) = ok.iter().fold((0, 0), |a, p| (a.0 + p.front.recalled, a.1 + p.front.n_truth));
    let (ac, an) = ok.iter().fold((0, 0), |a, p| (a.0 + p.front.assoc_correct, a.1 + p.front.assoc_of));
    md.push_str("\n## Front end\n\n| quantity | value |\n|---|---|\n");
    let mut row = |k: &str, v: String| {
        let _ = writeln!(md, "| {k} | {v} |");
    };
    row("labelled truth passes recalled", frac(recalled, n_truth));
    row("events found", sum(|c| c.found).to_string());
    row("… labelled / other ADS-B pass / spurious", format!("{} / {} / {}", sum(|c| c.labelled), sum(|c| c.adsb), sum(|c| c.spurious)));
    row("precision (real passes / found)", frac(sum(|c| c.labelled) + sum(|c| c.adsb), sum(|c| c.found)));
    row("spurious per minute", format!("{:.2}", sum(|c| c.spurious) as f64 / minutes.max(1e-9)));
    row("searched events / of them spurious", format!("{} / {}", sum(|c| c.used), sum(|c| c.used_spurious)));
    row("searched events matched to their true flight", frac(ac, an));
    let rows: Vec<&super::EventRow> = ok.iter().flat_map(|p| &p.front.events).collect();
    for (name, src) in [("labelled", Source::Labelled), ("ADS-B", Source::Adsb)] {
        let pick = |f: fn(&super::truth::Truthed) -> Option<f64>, scale: f64| {
            let v: Vec<f64> = rows.iter().filter(|r| r.truth.source == Some(src)).filter_map(|r| f(&r.truth)).map(|x| x * scale).collect();
            let abs: Vec<f64> = v.iter().map(|x| x.abs()).collect();
            if v.is_empty() { "–".into() } else { format!("{:+.2} / {:.2} (n {})", median(&v), median(&abs), v.len()) }
        };
        row(&format!("{name}: CPA time error s, median / median |·|"), pick(|t| t.dt_cpa_s, 1.0));
        row(&format!("{name}: f0 error %"), pick(|t| t.f0_err, 100.0));
        row(&format!("{name}: CPA slant error %"), pick(|t| t.d_err, 100.0));
        row(&format!("{name}: speed error % (vs ground speed)"), pick(|t| t.v_err, 100.0));
        let of_src: Vec<&super::truth::Truthed> = rows.iter().map(|r| &r.truth).filter(|t| t.source == Some(src)).collect();
        let gate = |f: fn(&super::truth::Truthed) -> Option<bool>| {
            let v: Vec<bool> = of_src.iter().filter_map(|t| f(t)).collect();
            frac(v.iter().filter(|&&b| b).count(), v.len())
        };
        let all = |t: &super::truth::Truthed| Some(t.d_gate? && t.v_gate? && t.clock_gate?);
        row(
            &format!("{name}: true flight passes assoc gates d / v / clock / all"),
            format!("{} / {} / {} / {}", gate(|t| t.d_gate), gate(|t| t.v_gate), gate(|t| t.clock_gate), gate(all)),
        );
    }
}

fn per_case(md: &mut String, ok: &[&Paired]) {
    let lost = |p: &Paired| (err(&p.discovered).min(1e7) - err(&p.oracle).min(1e7), p.front.id.clone());
    let mut v: Vec<&&Paired> = ok.iter().collect();
    v.sort_by(|a, b| lost(b).0.total_cmp(&lost(a).0).then(lost(a).1.cmp(&lost(b).1)));
    md.push_str("\n## Cases, most accuracy lost first\n\n");
    md.push_str("| case | oracle m | discovered m | flights o/d | found | real | spurious | searched | recalled | assoc | lost at (d) | Δ log score |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    let b = |x: Option<bool>| x.map_or("–", |b| if b { "y" } else { "n" });
    for p in v {
        let c = counts(&p.front);
        let dls = p.discovered.metrics.log_score.zip(p.oracle.metrics.log_score).map_or("–".into(), |(d, o)| format!("{:+.2}", d - o));
        let _ = writeln!(
            md,
            "| {} | {} | {} | {}/{} | {} | {} | {} | {} | {}/{} | {}/{} | {} | {} |",
            p.front.id,
            fmt_m(err(&p.oracle)),
            fmt_m(err(&p.discovered)),
            b(p.oracle.flights_correct),
            b(p.discovered.flights_correct),
            c.found,
            c.labelled + c.adsb,
            c.spurious,
            c.used,
            p.front.recalled,
            p.front.n_truth,
            p.front.assoc_correct,
            p.front.assoc_of,
            p.discovered.metrics.lost_at.as_deref().unwrap_or("–"),
            dls,
        );
    }
}
