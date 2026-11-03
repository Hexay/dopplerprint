use std::path::PathBuf;

use dp_core::audio::{self, SpecParams, spectrogram::log_spectrogram};

#[derive(clap::Args)]
pub struct Args {
    pub audio: PathBuf,
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let audio = audio::decode(&a.audio)?;
    let spec = log_spectrogram(&audio, &SpecParams::default());
    let events = dp_core::events::discover(&spec);
    let json = serde_json::to_string_pretty(&events)?;
    match a.out {
        Some(p) => std::fs::write(p, json)?,
        None => println!("{json}"),
    }
    Ok(())
}
