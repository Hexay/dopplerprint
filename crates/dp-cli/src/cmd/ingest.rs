use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    pub archive: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    dp_core::adsb::ingest_adsblol_day(&a.archive, &a.out)
}
