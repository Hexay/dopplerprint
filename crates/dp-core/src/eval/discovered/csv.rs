//! cases.csv: one row per case, oracle and discovered side by side, plus front-end counts and the fit
//! errors of the first recalled labelled pass.

use std::fmt::Write as _;
use std::path::Path;

use super::Paired;
use super::summary::counts;
use crate::eval::CaseResult;

const HEADER: &str = "case,status,oracle_err_m,disc_err_m,oracle_flights,disc_flights,oracle_log_score,disc_log_score,\
oracle_lost_at,disc_lost_at,n_truth,recalled,found,labelled,adsb,spurious,searched,searched_spurious,assoc_correct,\
assoc_of,dt_cpa_s,f0_err,d_err,v_err,d_gate,v_gate,clock_gate,discover_s,disc_runtime_s";

fn o<T: ToString>(v: Option<T>) -> String {
    v.map_or(String::new(), |x| x.to_string())
}

fn mode(r: &CaseResult) -> [String; 4] {
    [o(r.error_m), o(r.flights_correct), o(r.metrics.log_score), o(r.metrics.lost_at.clone())]
}

pub fn write(path: &Path, pairs: &[Paired]) -> anyhow::Result<()> {
    let mut s = format!("{HEADER}\n");
    for p in pairs {
        let (od, dd) = (mode(&p.oracle), mode(&p.discovered));
        let c = counts(&p.front);
        let first = p.front.events.iter().find(|e| e.truth.truth_index.is_some()).map(|e| &e.truth);
        let fit = |f: fn(&super::truth::Truthed) -> Option<f64>| o(first.and_then(f));
        let gate = |f: fn(&super::truth::Truthed) -> Option<bool>| o(first.and_then(f));
        let status = p.discovered.status.split(':').next().unwrap_or_default();
        let _ = writeln!(
            s,
            "{},{status},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.2},{:.2}",
            p.front.id,
            od[0], dd[0], od[1], dd[1], od[2], dd[2], od[3], dd[3],
            p.front.n_truth,
            p.front.recalled,
            c.found,
            c.labelled,
            c.adsb,
            c.spurious,
            c.used,
            c.used_spurious,
            p.front.assoc_correct,
            p.front.assoc_of,
            fit(|t| t.dt_cpa_s),
            fit(|t| t.f0_err),
            fit(|t| t.d_err),
            fit(|t| t.v_err),
            gate(|t| t.d_gate),
            gate(|t| t.v_gate),
            gate(|t| t.clock_gate),
            p.front.discover_s,
            p.discovered.runtime_s,
        );
    }
    std::fs::write(path, s)?;
    Ok(())
}
