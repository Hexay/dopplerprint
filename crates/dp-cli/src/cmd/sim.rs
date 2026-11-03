use std::path::PathBuf;

use anyhow::Context;
use dp_core::sim;

#[derive(clap::Args)]
pub struct Args {
    /// Scenario TOML (observer, tracks or track generator, source spectrum, noise).
    pub scenario: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(&a.scenario).with_context(|| format!("reading {}", a.scenario.display()))?;
    let r = sim::render_toml(&text).with_context(|| format!("scenario {}", a.scenario.display()))?;
    sim::write_outputs(&r, &a.out)?;
    println!("wrote {} ({} samples @ {} Hz)", a.out.join("audio.wav").display(), r.samples.len(), r.sample_rate);
    for e in &r.truth.events {
        println!(
            "  {:<12} CPA arrival {:8.2} s  slant {:7.1} m  speed {:5.1} m/s  f0 {:6.1} Hz{}",
            e.track_id,
            e.cpa_arrival_audio_s,
            e.slant_m,
            e.speed_mps,
            e.f0,
            if e.in_recording { "" } else { "  (outside recording)" }
        );
    }
    Ok(())
}
