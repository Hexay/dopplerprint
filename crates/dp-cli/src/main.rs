mod cmd;

use clap::{Parser, Subcommand};

/// Locate an audio recording from aircraft Doppler curves matched to historical ADS-B.
#[derive(Parser)]
#[command(name = "doppler-print", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert an adsb.lol day archive into the Parquet track store.
    Ingest(cmd::ingest::Args),
    /// Query the track store for a time window and region; optionally write GeoJSON.
    Tracks(cmd::tracks::Args),
    /// Find Doppler events in a recording and fit constant-velocity parameters.
    Discover(cmd::discover::Args),
    /// Render a synthetic recording from tracks and an observer position.
    Sim(cmd::sim::Args),
    /// Run the corpus + sim evaluation and write runs/<stamp>/summary.md.
    Eval(cmd::eval::Args),
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Ingest(a) => cmd::ingest::run(a),
        Command::Tracks(a) => cmd::tracks::run(a),
        Command::Discover(a) => cmd::discover::run(a),
        Command::Sim(a) => cmd::sim::run(a),
        Command::Eval(a) => cmd::eval::run(a),
    }
}
