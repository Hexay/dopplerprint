//! The `dopplerprint` command line, shared by the Rust binary and the Python entry point (`_core.cli`).

mod cmd;

use std::ffi::OsString;

use clap::{Parser, Subcommand};

pub use cmd::serve_backend::AppBackend;

/// Locate an audio recording from the aircraft heard in it, matched to historical ADS-B.
#[derive(Parser)]
#[command(name = "dopplerprint", version, after_help = "Start here: dopplerprint locate --help")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Find where a recording was made (downloads and caches the ADS-B it needs).
    Locate(cmd::locate::Args),
    /// Open a drag-and-drop page in your browser that runs locate (local web app).
    Serve(cmd::serve::Args),
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
    /// Paired per-case comparison of two eval runs (B − A) with bootstrap CIs.
    Compare(cmd::compare::Args),
}

/// Parse `args` (including the program name) and run; returns the process exit code:
/// 0 found / ok, 1 error, 2 needs input (or bad usage), 3 inconclusive.
pub fn run<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return e.exit_code() as u8;
        }
    };
    let done = match cli.command {
        Command::Locate(a) => cmd::locate::run(a),
        Command::Serve(a) => cmd::serve::run(a),
        Command::Ingest(a) => cmd::ingest::run(a).map(|_| 0),
        Command::Tracks(a) => cmd::tracks::run(a).map(|_| 0),
        Command::Discover(a) => cmd::discover::run(a).map(|_| 0),
        Command::Sim(a) => cmd::sim::run(a).map(|_| 0),
        Command::Eval(a) => cmd::eval::run(a).map(|_| 0),
        Command::Compare(a) => cmd::compare::run(a).map(|_| 0),
    };
    done.unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        1
    })
}
