//! ADS-B ingest (adsb.lol globe_history → Parquet) and windowed track queries.
//! On-disk layout: see docs/DATA.md.

pub mod typedb;

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

/// Convert one adsb.lol day archive into the Parquet store under `out_dir`.
pub fn ingest_adsblol_day(_archive: &std::path::Path, _out_dir: &std::path::Path) -> anyhow::Result<()> {
    todo!("adsb agent")
}

pub fn load_tracks(_store: &std::path::Path, _q: &TrackQuery) -> anyhow::Result<Vec<Track>> {
    todo!("adsb agent")
}
