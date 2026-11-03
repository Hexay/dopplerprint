use std::path::PathBuf;

use anyhow::Context;
use dp_core::adsb::manifest::parse_utc;
use dp_core::adsb::model::engine_from_name;
use dp_core::adsb::{TrackQuery, geojson::tracks_geojson, load_tracks};

#[derive(clap::Args)]
pub struct Args {
    pub store: PathBuf,
    /// Window start: UNIX seconds or UTC `YYYY-MM-DDTHH:MM:SS`.
    #[arg(long)]
    pub t0: String,
    #[arg(long)]
    pub t1: String,
    /// lat_min,lon_min,lat_max,lon_max
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    pub bbox: Option<Vec<f64>>,
    #[arg(long)]
    pub max_alt_m: Option<f64>,
    /// Engine classes to drop, e.g. `--exclude jet,helicopter`.
    #[arg(long, value_delimiter = ',')]
    pub exclude: Vec<String>,
    #[arg(long)]
    pub geojson: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let time = |s: &str| parse_utc(s).with_context(|| format!("bad time {s:?}"));
    anyhow::ensure!(a.bbox.as_ref().is_none_or(|b| b.len() == 4), "--bbox takes lat_min,lon_min,lat_max,lon_max");
    let q = TrackQuery {
        t_start: time(&a.t0)?,
        t_end: time(&a.t1)?,
        bbox: a.bbox.map(|b| (b[0], b[1], b[2], b[3])),
        max_alt_m: a.max_alt_m,
        exclude_engines: a
            .exclude
            .iter()
            .map(|e| engine_from_name(e).with_context(|| format!("unknown engine class {e:?}")))
            .collect::<anyhow::Result<_>>()?,
    };
    let tracks = load_tracks(&a.store, &q)?;
    let points: usize = tracks.iter().map(|t| t.points.len()).sum();
    println!("{} tracks, {} points", tracks.len(), points);
    for t in tracks.iter().take(25) {
        let ty = t.type_code.as_deref().unwrap_or("-");
        println!("  {:<24} {:<5} {:<10?} {:.0}..{:.0} n={}", t.id, ty, t.engine, t.t_start(), t.t_end(), t.points.len());
    }
    if let Some(path) = a.geojson {
        std::fs::write(&path, serde_json::to_vec(&tracks_geojson(&tracks))?)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
