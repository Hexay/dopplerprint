use std::path::PathBuf;

use dp_core::eval::calibrate::{fit, load, write_toml};
use dp_core::eval::calib_report::markdown;
use dp_core::eval::coverage::Shape;

#[derive(clap::Args)]
pub struct Args {
    /// Eval run directories to fit on (never the frozen test split).
    #[arg(long = "calib", required = true)]
    pub calib: Vec<PathBuf>,
    /// Held-out run directories, scored once with the fitted values.
    #[arg(long = "test")]
    pub test: Vec<PathBuf>,
    /// Fit T only on calibration cases whose id contains this (repeatable; default all).
    #[arg(long = "t-case")]
    pub t_cases: Vec<String>,
    /// "Before" temperature for the comparison.
    #[arg(long, default_value_t = 3.0)]
    pub before_t: f64,
    /// Write the fitted calibration TOML here (e.g. crates/dp-core/src/search/calibration.toml).
    #[arg(long)]
    pub write: Option<PathBuf>,
    #[arg(long, default_value = "")]
    pub provenance: String,
    /// Markdown report path (printed when absent).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let calib = load(&a.calib)?;
    let test = load(&a.test)?;
    let shipped = Shape::shipped();
    let base = Shape { temperature: a.before_t, ..shipped };
    let f = fit(&calib, &a.t_cases, base, &a.provenance);
    let md = markdown(&f, &calib, &test, base);
    if let Some(p) = &a.write {
        write_toml(p, &f.calibration)?;
    }
    match &a.out {
        Some(p) => std::fs::write(p, &md)?,
        None => println!("{md}"),
    }
    Ok(())
}
