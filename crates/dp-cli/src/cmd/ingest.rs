use std::path::PathBuf;

use dp_core::adsb::{IngestOptions, ingest_day};

#[derive(clap::Args)]
pub struct Args {
    /// A `*.tar.aa` part (siblings are chained), a single tar, a release dir, or a dir of extracted traces.
    pub archive: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    /// tar1090-db dir (icao_aircraft_types.js[on], aircraft.csv[.gz]); default $DP_TYPEDB or ./data/typedb.
    #[arg(long)]
    pub typedb: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let mut opts = IngestOptions::default();
    if a.typedb.is_some() {
        opts.typedb_dir = a.typedb;
    }
    let mf = ingest_day(&a.archive, &a.out, &opts)?;
    println!(
        "{}: {} traces, {} aircraft, {} tracks, {} points ({} in), {} segments, {} coverage rows in {:.1} s",
        mf.date,
        mf.trace_files,
        mf.aircraft,
        mf.tracks,
        mf.points_rows,
        mf.points_in,
        mf.segment_rows,
        mf.coverage_rows,
        mf.elapsed_s
    );
    println!("dropped {:?}; parse errors {}; truncated {}", mf.dropped, mf.parse_errors, mf.truncated_archive);
    println!("engines {:?}; altitude basis {:?}", mf.aircraft_by_engine, mf.alt_source_points);
    Ok(())
}
