use std::path::PathBuf;

use dp_core::eval::calib_report::markdown;
use dp_core::eval::calibrate::{Case, Events, Set, fit, is_discovered_run, load, write_toml};use dp_core::eval::coverage::Shape;
use dp_core::search::calibration::{self, Calibration};

#[derive(clap::Args)]
pub struct Args {
    /// Eval run directories to fit on (never the frozen test split).
    #[arg(long = "calib", required = true)]
    pub calib: Vec<PathBuf>,
    /// Events the calibration runs are read with: `discovered` (what `locate` searches; needs runs made with
    /// eval.oracle_events = false) or `oracle` (truth labels; a discovered run's oracle/ baseline).
    #[arg(long, default_value = "discovered", value_parser = ["discovered", "oracle"])]
    pub events: String,
    /// Held-out run directories, scored once with the fitted values: discovered runs in both event modes.
    #[arg(long = "test")]
    pub test: Vec<PathBuf>,
    /// Fit T (and ε) only on calibration cases whose id contains this (repeatable; default all).
    #[arg(long = "t-case")]
    pub t_cases: Vec<String>,
    /// Also fit the uniform floor ε by log score (coordinate ascent with T); otherwise keep the before ε.
    #[arg(long)]
    pub fit_floor: bool,
    /// Calibration TOML to compare against (default: the shipped one).
    #[arg(long)]
    pub before: Option<PathBuf>,
    /// Case ids (substrings) to list per case in every section (repeatable).
    #[arg(long = "detail")]
    pub detail: Vec<String>,
    /// Write the fitted calibration TOML here (e.g. crates/dp-core/src/search/calibration.toml).
    #[arg(long)]
    pub write: Option<PathBuf>,
    /// Free text put before the generated provenance (date, what and why).
    #[arg(long, default_value = "")]
    pub provenance: String,
    /// Markdown report path (printed when absent).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let ev = if a.events == "oracle" { Events::Oracle } else { Events::Discovered };
    let calib: Vec<Set> = a.calib.iter().map(|d| load(d, ev)).collect::<anyhow::Result<_>>()?;
    let mut tests = Vec::new();
    for d in &a.test {
        if is_discovered_run(d) {
            tests.push(load(d, Events::Discovered)?);
        }
        tests.push(load(d, Events::Oracle)?);
    }
    let before: Calibration = match &a.before {
        Some(p) => Calibration::from_file(p)?,
        None => calibration::get().clone(),
    };
    let base = Shape { temperature: before.temperature, floor: before.floor, persistence_nats: before.persistence_nats };
    let pooled: Vec<&Case> = calib.iter().flat_map(|s| &s.cases).collect();
    let mut f = fit(&pooled, &a.t_cases, base, a.fit_floor, "");
    f.calibration.provenance = provenance(&a, &pooled, f.n_t_fit);
    let md = markdown(&f, &calib, &tests, &before, &a.detail);
    if let Some(p) = &a.write {
        write_toml(p, &f.calibration)?;
    }
    match &a.out {
        Some(p) => std::fs::write(p, &md)?,
        None => println!("{md}"),
    }
    Ok(())
}

fn provenance(a: &Args, cases: &[&Case], n_t_fit: usize) -> String {
    let runs: Vec<String> = a.calib.iter().map(|p| p.display().to_string()).collect();
    let n_sim = cases.iter().filter(|c| c.is_sim()).count();
    let t_on = if a.t_cases.is_empty() { "all".to_string() } else { a.t_cases.join(", ") };
    format!(
        "{} Fitted by `dopplerprint calibrate` on {} events of runs [{}]: {} real + {} sim answered cases. \
         T{} by mean log score on cases matching [{t_on}] ({n_t_fit}); q̂ split-conformal HPD per stratum (real alone when ≥ 9).",
        a.provenance,
        a.events,
        runs.join(", "),
        cases.len() - n_sim,
        n_sim,
        if a.fit_floor { " and ε" } else { "" },
    )
    .trim_start()
    .to_string()
}
