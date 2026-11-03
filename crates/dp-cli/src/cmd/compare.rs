use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// Baseline run directory (holds summary.json).
    pub a: PathBuf,
    /// Variant run directory.
    pub b: PathBuf,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    println!("{}", dp_core::eval::compare::compare(&a.a, &a.b)?);
    Ok(())
}
