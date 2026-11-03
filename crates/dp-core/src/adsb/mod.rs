//! ADS-B ingest (adsb.lol globe_history → Parquet) and windowed track queries.
//! On-disk layout: see docs/DATA.md.

pub mod altitude;
pub mod clean;
pub mod coverage;
pub mod dedup;
pub mod finalize;
pub mod geojson;
pub mod index;
pub mod ingest;
pub mod manifest;
pub mod model;
pub mod neighbour;
pub mod pieces;
pub mod query;
pub mod schema;
pub mod source;
pub mod spill;
pub mod trace;
pub mod typedb;

pub use index::{SegmentHit, SegmentIndex};
pub use ingest::{IngestOptions, ingest_day};
pub use manifest::Manifest;
pub use model::AltSource;
pub use query::{LoadOptions, load_tracks_with};

use crate::track::{EngineClass, Track};

/// Query for tracks touching a space-time window.
#[derive(Clone, Debug)]
pub struct TrackQuery {
    pub t_start: f64,
    pub t_end: f64,
    /// (lat_min, lon_min, lat_max, lon_max); `None` = everywhere.
    pub bbox: Option<(f64, f64, f64, f64)>,
    pub max_alt_m: Option<f64>,
    pub exclude_engines: Vec<EngineClass>,
}

/// Convert one adsb.lol day archive into the Parquet store under `out_dir`, with default options.
pub fn ingest_adsblol_day(archive: &std::path::Path, out_dir: &std::path::Path) -> anyhow::Result<()> {
    ingest_day(archive, out_dir, &IngestOptions::default()).map(|_| ())
}

pub fn load_tracks(store: &std::path::Path, q: &TrackQuery) -> anyhow::Result<Vec<Track>> {
    query::load_tracks(store, q)
}
