use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// Baseline run directory (holds summary.json).
    pub a: PathBuf,
    /// Variant run directory.
    pub b: PathBuf,
    /// Only cases whose id contains this (repeatable), e.g. to stratify single vs multi-event.
    #[arg(long = "case")]
    pub cases: Vec<String>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    println!("{}", dp_core::eval::compare::compare(&a.a, &a.b, &a.cases)?);
    Ok(())
}
