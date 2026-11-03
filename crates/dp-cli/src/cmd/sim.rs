use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// Scenario TOML (observer, tracks or track generator, source spectrum, noise).
    pub scenario: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
}

pub fn run(_a: Args) -> anyhow::Result<()> {
    todo!("physics agent: load scenario, sim::render, write wav + truth.json")
}
